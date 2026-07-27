NIP-MP
======

Multi-Repository Projects
-------------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format), NIP-33 (parameterized replaceable events), NIP-34 (git repositories), NIP-09 (event deletion). Interacts with NIP-29 (the channel a project links to) and NIP-OA (owner attestation, for how agents inherit repo push access).

## Abstract

This NIP defines `kind:30621`, an addressable **project** event: a signed, named grouping of NIP-34 repository announcements (`kind:30617`). A project references its member repositories by coordinate, so one project may span repositories owned by different pubkeys, and one repository may belong to several projects.

A project is metadata only. Its signer gains no authority over any member repository — not to edit it, delete it, push to it, or administer it. Membership is an assertion about grouping, not a grant of permission.

## Motivation

Buzz renders one card per `kind:30617`, so "the platform" — a relay, a desktop app, and a mobile app — appears as three unrelated repositories. Real work spans repositories; the model does not.

[VISION_PROJECTS.md](../../VISION_PROJECTS.md) sets the bar as "standard kinds as substrate, custom kinds only where genuinely novel," and every other forge concept in Buzz clears it: repositories, patches, issues, statuses, and ref state are all standard NIP-34 kinds. Multi-repository grouping is the one semantic that cannot be:

- **Per-repository tags cannot express cross-owner grouping.** If membership lived in each `kind:30617`, a project spanning Alice's and Bob's repositories would require *both* Alice and Bob to publish a tag naming the group. Alice cannot enroll Bob's repository; she cannot sign for his key. Grouping would be possible only within a single owner's repositories, and would break the moment a repository changed hands or a fork joined.
- **Project-level metadata has no owner.** A project name, description, and linked channel describe the *group*, not any one repository. Scattered across per-repository tags they have no single writer, no replacement semantics, and no deletion story: removing a repository from the group means editing an event you may not control.
- **Existing list kinds do not fit.** NIP-51 sets (`kind:30004` curation sets and friends) are private-or-public user bookmarks over arbitrary content, not a shared, named, addressable container for a forge collection with its own channel binding and visibility. Overloading a curation set would make every project indistinguishable from a user's reading list.

One custom kind, held by one signer, with all group state in one replaceable event, resolves all three. The cost is bounded and stated plainly: `kind:30621` is Buzz-specific, so a third-party NIP-34 client sees the member repositories individually and ignores the grouping. Nothing degrades — the repositories remain standard, portable `kind:30617` events, discoverable and renderable exactly as before.

## Non-Goals

