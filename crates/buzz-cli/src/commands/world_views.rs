use buzz_core::kind::KIND_WORLD_VIEW_BINDINGS;
use buzz_core::verification::verify_event;
use buzz_core::world_view::{
    effective_world_view_bindings, world_view_bindings_snapshot_from_verified_event,
    EffectiveWorldViewBinding, WorldAuthorityRegistry, WorldViewBinding, WorldViewBindingScope,
    WorldViewBindingsDocument, WorldViewBindingsSnapshot, WorldViewDisplayMode,
    WorldViewReference, WORLD_AUTHORITY_REGISTRY_FILE_NAME, WORLD_VIEW_BINDINGS_VERSION,
};
use buzz_world_view_resolver::{
    catalog_world_views_with_access, publish_hosted_live_view_share,
    resolve_world_view_with_access, PublishedHostedLiveViewShare, WorldViewCatalog,
    WorldViewResolutionAccess, WorldViewResolutionRequest,
};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::validate::{parse_uuid, read_or_stdin};
use crate::{CliError, WorldViewsCmd};

fn exact_scope(thread_root: Option<&str>) -> Result<WorldViewBindingScope, CliError> {
    thread_root.map_or(Ok(WorldViewBindingScope::Channel), |event_id| {
        WorldViewBindingScope::thread(event_id)
            .map_err(|error| CliError::Usage(format!("invalid thread scope: {error}")))
    })
}

async fn fetch_snapshot(
    client: &BuzzClient,
    channel_id: &str,
    scope: &WorldViewBindingScope,
) -> Result<WorldViewBindingsSnapshot, CliError> {
    let expected_channel_id = parse_uuid(channel_id)?;
    let d_tag = scope.d_tag();
    let filter = serde_json::json!({
        "kinds": [KIND_WORLD_VIEW_BINDINGS],
        "#h": [channel_id],
        "#d": [d_tag],
        "limit": 1
    });
    let response = client.query(&filter).await?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("decode world view bindings query: {error}")))?;
    let Some(event_value) = events.into_iter().next() else {
        return Ok(WorldViewBindingsSnapshot::empty(scope.clone()));
    };
    let event: nostr::Event = serde_json::from_value(event_value)
        .map_err(|error| CliError::Other(format!("decode world view bindings event: {error}")))?;
    let expected_scope = scope.clone();
    tokio::task::spawn_blocking(move || {
        verify_event(&event)
            .map_err(|error| format!("verify world view bindings event: {error}"))?;
        world_view_bindings_snapshot_from_verified_event(
            &event,
            expected_channel_id,
            &expected_scope,
        )
        .map_err(|error| format!("decode world view bindings event: {error}"))
    })
    .await
    .map_err(|error| {
        CliError::Other(format!(
            "world view bindings verification task failed: {error}"
        ))
    })?
    .map_err(CliError::Other)
}

fn read_command(channel_id: &str, scope: &WorldViewBindingScope) -> String {
    let mut command = format!("buzz world-views get --channel {channel_id}");
    if let Some(thread_root_event_id) = scope.thread_root_event_id() {
        command.push_str(" --thread-root ");
        command.push_str(thread_root_event_id);
    }
    command
}

async fn cmd_get(
    client: &BuzzClient,
    channel_id: &str,
    thread_root: Option<&str>,
) -> Result<(), CliError> {
    let scope = exact_scope(thread_root)?;
    let snapshot = fetch_snapshot(client, channel_id, &scope).await?;
    let expected_revision = snapshot.revision_event_id.as_deref().unwrap_or("none");
    let next_set_command = format!(
        "buzz world-views set --channel {channel_id} \
         --expected-revision {expected_revision} --document -"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "document": snapshot.document,
            "revisionEventId": snapshot.revision_event_id,
            "updatedAt": snapshot.updated_at,
            "author": snapshot.author,
            "nextReadCommand": read_command(channel_id, &scope),
            "nextSetCommand": next_set_command,
        }))
        .map_err(|error| CliError::Other(format!("encode world view bindings: {error}")))?
    );
    Ok(())
}

fn parse_expected_revision(value: &str) -> Result<Option<String>, CliError> {
    if value == "none" {
        return Ok(None);
    }
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CliError::Usage(
            "--expected-revision must be `none` or 64 lowercase hex characters".into(),
        ));
    }
    Ok(Some(value.into()))
}

