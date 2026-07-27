mod presentation;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use buzz_core::world_view::{
    WorldViewBinding, WorldViewBindingScope, WorldViewBindingsDocument, WorldViewReference,
    WORLD_VIEW_BINDINGS_VERSION,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub use presentation::*;

const WORLD_VIEW_RESOLUTION_FORMAT_VERSION: u8 = 1;
const WORLD_VIEW_CATALOG_FORMAT_VERSION: u8 = 1;

/// Everything needed to resolve one binding without consulting ambient UI state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldViewResolutionRequest {
    pub channel_id: Uuid,
    pub binding: WorldViewBinding,
    pub declared_scope: WorldViewBindingScope,
    pub effective_scope: WorldViewBindingScope,
    pub binding_revision_event_id: String,
}

impl WorldViewResolutionRequest {
    pub fn validate(&self) -> Result<(), WorldViewResolutionError> {
        WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            scope: self.declared_scope.clone(),
            bindings: vec![self.binding.clone()],
        }
        .validate()
        .map_err(WorldViewResolutionError::InvalidRequest)?;
        self.effective_scope
            .validate()
            .map_err(WorldViewResolutionError::InvalidRequest)?;
        validate_event_id("bindingRevisionEventId", &self.binding_revision_event_id)?;
        Ok(())
    }
}
/// Private machine-local authority supplied by the caller for one resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldViewResolutionAccess {
    None,
    HostedEditShareFile { credential_file: PathBuf },
}

impl Default for WorldViewResolutionAccess {
    fn default() -> Self {
        Self::None
    }
}

