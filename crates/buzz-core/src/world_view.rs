//! Channel-scoped Shivai world view binding contracts.

use std::collections::{HashMap, HashSet};

use nostr::{Event, EventId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Current serialized scoped world-view binding document version.
pub const WORLD_VIEW_BINDINGS_VERSION: u8 = 2;
/// Maximum number of world views that one channel or thread scope may bind.
pub const MAX_WORLD_VIEW_BINDINGS_PER_SCOPE: usize = 8;
/// Canonical parameterized-replaceable coordinate for channel bindings.
pub const CHANNEL_WORLD_VIEW_BINDINGS_D_TAG: &str = "world-view-bindings:channel";
/// Current private local-world authority registry version.
pub const LOCAL_WORLD_AUTHORITY_REGISTRY_VERSION: u8 = 1;
/// Registry file shared by the desktop host and locally running ACP agents.
pub const LOCAL_WORLD_AUTHORITY_REGISTRY_FILE_NAME: &str = "world-authorities.json";

/// Private machine-local mappings from public mirror identities to mutable sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalWorldAuthorityRegistry {
    /// Contract version for explicit forward evolution.
    pub version: u8,
    /// One authoritative source per hosted mirror identity.
    pub authorities: Vec<LocalWorldAuthority>,
}

/// One private authority mapping. This shape must never be published to Nostr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalWorldAuthority {
    /// Hosted Shivai origin that owns the mirror identity.
    pub origin: String,
    /// Stable public mirror identity.
    pub mirror_id: String,
    /// Canonical absolute path of the mutable local `.world` package.
    pub source_root: String,
}

impl Default for LocalWorldAuthorityRegistry {
    fn default() -> Self {
        Self {
            version: LOCAL_WORLD_AUTHORITY_REGISTRY_VERSION,
            authorities: Vec::new(),
        }
    }
}

impl LocalWorldAuthorityRegistry {
    /// Validate registry identity, path, and one-to-one mapping invariants.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != LOCAL_WORLD_AUTHORITY_REGISTRY_VERSION {
            return Err(format!(
                "unsupported local world authority registry version: {}",
                self.version
            ));
        }
        let mut references = HashSet::with_capacity(self.authorities.len());
        let mut roots = HashSet::with_capacity(self.authorities.len());
        for authority in &self.authorities {
            validate_hosted_origin(&authority.origin)?;
            validate_required_text("mirrorId", &authority.mirror_id, 1024)?;
            validate_required_text("sourceRoot", &authority.source_root, 4096)?;
            if !std::path::Path::new(&authority.source_root).is_absolute() {
                return Err("sourceRoot must be an absolute path".into());
            }
            if !references.insert((&authority.origin, &authority.mirror_id)) {
                return Err(format!(
                    "duplicate local world authority: {} {}",
                    authority.origin, authority.mirror_id
                ));
            }
            if !roots.insert(&authority.source_root) {
                return Err(format!(
                    "duplicate local world source root: {}",
                    authority.source_root
                ));
            }
        }
        Ok(())
    }

    /// Resolve mutable local authority for one public mirror reference.
    pub fn resolve(&self, origin: &str, mirror_id: &str) -> Option<&LocalWorldAuthority> {
        self.authorities
            .iter()
            .find(|authority| authority.origin == origin && authority.mirror_id == mirror_id)
    }

    /// Replace mappings that share either identity or local source, then validate.
    pub fn upsert(&mut self, authority: LocalWorldAuthority) -> Result<(), String> {
        self.authorities.retain(|candidate| {
            (candidate.origin != authority.origin || candidate.mirror_id != authority.mirror_id)
                && candidate.source_root != authority.source_root
        });
        self.authorities.push(authority);
        self.authorities.sort_by(|left, right| {
            (&left.origin, &left.mirror_id).cmp(&(&right.origin, &right.mirror_id))
        });
        self.validate()
    }
}