async fn cmd_set(
    client: &BuzzClient,
    channel_id: &str,
    document_json: &str,
    expected_revision: &str,
) -> Result<(), CliError> {
    let channel_uuid = parse_uuid(channel_id)?;
    let document_text = read_or_stdin(document_json)?;
    let document: WorldViewBindingsDocument = serde_json::from_str(&document_text)
        .map_err(|error| CliError::Usage(format!("invalid world view bindings JSON: {error}")))?;
    document
        .validate()
        .map_err(|error| CliError::Usage(format!("invalid world view bindings: {error}")))?;
    let expected_revision = parse_expected_revision(expected_revision)?;
    let current = fetch_snapshot(client, channel_id, &document.scope).await?;
    if current.revision_event_id != expected_revision {
        return Err(CliError::Other(format!(
            "world-view bindings revision conflict: expected {}, current {}; refresh with `{}`",
            expected_revision.as_deref().unwrap_or("none"),
            current.revision_event_id.as_deref().unwrap_or("none"),
            read_command(channel_id, &document.scope)
        )));
    }
    let builder = buzz_sdk::build_set_world_view_bindings(
        channel_uuid,
        expected_revision.as_deref(),
        &document,
    )
    .map_err(|error| CliError::Other(format!("build world view bindings event: {error}")))?;
    let event = client.sign_event(builder)?;
    let response = client.submit_event(event).await?;
    println!("{response}");
    Ok(())
}

fn load_world_authority_registry() -> Result<WorldAuthorityRegistry, CliError> {
    let cwd = std::env::current_dir()
        .map_err(|error| CliError::Other(format!("resolve current directory: {error}")))?;
    let path = cwd.join(WORLD_AUTHORITY_REGISTRY_FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorldAuthorityRegistry::default())
        }
        Err(error) => {
            return Err(CliError::Other(format!(
                "read world authority registry {}: {error}",
                path.display()
            )))
        }
    };
    let registry: WorldAuthorityRegistry = serde_json::from_str(&text).map_err(|error| {
        CliError::Other(format!(
            "decode world authority registry {}: {error}",
            path.display()
        ))
    })?;
    registry
        .validate()
        .map_err(|error| CliError::Other(format!("invalid world authority registry: {error}")))?;
    Ok(registry)
}

#[derive(Debug, Clone)]
enum ConnectedWorldSource {
    Local {
        reference: WorldViewReference,
    },
    Hosted {
        credential_file: String,
        reference: WorldViewReference,
    },
}

impl ConnectedWorldSource {
    fn reference(&self) -> &WorldViewReference {
        match self {
            Self::Local { reference } | Self::Hosted { reference, .. } => reference,
        }
    }

    fn access(&self) -> WorldViewResolutionAccess {
        match self {
            Self::Local { .. } => WorldViewResolutionAccess::None,
            Self::Hosted {
                credential_file, ..
            } => WorldViewResolutionAccess::HostedEditShareFile {
                credential_file: credential_file.into(),
            },
        }
    }
}

fn connected_world_source(
    registry: &WorldAuthorityRegistry,
    source: &str,
    origin: &str,
) -> Result<ConnectedWorldSource, CliError> {
    if let Some(hosted_world_id) = source.strip_prefix("hosted:") {
        let authority = registry
            .resolve_hosted(origin, hosted_world_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "unknown connected world source `{source}` at `{origin}`; refresh with `buzz world-views sources`"
                ))
            })?;
        return Ok(ConnectedWorldSource::Hosted {
            credential_file: authority.credential_file.clone(),
            reference: WorldViewReference::HostedWorldLatest {
                origin: authority.origin.clone(),
                hosted_world_id: authority.hosted_world_id.clone(),
            },
        });
    }
    if let Some(mirror_id) = source.strip_prefix("local:") {
        let authority = registry.resolve_local(origin, mirror_id).ok_or_else(|| {
            CliError::Usage(format!(
                "unknown connected world source `{source}` at `{origin}`; refresh with `buzz world-views sources`"
            ))
        })?;
        return Ok(ConnectedWorldSource::Local {
            reference: WorldViewReference::LocalWorldMirrorLatest {
                origin: authority.origin.clone(),
                mirror_id: authority.mirror_id.clone(),
            },
        });
    }
    Err(CliError::Usage(
        "--source must be an exact `hosted:<id>` or `local:<id>` from `buzz world-views sources`"
            .into(),
    ))
}

async fn catalog_connected_world_source(
    source: &ConnectedWorldSource,
) -> Result<WorldViewCatalog, CliError> {
    catalog_world_views_with_access(source.reference().clone(), source.access())
        .await
        .map_err(|error| CliError::Other(error.to_string()))
}

