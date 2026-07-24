import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

import type { ResolvedWorldView } from "../../src/shared/api/worldViewTypes";
import { installMockBridge } from "../helpers/bridge";

const SEEDED_BINDING_ID = "11111111-1111-4111-8111-111111111111";
const ADDED_BINDING_ID = "22222222-2222-4222-8222-222222222222";

function resolvedWorldView(
  bindingId: string,
  label: string,
): ResolvedWorldView {
  const presentationModel: ResolvedWorldView["presentation"]["dark"] = {
    graph: {
      kind: "ready",
      graphBackgroundHex: "#111113",
      graphPattern: "dots",
      clusters: [],
      nodes: [
        {
          id: `${bindingId}:bind`,
          label: "Bind channel mirror",
          preferenceQualifiedName: "world::main::BindChannelMirror",
          status: "ready",
          targetState: null,
          isReady: true,
          isLeaf: false,
          fillHex: "#1c2024",
          borderHex: "#3e63dd",
          textHex: "#f0f0f3",
          deemphasis: null,
          effect: null,
          position: { x: 150, y: 57.5 },
          size: { width: 300, height: 115 },
        },
        {
          id: `${bindingId}:render`,
          label: "Render shared view",
          preferenceQualifiedName: "world::main::RenderSharedView",
          status: "ready",
          targetState: null,
          isReady: true,
          isLeaf: false,
          fillHex: "#1c2024",
          borderHex: "#8e4ec6",
          textHex: "#f0f0f3",
          deemphasis: null,
          effect: null,
          position: { x: 150, y: 297.5 },
          size: { width: 300, height: 115 },
        },
        {
          id: `${bindingId}:refresh`,
          label: "Refresh after agent turn",
          preferenceQualifiedName: "world::main::RefreshAfterAgentTurn",
          status: "ready",
          targetState: null,
          isReady: true,
          isLeaf: false,
          fillHex: "#1c2024",
          borderHex: "#12a594",
          textHex: "#f0f0f3",
          deemphasis: null,
          effect: null,
          position: { x: 150, y: 537.5 },
          size: { width: 300, height: 115 },
        },
        {
          id: `${bindingId}:ship`,
          label,
          preferenceQualifiedName: "world::main::ShipBuzzIntegration",
          status: "ready",
          targetState: null,
          isReady: true,
          isLeaf: true,
          fillHex: "#1c2024",
          borderHex: "#e5484d",
          textHex: "#f0f0f3",
          deemphasis: null,
          effect: null,
          position: { x: 150, y: 777.5 },
          size: { width: 300, height: 115 },
        },
      ],
      edges: [
        {
          id: `${bindingId}:bind-render`,
          sourceId: "world::main::BindChannelMirror",
          targetId: "world::main::RenderSharedView",
          connectionType: "foundational",
          flowspaceQualifiedName: "%main::Plan",
          lineHex: "#6e6ade",
          deemphasis: null,
        },
        {
          id: `${bindingId}:render-refresh`,
          sourceId: "world::main::RenderSharedView",
          targetId: "world::main::RefreshAfterAgentTurn",
          connectionType: "foundational",
          flowspaceQualifiedName: "%main::Plan",
          lineHex: "#6e6ade",
          deemphasis: null,
        },
        {
          id: `${bindingId}:refresh-ship`,
          sourceId: "world::main::RefreshAfterAgentTurn",
          targetId: "world::main::ShipBuzzIntegration",
          connectionType: "foundational",
          flowspaceQualifiedName: "%main::Plan",
          lineHex: "#6e6ade",
          deemphasis: null,
        },
      ],
      bounds: { width: 420, height: 955 },
    },
    revision: "revision-world-view-1",
    selection: {
      realmQualifiedName: "world::main",
      viewQualifiedName: "world::main::@Board",
    },
  };

  return {
    bindingId,
    presentation: {
      formatVersion: 1,
      dark: presentationModel,
      light: {
        ...presentationModel,
        graph: {
          ...presentationModel.graph,
          graphBackgroundHex: "#f9f9fb",
          nodes: presentationModel.graph.nodes.map((node) => ({
            ...node,
            fillHex: "#ffffff",
            textHex: "#1c2024",
          })),
        },
      },
    },
    resolvedAt: "2026-07-24T12:00:00Z",
    revision: "revision-world-view-1",
    realm: { name: "main", qualifiedName: "world::main" },
    view: { name: "Board", qualifiedName: "world::main::@Board" },
  };
}

async function openGeneralChannel(page: Page) {
  await page.goto("/");
  await page.getByTestId("channel-general").click();
  await expect(page.getByTestId("chat-title")).toHaveText("general");
}