/// Credential-free authority readback for the source that produced a resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorldViewResolutionAuthority {
    HostedWorldViewExport {
        origin: String,
    },
    HostedWorldLiveViewShare {
        origin: String,
        #[serde(rename = "hostedWorldId")]
        hosted_world_id: String,
    },
    LocalWorldMirrorLatest {
        origin: String,
        #[serde(rename = "mirrorId")]
        mirror_id: String,
    },
    HostedWorldLatest {
        origin: String,
        #[serde(rename = "hostedWorldId")]
        hosted_world_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldViewResolutionFreshness {
    Pinned,
    LatestAtResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorldViewEntity {
    pub name: String,
    pub qualified_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldViewCatalogEntry {
    pub name: String,
    pub qualified_name: String,
    pub realm: ResolvedWorldViewEntity,
}

/// Canonical authored view identities available through one public source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldViewCatalog {
    pub format_version: u8,
    pub revision: String,
    pub world_qualified_name: String,
    pub views: Vec<WorldViewCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewCounts {
    pub nodes: usize,
    pub edges: usize,
    pub ready: usize,
    pub actionable_ready: usize,
    pub satisfied: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedWorldViewNodeStatus {
    Satisfied,
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewNote {
    pub preview: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewSignalCase {
    pub name: String,
    pub evidence: Vec<WorldViewSignalEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedWorldViewSignalTarget {
    Preference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedWorldViewSignalMode {
    First,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewSignal {
    pub name: String,
    pub target: ResolvedWorldViewSignalTarget,
    pub mode: ResolvedWorldViewSignalMode,
    pub cases: Vec<ResolvedWorldViewSignalCase>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewNode {
    pub preference: String,
    pub qualified_name: String,
    pub status: ResolvedWorldViewNodeStatus,
    pub actionable: bool,
    pub leaf: bool,
    pub in_focus: bool,
    pub in_satisfied: bool,
    pub blockers: Vec<String>,
    pub enablers: Vec<String>,
    pub note: ResolvedWorldViewNote,
    pub signals: Vec<ResolvedWorldViewSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewEdge {
    pub downstream: String,
    pub upstream: String,
    pub relation: ResolvedWorldViewEdgeRelation,
    pub connection_type: WorldViewConnectionType,
    pub flowspace: String,
    pub flowspace_qualified_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedWorldViewEdgeRelation {
    Blocker,
    Enabler,
}

/// Compact, agent-ready subset of `world view dump` with no untyped JSON payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldViewDump {
    pub counts: ResolvedWorldViewCounts,
    pub nodes: Vec<ResolvedWorldViewNode>,
    pub ready_leaves: Vec<ResolvedWorldViewNode>,
    pub satisfied_nodes: Vec<ResolvedWorldViewNode>,
    pub blocked_nodes: Vec<ResolvedWorldViewNode>,
    pub edges: Vec<ResolvedWorldViewEdge>,
}

/// Canonical result shared by Buzz CLI, desktop, and agent prompt delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedWorldView {
    pub format_version: u8,
    pub binding_id: Uuid,
    pub channel_id: Uuid,
    pub declared_scope: WorldViewBindingScope,
    pub effective_scope: WorldViewBindingScope,
    pub binding_revision_event_id: String,
    pub source_revision: String,
    pub freshness: WorldViewResolutionFreshness,
    pub authority: WorldViewResolutionAuthority,
    pub realm: ResolvedWorldViewEntity,
    pub view: ResolvedWorldViewEntity,
    pub view_dump: ResolvedWorldViewDump,
    pub presentation: WorldViewPresentationVariants,
    pub resolved_at: DateTime<Utc>,
    pub next_command: String,
}

#[derive(Debug, Error)]
pub enum WorldViewResolutionError {
    #[error("invalid world-view resolution request: {0}")]
    InvalidRequest(String),
    #[error(
        "hosted world `{hosted_world_id}` has no private edit-share authority registered on this client"
    )]
    MissingHostedAuthority { hosted_world_id: String },
    #[error("could not launch the Shivai world resolver `{binary}`: {source}")]
    Launch {
        binary: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not send the hosted view capability to `{binary}` over stdin: {source}")]
    Input {
        binary: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Shivai world-view resolution failed: {0}")]
    CommandFailed(String),
    #[error("Shivai world resolver returned invalid JSON: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("Shivai world resolver returned an invalid result: {0}")]
    InvalidResult(String),
}

#[derive(Debug, Deserialize)]
struct WorldResultEnvelope<T> {
    ok: bool,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldViewDumpResult {
    revision: String,
    hosted_world_id: Option<String>,
    realm: ResolvedWorldViewEntity,
    view: ResolvedWorldViewEntity,
    counts: ResolvedWorldViewCounts,
    presentation: WorldViewPresentationVariants,
    nodes: Vec<ResolvedWorldViewNode>,
    ready_leaves: Vec<ResolvedWorldViewNode>,
    satisfied_nodes: Vec<ResolvedWorldViewNode>,
    blocked_nodes: Vec<ResolvedWorldViewNode>,
    edges: Vec<ResolvedWorldViewEdge>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldViewCatalogResult {
    command: String,
    format_version: u8,
    revision: String,
    world_qualified_name: String,
    views: Vec<WorldViewCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostedEditShareInspection {
    pub hosted_world_id: String,
    pub revision: String,
}

/// Stable public live-view capability minted from private hosted authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedHostedLiveViewShare {
    pub hosted_world_id: String,
    pub source_revision: String,
    pub package_revision: String,
    pub realm_qualified_name: String,
    pub view_qualified_name: String,
    pub share_token: String,
    pub share_url_path: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldHostedPublishLiveViewShareResult {
    command: String,
    live_view_share: WorldHostedLiveViewShare,
    source: WorldHostedLiveViewShareSource,
    selection: WorldHostedLiveViewShareSelection,
    revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldHostedLiveViewShare {
    share_token: String,
    share_url_path: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldHostedLiveViewShareSource {
    hosted_world_id: String,
    revision_id: String,
    package_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldHostedLiveViewShareSelection {
    realm_qualified_name: String,
    view_qualified_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldHostedLatestResult {
    projection: WorldHostedLatestProjection,
    revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldHostedLatestProjection {
    hosted_world_id: String,
}

/// Resolve using `SHIVAI_WORLD_BIN`, or `world` when the override is absent.
pub async fn resolve_world_view(
    request: WorldViewResolutionRequest,
) -> Result<ResolvedWorldView, WorldViewResolutionError> {
    resolve_world_view_with_access(request, WorldViewResolutionAccess::None).await
}

/// Resolve with explicit private machine-local authority.
pub async fn resolve_world_view_with_access(
    request: WorldViewResolutionRequest,
    access: WorldViewResolutionAccess,
) -> Result<ResolvedWorldView, WorldViewResolutionError> {
    let binary = std::env::var_os("SHIVAI_WORLD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("world"));
    resolve_world_view_with_binary_and_access(request, binary, access).await
}

/// Resolve through one explicitly selected source `world` binary.
pub async fn resolve_world_view_with_binary(
    request: WorldViewResolutionRequest,
    binary: impl AsRef<Path>,
) -> Result<ResolvedWorldView, WorldViewResolutionError> {
    resolve_world_view_with_binary_and_access(request, binary, WorldViewResolutionAccess::None)
        .await
}

/// Resolve through one source `world` binary with explicit private authority.
pub async fn resolve_world_view_with_binary_and_access(
    request: WorldViewResolutionRequest,
    binary: impl AsRef<Path>,
    access: WorldViewResolutionAccess,
) -> Result<ResolvedWorldView, WorldViewResolutionError> {
    request.validate()?;
    let binary = binary.as_ref();
    let invocation = world_cli_invocation(&request.binding, &access)?;
    let stdout = run_world_cli_invocation(binary, invocation, &request.binding.reference).await?;
    decode_world_view_resolution(&request, &stdout, Utc::now())
}

/// List canonical authored views using `SHIVAI_WORLD_BIN`, or `world` when absent.
pub async fn catalog_world_views(
    reference: WorldViewReference,
) -> Result<WorldViewCatalog, WorldViewResolutionError> {
    catalog_world_views_with_access(reference, WorldViewResolutionAccess::None).await
}

/// List canonical authored views with explicit private machine-local authority.
pub async fn catalog_world_views_with_access(
    reference: WorldViewReference,
    access: WorldViewResolutionAccess,
) -> Result<WorldViewCatalog, WorldViewResolutionError> {
    let binary = std::env::var_os("SHIVAI_WORLD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("world"));
    catalog_world_views_with_binary_and_access(reference, binary, access).await
}

/// List canonical authored views through one explicitly selected source binary.
pub async fn catalog_world_views_with_binary_and_access(
    reference: WorldViewReference,
    binary: impl AsRef<Path>,
    access: WorldViewResolutionAccess,
) -> Result<WorldViewCatalog, WorldViewResolutionError> {
    reference
        .validate()
        .map_err(WorldViewResolutionError::InvalidRequest)?;
    let binary = binary.as_ref();
    let invocation = world_view_cli_invocation(&reference, &access, "catalog")?;
    let stdout = run_world_cli_invocation(binary, invocation, &reference).await?;
    decode_world_view_catalog(&stdout)
}

async fn run_world_cli_invocation(
    binary: &Path,
    invocation: WorldCliInvocation<'_>,
    reference: &WorldViewReference,
) -> Result<Vec<u8>, WorldViewResolutionError> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(&invocation.args)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if invocation.stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .map_err(|source| WorldViewResolutionError::Launch {
            binary: binary.to_owned(),
            source,
        })?;
    if let Some(input) = invocation.stdin {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| WorldViewResolutionError::Input {
                binary: binary.to_owned(),
                source: std::io::Error::other("resolver stdin pipe was not available"),
            })?;
        stdin.write_all(input.as_bytes()).await.map_err(|source| {
            WorldViewResolutionError::Input {
                binary: binary.to_owned(),
                source,
            }
        })?;
    }
    let output =
        child
            .wait_with_output()
            .await
            .map_err(|source| WorldViewResolutionError::Launch {
                binary: binary.to_owned(),
                source,
            })?;
    if !output.status.success() {
        let diagnostics =
            redact_diagnostics(String::from_utf8_lossy(&output.stderr).trim(), reference);
        return Err(WorldViewResolutionError::CommandFailed(
            if diagnostics.is_empty() {
                "the command exited without diagnostics".into()
            } else {
                diagnostics
            },
        ));
    }
    Ok(output.stdout)
}

/// Inspect one private edit-share credential without placing it in process arguments.
pub async fn inspect_hosted_edit_share(
    origin: &str,
    credential_file: impl AsRef<Path>,
) -> Result<HostedEditShareInspection, WorldViewResolutionError> {
    let binary = std::env::var_os("SHIVAI_WORLD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("world"));
    inspect_hosted_edit_share_with_binary(origin, credential_file, binary).await
}

/// Inspect one private edit-share credential through an explicit source `world` binary.
pub async fn inspect_hosted_edit_share_with_binary(
    origin: &str,
    credential_file: impl AsRef<Path>,
    binary: impl AsRef<Path>,
) -> Result<HostedEditShareInspection, WorldViewResolutionError> {
    let binary = binary.as_ref();
    let output = tokio::process::Command::new(binary)
        .args([
            "hosted",
            "latest",
            "--json",
            "--base-url",
            origin,
            "--edit-share-file",
            &credential_file.as_ref().to_string_lossy(),
            "--anonymous-session",
        ])
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|source| WorldViewResolutionError::Launch {
            binary: binary.to_owned(),
            source,
        })?;
    if !output.status.success() {
        let diagnostics = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorldViewResolutionError::CommandFailed(
            if diagnostics.is_empty() {
                "the command exited without diagnostics".into()
            } else {
                diagnostics
            },
        ));
    }

    let envelope: WorldResultEnvelope<WorldHostedLatestResult> =
        serde_json::from_slice(&output.stdout)?;
    if !envelope.ok {
        return Err(WorldViewResolutionError::InvalidResult(
            "the command returned a non-success envelope".into(),
        ));
    }
    let result = envelope.result.ok_or_else(|| {
        WorldViewResolutionError::InvalidResult("the success envelope omitted `result`".into())
    })?;
    if result.projection.hosted_world_id.trim().is_empty() {
        return Err(WorldViewResolutionError::InvalidResult(
            "the hosted-world id is blank".into(),
        ));
    }
    if result.revision.trim().is_empty() {
        return Err(WorldViewResolutionError::InvalidResult(
            "the hosted-world revision is blank".into(),
        ));
    }
    Ok(HostedEditShareInspection {
        hosted_world_id: result.projection.hosted_world_id,
        revision: result.revision,
    })
}

/// Mint or reuse a stable public live-view share using `SHIVAI_WORLD_BIN`.
pub async fn publish_hosted_live_view_share(
    origin: &str,
    credential_file: impl AsRef<Path>,
    view_qualified_name: &str,
) -> Result<PublishedHostedLiveViewShare, WorldViewResolutionError> {
    let binary = std::env::var_os("SHIVAI_WORLD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("world"));
    publish_hosted_live_view_share_with_binary(
        origin,
        credential_file,
        view_qualified_name,
        binary,
    )
    .await
}

/// Mint or reuse a stable public live-view share through an explicit `world` binary.
pub async fn publish_hosted_live_view_share_with_binary(
    origin: &str,
    credential_file: impl AsRef<Path>,
    view_qualified_name: &str,
    binary: impl AsRef<Path>,
) -> Result<PublishedHostedLiveViewShare, WorldViewResolutionError> {
    let binary = binary.as_ref();
    let output = tokio::process::Command::new(binary)
        .args([
            "hosted",
            "view",
            "share-live",
            "--json",
            "--base-url",
            origin,
            "--edit-share-file",
            &credential_file.as_ref().to_string_lossy(),
            "--anonymous-session",
            "--view",
            view_qualified_name,
        ])
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|source| WorldViewResolutionError::Launch {
            binary: binary.to_owned(),
            source,
        })?;
    if !output.status.success() {
        let diagnostics = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorldViewResolutionError::CommandFailed(
            if diagnostics.is_empty() {
                "the command exited without diagnostics".into()
            } else {
                diagnostics
            },
        ));
    }

    let envelope: WorldResultEnvelope<WorldHostedPublishLiveViewShareResult> =
        serde_json::from_slice(&output.stdout)?;
    if !envelope.ok {
        return invalid_result("the command returned a non-success envelope");
    }
    let result = envelope.result.ok_or_else(|| {
        WorldViewResolutionError::InvalidResult("the success envelope omitted `result`".into())
    })?;
    if result.command != "view.share-live" {
        return invalid_result(format!(
            "unexpected live-share command `{}`",
            result.command
        ));
    }
    for (field, value) in [
        ("revision", result.revision.as_str()),
        ("source.hostedWorldId", result.source.hosted_world_id.as_str()),
        ("source.revisionId", result.source.revision_id.as_str()),
        (
            "source.packageRevision",
            result.source.package_revision.as_str(),
        ),
        (
            "selection.realmQualifiedName",
            result.selection.realm_qualified_name.as_str(),
        ),
        (
            "selection.viewQualifiedName",
            result.selection.view_qualified_name.as_str(),
        ),
        (
            "liveViewShare.shareToken",
            result.live_view_share.share_token.as_str(),
        ),
        (
            "liveViewShare.shareUrlPath",
            result.live_view_share.share_url_path.as_str(),
        ),
        ("liveViewShare.title", result.live_view_share.title.as_str()),
    ] {
        if value.trim().is_empty() {
            return invalid_result(format!("live-share `{field}` is blank"));
        }
    }
    if result.revision != result.source.package_revision {
        return invalid_result(
            "live-share result revision did not match its source package revision",
        );
    }
    if result.source.revision_id == result.source.package_revision {
        return invalid_result(
            "live-share source revision id unexpectedly matched its package revision",
        );
    }
    if result.selection.view_qualified_name != view_qualified_name {
        return invalid_result(format!(
            "live-share view `{}` did not match requested `{view_qualified_name}`",
            result.selection.view_qualified_name
        ));
    }

    Ok(PublishedHostedLiveViewShare {
        hosted_world_id: result.source.hosted_world_id,
        source_revision: result.source.revision_id,
        package_revision: result.source.package_revision,
        realm_qualified_name: result.selection.realm_qualified_name,
        view_qualified_name: result.selection.view_qualified_name,
        share_token: result.live_view_share.share_token,
        share_url_path: result.live_view_share.share_url_path,
        title: result.live_view_share.title,
    })
}

fn decode_world_view_catalog(stdout: &[u8]) -> Result<WorldViewCatalog, WorldViewResolutionError> {
    let envelope: WorldResultEnvelope<WorldViewCatalogResult> = serde_json::from_slice(stdout)?;
    if !envelope.ok {
        return invalid_result("the command returned a non-success envelope");
    }
    let result = envelope.result.ok_or_else(|| {
        WorldViewResolutionError::InvalidResult("the success envelope omitted `result`".into())
    })?;
    if result.command != "view.catalog" {
        return invalid_result(format!("unexpected catalog command `{}`", result.command));
    }
    if result.format_version != WORLD_VIEW_CATALOG_FORMAT_VERSION {
        return invalid_result(format!(
            "unsupported catalog format version {}",
            result.format_version
        ));
    }
    if result.revision.trim().is_empty() {
        return invalid_result("catalog `revision` is blank");
    }
    if result.world_qualified_name.trim().is_empty() {
        return invalid_result("catalog `worldQualifiedName` is blank");
    }

    let mut qualified_names = HashSet::with_capacity(result.views.len());
    for view in &result.views {
        if view.name.trim().is_empty()
            || view.qualified_name.trim().is_empty()
            || view.realm.name.trim().is_empty()
            || view.realm.qualified_name.trim().is_empty()
        {
            return invalid_result("catalog view names and realm identities must not be blank");
        }
        if !qualified_names.insert(&view.qualified_name) {
            return invalid_result(format!(
                "duplicate catalog view qualified name `{}`",
                view.qualified_name
            ));
        }
    }

    Ok(WorldViewCatalog {
        format_version: result.format_version,
        revision: result.revision,
        world_qualified_name: result.world_qualified_name,
        views: result.views,
    })
}

fn decode_world_view_resolution(
    request: &WorldViewResolutionRequest,
    stdout: &[u8],
    resolved_at: DateTime<Utc>,
) -> Result<ResolvedWorldView, WorldViewResolutionError> {
    let envelope: WorldResultEnvelope<WorldViewDumpResult> = serde_json::from_slice(stdout)?;
    if !envelope.ok {
        return Err(WorldViewResolutionError::InvalidResult(
            "the command returned a non-success envelope".into(),
        ));
    }
    let result = envelope.result.ok_or_else(|| {
        WorldViewResolutionError::InvalidResult("the success envelope omitted `result`".into())
    })?;
    validate_dump_result(request, &result)?;

    let (authority, freshness) = authority_readback(&request.binding.reference, &result)?;
    Ok(ResolvedWorldView {
        format_version: WORLD_VIEW_RESOLUTION_FORMAT_VERSION,
        binding_id: request.binding.id,
        channel_id: request.channel_id,
        declared_scope: request.declared_scope.clone(),
        effective_scope: request.effective_scope.clone(),
        binding_revision_event_id: request.binding_revision_event_id.clone(),
        source_revision: result.revision,
        freshness,
        authority,
        realm: result.realm,
        view: result.view,
        view_dump: ResolvedWorldViewDump {
            counts: result.counts,
            nodes: result.nodes,
            ready_leaves: result.ready_leaves,
            satisfied_nodes: result.satisfied_nodes,
            blocked_nodes: result.blocked_nodes,
            edges: result.edges,
        },
        presentation: result.presentation,
        resolved_at,
        next_command: next_command(request),
    })
}

fn validate_dump_result(
    request: &WorldViewResolutionRequest,
    result: &WorldViewDumpResult,
) -> Result<(), WorldViewResolutionError> {
    if result.revision.trim().is_empty() {
        return invalid_result("`revision` is blank");
    }
    if result.realm.qualified_name != request.binding.realm_qualified_name {
        return invalid_result(format!(
            "realm `{}` did not match requested `{}`",
            result.realm.qualified_name, request.binding.realm_qualified_name
        ));
    }
    if result.view.qualified_name != request.binding.view_qualified_name {
        return invalid_result(format!(
            "view `{}` did not match requested `{}`",
            result.view.qualified_name, request.binding.view_qualified_name
        ));
    }
    if result.counts.nodes != result.nodes.len() {
        return invalid_result(format!(
            "node count {} did not match {} returned nodes",
            result.counts.nodes,
            result.nodes.len()
        ));
    }
    if result.counts.edges != result.edges.len() {
        return invalid_result(format!(
            "edge count {} did not match {} returned edges",
            result.counts.edges,
            result.edges.len()
        ));
    }
    if result.presentation.format_version != WORLD_VIEW_RESOLUTION_FORMAT_VERSION {
        return invalid_result(format!(
            "unsupported presentation format version {}",
            result.presentation.format_version
        ));
    }
    for (appearance, model) in [
        ("dark", &result.presentation.dark),
        ("light", &result.presentation.light),
    ] {
        if model.selection.realm_qualified_name != request.binding.realm_qualified_name
            || model.selection.view_qualified_name != request.binding.view_qualified_name
        {
            return invalid_result(format!(
                "{appearance} presentation selection did not match the requested realm/view"
            ));
        }
        if model.revision.as_deref() != Some(result.revision.as_str()) {
            return invalid_result(format!(
                "{appearance} presentation revision did not match the resolved source revision"
            ));
        }
    }
    Ok(())
}

fn invalid_result<T>(message: impl Into<String>) -> Result<T, WorldViewResolutionError> {
    Err(WorldViewResolutionError::InvalidResult(message.into()))
}

fn authority_readback(
    reference: &WorldViewReference,
    result: &WorldViewDumpResult,
) -> Result<
    (WorldViewResolutionAuthority, WorldViewResolutionFreshness),
    WorldViewResolutionError,
> {
    match reference {
        WorldViewReference::HostedWorldViewExport { origin, .. } => Ok((
            WorldViewResolutionAuthority::HostedWorldViewExport {
                origin: origin.clone(),
            },
            WorldViewResolutionFreshness::Pinned,
        )),
        WorldViewReference::HostedWorldLiveViewShare { origin, .. } => {
            let hosted_world_id = result
                .hosted_world_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    WorldViewResolutionError::InvalidResult(
                        "hosted live-view resolution omitted `hostedWorldId`".into(),
                    )
                })?;
            Ok((
                WorldViewResolutionAuthority::HostedWorldLiveViewShare {
                    origin: origin.clone(),
                    hosted_world_id: hosted_world_id.to_owned(),
                },
                WorldViewResolutionFreshness::LatestAtResolution,
            ))
        }
        WorldViewReference::LocalWorldMirrorLatest { origin, mirror_id } => Ok((
            WorldViewResolutionAuthority::LocalWorldMirrorLatest {
                origin: origin.clone(),
                mirror_id: mirror_id.clone(),
            },
            WorldViewResolutionFreshness::LatestAtResolution,
        )),
        WorldViewReference::HostedWorldLatest {
            origin,
            hosted_world_id,
        } => Ok((
            WorldViewResolutionAuthority::HostedWorldLatest {
                origin: origin.clone(),
                hosted_world_id: hosted_world_id.clone(),
            },
            WorldViewResolutionFreshness::LatestAtResolution,
        )),
    }
}

struct WorldCliInvocation<'a> {
    args: Vec<String>,
    stdin: Option<&'a str>,
}

fn world_cli_invocation<'a>(
    binding: &'a WorldViewBinding,
    access: &WorldViewResolutionAccess,
) -> Result<WorldCliInvocation<'a>, WorldViewResolutionError> {
    let mut invocation = world_view_cli_invocation(&binding.reference, access, "dump")?;
    invocation.args.extend([
        "--realm".into(),
        binding.realm_qualified_name.clone(),
        "--view".into(),
        binding.view_qualified_name.clone(),
    ]);
    Ok(invocation)
}

fn world_view_cli_invocation<'a>(
    reference: &'a WorldViewReference,
    access: &WorldViewResolutionAccess,
    subcommand: &str,
) -> Result<WorldCliInvocation<'a>, WorldViewResolutionError> {
    let (args, stdin) = match reference {
        WorldViewReference::LocalWorldMirrorLatest { origin, mirror_id } => (
            vec![
                "hosted".into(),
                "view".into(),
                subcommand.into(),
                "--json".into(),
                "--base-url".into(),
                origin.clone(),
                "--local-mirror".into(),
                mirror_id.clone(),
            ],
            None,
        ),
        WorldViewReference::HostedWorldViewExport {
            origin,
            share_token,
        } => (
            vec![
                "hosted".into(),
                "view".into(),
                subcommand.into(),
                "--json".into(),
                "--base-url".into(),
                origin.clone(),
                "--share-token-stdin".into(),
            ],
            Some(share_token.as_str()),
        ),
        WorldViewReference::HostedWorldLiveViewShare {
            origin,
            share_token,
        } => (
            vec![
                "hosted".into(),
                "view".into(),
                subcommand.into(),
                "--json".into(),
                "--base-url".into(),
                origin.clone(),
                "--live-share-token-stdin".into(),
            ],
            Some(share_token.as_str()),
        ),
        WorldViewReference::HostedWorldLatest {
            origin,
            hosted_world_id,
        } => {
            let WorldViewResolutionAccess::HostedEditShareFile { credential_file } = access else {
                return Err(WorldViewResolutionError::MissingHostedAuthority {
                    hosted_world_id: hosted_world_id.clone(),
                });
            };
            (
                vec![
                    "hosted".into(),
                    "view".into(),
                    subcommand.into(),
                    "--json".into(),
                    "--base-url".into(),
                    origin.clone(),
                    "--edit-share-file".into(),
                    credential_file.to_string_lossy().into_owned(),
                    "--anonymous-session".into(),
                ],
                None,
            )
        }
    };
    Ok(WorldCliInvocation { args, stdin })
}

fn redact_diagnostics(diagnostics: &str, reference: &WorldViewReference) -> String {
    match reference {
        WorldViewReference::HostedWorldViewExport { share_token, .. } => {
            diagnostics.replace(share_token, "<redacted>")
        }
        WorldViewReference::HostedWorldLiveViewShare { share_token, .. } => {
            diagnostics.replace(share_token, "<redacted>")
        }
        WorldViewReference::LocalWorldMirrorLatest { .. } => diagnostics.to_owned(),
        WorldViewReference::HostedWorldLatest { .. } => diagnostics.to_owned(),
    }
}

fn next_command(request: &WorldViewResolutionRequest) -> String {
    let mut command = format!("buzz world-views resolve --channel {}", request.channel_id);
    if let Some(thread_root_event_id) = request.declared_scope.thread_root_event_id() {
        command.push_str(" --thread-root ");
        command.push_str(thread_root_event_id);
    }
    command.push_str(" --binding ");
    command.push_str(&request.binding.id.to_string());
    command
}

fn validate_event_id(field: &str, value: &str) -> Result<(), WorldViewResolutionError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(WorldViewResolutionError::InvalidRequest(format!(
            "{field} must be 64 lowercase hex characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::world_view::WorldViewDisplayMode;
    use chrono::TimeZone;
    use serde_json::json;

    fn request(reference: WorldViewReference) -> WorldViewResolutionRequest {
        WorldViewResolutionRequest {
            channel_id: Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
            binding: WorldViewBinding {
                id: Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
                label: Some("Launch board".into()),
                reference,
                realm_qualified_name: "world::main".into(),
                view_qualified_name: "world::main::@Board".into(),
                display_mode: WorldViewDisplayMode::Graph,
            },
            declared_scope: WorldViewBindingScope::Channel,
            effective_scope: WorldViewBindingScope::Channel,
            binding_revision_event_id: "b".repeat(64),
        }
    }

    fn presentation(revision: &str) -> serde_json::Value {
        let graph = json!({
            "kind": "ready",
            "graphBackgroundHex": "#111113",
            "graphPattern": "dots",
            "clusters": [],
            "nodes": [{
                "id": "world::main::Ship",
                "label": "Ship",
                "preferenceQualifiedName": "world::main::Ship",
                "status": "ready",
                "targetState": null,
                "isReady": true,
                "isLeaf": true,
                "signalCases": [{
                    "caseName": "implementing",
                    "evidence": [{
                        "kind": "typedForm",
                        "appearance": null,
                        "formQualifiedName": "coordination::AgentAssignment",
                        "matchedEntries": [],
                        "value": {
                            "kind": "object",
                            "entries": [{
                                "key": "state",
                                "value": {
                                    "kind": "string",
                                    "value": "active"
                                }
                            }]
                        }
                    }],
                    "signalName": "CodexThread",
                    "meanings": [
                        { "kind": "targetState", "state": "implementing" },
                        { "kind": "codexThread" }
                    ]
                }],
                "signalCaseNames": ["implementing"],
                "fillHex": "#1c2024",
                "borderHex": "#3e63dd",
                "textHex": "#f0f0f3",
                "deemphasis": null,
                "effect": null,
                "position": { "x": 150, "y": 57.5 },
                "size": { "width": 300, "height": 115 }
            }],
            "edges": [],
            "bounds": { "width": 420, "height": 235 }
        });
        let model = json!({
            "graph": graph,
            "revision": revision,
            "selection": {
                "realmQualifiedName": "world::main",
                "viewQualifiedName": "world::main::@Board"
            }
        });
        json!({ "formatVersion": 1, "dark": model, "light": model })
    }

    fn success_stdout(revision: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "ok": true,
            "result": {
                "revision": revision,
                "realm": { "name": "main", "qualifiedName": "world::main" },
                "view": {
                    "name": "Board",
                    "qualifiedName": "world::main::@Board",
                    "slots": {},
                    "flowspaces": [],
                    "lenses": []
                },
                "counts": {
                    "nodes": 1,
                    "edges": 0,
                    "ready": 1,
                    "actionableReady": 1,
                    "satisfied": 0,
                    "blocked": 0
                },
                "presentation": presentation(revision),
                "nodes": [{
                    "preference": "Ship",
                    "qualifiedName": "world::main::Ship",
                    "status": "ready",
                    "actionable": true,
                    "leaf": true,
                    "inFocus": false,
                    "inSatisfied": false,
                    "blockers": [],
                    "enablers": [],
                    "note": { "preview": null, "truncated": false },
                    "signals": []
                }],
                "readyLeaves": [{
                    "preference": "Ship",
                    "qualifiedName": "world::main::Ship",
                    "status": "ready",
                    "actionable": true,
                    "leaf": true,
                    "inFocus": false,
                    "inSatisfied": false,
                    "blockers": [],
                    "enablers": [],
                    "note": { "preview": null, "truncated": false },
                    "signals": []
                }],
                "satisfiedNodes": [],
                "blockedNodes": [],
                "edges": []
            }
        }))
        .unwrap()
    }

    #[test]
    fn decodes_canonical_view_catalog_identities() {
        let stdout = serde_json::to_vec(&json!({
            "ok": true,
            "result": {
                "command": "view.catalog",
                "formatVersion": 1,
                "revision": "source-revision-1",
                "root": "hosted-local-mirror:mirror-1",
                "worldQualifiedName": "world",
                "views": [{
                    "name": "@Board",
                    "qualifiedName": "@main::Board",
                    "realm": {
                        "name": "main",
                        "qualifiedName": "world::main"
                    }
                }]
            },
            "diagnostics": []
        }))
        .unwrap();

        let catalog = decode_world_view_catalog(&stdout).unwrap();

        assert_eq!(catalog.world_qualified_name, "world");
        assert_eq!(catalog.views[0].qualified_name, "@main::Board");
        assert_eq!(catalog.views[0].realm.qualified_name, "world::main");
    }

    #[test]
    fn catalog_routes_export_capability_over_stdin_without_a_selection() {
        let reference = WorldViewReference::HostedWorldViewExport {
            origin: "https://manifest.shivai.space".into(),
            share_token: "secret-view-token".into(),
        };

        let invocation =
            world_view_cli_invocation(&reference, &WorldViewResolutionAccess::None, "catalog")
                .unwrap();

        assert_eq!(&invocation.args[..3], ["hosted", "view", "catalog"]);
        assert!(invocation
            .args
            .iter()
            .any(|argument| argument == "--share-token-stdin"));
        assert!(!invocation.args.iter().any(|argument| argument == "--realm"));
        assert_eq!(invocation.stdin, Some("secret-view-token"));
    }

    #[test]
    fn decodes_one_typed_resolution_and_omits_the_hosted_token() {
        let request = request(WorldViewReference::HostedWorldViewExport {
            origin: "https://manifest.shivai.space".into(),
            share_token: "secret-view-token".into(),
        });
        let resolved_at = Utc.with_ymd_and_hms(2026, 7, 24, 12, 0, 0).unwrap();
        let resolved = decode_world_view_resolution(
            &request,
            &success_stdout("source-revision-1"),
            resolved_at,
        )
        .unwrap();

        assert_eq!(resolved.binding_id, request.binding.id);
        assert_eq!(resolved.source_revision, "source-revision-1");
        assert_eq!(resolved.view_dump.counts.nodes, 1);
        assert_eq!(resolved.freshness, WorldViewResolutionFreshness::Pinned);
        let encoded = serde_json::to_string(&resolved).unwrap();
        assert!(!encoded.contains("secret-view-token"));
        assert!(encoded.contains("buzz world-views resolve"));
    }

    #[test]
    fn rejects_a_selection_that_does_not_match_the_binding() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&success_stdout("source-revision-1")).unwrap();
        value["result"]["view"]["qualifiedName"] = json!("world::main::@Wrong");
        let error = decode_world_view_resolution(
            &request(WorldViewReference::LocalWorldMirrorLatest {
                origin: "https://manifest.shivai.space".into(),
                mirror_id: "mirror-1".into(),
            }),
            &serde_json::to_vec(&value).unwrap(),
            Utc::now(),
        )
        .expect_err("mismatched view must fail");

        assert!(error.to_string().contains("did not match requested"));
    }

    #[test]
    fn redacts_hosted_tokens_from_command_diagnostics() {
        let reference = WorldViewReference::HostedWorldViewExport {
            origin: "https://manifest.shivai.space".into(),
            share_token: "secret-view-token".into(),
        };
        assert_eq!(
            redact_diagnostics("failed secret-view-token", &reference),
            "failed <redacted>"
        );
    }

    #[test]
    fn routes_hosted_export_capability_over_stdin_not_process_arguments() {
        let request = request(WorldViewReference::HostedWorldViewExport {
            origin: "https://manifest.shivai.space".into(),
            share_token: "secret-view-token".into(),
        });

        let invocation =
            world_cli_invocation(&request.binding, &WorldViewResolutionAccess::None).unwrap();

        assert!(invocation
            .args
            .iter()
            .any(|argument| argument == "--share-token-stdin"));
        assert!(!invocation.args.join(" ").contains("secret-view-token"));
        assert_eq!(invocation.stdin, Some("secret-view-token"));
    }

    #[test]
    fn routes_hosted_mutation_authority_through_a_credential_file() {
        let request = request(WorldViewReference::HostedWorldLatest {
            origin: "https://manifest.shivai.space".into(),
            hosted_world_id: "hosted-1".into(),
        });
        let access = WorldViewResolutionAccess::HostedEditShareFile {
            credential_file: PathBuf::from("/private/edit-share.txt"),
        };

        let invocation = world_cli_invocation(&request.binding, &access).unwrap();

        assert!(invocation
            .args
            .windows(2)
            .any(|pair| { pair == ["--edit-share-file", "/private/edit-share.txt"] }));
        assert!(invocation
            .args
            .iter()
            .any(|argument| argument == "--anonymous-session"));
        assert_eq!(invocation.stdin, None);
        let envelope: WorldResultEnvelope<WorldViewDumpResult> =
            serde_json::from_slice(&success_stdout("source-revision-1")).unwrap();
        let result = envelope.result.unwrap();
        assert_eq!(
            authority_readback(&request.binding.reference, &result).unwrap(),
            (
                WorldViewResolutionAuthority::HostedWorldLatest {
                    origin: "https://manifest.shivai.space".into(),
                    hosted_world_id: "hosted-1".into(),
                },
                WorldViewResolutionFreshness::LatestAtResolution,
            )
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn writes_hosted_export_capability_to_the_child_stdin_pipe() {
        use std::os::unix::fs::PermissionsExt;

        let temp_root =
            std::env::temp_dir().join(format!("buzz-world-view-resolver-{}", Uuid::new_v4()));
        std::fs::create_dir(&temp_root).expect("create temp resolver root");
        let output_path = temp_root.join("world-output.json");
        std::fs::write(&output_path, success_stdout("source-revision-1"))
            .expect("write fake world output");
        let binary_path = temp_root.join("world");
        std::fs::write(
            &binary_path,
            format!(
                "#!/bin/sh\n\
                 token=$(cat)\n\
                 test \"$token\" = \"secret-view-token\" || exit 41\n\
                 cat '{}'\n",
                output_path.display()
            ),
        )
        .expect("write fake world binary");
        let mut permissions = std::fs::metadata(&binary_path)
            .expect("read fake world metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&binary_path, permissions).expect("make fake world executable");

        let resolved = resolve_world_view_with_binary(
            request(WorldViewReference::HostedWorldViewExport {
                origin: "https://manifest.shivai.space".into(),
                share_token: "secret-view-token".into(),
            }),
            &binary_path,
        )
        .await
        .expect("resolve through stdin-aware child");

        assert_eq!(resolved.source_revision, "source-revision-1");
        std::fs::remove_dir_all(temp_root).expect("remove temp resolver root");
    }
}
