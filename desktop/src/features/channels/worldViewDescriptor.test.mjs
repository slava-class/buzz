import assert from "node:assert/strict";
import test from "node:test";

import { parseShareableWorldViewDescriptor } from "./worldViewDescriptor.ts";

test("parses a read-only hosted view export descriptor", () => {
  const result = parseShareableWorldViewDescriptor(`Shivai view reference
Source: hosted view export "public-view-token"
Realm: board::main
View qualified: @main::board`);

  assert.deepEqual(result, {
    ok: true,
    value: {
      reference: {
        kind: "hosted-world-view-export",
        origin: "https://manifest.shivai.space",
        shareToken: "public-view-token",
      },
      realmQualifiedName: "board::main",
      viewQualifiedName: "@main::board",
    },
  });
});

test("rejects local paths without echoing the path", () => {
  const privatePath = "/Users/alice/private/project.world";
  const result = parseShareableWorldViewDescriptor(`Shivai view reference
Source: local world "${privatePath}"
Realm: world::main
View qualified: world::main::@Board`);

  assert.equal(result.ok, false);
  if (!result.ok) {
    assert.match(result.error, /Local paths cannot be published/);
    assert.doesNotMatch(result.error, /alice|private|project\.world/);
  }
});

test("rejects edit-share capabilities without echoing the token", () => {
  const editToken = "edit-secret-token";
  const result = parseShareableWorldViewDescriptor(`Shivai view reference
Source: hosted edit share "${editToken}"
Realm: world::main
View qualified: world::main::@Board`);

  assert.equal(result.ok, false);
  if (!result.ok) {
    assert.match(result.error, /Edit-share capabilities cannot be published/);
    assert.doesNotMatch(result.error, /edit-secret-token/);
  }
});