fn cmd_sources() -> Result<(), CliError> {
    let registry = load_world_authority_registry()?;
    let local = registry.local_authorities.iter().map(|authority| {
        let source = format!("local:{}", authority.mirror_id);
        serde_json::json!({
            "source": source,
            "kind": "local-world-mirror-latest",
            "origin": authority.origin,
            "mirrorId": authority.mirror_id,
            "nextCatalogCommandArgs": [
                "world-views", "catalog", "--source", source, "--origin", authority.origin
            ],
        })
    });
    let hosted = registry.hosted_authorities.iter().map(|authority| {
        let source = format!("hosted:{}", authority.hosted_world_id);
        serde_json::json!({
            "source": source,
            "kind": "hosted-world-latest",
            "origin": authority.origin,
            "hostedWorldId": authority.hosted_world_id,
            "nextCatalogCommandArgs": [
                "world-views", "catalog", "--source", source, "--origin", authority.origin
            ],
        })
    });
    let sources = local.chain(hosted).collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "sources": sources,
            "nextCommand": sources.first().map(|source| source["nextCatalogCommandArgs"].clone()),
        }))
        .map_err(|error| CliError::Other(format!("encode connected world sources: {error}")))?
    );
    Ok(())
}

async fn cmd_catalog(source: &str, origin: &str) -> Result<(), CliError> {
    let registry = load_world_authority_registry()?;
    let source = connected_world_source(&registry, source, origin)?;
    let catalog = catalog_connected_world_source(&source).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&catalog)
            .map_err(|error| CliError::Other(format!("encode world view catalog: {error}")))?
    );
    Ok(())
}

fn parse_display_mode(value: &str) -> Result<WorldViewDisplayMode, CliError> {
    match value {
        "graph" => Ok(WorldViewDisplayMode::Graph),
        "tasks" => Ok(WorldViewDisplayMode::Tasks),
        _ => Err(CliError::Usage(
            "--display must be either `graph` or `tasks`".into(),
        )),
    }
}

async fn publish_connected_world_reference(
    source: &ConnectedWorldSource,
    view_qualified_name: &str,
) -> Result<(WorldViewReference, Option<PublishedHostedLiveViewShare>), CliError> {
    match source {
        ConnectedWorldSource::Local { reference } => Ok((reference.clone(), None)),
        ConnectedWorldSource::Hosted {
            credential_file,
            reference:
                WorldViewReference::HostedWorldLatest {
                    origin,
                    hosted_world_id,
                },
        } => {
            let share =
                publish_hosted_live_view_share(origin, credential_file, view_qualified_name)
                    .await
                    .map_err(|error| CliError::Other(error.to_string()))?;
            if &share.hosted_world_id != hosted_world_id {
                return Err(CliError::Other(format!(
                    "live-view share resolved hosted world `{}` instead of `{hosted_world_id}`",
                    share.hosted_world_id
                )));
            }
            Ok((
                WorldViewReference::HostedWorldLiveViewShare {
                    origin: origin.clone(),
                    share_token: share.share_token.clone(),
                },
                Some(share),
            ))
        }
        ConnectedWorldSource::Hosted { .. } => Err(CliError::Other(
            "connected hosted source carried an invalid reference".into(),
        )),
    }
}

