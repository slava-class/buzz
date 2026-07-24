import { ChevronDown, ChevronUp, Network, Plus, X } from "lucide-react";
import * as React from "react";
import { WorldViewTileSurface } from "@shivai.space/world-view-react";

import {
  useResolvedWorldViewQuery,
  useRegisterLocalWorldAuthorityMutation,
  useSetWorldViewBindingsMutation,
  useWorldViewBindingsQuery,
} from "@/features/channels/hooks";
import type {
  WorldViewBinding,
  WorldViewBindingsDocument,
} from "@/shared/api/worldViewTypes";
import { useTheme } from "@/shared/theme/ThemeProvider";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

export type ChannelWorldViewsProps = {
  channelId: string;
  canEdit: boolean;
};

export function ChannelWorldViews({
  channelId,
  canEdit,
}: ChannelWorldViewsProps) {
  const bindingsQuery = useWorldViewBindingsQuery(channelId);
  const mutation = useSetWorldViewBindingsMutation(channelId);
  const authorityMutation = useRegisterLocalWorldAuthorityMutation();
  const originInputId = React.useId();
  const sourceValueInputId = React.useId();
  const localSourceRootInputId = React.useId();
  const realmQualifiedNameInputId = React.useId();
  const viewQualifiedNameInputId = React.useId();
  const labelInputId = React.useId();
  const [expanded, setExpanded] = React.useState(true);
  const [editing, setEditing] = React.useState(false);
  const [sourceKind, setSourceKind] = React.useState<
    WorldViewBinding["reference"]["kind"]
  >("local-world-mirror-latest");
  const [origin, setOrigin] = React.useState("https://manifest.shivai.space");
  const [label, setLabel] = React.useState("");
  const [sourceValue, setSourceValue] = React.useState("");
  const [localSourceRoot, setLocalSourceRoot] = React.useState("");
  const [realmQualifiedName, setRealmQualifiedName] = React.useState("");
  const [viewQualifiedName, setViewQualifiedName] = React.useState("");
  const [displayMode, setDisplayMode] =
    React.useState<WorldViewBinding["displayMode"]>("graph");
  const document = bindingsQuery.data?.document;
  const bindings = document?.bindings ?? [];

  async function publishDocument(nextDocument: WorldViewBindingsDocument) {
    await mutation.mutateAsync(nextDocument);
  }

  async function handleAddBinding(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedOrigin = origin.trim();
    const normalizedSourceValue = sourceValue.trim();
    const normalizedRealm = realmQualifiedName.trim();
    const normalizedView = viewQualifiedName.trim();
    if (
      !normalizedOrigin ||
      !normalizedSourceValue ||
      !normalizedRealm ||
      !normalizedView
    ) {
      return;
    }

    const reference: WorldViewBinding["reference"] =
      sourceKind === "local-world-mirror-latest"
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
    if (reference.kind === "local-world-mirror-latest") {
      const normalizedLocalSourceRoot = localSourceRoot.trim();
      if (!normalizedLocalSourceRoot) return;
      await authorityMutation.mutateAsync({
        origin: reference.origin,
        mirrorId: reference.mirrorId,
        sourceRoot: normalizedLocalSourceRoot,
      });
    }
    const binding: WorldViewBinding = {
      id: crypto.randomUUID(),
      ...(label.trim() ? { label: label.trim() } : {}),
      reference,
      realmQualifiedName: normalizedRealm,
      viewQualifiedName: normalizedView,
      displayMode,
    };
    await publishDocument({
      version: 1,
      bindings: [...bindings, binding],
    });
    setLabel("");
    setSourceValue("");
    setLocalSourceRoot("");
    setRealmQualifiedName("");
    setViewQualifiedName("");
    setEditing(false);
    setExpanded(true);
  }

  if (bindingsQuery.isLoading) {
    return (
      <div className="border-b border-border/70 px-5 py-2 text-xs text-muted-foreground">
        Loading Shivai views...
      </div>
    );
  }

  if (bindingsQuery.error instanceof Error) {
    return (
      <div className="border-b border-destructive/30 bg-destructive/5 px-5 py-2 text-xs text-destructive">
        {bindingsQuery.error.message}
      </div>
    );
  }

  if (bindings.length === 0 && !editing) {
    if (!canEdit) return null;
    return (
      <div className="border-b border-border/70 bg-card/30 px-5 py-2">
        <Button
          className="h-7 gap-1.5 text-xs"
          onClick={() => {
            setEditing(true);
            setExpanded(true);
          }}
          size="sm"
          variant="ghost"
        >
          <Network className="h-3.5 w-3.5" />
          Bind a Shivai world view
        </Button>
      </div>
    );
  }

  return (
    <section
      aria-label="Shivai world views"
      className="border-b border-border/70 bg-card/25"
      data-testid="channel-world-views"
    >
      <div className="flex min-h-10 items-center justify-between gap-3 px-5 py-1.5">
        <button
          className="flex min-w-0 items-center gap-2 text-left text-xs font-semibold text-foreground"
          onClick={() => setExpanded((current) => !current)}
          type="button"
        >
          <Network className="h-3.5 w-3.5 shrink-0 text-primary" />
          <span className="truncate">Shivai world views</span>
          <span className="rounded-full bg-muted px-1.5 py-0.5 text-3xs font-medium text-muted-foreground">
            {bindings.length}
          </span>
          {expanded ? (
            <ChevronUp className="h-3.5 w-3.5 text-muted-foreground" />
          ) : (
            <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
          )}
        </button>
        {canEdit ? (
          <Button
            className="h-7 gap-1 text-xs"
            onClick={() => {
              setEditing((current) => !current);
              setExpanded(true);
            }}
            size="sm"
            variant="ghost"
          >
            {editing ? (
              <X className="h-3.5 w-3.5" />
            ) : (
              <Plus className="h-3.5 w-3.5" />
            )}
            {editing ? "Cancel" : "Add view"}
          </Button>
        ) : null}
      </div>

      {expanded ? (
        <div className="space-y-3 px-4 pb-4">
          {editing ? (
            <form
              className="grid gap-2 rounded-xl border border-border/70 bg-background/70 p-3 md:grid-cols-2"
              onSubmit={(event) => void handleAddBinding(event)}
            >
              <label className="grid gap-1 text-2xs font-medium text-muted-foreground">
                Source
                <select
                  className="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
                  onChange={(event) => {
                    setSourceKind(
                      event.target
                        .value as WorldViewBinding["reference"]["kind"],
                    );
                    setSourceValue("");
                  }}
                  value={sourceKind}
                >
                  <option value="local-world-mirror-latest">
                    Published local world
                  </option>
                  <option value="hosted-world-view-export">
                    Hosted view export
                  </option>
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
                  onChange={(event) => setOrigin(event.target.value)}
                  placeholder="https://manifest.shivai.space"
                  required
                  type="url"
                  value={origin}
                />
              </label>
              <label
                className="grid gap-1 text-2xs font-medium text-muted-foreground"
                htmlFor={sourceValueInputId}
              >
                {sourceKind === "local-world-mirror-latest"
                  ? "Mirror ID"
                  : "Share token"}
                <Input
                  autoComplete="off"
                  id={sourceValueInputId}
                  onChange={(event) => setSourceValue(event.target.value)}
                  placeholder={
                    sourceKind === "local-world-mirror-latest"
                      ? "Published mirror ID"
                      : "Hosted export token"
                  }
                  required
                  value={sourceValue}
                />
              </label>
              {sourceKind === "local-world-mirror-latest" ? (
                <label
                  className="grid gap-1 text-2xs font-medium text-muted-foreground md:col-span-2"
                  htmlFor={localSourceRootInputId}
                >
                  Local source root
                  <Input
                    autoComplete="off"
                    id={localSourceRootInputId}
                    onChange={(event) => setLocalSourceRoot(event.target.value)}
                    placeholder="/path/to/project.world"
                    required
                    value={localSourceRoot}
                  />
                  <span className="font-normal text-muted-foreground/80">
                    Kept on this device. The selected package must already be
                    published as this mirror.
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
                    setRealmQualifiedName(event.target.value)
                  }
                  placeholder="world::main"
                  required
                  value={realmQualifiedName}
                />
              </label>
              <label
                className="grid gap-1 text-2xs font-medium text-muted-foreground"
                htmlFor={viewQualifiedNameInputId}
              >
                View qualified name
                <Input
                  id={viewQualifiedNameInputId}
                  onChange={(event) => setViewQualifiedName(event.target.value)}
                  placeholder="world::main::@Board"
                  required
                  value={viewQualifiedName}
                />
              </label>
              <label
                className="grid gap-1 text-2xs font-medium text-muted-foreground"
                htmlFor={labelInputId}
              >
                Label
                <Input
                  id={labelInputId}
                  onChange={(event) => setLabel(event.target.value)}
                  placeholder="Launch board"
                  value={label}
                />
              </label>
              <label className="grid gap-1 text-2xs font-medium text-muted-foreground">
                Initial display
                <select
                  className="h-9 rounded-md border border-input bg-background px-2 text-sm text-foreground"
                  onChange={(event) =>
                    setDisplayMode(
                      event.target.value as WorldViewBinding["displayMode"],
                    )
                  }
                  value={displayMode}
                >
                  <option value="graph">Graph</option>
                  <option value="tasks">Tasks</option>
                </select>
              </label>
              {authorityMutation.error instanceof Error ? (
                <p className="text-xs text-destructive md:col-span-2">
                  {authorityMutation.error.message}
                </p>
              ) : mutation.error instanceof Error ? (
                <p className="text-xs text-destructive md:col-span-2">
                  {mutation.error.message}
                </p>
              ) : null}
              <div className="flex justify-end md:col-span-2">
                <Button
                  disabled={mutation.isPending || authorityMutation.isPending}
                  size="sm"
                  type="submit"
                >
                  {mutation.isPending || authorityMutation.isPending
                    ? "Binding..."
                    : "Bind world view"}
                </Button>
              </div>
            </form>
          ) : null}

          {bindings.length > 0 ? (
            <div className="flex snap-x gap-3 overflow-x-auto pb-1">
              {bindings.map((binding) => (
                <div
                  className="relative min-w-[min(42rem,calc(100vw-4rem))] flex-1 snap-start"
                  key={binding.id}
                >
                  {canEdit ? (
                    <Button
                      aria-label={`Remove ${binding.label ?? "world view"}`}
                      className="absolute right-2 top-2 z-[80] h-7 w-7 rounded-full bg-background/80 p-0 shadow-sm backdrop-blur"
                      disabled={mutation.isPending}
                      onClick={() =>
                        void publishDocument({
                          version: 1,
                          bindings: bindings.filter(
                            (candidate) => candidate.id !== binding.id,
                          ),
                        })
                      }
                      size="icon"
                      variant="ghost"
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  ) : null}
                  <ChannelWorldViewTile binding={binding} />
                </div>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

function ChannelWorldViewTile({ binding }: { binding: WorldViewBinding }) {
  const { isDark } = useTheme();
  const resolvedQuery = useResolvedWorldViewQuery(binding);

  if (resolvedQuery.isLoading) {
    return (
      <div className="grid h-full place-items-center rounded-2xl border border-border/70 bg-background/70 text-xs text-muted-foreground">
        Resolving Shivai world view...
      </div>
    );
  }

  if (resolvedQuery.error instanceof Error) {
    return (
      <div className="grid h-full place-items-center rounded-2xl border border-destructive/30 bg-destructive/5 p-6 text-center">
        <div className="max-w-md space-y-3">
          <strong className="text-sm text-destructive">
            World view unavailable
          </strong>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {resolvedQuery.error.message}
          </p>
          <Button
            onClick={() => void resolvedQuery.refetch()}
            size="sm"
            variant="outline"
          >
            Retry
          </Button>
        </div>
      </div>
    );
  }

  if (!resolvedQuery.data) return null;
  return (
    <WorldViewTileSurface
      appearance={isDark ? "dark" : "light"}
      defaultDisplayMode={binding.displayMode}
      isRefreshing={resolvedQuery.isFetching}
      onRefresh={async () => {
        await resolvedQuery.refetch();
      }}
      presentation={resolvedQuery.data.presentation}
      subtitle={resolvedQuery.data.realm.qualifiedName}
      title={binding.label ?? resolvedQuery.data.view.name}
    />
  );
}