test.describe("Shivai world views", () => {
  test("renders a bound task view from the shared Shivai presentation", async ({
    page,
  }) => {
    const consoleErrors: string[] = [];
    page.on("console", (message) => {
      if (message.type() === "error") {
        consoleErrors.push(message.text());
      }
    });
    await installMockBridge(page, {
      worldViewBindings: {
        version: 1,
        bindings: [
          {
            id: SEEDED_BINDING_ID,
            label: "Launch board",
            reference: {
              kind: "hosted-world-view-export",
              origin: "https://manifest.shivai.space",
              shareToken: "view-token",
            },
            realmQualifiedName: "world::main",
            viewQualifiedName: "world::main::@Board",
            displayMode: "tasks",
          },
        ],
      },
      resolvedWorldViews: {
        [SEEDED_BINDING_ID]: resolvedWorldView(
          SEEDED_BINDING_ID,
          "Ship Buzz integration",
        ),
      },
    });

    await openGeneralChannel(page);

    const worldViews = page.getByTestId("channel-world-views");
    await expect(worldViews).toBeVisible();
    await expect(worldViews).toContainText("Shivai world views");
    await expect(worldViews).toContainText("Launch board");
    await expect(worldViews).toContainText("Ship Buzz integration");
    await expect(worldViews.getByLabel("World view summary")).toContainText(
      "4 ready",
    );
    await expect(
      worldViews.locator('[data-world-view-tile-surface="true"]'),
    ).toHaveCount(1);

    const graphButton = worldViews.getByRole("button", {
      name: "Graph",
      exact: true,
    });
    await graphButton.click();
    await expect(graphButton).toHaveAttribute("aria-pressed", "true");
    const tile = worldViews.locator('[data-world-view-tile-surface="true"]');
    await expect(tile).toHaveAttribute("data-display-mode", "graph");
    await expect(
      worldViews.getByTestId("workbench-graph-canvas"),
    ).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(() =>
          document.fonts.check('700 16px "Averia Serif Libre", serif'),
        ),
      )
      .toBe(true);
    expect(consoleErrors).toEqual([]);

    const commands = await page.evaluate(
      () => window.__BUZZ_E2E_COMMANDS__ ?? [],
    );
    expect(commands).toEqual(
      expect.arrayContaining(["get_world_view_bindings", "resolve_world_view"]),
    );
  });

  test("registers local authority before publishing a mirror binding", async ({
    page,
  }) => {
    await installMockBridge(page, {
      resolvedWorldViews: {
        [ADDED_BINDING_ID]: resolvedWorldView(
          ADDED_BINDING_ID,
          "Verify local mirror binding",
        ),
      },
    });
    await openGeneralChannel(page);
    await page.evaluate((bindingId) => {
      Object.defineProperty(window.crypto, "randomUUID", {
        configurable: true,
        value: () => bindingId,
      });
    }, ADDED_BINDING_ID);

    await page
      .getByRole("button", { name: "Bind a Shivai world view" })
      .click();
    await page.getByLabel("Mirror ID").fill("mirror-buzz-main");
    await page
      .getByLabel("Local source root")
      .fill("/workspace/buzz-integration.world");
    await page.getByLabel("Realm qualified name").fill("world::main");
    await page.getByLabel("View qualified name").fill("world::main::@Board");
    await page.getByLabel("Label").fill("Local launch board");
    await page.getByLabel("Initial display").selectOption("tasks");
    await page.getByRole("button", { name: "Bind world view" }).click();

    const worldViews = page.getByTestId("channel-world-views");
    await expect(worldViews).toContainText("Local launch board");
    await expect(worldViews).toContainText("Verify local mirror binding");

    const commandLog = await page.evaluate(
      () => window.__BUZZ_E2E_COMMAND_PAYLOADS__ ?? [],
    );
    const authorityCall = commandLog.find(
      (entry) => entry.command === "register_local_world_authority",
    );
    const publishCall = commandLog.find(
      (entry) => entry.command === "set_world_view_bindings",
    );
    expect(authorityCall?.payload).toEqual({
      origin: "https://manifest.shivai.space",
      mirrorId: "mirror-buzz-main",
      sourceRoot: "/workspace/buzz-integration.world",
    });
    expect(publishCall?.payload).toMatchObject({
      document: {
        version: 1,
        bindings: [
          {
            id: ADDED_BINDING_ID,
            label: "Local launch board",
            reference: {
              kind: "local-world-mirror-latest",
              origin: "https://manifest.shivai.space",
              mirrorId: "mirror-buzz-main",
            },
            realmQualifiedName: "world::main",
            viewQualifiedName: "world::main::@Board",
            displayMode: "tasks",
          },
        ],
      },
    });
    expect(
      commandLog.findIndex(
        (entry) => entry.command === "register_local_world_authority",
      ),
    ).toBeLessThan(
      commandLog.findIndex(
        (entry) => entry.command === "set_world_view_bindings",
      ),
    );
  });
});
