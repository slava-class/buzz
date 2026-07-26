import type { WorldViewTileSurfaceProps } from "@shivai.space/world-view-react";
import { z } from "zod";

export type WorldViewReference =
  | {
      kind: "local-world-mirror-latest";
      origin: string;
      mirrorId: string;
    }
  | {
      kind: "hosted-world-view-export";
      origin: string;
      shareToken: string;
    };

export type RegisterLocalWorldAuthorityInput = {
  origin: string;
  mirrorId: string;
  sourceRoot: string;
};

export type RegisterLocalWorldAuthorityResult = {
  authority: {
    origin: string;
    mirrorId: string;
    sourceRoot: string;
  };
};

export type WorldViewBindingScope =
  | { kind: "channel" }
  | { kind: "thread"; threadRootEventId: string };

export type WorldViewBinding = {
  id: string;
  label?: string;
  reference: WorldViewReference;
  realmQualifiedName: string;
  viewQualifiedName: string;
  displayMode: "graph" | "tasks";
};

export type WorldViewBindingsDocument = {
  version: 2;
  scope: WorldViewBindingScope;
  bindings: WorldViewBinding[];
};

export type WorldViewBindingsResponse = {
  document: WorldViewBindingsDocument;
  revisionEventId: string | null;
  updatedAt: number | null;
  author: string | null;
  nextReadCommand: string;
};

export type EffectiveWorldViewBinding = {
  binding: WorldViewBinding;
  declaredScope: WorldViewBindingScope;
  bindingRevisionEventId: string;
};

export type EffectiveWorldViewBindings = {
  effectiveScope: WorldViewBindingScope;
  bindings: EffectiveWorldViewBinding[];
  channelRevisionEventId: string | null;
  threadRevisionEventId: string | null;
  nextReadCommands: string[];
};

export type SetWorldViewBindingsInput = {
  channelId: string;
  expectedRevisionEventId: string | null;
  document: WorldViewBindingsDocument;
};

export type SetWorldViewBindingsResult = {
  ok: boolean;
  revisionEventId: string;
  nextReadCommand: string;
};

export type WorldViewResolutionRequest = {
  channelId: string;
  binding: WorldViewBinding;
  declaredScope: WorldViewBindingScope;
  effectiveScope: WorldViewBindingScope;
  bindingRevisionEventId: string;
};

const worldViewBindingScopeSchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("channel") }),
  z.strictObject({
    kind: z.literal("thread"),
    threadRootEventId: z.string().regex(/^[0-9a-f]{64}$/),
  }),
]);

const worldViewReferenceSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("local-world-mirror-latest"),
    origin: z.url(),
    mirrorId: z.string().min(1),
  }),
  z.strictObject({
    kind: z.literal("hosted-world-view-export"),
    origin: z.url(),
    shareToken: z.string().min(1),
  }),
]);

const worldViewBindingSchema = z.strictObject({
  id: z.uuid(),
  label: z.string().min(1).optional(),
  reference: worldViewReferenceSchema,
  realmQualifiedName: z.string().min(1),
  viewQualifiedName: z.string().min(1),
  displayMode: z.enum(["graph", "tasks"]),
});

const revisionEventIdSchema = z.string().regex(/^[0-9a-f]{64}$/);

const effectiveWorldViewBindingsSchema = z.strictObject({
  effectiveScope: worldViewBindingScopeSchema,
  bindings: z.array(
    z.strictObject({
      binding: worldViewBindingSchema,
      declaredScope: worldViewBindingScopeSchema,
      bindingRevisionEventId: revisionEventIdSchema,
    }),
  ),
  channelRevisionEventId: revisionEventIdSchema.nullable(),
  threadRevisionEventId: revisionEventIdSchema.nullable(),
  nextReadCommands: z.array(z.string().min(1)),
});

const worldViewEntitySchema = z.strictObject({
  name: z.string(),
  qualifiedName: z.string(),
});

const worldViewGraphNodeSchema = z.looseObject({
  id: z.string(),
  clusterId: z.string().optional(),
  label: z.string(),
  preferenceQualifiedName: z.string(),
  status: z.enum(["default", "leaf", "ready", "done", "goal", "focus"]),
  targetState: z.enum(["actionable", "implementing", "satisfied"]).nullable(),
  isReady: z.boolean().optional(),
  isLeaf: z.boolean(),
  signalCaseNames: z.array(z.string()).optional(),
  fillHex: z.string(),
  borderHex: z.string(),
  textHex: z.string(),
  deemphasis: z.enum(["fade", "ghost", "hide"]).nullable(),
  effect: z
    .enum(["blur", "dreamy", "focused", "glow", "prismatic", "clear"])
    .nullable(),
  position: z.strictObject({ x: z.number(), y: z.number() }),
  size: z.strictObject({ width: z.number(), height: z.number() }),
});

const worldViewGraphEdgeSchema = z.looseObject({
  deemphasis: z.enum(["fade", "ghost", "hide"]).nullable(),
  id: z.string(),
  lineHex: z.string(),
  flowspaceQualifiedName: z.string().optional(),
  policyQualifiedNames: z.array(z.string()).optional(),
  sourceId: z.string(),
  targetId: z.string(),
  connectionType: z.enum(["foundational", "alternative"]),
});

const worldViewGraphModelSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("ready"),
    graphBackgroundHex: z.string(),
    graphPattern: z.enum(["none", "grid", "dots"]),
    clusters: z.array(
      z.strictObject({
        backgroundRgba: z.string(),
        badgeBackgroundRgba: z.string(),
        badgeTextHex: z.string(),
        id: z.string(),
        label: z.string(),
        parentId: z.string().optional(),
      }),
    ),
    nodes: z.array(worldViewGraphNodeSchema),
    edges: z.array(worldViewGraphEdgeSchema),
    bounds: z.strictObject({ width: z.number(), height: z.number() }),
  }),
  z.strictObject({
    kind: z.literal("unavailable"),
    reason: z.enum(["missing-realm", "missing-scope", "missing-view"]),
  }),
  z.strictObject({
    kind: z.literal("empty"),
    reason: z.enum(["no-preferences", "no-visible-preferences"]),
    graphBackgroundHex: z.string().optional(),
    graphPattern: z.enum(["none", "grid", "dots"]).optional(),
  }),
]);

const worldViewPresentationModelSchema = z.strictObject({
  graph: worldViewGraphModelSchema,
  revision: z.string().nullable(),
  selection: z.strictObject({
    realmQualifiedName: z.string(),
    scopePreferenceQualifiedName: z.string().optional(),
    viewQualifiedName: z.string(),
  }),
});

const worldViewPresentationSchema = z.strictObject({
  formatVersion: z.literal(1),
  dark: worldViewPresentationModelSchema,
  light: worldViewPresentationModelSchema,
});

const worldViewDumpNodeSchema = z.strictObject({
  preference: z.string(),
  qualifiedName: z.string(),
  status: z.enum(["satisfied", "ready", "blocked"]),
  actionable: z.boolean(),
  leaf: z.boolean(),
  inFocus: z.boolean(),
  inSatisfied: z.boolean(),
  blockers: z.array(z.string()),
  enablers: z.array(z.string()),
  note: z.strictObject({
    preview: z.string().nullable(),
    truncated: z.boolean(),
  }),
  signals: z.array(
    z.object({
      name: z.string(),
      target: z.literal("preference"),
      mode: z.enum(["first", "all"]),
      cases: z.array(
        z.object({
          name: z.string(),
          evidence: z.array(z.looseObject({ kind: z.string() })),
        }),
      ),
    }),
  ),
});

const resolvedWorldViewSchema = z.strictObject({
  formatVersion: z.literal(1),
  bindingId: z.uuid(),
  channelId: z.uuid(),
  declaredScope: worldViewBindingScopeSchema,
  effectiveScope: worldViewBindingScopeSchema,
  bindingRevisionEventId: z.string().regex(/^[0-9a-f]{64}$/),
  sourceRevision: z.string().min(1),
  freshness: z.enum(["pinned", "latest-at-resolution"]),
  authority: z.discriminatedUnion("kind", [
    z.strictObject({
      kind: z.literal("hosted-world-view-export"),
      origin: z.url(),
    }),
    z.strictObject({
      kind: z.literal("local-world-mirror-latest"),
      origin: z.url(),
      mirrorId: z.string().min(1),
    }),
  ]),
  realm: worldViewEntitySchema,
  view: worldViewEntitySchema,
  viewDump: z.strictObject({
    counts: z.strictObject({
      nodes: z.number().int().nonnegative(),
      edges: z.number().int().nonnegative(),
      ready: z.number().int().nonnegative(),
      actionableReady: z.number().int().nonnegative(),
      satisfied: z.number().int().nonnegative(),
      blocked: z.number().int().nonnegative(),
    }),
    nodes: z.array(worldViewDumpNodeSchema),
    readyLeaves: z.array(worldViewDumpNodeSchema),
    satisfiedNodes: z.array(worldViewDumpNodeSchema),
    blockedNodes: z.array(worldViewDumpNodeSchema),
    edges: z.array(
      z.strictObject({
        downstream: z.string(),
        upstream: z.string(),
        relation: z.enum(["blocker", "enabler"]),
        connectionType: z.enum(["foundational", "alternative"]),
        flowspace: z.string(),
        flowspaceQualifiedName: z.string(),
      }),
    ),
  }),
  presentation: worldViewPresentationSchema,
  resolvedAt: z.iso.datetime({ offset: true }),
  nextCommand: z.string().min(1),
});

export type ResolvedWorldView = z.infer<typeof resolvedWorldViewSchema>;

type PresentationContractCheck =
  ResolvedWorldView["presentation"] extends WorldViewTileSurfaceProps["presentation"]
    ? true
    : never;
const presentationContractCheck: PresentationContractCheck = true;
void presentationContractCheck;

export function decodeResolvedWorldView(value: unknown): ResolvedWorldView {
  return resolvedWorldViewSchema.parse(value);
}

export function decodeEffectiveWorldViewBindings(
  value: unknown,
): EffectiveWorldViewBindings {
  return effectiveWorldViewBindingsSchema.parse(value);
}
