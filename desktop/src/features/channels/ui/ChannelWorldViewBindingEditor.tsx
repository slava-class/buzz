import * as React from "react";

import { useRegisterLocalWorldAuthorityMutation } from "@/features/channels/hooks";
import { parseShareableWorldViewDescriptor } from "@/features/channels/worldViewDescriptor";
import type {
  WorldViewBinding,
  WorldViewBindingScope,
  WorldViewBindingsDocument,
} from "@/shared/api/worldViewTypes";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

type WorldViewBindingEditorState = {
  descriptor: string;
  descriptorError: string | null;
  displayMode: WorldViewBinding["displayMode"];
  label: string;
  localSourceRoot: string;
  origin: string;
  realmQualifiedName: string;
  sourceKind: WorldViewBinding["reference"]["kind"];
  sourceValue: string;
  viewQualifiedName: string;
};

type WorldViewBindingEditorAction = {
  type: "patch";
  value: Partial<WorldViewBindingEditorState>;
};

function createWorldViewBindingEditorState(
  binding: WorldViewBinding | null,
): WorldViewBindingEditorState {
  return {
    descriptor: "",
    descriptorError: null,
    displayMode: binding?.displayMode ?? "graph",
    label: binding?.label ?? "",
    localSourceRoot: "",
    origin: binding?.reference.origin ?? "https://manifest.shivai.space",
    realmQualifiedName: binding?.realmQualifiedName ?? "",
    sourceKind: binding?.reference.kind ?? "local-world-mirror-latest",
    sourceValue:
      binding?.reference.kind === "local-world-mirror-latest"
        ? binding.reference.mirrorId
        : (binding?.reference.shareToken ?? ""),
    viewQualifiedName: binding?.viewQualifiedName ?? "",
  };
}

function worldViewBindingEditorReducer(
  state: WorldViewBindingEditorState,
  action: WorldViewBindingEditorAction,
): WorldViewBindingEditorState {
  return { ...state, ...action.value };
}

export type ChannelWorldViewBindingEditorProps = {
  binding: WorldViewBinding | null;
  bindings: readonly WorldViewBinding[];
  bindingScope: WorldViewBindingScope;
  isPublishing: boolean;
  onComplete: () => void;
  onPublish: (document: WorldViewBindingsDocument) => Promise<void>;
  publishError: Error | null;
};