/// Exact channel or thread-root scope owned by one binding document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorldViewBindingScope {
    /// Bindings declared directly on the channel.
    Channel,
    /// Bindings declared on one canonical thread root.
    Thread {
        /// Lowercase Nostr event id of the canonical thread root.
        #[serde(rename = "threadRootEventId")]
        thread_root_event_id: String,
    },
}

impl WorldViewBindingScope {
    /// Construct a validated thread scope.
    pub fn thread(thread_root_event_id: impl Into<String>) -> Result<Self, String> {
        let scope = Self::Thread {
            thread_root_event_id: thread_root_event_id.into(),
        };
        scope.validate()?;
        Ok(scope)
    }

    /// Stable `d` tag used by relay replacement and exact-scope reads.
    pub fn d_tag(&self) -> String {
        match self {
            Self::Channel => CHANNEL_WORLD_VIEW_BINDINGS_D_TAG.into(),
            Self::Thread {
                thread_root_event_id,
            } => format!("world-view-bindings:thread:{thread_root_event_id}"),
        }
    }

    /// Canonical thread root when this is a thread scope.
    pub fn thread_root_event_id(&self) -> Option<&str> {
        match self {
            Self::Channel => None,
            Self::Thread {
                thread_root_event_id,
            } => Some(thread_root_event_id),
        }
    }

    /// Validate the serialized scope identity.
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Thread {
            thread_root_event_id,
        } = self
        {
            validate_nostr_event_id("scope.threadRootEventId", thread_root_event_id)?;
        }
        Ok(())
    }
}

impl Default for WorldViewBindingScope {
    fn default() -> Self {
        Self::Channel
    }
}

/// One exact-scope document containing every bound Shivai world view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldViewBindingsDocument {
    /// Contract version for explicit forward evolution.
    pub version: u8,
    /// Exact channel or thread-root scope represented by this document.
    pub scope: WorldViewBindingScope,
    /// Ordered views rendered by clients and supplied to agents.
    pub bindings: Vec<WorldViewBinding>,
}

/// Exact-scope binding state plus the relay revision needed for the next write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldViewBindingsSnapshot {
    /// Current document, or an empty document for an absent coordinate.
    pub document: WorldViewBindingsDocument,
    /// Current event id; `None` means the next write must explicitly expect creation.
    pub revision_event_id: Option<String>,
    /// Relay event timestamp for the current revision.
    pub updated_at: Option<u64>,
    /// Public key that authored the current revision.
    pub author: Option<String>,
}

impl WorldViewBindingsSnapshot {
    /// Construct an absent exact-scope snapshot.
    pub fn empty(scope: WorldViewBindingScope) -> Self {
        Self {
            document: WorldViewBindingsDocument {
                version: WORLD_VIEW_BINDINGS_VERSION,
                scope,
                bindings: Vec::new(),
            },
            revision_event_id: None,
            updated_at: None,
            author: None,
        }
    }
}

/// Structurally decoded state from one already signature-verified bindings event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedWorldViewBindingsEvent {
    /// Exact channel coordinate carried by the event's sole `h` tag.
    pub channel_id: Uuid,
    /// Exact-scope state represented by this event.
    pub snapshot: WorldViewBindingsSnapshot,
    /// Revision that this event expected to replace; `None` means creation.
    pub previous_revision_event_id: Option<EventId>,
}