async fn cmd_bind(
    client: &BuzzClient,
    channel_id: &str,
    thread_root: Option<&str>,
    source_id: &str,
    origin: &str,
    view_qualified_name: &str,
    label: Option<&str>,
    display: &str,
    binding_id: Option<&str>,
    expected_revision: &str,
) -> Result<(), CliError> {
    let channel_uuid = parse_uuid(channel_id)?;
    let scope = exact_scope(thread_root)?;
    let expected_revision = parse_expected_revision(expected_revision)?;
    let current = fetch_snapshot(client, channel_id, &scope).await?;
    if current.revision_event_id != expected_revision {
        return Err(CliError::Other(format!(
            "world-view bindings revision conflict: expected {}, current {}; refresh with `{}`",
            expected_revision.as_deref().unwrap_or("none"),
            current.revision_event_id.as_deref().unwrap_or("none"),
            read_command(channel_id, &scope)
        )));
    }

    let registry = load_world_authority_registry()?;
    let source = connected_world_source(&registry, source_id, origin)?;
    let catalog = catalog_connected_world_source(&source).await?;
    let selected_view = catalog
        .views
        .iter()
        .find(|view| view.qualified_name == view_qualified_name)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "unknown view `{view_qualified_name}` for `{source_id}`; refresh with `buzz world-views catalog --source {source_id} --origin {origin}`"
            ))
        })?
        .clone();
    let (reference, live_share) =
        publish_connected_world_reference(&source, view_qualified_name).await?;
    if let Some(share) = &live_share {
        if share.realm_qualified_name != selected_view.realm.qualified_name {
            return Err(CliError::Other(format!(
                "live-view share resolved realm `{}` instead of catalog realm `{}`",
                share.realm_qualified_name, selected_view.realm.qualified_name
            )));
        }
    }

    let explicit_binding_id = binding_id
        .map(|value| {
            Uuid::parse_str(value)
                .map_err(|_| CliError::Usage(format!("invalid binding UUID: {value}")))
        })
        .transpose()?;
    if let Some(id) = explicit_binding_id {
        if !current
            .document
            .bindings
            .iter()
            .any(|binding| binding.id == id)
        {
            return Err(CliError::Usage(format!(
                "unknown world view binding `{id}`; refresh with `{}`",
                read_command(channel_id, &scope)
            )));
        }
    }
    let binding_id = explicit_binding_id
        .or_else(|| {
            current
                .document
                .bindings
                .iter()
                .find(|binding| {
                    binding.reference == reference
                        && binding.view_qualified_name == selected_view.qualified_name
                })
                .map(|binding| binding.id)
        })
        .unwrap_or_else(Uuid::new_v4);
    let label = label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let binding = WorldViewBinding {
        id: binding_id,
        label,
        reference,
        realm_qualified_name: selected_view.realm.qualified_name,
        view_qualified_name: selected_view.qualified_name,
        display_mode: parse_display_mode(display)?,
    };
    let replaces_binding = current
        .document
        .bindings
        .iter()
        .any(|candidate| candidate.id == binding_id);
    let bindings = if replaces_binding {
        current
            .document
            .bindings
            .into_iter()
            .map(|candidate| {
                if candidate.id == binding_id {
                    binding.clone()
                } else {
                    candidate
                }
            })
            .collect()
    } else {
        let mut bindings = current.document.bindings;
        bindings.push(binding.clone());
        bindings
    };
    let document = WorldViewBindingsDocument {
        version: WORLD_VIEW_BINDINGS_VERSION,
        scope: scope.clone(),
        bindings,
    };
    document
        .validate()
        .map_err(|error| CliError::Usage(format!("invalid world view binding: {error}")))?;
    let builder = buzz_sdk::build_set_world_view_bindings(
        channel_uuid,
        expected_revision.as_deref(),
        &document,
    )
    .map_err(|error| CliError::Other(format!("build world view bindings event: {error}")))?;
    let event = client.sign_event(builder)?;
    let revision_event_id = event.id.to_hex();
    let relay_response = client.submit_event(event).await?;
    let mut next_resolve_command =
        format!("buzz world-views resolve --channel {channel_id}");
    if let Some(thread_root_event_id) = scope.thread_root_event_id() {
        next_resolve_command.push_str(" --thread-root ");
        next_resolve_command.push_str(thread_root_event_id);
    }
    next_resolve_command.push_str(" --binding ");
    next_resolve_command.push_str(&binding_id.to_string());
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "binding": binding,
            "revisionEventId": revision_event_id,
            "sourceRevision": live_share
                .as_ref()
                .map(|share| share.source_revision.as_str())
                .unwrap_or(catalog.revision.as_str()),
            "nextReadCommand": read_command(channel_id, &scope),
            "nextResolveCommand": next_resolve_command,
            "relayResponse": serde_json::from_str::<serde_json::Value>(&relay_response)
                .unwrap_or_else(|_| serde_json::Value::String(relay_response)),
        }))
        .map_err(|error| CliError::Other(format!("encode bound world view: {error}")))?
    );
    Ok(())
}