export function ChannelWorldViewBindingEditor({
  binding,
  bindings,
  bindingScope,
  isPublishing,
  onComplete,
  onPublish,
  publishError,
}: ChannelWorldViewBindingEditorProps) {
  const authorityMutation = useRegisterLocalWorldAuthorityMutation();
  const [state, dispatch] = React.useReducer(
    worldViewBindingEditorReducer,
    createWorldViewBindingEditorState(binding),
  );
  const descriptorInputId = React.useId();
  const originInputId = React.useId();
  const sourceValueInputId = React.useId();
  const localSourceRootInputId = React.useId();
  const realmQualifiedNameInputId = React.useId();
  const viewQualifiedNameInputId = React.useId();
  const labelInputId = React.useId();
  const reusesLocalReference =
    state.sourceKind === "local-world-mirror-latest" &&
    binding?.reference.kind === "local-world-mirror-latest" &&
    binding.reference.origin === state.origin.trim() &&
    binding.reference.mirrorId === state.sourceValue.trim();
  const localSourceRootRequired =
    state.sourceKind === "local-world-mirror-latest" && !reusesLocalReference;

  function patch(value: Partial<WorldViewBindingEditorState>): void {
    dispatch({ type: "patch", value });
  }

  function handleUseDescriptor(): void {
    const parsed = parseShareableWorldViewDescriptor(state.descriptor);
    if (!parsed.ok) {
      patch({ descriptorError: parsed.error });
      return;
    }
    patch({
      descriptorError: null,
      localSourceRoot: "",
      origin: parsed.value.reference.origin,
      realmQualifiedName: parsed.value.realmQualifiedName,
      sourceKind: parsed.value.reference.kind,
      sourceValue: parsed.value.reference.shareToken,
      viewQualifiedName: parsed.value.viewQualifiedName,
    });
  }

  async function handleSubmit(
    event: React.FormEvent<HTMLFormElement>,
  ): Promise<void> {
    event.preventDefault();
    const normalizedOrigin = state.origin.trim();
    const normalizedSourceValue = state.sourceValue.trim();
    const normalizedRealm = state.realmQualifiedName.trim();
    const normalizedView = state.viewQualifiedName.trim();
    if (
      !normalizedOrigin ||
      !normalizedSourceValue ||
      !normalizedRealm ||
      !normalizedView
    ) {
      return;
    }

    const reference: WorldViewBinding["reference"] =
      state.sourceKind === "local-world-mirror-latest"
        ? {
            kind: "local-world-mirror-latest",
            origin: normalizedOrigin,
            mirrorId: normalizedSourceValue,
          }
        : {
            kind: "hosted-world-view-export",
            origin: normalizedOrigin,
            shareToken: normalizedSourceValue,
          };
    if (
      reference.kind === "local-world-mirror-latest" &&
      !reusesLocalReference
    ) {
      const normalizedLocalSourceRoot = state.localSourceRoot.trim();
      if (!normalizedLocalSourceRoot) return;
      await authorityMutation.mutateAsync({
        origin: reference.origin,
        mirrorId: reference.mirrorId,
        sourceRoot: normalizedLocalSourceRoot,
      });
    }

    const nextBinding: WorldViewBinding = {
      id: binding?.id ?? crypto.randomUUID(),
      ...(state.label.trim() ? { label: state.label.trim() } : {}),
      reference,
      realmQualifiedName: normalizedRealm,
      viewQualifiedName: normalizedView,
      displayMode: state.displayMode,
    };
    const replacesExactBinding = bindings.some(
      (candidate) => candidate.id === nextBinding.id,
    );
    const nextBindings = replacesExactBinding
      ? bindings.map((candidate) =>
          candidate.id === nextBinding.id ? nextBinding : candidate,
        )
      : [...bindings, nextBinding];
    await onPublish({
      version: 2,
      scope: bindingScope,
      bindings: nextBindings,
    });
    onComplete();
  }

  return (
    <form
      className="grid gap-2 rounded-xl border border-border/70 bg-background/70 p-3"
      onSubmit={(event) => void handleSubmit(event)}
    >
      <div className="grid gap-1 rounded-lg border border-border/60 bg-muted/20 p-2">
        <label
          className="text-2xs font-medium text-muted-foreground"
          htmlFor={descriptorInputId}
        >
          Paste Shivai view reference
        </label>
        <textarea
          className="min-h-24 resize-y rounded-md border border-input bg-background px-3 py-2 font-mono text-xs text-foreground"
          id={descriptorInputId}
          onChange={(event) =>
            patch({
              descriptor: event.target.value,
              descriptorError: null,
            })
          }
          placeholder={
            'Shivai view reference\nSource: hosted view export "..."\nRealm: world::main\nView qualified: world::main::@Board'
          }
          value={state.descriptor}
        />
        <div className="flex items-center justify-between gap-3">
          <span className="text-3xs leading-relaxed text-muted-foreground">
            Local paths and edit-share capabilities are rejected before
            publication.
          </span>
          <Button
            disabled={!state.descriptor.trim()}
            onClick={handleUseDescriptor}
            size="sm"
            type="button"
            variant="outline"
          >
            Use reference
          </Button>
        </div>
        {state.descriptorError ? (
          <p className="text-xs text-destructive">{state.descriptorError}</p>
        ) : null}
      </div>
      <label className="grid gap-1 text-2xs font-medium text-muted-foreground">
        Source
        <select
          className="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
          onChange={(event) =>
            patch({
              sourceKind: event.target
                .value as WorldViewBinding["reference"]["kind"],
              sourceValue: "",
              localSourceRoot: "",
            })
          }
          value={state.sourceKind}
        >
          <option value="local-world-mirror-latest">
            Published local world
          </option>
          <option value="hosted-world-view-export">Hosted view export</option>
        </select>
      </label>
      <label
        className="grid gap-1 text-2xs font-medium text-muted-foreground"
        htmlFor={originInputId}
      >
        Hosted origin
        <Input
          autoComplete="url"
          id={originInputId}
          onChange={(event) => patch({ origin: event.target.value })}
          placeholder="https://manifest.shivai.space"
          required
          type="url"
          value={state.origin}
        />
      </label>
      <label
        className="grid gap-1 text-2xs font-medium text-muted-foreground"
        htmlFor={sourceValueInputId}
      >
        {state.sourceKind === "local-world-mirror-latest"
          ? "Mirror ID"
          : "Share token"}
        <Input
          autoComplete="off"
          id={sourceValueInputId}
          onChange={(event) => patch({ sourceValue: event.target.value })}
          placeholder={
            state.sourceKind === "local-world-mirror-latest"
              ? "Published mirror ID"
              : "Hosted export token"
          }
          required
          value={state.sourceValue}
        />
        {state.sourceKind === "hosted-world-view-export" ? (
          <span className="font-normal leading-relaxed text-amber-11">
            A read-only export token is an intentional bearer capability.
            Binding publishes it in this channel or thread scope.
          </span>
        ) : null}
      </label>
      {state.sourceKind === "local-world-mirror-latest" ? (
        <label
          className="grid gap-1 text-2xs font-medium text-muted-foreground"
          htmlFor={localSourceRootInputId}
        >
          Local source root
          <Input
            autoComplete="off"
            id={localSourceRootInputId}
            onChange={(event) => patch({ localSourceRoot: event.target.value })}
            placeholder={
              localSourceRootRequired
                ? "/path/to/project.world"
                : "Source unchanged; private path not required"
            }
            required={localSourceRootRequired}
            value={state.localSourceRoot}
          />
          <span className="font-normal text-muted-foreground/80">
            Kept on this device. Only the published mirror ID enters the binding
            event.
          </span>
        </label>
      ) : null}
      <label
        className="grid gap-1 text-2xs font-medium text-muted-foreground"
        htmlFor={realmQualifiedNameInputId}
      >
        Realm qualified name
        <Input
          id={realmQualifiedNameInputId}
          onChange={(event) =>
            patch({ realmQualifiedName: event.target.value })
          }
          placeholder="world::main"
          required
          value={state.realmQualifiedName}
        />
      </label>
      <label
        className="grid gap-1 text-2xs font-medium text-muted-foreground"
        htmlFor={viewQualifiedNameInputId}
      >
        View qualified name
        <Input
          id={viewQualifiedNameInputId}
          onChange={(event) => patch({ viewQualifiedName: event.target.value })}
          placeholder="world::main::@Board"
          required
          value={state.viewQualifiedName}
        />
      </label>
      <label
        className="grid gap-1 text-2xs font-medium text-muted-foreground"
        htmlFor={labelInputId}
      >
        Label
        <Input
          id={labelInputId}
          onChange={(event) => patch({ label: event.target.value })}
          placeholder="Launch board"
          value={state.label}
        />
      </label>
      <label className="grid gap-1 text-2xs font-medium text-muted-foreground">
        Initial display
        <select
          className="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
          onChange={(event) =>
            patch({
              displayMode: event.target
                .value as WorldViewBinding["displayMode"],
            })
          }
          value={state.displayMode}
        >
          <option value="graph">Graph</option>
          <option value="tasks">Tasks</option>
        </select>
      </label>
      {authorityMutation.error instanceof Error ? (
        <p className="text-xs text-destructive">
          {authorityMutation.error.message}
        </p>
      ) : publishError ? (
        <p className="text-xs text-destructive">{publishError.message}</p>
      ) : null}
      <div className="flex justify-end">
        <Button
          disabled={isPublishing || authorityMutation.isPending}
          size="sm"
          type="submit"
        >
          {isPublishing || authorityMutation.isPending
            ? "Binding..."
            : binding
              ? "Save view"
              : "Bind world view"}
        </Button>
      </div>
    </form>
  );
}