/// Decode the canonical signed-event envelope after signature verification.
///
/// This validates kind, channel/scope coordinates, optimistic revision, thread
/// root tags, and strict JSON content in one source-shaped boundary. Callers at
/// untrusted relay boundaries must verify the event signature before invoking
/// this CPU-cheap structural decoder.
pub fn decode_verified_world_view_bindings_event(
    event: &Event,
) -> Result<DecodedWorldViewBindingsEvent, String> {
    if event.kind.as_u16() as u32 != crate::kind::KIND_WORLD_VIEW_BINDINGS {
        return Err("world-view bindings event has the wrong kind".into());
    }

    let document: WorldViewBindingsDocument = serde_json::from_str(&event.content)
        .map_err(|error| format!("world-view bindings content is invalid: {error}"))?;
    document.validate()?;

    let h_tags = exact_event_tags(event, "h");
    if h_tags.len() != 1 || h_tags[0].len() != 2 {
        return Err("world-view bindings require one exact channel h tag".into());
    }
    let channel_id = Uuid::parse_str(&h_tags[0][1])
        .map_err(|_| "world-view bindings require one exact channel h tag".to_string())?;

    let d_tag = document.scope.d_tag();
    let d_tags = exact_event_tags(event, "d");
    if d_tags.len() != 1 || d_tags[0].len() != 2 || d_tags[0][1] != d_tag {
        return Err("world-view bindings d tag does not match document scope".into());
    }

    let previous_tags = exact_event_tags(event, "prev");
    if previous_tags.len() != 1 || previous_tags[0].len() != 2 {
        return Err("world-view bindings require one exact prev tag".into());
    }
    let previous = &previous_tags[0][1];
    let previous_revision_event_id = if previous.is_empty() {
        None
    } else {
        validate_nostr_event_id("world-view bindings prev tag", previous)?;
        Some(
            EventId::from_hex(previous)
                .map_err(|_| "world-view bindings prev tag is not valid hex".to_string())?,
        )
    };

    let root_event_id = document.scope.thread_root_event_id();
    let e_tags = exact_event_tags(event, "e");
    match root_event_id {
        Some(root)
            if e_tags.len() != 1
                || e_tags[0].len() != 4
                || e_tags[0][1] != root
                || !e_tags[0][2].is_empty()
                || e_tags[0][3] != "root" =>
        {
            return Err("thread world-view bindings require one canonical root e tag".into());
        }
        None if !e_tags.is_empty() => {
            return Err("channel world-view bindings must not carry e tags".into());
        }
        Some(_) | None => {}
    }

    Ok(DecodedWorldViewBindingsEvent {
        channel_id,
        snapshot: WorldViewBindingsSnapshot {
            document,
            revision_event_id: Some(event.id.to_hex()),
            updated_at: Some(event.created_at.as_secs()),
            author: Some(event.pubkey.to_hex()),
        },
        previous_revision_event_id,
    })
}

/// Decode one already verified event and require an exact requested coordinate.
pub fn world_view_bindings_snapshot_from_verified_event(
    event: &Event,
    expected_channel_id: Uuid,
    expected_scope: &WorldViewBindingScope,
) -> Result<WorldViewBindingsSnapshot, String> {
    let decoded = decode_verified_world_view_bindings_event(event)?;
    if decoded.channel_id != expected_channel_id {
        return Err("world-view bindings event channel did not match its relay coordinate".into());
    }
    if &decoded.snapshot.document.scope != expected_scope {
        return Err("world-view bindings event scope did not match its relay coordinate".into());
    }
    Ok(decoded.snapshot)
}

fn exact_event_tags<'a>(event: &'a Event, name: &str) -> Vec<&'a [String]> {
    event
        .tags
        .iter()
        .map(|tag| tag.as_slice())
        .filter(|parts| parts.first().is_some_and(|part| part == name))
        .collect()
}
/// One effective binding with the exact declaration that currently owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveWorldViewBinding {
    /// Bound view selected after applying thread shadowing.
    pub binding: WorldViewBinding,
    /// Exact channel or thread-root scope that declared this binding.
    pub declared_scope: WorldViewBindingScope,
    /// Relay revision event that supplied this binding.
    pub binding_revision_event_id: String,
}

/// Effective bindings for one channel turn, with optional thread inheritance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveWorldViewBindings {
    /// Scope in which the views are being consumed.
    pub effective_scope: WorldViewBindingScope,
    /// Channel order with same-id thread overrides and new thread views appended.
    pub bindings: Vec<EffectiveWorldViewBinding>,
    /// Current exact channel document revision, when present.
    pub channel_revision_event_id: Option<String>,
    /// Current exact thread document revision, when present.
    pub thread_revision_event_id: Option<String>,
}