async fn cmd_resolve(
    client: &BuzzClient,
    channel_id: &str,
    thread_root: Option<&str>,
    binding_id: Option<&str>,
) -> Result<(), CliError> {
    let effective_scope = exact_scope(thread_root)?;
    let channel = fetch_snapshot(client, channel_id, &WorldViewBindingScope::Channel).await?;
    let thread = match &effective_scope {
        WorldViewBindingScope::Channel => None,
        WorldViewBindingScope::Thread { .. } => {
            Some(fetch_snapshot(client, channel_id, &effective_scope).await?)
        }
    };
    let effective = effective_world_view_bindings(&channel, thread.as_ref())
        .map_err(|error| CliError::Other(format!("merge effective world views: {error}")))?;
    let selected = select_binding(&effective.bindings, binding_id)?;
    let registry = load_world_authority_registry()?;
    let access = match &selected.binding.reference {
        WorldViewReference::HostedWorldLatest {
            origin,
            hosted_world_id,
        } => registry
            .resolve_hosted(origin, hosted_world_id)
            .map(|authority| WorldViewResolutionAccess::HostedEditShareFile {
                credential_file: authority.credential_file.clone().into(),
            })
            .unwrap_or(WorldViewResolutionAccess::None),
        WorldViewReference::HostedWorldViewExport { .. }
        | WorldViewReference::HostedWorldLiveViewShare { .. }
        | WorldViewReference::LocalWorldMirrorLatest { .. } => WorldViewResolutionAccess::None,
    };
    let resolved = resolve_world_view_with_access(
        WorldViewResolutionRequest {
            channel_id: parse_uuid(channel_id)?,
            binding: selected.binding.clone(),
            declared_scope: selected.declared_scope.clone(),
            effective_scope,
            binding_revision_event_id: selected.binding_revision_event_id.clone(),
        },
        access,
    )
    .await
    .map_err(|error| CliError::Other(error.to_string()))?;

    println!(
        "{}",
        serde_json::to_string_pretty(&resolved).map_err(|error| CliError::Other(format!(
            "encode resolved Shivai world view: {error}"
        )))?
    );
    Ok(())
}

fn select_binding<'a>(
    bindings: &'a [EffectiveWorldViewBinding],
    binding_id: Option<&str>,
) -> Result<&'a EffectiveWorldViewBinding, CliError> {
    if let Some(binding_id) = binding_id {
        let id = Uuid::parse_str(binding_id)
            .map_err(|_| CliError::Usage(format!("invalid binding UUID: {binding_id}")))?;
        return bindings
            .iter()
            .find(|entry| entry.binding.id == id)
            .ok_or_else(|| CliError::Usage(format!("unknown world view binding: {binding_id}")));
    }
    match bindings {
        [binding] => Ok(binding),
        [] => Err(CliError::Usage(
            "channel has no world view bindings to resolve".into(),
        )),
        _ => Err(CliError::Usage(
            "channel has multiple world views; pass --binding <uuid>".into(),
        )),
    }
}

pub async fn dispatch(cmd: WorldViewsCmd, client: &BuzzClient) -> Result<(), CliError> {
    match cmd {
        WorldViewsCmd::Get {
            channel,
            thread_root,
        } => cmd_get(client, &channel, thread_root.as_deref()).await,
        WorldViewsCmd::Set {
            channel,
            document,
            expected_revision,
        } => cmd_set(client, &channel, &document, &expected_revision).await,
        WorldViewsCmd::Sources => cmd_sources(),
        WorldViewsCmd::Catalog { source, origin } => cmd_catalog(&source, &origin).await,
        WorldViewsCmd::Bind {
            channel,
            thread_root,
            source,
            origin,
            view,
            label,
            display,
            binding,
            expected_revision,
        } => {
            cmd_bind(
                client,
                &channel,
                thread_root.as_deref(),
                &source,
                &origin,
                &view,
                label.as_deref(),
                &display,
                binding.as_deref(),
                &expected_revision,
            )
            .await
        }
        WorldViewsCmd::Resolve {
            channel,
            thread_root,
            binding,
        } => cmd_resolve(client, &channel, thread_root.as_deref(), binding.as_deref()).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::world_view::{WorldViewDisplayMode, WorldViewReference};

    fn binding(id: Uuid) -> WorldViewBinding {
        WorldViewBinding {
            id,
            label: None,
            reference: WorldViewReference::LocalWorldMirrorLatest {
                origin: "https://manifest.shivai.space".into(),
                mirror_id: "mirror-1".into(),
            },
            realm_qualified_name: "world::main".into(),
            view_qualified_name: "world::main::@Board".into(),
            display_mode: WorldViewDisplayMode::Graph,
        }
    }

    fn effective(binding: WorldViewBinding) -> EffectiveWorldViewBinding {
        EffectiveWorldViewBinding {
            binding,
            declared_scope: WorldViewBindingScope::Channel,
            binding_revision_event_id: "a".repeat(64),
        }
    }

    #[test]
    fn requires_an_id_when_multiple_bindings_exist() {
        let bindings = [
            effective(binding(Uuid::nil())),
            effective(binding(Uuid::new_v4())),
        ];
        let error = select_binding(&bindings, None).expect_err("ambiguous");
        assert!(error.to_string().contains("--binding"));
    }

    #[test]
    fn selects_one_binding_without_extra_choreography() {
        let expected = effective(binding(Uuid::nil()));
        assert_eq!(
            select_binding(std::slice::from_ref(&expected), None).unwrap(),
            &expected
        );
    }
}
