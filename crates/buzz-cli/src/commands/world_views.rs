use buzz_core::kind::KIND_WORLD_VIEW_BINDINGS;
use buzz_core::verification::verify_event;
use buzz_core::world_view::{
    effective_world_view_bindings, world_view_bindings_snapshot_from_verified_event,
    EffectiveWorldViewBinding, WorldViewBinding, WorldViewBindingScope, WorldViewBindingsDocument,
    WorldViewBindingsSnapshot,
};
use buzz_world_view_resolver::{resolve_world_view, WorldViewResolutionRequest};
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
    let resolved = resolve_world_view(WorldViewResolutionRequest {
        channel_id: parse_uuid(channel_id)?,
        binding: selected.binding.clone(),
        declared_scope: selected.declared_scope.clone(),
        effective_scope,
        binding_revision_event_id: selected.binding_revision_event_id.clone(),
    })
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