This NIP does not define shared or delegated project editing — a project is replaceable only by its own signer (see [Authority](#authority)).
This NIP does not define any authorization over member repositories. Membership is not a permission grant, and a project is never consulted by git push policy.
This NIP does not define project-level branch protection, CI, or workflow configuration.
This NIP does not define nested projects. A project's members are repositories, never other projects.
This NIP does not require relays to verify that a member coordinate resolves to an existing repository — a project may reference a repository that does not exist yet, or no longer does.

## Terminology

This document uses MUST, MUST NOT, SHOULD, SHOULD NOT, MAY, and RECOMMENDED as defined in RFC 2119.

- **project**: A `kind:30621` event. Also called the *container*.
- **member**: A repository referenced by a project, named by an `a` tag holding a repository coordinate.
- **coordinate**: The NIP-33 address of a repository announcement, `30617:<owner-pubkey-hex>:<repo-d-tag>`.
- **explicit project**: A project that exists as a `kind:30621` event.
- **implicit project**: The single-repository card a client renders for a `kind:30617` that no listing-eligible explicit project claims. Not an event — a rendering fallback.
- **listing eligible**: A project a client is currently rendering in its project collection. See [Listing eligibility](#listing-eligibility).

## Kinds

| Kind | Name | Signer | Class | Purpose |
|------|------|--------|-------|---------|
| `30621` | Project | user | parameterized replaceable | A named grouping of `kind:30617` repository announcements |

`kind:30621` is parameterized replaceable per NIP-33 (`30000 <= n < 40000`), addressed by `(pubkey, 30621, d)`. Two signers may use the same `d` value; those are two distinct projects.

### Kind allocation

`30621` sits in the NIP-34 git block (`30617` repository announcement, `30618` repository state), which is where a reader looks for a forge concept. Checks performed before freezing the number:

| Registry | Checked | Result |
|----------|---------|--------|
| Upstream nostr NIPs event-kind table (`nostr-protocol/nips` `README.md`, at commit `6d2979b3f503a8539c983efbcdcf901bbcf9ed23`) | `30610`–`30629` | Only `30617` and `30618` are assigned. `30621` is unassigned. |
| nostrbook.dev kind registry (`https://nostrbook.dev/kinds/<n>`) | `30617`, `30618`, `30620`, `30621`, `30622` | `30617` and `30618` documented (HTTP 200). `30620`, `30621`, `30622` all HTTP 404 — no entry. |
| This repository (`crates/buzz-core/src/kind.rs`) | full range | `30620` is `KIND_WORKFLOW_DEF`, `30622` is `KIND_DM_VISIBILITY` (NIP-DV). `30621` is the one free number between them. |

Both external registries are advisory, not authoritative allocators: neither reserves numbers, and an unregistered kind may still be in use by an unpublished client. A future upstream assignment of `30621` would be a collision Buzz absorbs the same way it already does for its other custom kinds — the number is Buzz-specific, and interoperability rests on the member `kind:30617` events, which remain standard.

## Event Format

```jsonc
{
  "kind": 30621,
  "pubkey": "<project-signer-pubkey-hex>",
  "content": "",
  "tags": [
    ["d", "platform"],
    ["name", "Platform"],
    ["description", "Relay, desktop, and mobile for the platform team."],
    ["a", "30617:<owner-a-pubkey-hex>:buzz"],
    ["a", "30617:<owner-b-pubkey-hex>:buzz-infra"],
    ["buzz-channel", "<channel-uuid>"],
    ["buzz-visibility", "listed"]
  ]
}
```

| Tag | Cardinality | Meaning |
|-----|-------------|---------|
| `d` | exactly 1, non-empty | Project slug. The NIP-33 identifier. |
| `name` | 0 or 1 | Human-readable display name. Clients fall back to `d` when absent. |
| `description` | 0 or 1 | Free text describing the project. |
| `a` | 0 to 64 | One member repository coordinate each. Order is not significant. |
| `buzz-channel` | 0 or 1 | UUID of the channel this project's discussion lives in. Metadata only — see [Authority](#authority). |
| `buzz-visibility` | 0 or 1 | `listed` (default) or `unlisted`. Feeds [listing eligibility](#listing-eligibility). |

`content` is empty and carries no meaning. Clients and relays MUST NOT parse semantics from it.

Unrecognized tags MUST be ignored rather than rejected, so a newer writer can add metadata without invalidating its events for older readers.

### Member coordinates

A member `a` tag value MUST be exactly `30617:<owner>:<repo-d>` where:

- the kind segment is the literal `30617`. A project groups repository *announcements*; a coordinate naming any other kind (notably `30618` repository state) is malformed.
- `<owner>` is 64 lowercase hex characters. Uppercase is rejected: `#a` filter matching is byte-exact, so an uppercase-owner head would be invisible to the lowercase-coordinate queries every reader issues.
- `<repo-d>` is non-empty and is the `d` tag of the member repository announcement, taken **verbatim**.

Parsing splits on the first two colons only; everything after the second colon is `<repo-d>`. A repository whose `d` tag contains a colon is therefore addressable. Splitting on every colon would make such a repository permanently unaddressable by any project.

Buzz-hosted repositories cannot currently produce such a coordinate: their `d` values are validated as `[a-zA-Z0-9._-]{1,64}` (`crates/buzz-relay/src/handlers/side_effects.rs`, `crates/buzz-sdk/src/builders.rs`). The tolerance is for the repositories this NIP does not control — NIP-34 announcements from other clients, and any future relaxation of Buzz's own rule — and it matches how Buzz already parses coordinates in NIP-09 deletion handling, so a project coordinate and a deletion coordinate can never disagree about where a repository's `d` value begins.

Coordinate identity is the whole string. Two members sharing a `<repo-d>` under different owners — the NIP-34 fork case — are distinct members, not duplicates.

A project MAY reference a coordinate that resolves to nothing: a repository not yet announced, deleted, or announced on another relay. Clients render those members as explicitly unavailable ([Client Behavior](#client-behavior), step 6).

## Semantics

### Authority

The project signer's authority begins and ends at the container.

- **Over the container**: total. Only the signer can replace or delete their `(pubkey, 30621, d)` coordinate.
- **Over member repositories**: none. No edit, no delete, no push, no administration, no ability to change a member repository's own metadata or protections. Adding Bob's repository to Alice's project changes nothing about Bob's repository or who may push to it. It is Alice's signed assertion that the two belong together, and it is attributable to her key.

Clients MUST preserve each member repository's own owner provenance in the UI. A repository rendered inside a project must not appear to be owned or governed by the project signer.

`buzz-channel` on a project is **metadata only**. Git push policy reads the `buzz-channel` of the repository's own `kind:30617` (`crates/buzz-relay/src/api/git/policy.rs`); a project neither overrides that binding nor supplies one to a member that lacks it. A project's channel binding therefore cannot widen or narrow push access to anything.

### Editing model

Editing is **owner-only**: publish a replacement `kind:30621` with the same `d` and a newer `created_at`. Adding, removing, or reordering members and changing metadata are all one operation — replacing the container. This falls out of the addressable-event model with no relay-side permission machinery; NIP-33 replacement already refuses to let one pubkey overwrite another's coordinate.

Delegated or maintainer editing is deliberately out of scope for this version. Adding it later needs no change to this event shape — only a new rule about who may replace a coordinate.

### Zero-member projects

A project with no `a` tags is valid. It is the natural state after removing a final member, and it carries only bounded metadata either way. Deleting the container — with its name, description, and channel binding — because its last repository was removed would be a destructive surprise for a reversible action.

Clients SHOULD require at least one member when *creating* a project, since an empty new project is almost always a mistake, and MUST render an existing empty project as an empty container rather than hiding it or treating it as malformed.

### Multiple membership

A repository may be a member of any number of projects. It renders inside each ([Client Behavior](#client-behavior), step 4). Membership is not exclusive and not a move: nothing about the repository event changes when it joins or leaves a project.

### Deletion

Deleting a project (NIP-09 `kind:5` naming the project coordinate) deletes the `kind:30621` only. Member repositories are untouched — their `kind:30617` events, refs, channels, and protections all survive, and each falls back to an implicit card unless another listing-eligible project claims it.

There is no cascade, in either direction. Deleting a member repository does not modify the project; the project keeps a coordinate that no longer resolves, and clients render it as unavailable.

## Relay Processing Algorithm

A relay accepting `kind:30621` MUST validate the envelope at ingest. The rule names below are the identifiers the shared fixtures use.

1. **`d-cardinality`** — exactly one `d` tag. Zero or several is rejected. Under NIP-33 a missing `d` is treated as empty, which collapses every such event into the `(pubkey, 30621, "")` slot where unrelated projects silently overwrite each other; several `d` tags make the address reader-dependent.
2. **`d-empty`** — the `d` value is non-empty. Same collapse hazard. Its length is bounded by the relay's existing generic `d`-tag limit (`buzz_db::event::D_TAG_MAX_LEN`, 1024 bytes); this NIP adds no second bound.
3. **`member-cap`** — at most 64 member `a` tags, counting **every** `a` tag rather than distinct coordinates. Counting distinct coordinates would leave parse volume bounded only by the relay frame limit (512 KiB by default, `crates/buzz-relay/src/config.rs`), since a duplicate-heavy event could carry thousands of tags naming one coordinate. The cap is inclusive: 64 is accepted, 65 is not.
4. **`member-coordinate-malformed`** — every member `a` tag parses per [Member coordinates](#member-coordinates).
5. **`member-duplicate`** — no two member `a` tags hold the same coordinate, compared as exact strings on the canonical form.
6. **`metadata-cardinality`** — at most one each of `name`, `description`, `buzz-channel`, `buzz-visibility`. Duplicates would make the effective value reader-dependent.
7. **`metadata-length`** — `name` at most 256 bytes; `description` at most 2048 bytes.

Rules 3 through 5 are evaluated in that order, so an oversized tag list is refused on count before any set proportional to it is built.

**Duplicates are rejected, never normalized.** A relay cannot dedupe tags inside a signed event: rewriting the tag array changes the event id and invalidates the signature. The choices are reject, or accept and require every present and future consumer to apply a first-wins interpretation rule. Rejecting keeps every stored head canonical and spares all consumers a defensive parse.

**No membership authorization.** The relay MUST NOT check whether the signer owns, maintains, or has any relationship to a member repository. Referencing another owner's repository is legal and is the point of the kind. Because membership grants nothing ([Authority](#authority)), there is nothing to authorize.

**Routing.** `kind:30621` is global-only, like every other NIP-34 kind in Buzz: it is addressed by `(pubkey, kind, d)` and is never channel-scoped. A stray `h` tag MUST NOT scope it to a channel — the `buzz-channel` tag is a metadata reference, not a routing directive.

**Scope.** Writes require the `repos:write` scope, matching `kind:30617` and `kind:30618`. A project is repository metadata; a client authorized to announce repositories is authorized to group them.

**Replacement and deletion** follow NIP-33 and NIP-09 with no special cases: newest `created_at` wins per `(pubkey, 30621, d)`, and a `kind:5` from the same pubkey naming the coordinate deletes it.

## Client Behavior

### Listing eligibility

A project is **listing eligible** for a client when that client is currently rendering it in its project collection. A project is not listing eligible when:

- its `buzz-visibility` is `unlisted`, or
- the viewer has hidden it locally, or
- it has been deleted, or its latest head is otherwise not being rendered.

Only listing-eligible projects claim members. This keeps visibility deterministic in the case that otherwise breaks: an unlisted project must not make a repository the viewer can plainly see disappear from the collection, because the container that claims it is not on screen to hold it.

### Claim authority

A project **claims** a member — suppressing that repository's implicit card, per step 3 of the fold — only when the project is listing eligible *and* its signer is authorized by the member repository itself: the signer is the repository's owner (the pubkey in the member coordinate), or is listed in a `maintainers` tag on the repository's own live `kind:30617`.

Authority is therefore read from the member repository's *content*, not merely its existence: a client that has resolved only a coordinate, and not the head it names, cannot yet decide whether a project claims it. `maintainers` is the standard NIP-34 multi-value tag; Buzz's own announcement builder does not emit it today, so in practice every current claim reduces to signer-is-owner, and the `maintainers` clause is what keeps a co-maintained repository working the day that changes.

Without this rule, membership would carry exactly the authority [Authority](#authority) says it does not. Anyone may publish a project naming anyone's repository, so an unauthorized project that suppressed implicit cards would let a stranger pull someone else's repository out of the collection and into a container the owner never consented to — a signed assertion silently becoming control over another owner's discovery surface.

An unauthorized project still renders, and still renders its members inside itself: cross-owner grouping works, which is the entire point of the kind. What it cannot do is *remove* a repository from where its owner expects to find it. The visible consequence is that a repository in a stranger's project renders in both places — inside that project and as its own card — which is the correct reading of an unendorsed grouping claim.

### The fold

Given the set of repositories and projects to render, a client MUST derive the collection as follows.

1. **Enumerate exhaustively.** Retrieve the latest live head of every `kind:30621` and `kind:30617` coordinate, plus the `kind:5` deletions bearing on them, using paginated queries that run to exhaustion. A fixed `limit` MUST NOT be used: with a limit of 200, repository 201 vanishes from the collection, which is precisely the compatibility guarantee this NIP owes existing repositories. Pagination MUST tolerate several events sharing one `created_at` — a timestamp-only cursor silently skips same-second events, so the cursor MUST break ties on event id.
2. **Resolve members.** For each project, resolve each member coordinate to its repository head, and determine whether the project [claims](#claim-authority) each one.
3. **Suppress claimed implicit cards.** A live repository claimed by at least one project does not also render as an implicit single-repository card.
4. **Render multiple membership.** A repository belonging to several listing-eligible projects renders inside each of them, claimed or not.
5. **Fall back.** A repository claimed by no project renders as an implicit single-repository card — including when an unauthorized project also renders it as a member.
6. **Mark unresolvable members.** A member coordinate that resolves to nothing — never announced, deleted, or not present on this relay — renders inside its project as explicitly unavailable. It MUST NOT become a phantom standalone card, and it MUST NOT be silently dropped: silence makes a project look smaller than its author declared.
7. **Hiding a container never hides repositories.** Locally hiding a project makes it not listing eligible, so it claims nothing and by step 5 its members return as implicit cards. Hiding a grouping is a statement about the grouping. A repository disappears from the collection only when the viewer hides that repository or it is deleted — and a repository the viewer has hidden is hidden everywhere, including inside every project that lists it, so hiding one cannot be undone by someone else's grouping.

The fold is deterministic: same heads in, same collection out, independent of arrival order or query shape. Every live, unhidden repository renders in at least one place — inside a project that claims it, or as its own card — and no repository renders twice within one container.

### Required fold cases

The fold cannot be expressed as accept/reject of a single event, so it is not in the [conformance fixtures](#conformance-fixtures) — those are the ingest contract. A client implementing the fold MUST cover at least these cases, each of which is a distinct branch above:

| Case | Expected collection |
|------|---------------------|
| Owner's own project lists their repository | Repository renders inside the project only |
| Stranger's project lists someone else's repository | Repository renders inside that project *and* as its own card |
| Project signer is in the member repository's `maintainers` tag | Repository renders inside the project only |
| Repository is a member of two projects that both claim it | Repository renders inside both; no implicit card |
| Repository removed from every project | Repository renders as an implicit card |
| Project is `unlisted`, or locally hidden | Project absent from the collection; its members render as implicit cards |
| Viewer has hidden a member repository | Repository absent from the collection *and* from inside every project listing it |
| Member coordinate resolves to nothing | Member renders inside its project as unavailable; no standalone card |
| Project head deleted | Project absent; its members render as implicit cards |
| More repositories and projects than one page holds, with several sharing one `created_at` | Every repository and project renders |

### Collection growth

Step 1's exhaustive enumeration is a correctness floor, not a scaling strategy: it says a client MUST NOT silently truncate its collection, because a repository absent from the list is indistinguishable from one that does not exist. It is not a mandate to hold the relay's entire repository set in memory on every load.

At Buzz's current scale (hundreds of repositories per community) exhaustive enumeration is the whole story. Past that, the way out is a narrower question — a server-side collection query, a scoped or searched subset, or resolving a project's members on demand — not a fixed client-side `limit`. Any such surface MUST report its own truncation so a client can say "showing N of M" rather than quietly presenting a partial collection as complete.

### Route resolution

A project route resolves to a container; a repository route resolves to a repository. Every repository-scoped operation — clone, fetch, issues, pull requests, activity, mutation, deletion — MUST take an explicit repository coordinate. None may infer its target from container state, or a two-repository project will silently operate on the wrong member.

Legacy `<owner>:<dtag>` repository routes remain valid and resolve to that repository, presented as a single-repository container.

## Conformance Fixtures

[`NIP-MP.fixtures.json`](NIP-MP.fixtures.json) holds the shared valid/invalid case set: 9 accepted and 16 rejected events covering minimal and full projects, zero members, the 64-member boundary from both sides, cross-owner and same-`d`-different-owner members, colon-bearing repository `d` values, and each rejection rule above.

The relay validator, the Rust builder, and the TypeScript builder all test against this one file, so a divergence between them is a test failure rather than a production surprise.

Each case carries an **unsigned** template — `kind`, `content`, `tags`. Consumers sign it with their own test key. Signed literals would be inert: the id and signature are fixed by the exact serialization, so any consumer that re-serializes would need to recompute both anyway. Rejection cases name their `reject_rules`, so an implementation cannot pass by rejecting a bad event for an unrelated reason.

## Security Considerations

**Unauthorized grouping claims are the accepted trade.** Anyone may publish a project referencing anyone's repositories. That claim is a signed statement attributable to its author and grants nothing ([Authority](#authority)) — the same trust model as NIP-51 lists, which likewise reference content their author does not own. A client MUST NOT present membership in a stranger's project as endorsement by, or authority over, the member repository's owner, and MUST show the project signer alongside a project it did not author.

**Resolution fan-out is bounded.** Each project resolves at most 64 coordinates, and the cap counts raw tags, so no single event can force unbounded resolution work regardless of how its tag list is shaped.

**Push policy is untouched.** A project cannot grant, widen, or narrow push access to any repository. Push policy reads only the repository's own `kind:30617`. This is a design invariant, not an implementation detail: if a project ever became an input to push authorization, publishing a project naming someone else's repository would become a privilege-escalation primitive.

## Relation to Other NIPs

- **NIP-34**: Supplies the member repositories. Members are `kind:30617` announcements referenced by coordinate; a NIP-34 client that does not know `kind:30621` still discovers and renders each repository normally.
- **NIP-33**: Supplies addressing, replacement, and the owner-only editing model. Owner-only editing is not enforcement code in Buzz — it is what NIP-33 replacement already means.
- **NIP-09**: Supplies container deletion, which deletes the container only.
- **NIP-29**: Supplies the channel a project's `buzz-channel` names. The reference is metadata; project state is never channel-scoped.
- **NIP-51**: The closest existing precedent — a signed, addressable list referencing content the author need not own. Not reused because a project is a shared named forge container with its own channel binding and visibility, not a user's private-or-public bookmark set.
- **NIP-OA**: Unaffected. Agents inherit repository push access from their owner through the repository's own protections; a project is never consulted.