/// Merge exact channel and thread-root snapshots into one effective declaration.
///
/// A thread binding with the same stable id replaces its channel declaration
/// in place. New thread bindings append in authored order.
pub fn effective_world_view_bindings(
    channel: &WorldViewBindingsSnapshot,
    thread: Option<&WorldViewBindingsSnapshot>,
) -> Result<EffectiveWorldViewBindings, String> {
    if channel.document.scope != WorldViewBindingScope::Channel {
        return Err("channel inheritance source must declare channel scope".into());
    }

    let effective_scope = thread
        .map(|snapshot| snapshot.document.scope.clone())
        .unwrap_or(WorldViewBindingScope::Channel);
    let mut bindings = Vec::with_capacity(
        channel.document.bindings.len()
            + thread
                .map(|snapshot| snapshot.document.bindings.len())
                .unwrap_or_default(),
    );
    let mut positions = HashMap::with_capacity(channel.document.bindings.len());

    if let Some(revision_event_id) = channel.revision_event_id.as_ref() {
        for binding in &channel.document.bindings {
            positions.insert(binding.id, bindings.len());
            bindings.push(EffectiveWorldViewBinding {
                binding: binding.clone(),
                declared_scope: WorldViewBindingScope::Channel,
                binding_revision_event_id: revision_event_id.clone(),
            });
        }
    } else if !channel.document.bindings.is_empty() {
        return Err("channel bindings require a source revision event id".into());
    }

    if let Some(thread) = thread {
        if !matches!(thread.document.scope, WorldViewBindingScope::Thread { .. }) {
            return Err("thread override source must declare thread scope".into());
        }
        if let Some(revision_event_id) = thread.revision_event_id.as_ref() {
            for binding in &thread.document.bindings {
                let effective = EffectiveWorldViewBinding {
                    binding: binding.clone(),
                    declared_scope: thread.document.scope.clone(),
                    binding_revision_event_id: revision_event_id.clone(),
                };
                if let Some(position) = positions.get(&binding.id).copied() {
                    bindings[position] = effective;
                } else {
                    positions.insert(binding.id, bindings.len());
                    bindings.push(effective);
                }
            }
        } else if !thread.document.bindings.is_empty() {
            return Err("thread bindings require a source revision event id".into());
        }
    }

    Ok(EffectiveWorldViewBindings {
        effective_scope,
        bindings,
        channel_revision_event_id: channel.revision_event_id.clone(),
        thread_revision_event_id: thread.and_then(|snapshot| snapshot.revision_event_id.clone()),
    })
}

/// A single channel-bound Shivai world view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldViewBinding {
    /// Stable binding identity used by clients when views are reordered or replaced.
    pub id: Uuid,
    /// Optional channel-authored label shown above the rendered view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Source authority for resolving the world snapshot.
    pub reference: WorldViewReference,
    /// Qualified realm name selected inside the world.
    pub realm_qualified_name: String,
    /// Qualified view name selected inside the realm.
    pub view_qualified_name: String,
    /// Initial presentation selected by the channel author.
    pub display_mode: WorldViewDisplayMode,
}

/// A supported authority for resolving a bound world view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorldViewReference {
    /// The latest read-only projection of a stable published local world mirror.
    LocalWorldMirrorLatest {
        /// Hosted Shivai origin serving the public mirror projection.
        origin: String,
        /// Stable mirror identity; revisions advance without replacing this binding.
        #[serde(rename = "mirrorId")]
        mirror_id: String,
    },
    /// A read-only hosted world view export shared by bearer token.
    HostedWorldViewExport {
        /// Hosted Shivai origin serving the public export.
        origin: String,
        /// Public read-only export token.
        #[serde(rename = "shareToken")]
        share_token: String,
    },
}

/// Initial channel presentation for a bound world view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldViewDisplayMode {
    /// Interactive Shivai dependency graph.
    Graph,
    /// Plain ordered task list.
    Tasks,
}

