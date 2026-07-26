export type ParsedHostedWorldViewDescriptor = {
  reference: {
    kind: "hosted-world-view-export";
    origin: "https://manifest.shivai.space";
    shareToken: string;
  };
  realmQualifiedName: string;
  viewQualifiedName: string;
};

export type ParseWorldViewDescriptorResult =
  | { ok: true; value: ParsedHostedWorldViewDescriptor }
  | { ok: false; error: string };

const DESCRIPTOR_HEADER = "Shivai view reference";
const SOURCE_PREFIX = "Source: ";
const REALM_PREFIX = "Realm: ";
const VIEW_PREFIX = "View qualified: ";

/** Parse the copyable Shivai reference format at Buzz's public event boundary. */
export function parseShareableWorldViewDescriptor(
  descriptor: string,
): ParseWorldViewDescriptorResult {
  const lines = descriptor
    .replaceAll("\r\n", "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (lines.length !== 4 || lines[0] !== DESCRIPTOR_HEADER) {
    return {
      ok: false,
      error: "Paste one complete Shivai view reference.",
    };
  }

  const source = lines[1]?.startsWith(SOURCE_PREFIX)
    ? lines[1].slice(SOURCE_PREFIX.length)
    : null;
  if (!source) {
    return { ok: false, error: "The Shivai reference has no source." };
  }
  if (source.startsWith("local world ")) {
    return {
      ok: false,
      error: "Local paths cannot be published as channel or thread bindings.",
    };
  }
  if (source.startsWith("hosted edit share ")) {
    return {
      ok: false,
      error: "Edit-share capabilities cannot be published as bindings.",
    };
  }
  if (source.startsWith("hosted world ")) {
    return {
      ok: false,
      error: "Create a read-only hosted view export before binding this view.",
    };
  }

  const quotedShareToken = source.startsWith("hosted view export ")
    ? source.slice("hosted view export ".length)
    : null;
  if (!quotedShareToken) {
    return { ok: false, error: "This Shivai source is not shareable." };
  }
  let shareToken: unknown;
  try {
    shareToken = JSON.parse(quotedShareToken);
  } catch {
    return { ok: false, error: "The hosted export reference is malformed." };
  }
  if (typeof shareToken !== "string" || shareToken.trim().length === 0) {
    return { ok: false, error: "The hosted export reference is malformed." };
  }

  const realmQualifiedName = lines[2]?.startsWith(REALM_PREFIX)
    ? lines[2].slice(REALM_PREFIX.length).trim()
    : "";
  const viewReference = lines[3]?.startsWith(VIEW_PREFIX)
    ? lines[3].slice(VIEW_PREFIX.length).trim()
    : "";
  if (!realmQualifiedName || !viewReference) {
    return {
      ok: false,
      error: "The Shivai reference must name both a realm and a view.",
    };
  }

  const viewQualifiedName = viewReference;

  return {
    ok: true,
    value: {
      reference: {
        kind: "hosted-world-view-export",
        origin: "https://manifest.shivai.space",
        shareToken,
      },
      realmQualifiedName,
      viewQualifiedName,
    },
  };
}
