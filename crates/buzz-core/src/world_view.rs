//! Channel-scoped Shivai world view binding contracts.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Current serialized channel world-view binding document version.
pub const WORLD_VIEW_BINDINGS_VERSION: u8 = 1;
/// Maximum number of world views that one channel may bind.
pub const MAX_CHANNEL_WORLD_VIEW_BINDINGS: usize = 8;
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

/// One channel-scoped document containing every bound Shivai world view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldViewBindingsDocument {
    /// Contract version for explicit forward evolution.
    pub version: u8,
    /// Ordered views rendered by channel clients and supplied to agents.
    pub bindings: Vec<WorldViewBinding>,
}

/// A single channel-bound Shivai world view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(tag = "kind", rename_all = "kebab-case")]
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

impl WorldViewBinding {
    /// Build the canonical read-only `world` CLI arguments for this binding.
    pub fn world_cli_args(&self) -> Vec<String> {
        let mut args = match &self.reference {
            WorldViewReference::LocalWorldMirrorLatest { origin, mirror_id } => vec![
                "hosted".into(),
                "view".into(),
                "dump".into(),
                "--json".into(),
                "--base-url".into(),
                origin.clone(),
                "--local-mirror".into(),
                mirror_id.clone(),
            ],
            WorldViewReference::HostedWorldViewExport {
                origin,
                share_token,
            } => vec![
                "hosted".into(),
                "view".into(),
                "dump".into(),
                "--json".into(),
                "--base-url".into(),
                origin.clone(),
                "--share-token".into(),
                share_token.clone(),
            ],
        };
        args.extend([
            "--realm".into(),
            self.realm_qualified_name.clone(),
            "--view".into(),
            self.view_qualified_name.clone(),
        ]);
        args
    }
}

impl WorldViewBindingsDocument {
    /// Validate limits and identity invariants before publishing or resolving.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != WORLD_VIEW_BINDINGS_VERSION {
            return Err(format!(
                "unsupported world view bindings version: {}",
                self.version
            ));
        }
        if self.bindings.len() > MAX_CHANNEL_WORLD_VIEW_BINDINGS {
            return Err(format!(
                "a channel may bind at most {MAX_CHANNEL_WORLD_VIEW_BINDINGS} world views"
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
    let normalized = value.trim_end_matches('/');
    if parsed.origin().ascii_serialization() != normalized {
        return Err("reference.origin must contain only scheme, host, and optional port".into());
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

    #[test]
    fn round_trips_the_versioned_binding_document() {
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
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
        assert_eq!(encoded["bindings"][0]["displayMode"], "graph");
        let decoded: WorldViewBindingsDocument =
            serde_json::from_value(encoded).expect("deserialize");
        assert_eq!(decoded, document);
        assert_eq!(decoded.validate(), Ok(()));
    }

    #[test]
    fn builds_public_latest_mirror_resolver_arguments() {
        let binding = WorldViewBinding {
            reference: WorldViewReference::LocalWorldMirrorLatest {
                origin: "http://127.0.0.1:3000".into(),
                mirror_id: "mirror-1".into(),
            },
            ..binding(Uuid::nil())
        };

        assert_eq!(
            binding.world_cli_args(),
            vec![
                "hosted",
                "view",
                "dump",
                "--json",
                "--base-url",
                "http://127.0.0.1:3000",
                "--local-mirror",
                "mirror-1",
                "--realm",
                "world::main",
                "--view",
                "world::main::@Board",
            ]
        );
    }

    #[test]
    fn rejects_non_origin_and_insecure_remote_urls() {
        let mut document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
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
    fn rejects_duplicate_binding_ids() {
        let id = Uuid::nil();
        let document = WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            bindings: vec![binding(id), binding(id)],
        };

        assert_eq!(
            document.validate(),
            Err(format!("duplicate world view binding id: {id}"))
        );
    }
}