impl WorldViewBindingsDocument {
    /// Validate scope, limits, and identity invariants before publication.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != WORLD_VIEW_BINDINGS_VERSION {
            return Err(format!(
                "unsupported world view bindings version: {}",
                self.version
            ));
        }
        self.scope.validate()?;
        if self.bindings.len() > MAX_WORLD_VIEW_BINDINGS_PER_SCOPE {
            return Err(format!(
                "a scope may bind at most {MAX_WORLD_VIEW_BINDINGS_PER_SCOPE} world views"
            ));
        }

        let mut ids = HashSet::with_capacity(self.bindings.len());
        for binding in &self.bindings {
            if !ids.insert(binding.id) {
                return Err(format!("duplicate world view binding id: {}", binding.id));
            }
            validate_required_text("realmQualifiedName", &binding.realm_qualified_name, 512)?;
            validate_required_text("viewQualifiedName", &binding.view_qualified_name, 512)?;
            if let Some(label) = &binding.label {
                validate_required_text("label", label, 160)?;
            }
            match &binding.reference {
                WorldViewReference::LocalWorldMirrorLatest { origin, mirror_id } => {
                    validate_hosted_origin(origin)?;
                    validate_required_text("reference.mirrorId", mirror_id, 1024)?;
                }
                WorldViewReference::HostedWorldViewExport {
                    origin,
                    share_token,
                } => {
                    validate_hosted_origin(origin)?;
                    validate_required_text("reference.shareToken", share_token, 1024)?;
                }
            }
        }
        Ok(())
    }
}

fn validate_hosted_origin(value: &str) -> Result<(), String> {
    validate_required_text("reference.origin", value, 2048)?;
    let parsed = url::Url::parse(value)
        .map_err(|error| format!("reference.origin must be an absolute URL: {error}"))?;
    let is_loopback_http = parsed.scheme() == "http"
        && parsed.host().is_some_and(|host| match host {
            url::Host::Domain(domain) => domain == "localhost",
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
        });
    if parsed.scheme() != "https" && !is_loopback_http {
        return Err(
            "reference.origin must use https (http is allowed only for loopback development)"
                .into(),
        );
    }
    if parsed.origin().ascii_serialization() != value {
        return Err("reference.origin must contain only scheme, host, and optional port".into());
    }
    Ok(())
}

fn validate_nostr_event_id(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("{field} must be 64 lowercase hex characters"));
    }
    Ok(())
}

