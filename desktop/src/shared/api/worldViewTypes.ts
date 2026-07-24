import type { WorldViewTileSurfaceProps } from "@shivai.space/world-view-react";

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
  requiresAgentRestart: boolean;
};

export type WorldViewBinding = {
  id: string;
  label?: string;
  reference: WorldViewReference;
  realmQualifiedName: string;
  viewQualifiedName: string;
  displayMode: "graph" | "tasks";
};

export type WorldViewBindingsDocument = {
  version: 1;
  bindings: WorldViewBinding[];
};

export type WorldViewBindingsResponse = {
  document: WorldViewBindingsDocument;
  eventId: string | null;
  updatedAt: number | null;
  author: string | null;
};

export type SetWorldViewBindingsInput = {
  channelId: string;
  document: WorldViewBindingsDocument;
};

export type SetWorldViewBindingsResult = {
  ok: boolean;
  eventId: string;
};

export type ResolvedWorldView = {
  bindingId: string;
  presentation: WorldViewTileSurfaceProps["presentation"];
  resolvedAt: string;
  revision: string;
  realm: {
    name: string;
    qualifiedName: string;
  };
  view: {
    name: string;
    qualifiedName: string;
  };
};