fn validate_required_text(field: &str, value: &str, max_len: usize) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if trimmed.len() > max_len {
        return Err(format!("{field} exceeds {max_len} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: Uuid) -> WorldViewBinding {
        WorldViewBinding {
            id,
            label: Some("Launch board".into()),
            reference: WorldViewReference::HostedWorldViewExport {
                origin: "https://manifest.shivai.space".into(),
                share_token: "view-token".into(),
            },
            realm_qualified_name: "world::main".into(),
            view_qualified_name: "world::main::@Board".into(),
            display_mode: WorldViewDisplayMode::Graph,
        }
    }

    fn signed_bindings_event(
        channel_id: Uuid,
        document: &WorldViewBindingsDocument,
        previous_revision_event_id: &str,
        root_override: Option<&str>,
    ) -> Event {
        use nostr::{EventBuilder, Kind, Tag};

        let mut tags = vec![
            Tag::parse(["h", channel_id.to_string().as_str()]).expect("h tag"),
            Tag::parse(["d", document.scope.d_tag().as_str()]).expect("d tag"),
            Tag::parse(["prev", previous_revision_event_id]).expect("prev tag"),
        ];
        if let Some(root) = root_override.or(document.scope.thread_root_event_id()) {
            tags.push(Tag::parse(["e", root, "", "root"]).expect("e tag"));
        }
        EventBuilder::new(
            Kind::Custom(crate::kind::KIND_WORLD_VIEW_BINDINGS as u16),
            serde_json::to_string(document).expect("serialize"),
        )
        .tags(tags)
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign")
    }

    #[test]
    fn round_trips_the_versioned_binding_document() {
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: WorldViewBindingScope::Channel,
            bindings: vec![binding(Uuid::nil())],
        };

        let encoded = serde_json::to_value(&document).expect("serialize");
        assert_eq!(
            encoded["bindings"][0]["reference"]["kind"],
            "hosted-world-view-export"
        );
        assert_eq!(
            encoded["bindings"][0]["reference"]["origin"],
            "https://manifest.shivai.space"
        );
        assert_eq!(encoded["scope"]["kind"], "channel");
        assert_eq!(encoded["bindings"][0]["displayMode"], "graph");
        let decoded: WorldViewBindingsDocument =
            serde_json::from_value(encoded).expect("deserialize");
        assert_eq!(decoded, document);
        assert_eq!(decoded.validate(), Ok(()));
    }

    #[test]
    fn decodes_verified_binding_event_into_typed_revision_state() {
        let channel_id = Uuid::new_v4();
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: WorldViewBindingScope::Channel,
            bindings: vec![binding(Uuid::nil())],
        };
        let event = signed_bindings_event(channel_id, &document, &"a".repeat(64), None);

        let decoded =
            decode_verified_world_view_bindings_event(&event).expect("decode canonical event");

        assert_eq!(decoded.channel_id, channel_id);
        assert_eq!(decoded.snapshot.document, document);
        assert_eq!(
            decoded.previous_revision_event_id.map(|id| id.to_hex()),
            Some("a".repeat(64))
        );
        assert_eq!(decoded.snapshot.revision_event_id, Some(event.id.to_hex()));
    }

    #[test]
    fn rejects_mismatched_thread_root_at_the_shared_event_boundary() {
        let channel_id = Uuid::new_v4();
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: WorldViewBindingScope::thread("a".repeat(64)).expect("thread scope"),
            bindings: Vec::new(),
        };
        let event = signed_bindings_event(
            channel_id,
            &document,
            &"b".repeat(64),
            Some(&"c".repeat(64)),
        );

        assert_eq!(
            decode_verified_world_view_bindings_event(&event).map(|_| ()),
            Err("thread world-view bindings require one canonical root e tag".into())
        );
    }

    #[test]
    fn rejects_unknown_nested_binding_and_reference_fields() {
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: WorldViewBindingScope::Channel,
            bindings: vec![binding(Uuid::nil())],
        };
        let mut binding_value = serde_json::to_value(&document).expect("serialize");
        binding_value["bindings"][0]["unexpected"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<WorldViewBindingsDocument>(binding_value)
                .expect_err("binding extras must fail")
                .to_string()
                .contains("unknown field")
        );

        let mut reference_value = serde_json::to_value(&document).expect("serialize");
        reference_value["bindings"][0]["reference"]["accessToken"] =
            serde_json::json!("must-not-cross-boundary");
        assert!(
            serde_json::from_value::<WorldViewBindingsDocument>(reference_value)
                .expect_err("reference extras must fail")
                .to_string()
                .contains("unknown field")
        );
    }

    #[test]
    fn thread_bindings_shadow_channel_ids_without_reordering_inherited_views() {
        let inherited_id = Uuid::nil();
        let shadowed_id = Uuid::from_u128(1);
        let appended_id = Uuid::from_u128(2);
        let channel = WorldViewBindingsSnapshot {
            document: WorldViewBindingsDocument {
                version: WORLD_VIEW_BINDINGS_VERSION,
                scope: WorldViewBindingScope::Channel,
                bindings: vec![binding(inherited_id), binding(shadowed_id)],
            },
            revision_event_id: Some("a".repeat(64)),
            updated_at: Some(1),
            author: Some("channel-author".into()),
        };
        let thread_scope = WorldViewBindingScope::Thread {
            thread_root_event_id: "b".repeat(64),
        };
        let mut shadow = binding(shadowed_id);
        shadow.label = Some("Thread override".into());
        let mut appended = binding(appended_id);
        appended.label = Some("Thread only".into());
        let thread = WorldViewBindingsSnapshot {
            document: WorldViewBindingsDocument {
                version: WORLD_VIEW_BINDINGS_VERSION,
                scope: thread_scope.clone(),
                bindings: vec![shadow, appended],
            },
            revision_event_id: Some("c".repeat(64)),
            updated_at: Some(2),
            author: Some("thread-author".into()),
        };

        let effective =
            effective_world_view_bindings(&channel, Some(&thread)).expect("merge effective views");

        assert_eq!(effective.effective_scope, thread_scope);
        assert_eq!(
            effective
                .bindings
                .iter()
                .map(|entry| entry.binding.id)
                .collect::<Vec<_>>(),
            vec![inherited_id, shadowed_id, appended_id]
        );
        assert_eq!(
            effective.bindings[0].declared_scope,
            WorldViewBindingScope::Channel
        );
        assert_eq!(
            effective.bindings[0].binding_revision_event_id,
            "a".repeat(64)
        );
        assert_eq!(
            effective.bindings[1].declared_scope,
            effective.effective_scope
        );
        assert_eq!(
            effective.bindings[1].binding.label.as_deref(),
            Some("Thread override")
        );
        assert_eq!(
            effective.bindings[2].binding.label.as_deref(),
            Some("Thread only")
        );
        assert_eq!(effective.thread_revision_event_id, Some("c".repeat(64)));
    }

    #[test]
    fn rejects_non_origin_and_insecure_remote_urls() {
        let mut document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: WorldViewBindingScope::Channel,
            bindings: vec![binding(Uuid::nil())],
        };
        if let WorldViewReference::HostedWorldViewExport { origin, .. } =
            &mut document.bindings[0].reference
        {
            *origin = "https://manifest.shivai.space/world/export".into();
        }
        assert_eq!(
            document.validate(),
            Err("reference.origin must contain only scheme, host, and optional port".into())
        );

        if let WorldViewReference::HostedWorldViewExport { origin, .. } =
            &mut document.bindings[0].reference
        {
            *origin = "https://manifest.shivai.space/".into();
        }
        assert_eq!(
            document.validate(),
            Err("reference.origin must contain only scheme, host, and optional port".into())
        );

        if let WorldViewReference::HostedWorldViewExport { origin, .. } =
            &mut document.bindings[0].reference
        {
            *origin = "http://manifest.shivai.space".into();
        }
        assert_eq!(
            document.validate(),
            Err(
                "reference.origin must use https (http is allowed only for loopback development)"
                    .into()
            )
        );
    }

    #[test]
    fn local_authority_registry_upsert_preserves_one_to_one_mappings() {
        let mut registry = LocalWorldAuthorityRegistry::default();
        registry
            .upsert(LocalWorldAuthority {
                origin: "https://manifest.shivai.space".into(),
                mirror_id: "mirror-1".into(),
                source_root: "/worlds/one.world".into(),
            })
            .unwrap();
        registry
            .upsert(LocalWorldAuthority {
                origin: "https://manifest.shivai.space".into(),
                mirror_id: "mirror-2".into(),
                source_root: "/worlds/one.world".into(),
            })
            .unwrap();

        assert_eq!(registry.authorities.len(), 1);
        assert!(registry
            .resolve("https://manifest.shivai.space", "mirror-1")
            .is_none());
        assert_eq!(
            registry
                .resolve("https://manifest.shivai.space", "mirror-2")
                .map(|authority| authority.source_root.as_str()),
            Some("/worlds/one.world")
        );
    }

    #[test]
    fn derives_stable_thread_scope_coordinate() {
        let root = "a".repeat(64);
        let scope = WorldViewBindingScope::thread(root.clone()).expect("valid thread scope");

        assert_eq!(scope.d_tag(), format!("world-view-bindings:thread:{root}"));
        assert_eq!(scope.thread_root_event_id(), Some(root.as_str()));
    }

    #[test]
    fn rejects_noncanonical_thread_event_ids() {
        assert_eq!(
            WorldViewBindingScope::thread("A".repeat(64)),
            Err("scope.threadRootEventId must be 64 lowercase hex characters".into())
        );
    }

    #[test]
    fn rejects_duplicate_binding_ids() {
        let id = Uuid::nil();
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: WorldViewBindingScope::Channel,
            bindings: vec![binding(id), binding(id)],
        };

        assert_eq!(
            document.validate(),
            Err(format!("duplicate world view binding id: {id}"))
        );
    }
}
