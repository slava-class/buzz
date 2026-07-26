use buzz_core_pkg::{
    kind::KIND_WORLD_VIEW_BINDINGS,
    verification::verify_event,
    world_view::{
        effective_world_view_bindings, world_view_bindings_snapshot_from_verified_event,
        WorldViewBindingScope, WorldViewBindingsDocument, WorldViewBindingsSnapshot,
    },
};
use buzz_world_view_resolver_pkg::{
    resolve_world_view as resolve_typed_world_view, ResolvedWorldView, WorldViewResolutionRequest,
};
use tauri::State;

use crate::{
    app_state::AppState,
    relay::{query_relay, submit_event},
};

fn exact_scope(thread_root_event_id: Option<String>) -> Result<WorldViewBindingScope, String> {
    thread_root_event_id.map_or(Ok(WorldViewBindingScope::Channel), |event_id| {
        WorldViewBindingScope::thread(event_id)
    })
}

fn read_command(channel_id: &str, scope: &WorldViewBindingScope) -> String {
    let mut command = format!("buzz world-views get --channel {channel_id}");
    if let Some(thread_root_event_id) = scope.thread_root_event_id() {
        command.push_str(" --thread-root ");
        command.push_str(thread_root_event_id);
    }
    command
}

async fn query_world_view_bindings_snapshot(
    channel_id: &str,
    scope: &WorldViewBindingScope,
    state: &AppState,
) -> Result<WorldViewBindingsSnapshot, String> {
    let d_tag = scope.d_tag();
    let events = query_relay(
        state,
        &[serde_json::json!({
            "kinds": [KIND_WORLD_VIEW_BINDINGS],
            "#h": [channel_id],
            "#d": [d_tag],
            "limit": 1
        })],
    )
    .await?;

    let Some(event) = events.into_iter().next() else {
        return Ok(WorldViewBindingsSnapshot::empty(scope.clone()));
    };
    let expected_channel_id = uuid::Uuid::parse_str(channel_id)
        .map_err(|_| format!("invalid channel UUID: {channel_id}"))?;
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
    .map_err(|error| format!("world view bindings verification task failed: {error}"))?
}

/// Read one exact channel or thread-root Shivai world-view bindings document.
#[tauri::command]
pub async fn get_world_view_bindings(
    channel_id: String,
    thread_root_event_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let uuid = uuid::Uuid::parse_str(&channel_id)
        .map_err(|_| format!("invalid channel UUID: {channel_id}"))?;
    let scope = exact_scope(thread_root_event_id)?;
    let snapshot = query_world_view_bindings_snapshot(&uuid.to_string(), &scope, &state).await?;
    let mut value = serde_json::to_value(snapshot)
        .map_err(|error| format!("encode world view bindings snapshot: {error}"))?;
    value
        .as_object_mut()
        .expect("snapshot serializes as an object")
        .insert(
            "nextReadCommand".into(),
            serde_json::Value::String(read_command(&channel_id, &scope)),
        );
    Ok(value)
}

/// Read effective channel bindings with exact thread-root shadowing when requested.
#[tauri::command]
pub async fn get_effective_world_view_bindings(
    channel_id: String,
    thread_root_event_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let uuid = uuid::Uuid::parse_str(&channel_id)
        .map_err(|_| format!("invalid channel UUID: {channel_id}"))?;
    let effective_scope = exact_scope(thread_root_event_id)?;
    let channel_scope = WorldViewBindingScope::Channel;
    let channel =
        query_world_view_bindings_snapshot(&uuid.to_string(), &channel_scope, &state).await?;
    let thread = match &effective_scope {
        WorldViewBindingScope::Channel => None,
        WorldViewBindingScope::Thread { .. } => Some(
            query_world_view_bindings_snapshot(&uuid.to_string(), &effective_scope, &state).await?,
        ),
    };
    let effective = effective_world_view_bindings(&channel, thread.as_ref())?;
    let mut value = serde_json::to_value(effective)
        .map_err(|error| format!("encode effective world view bindings: {error}"))?;
    let mut next_read_commands = vec![read_command(&channel_id, &channel_scope)];
    if matches!(effective_scope, WorldViewBindingScope::Thread { .. }) {
        next_read_commands.push(read_command(&channel_id, &effective_scope));
    }
    value
        .as_object_mut()
        .expect("effective bindings serialize as an object")
        .insert(
            "nextReadCommands".into(),
            serde_json::to_value(next_read_commands).expect("read commands serialize as strings"),
        );
    Ok(value)
}

/// Publish an optimistic complete replacement for one exact binding scope.
#[tauri::command]
pub async fn set_world_view_bindings(
    channel_id: String,
    expected_revision_event_id: Option<String>,
    document: WorldViewBindingsDocument,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let uuid = uuid::Uuid::parse_str(&channel_id)
        .map_err(|_| format!("invalid channel UUID: {channel_id}"))?;
    document.validate()?;
    if let Some(event_id) = &expected_revision_event_id {
        if event_id.len() != 64
            || !event_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("expected revision event id must be 64 lowercase hex characters".into());
        }
    }
    let current = query_world_view_bindings_snapshot(&channel_id, &document.scope, &state).await?;
    if current.revision_event_id != expected_revision_event_id {
        return Err(format!(
            "world-view bindings revision conflict: expected {}, current {}; refresh with `{}`",
            expected_revision_event_id.as_deref().unwrap_or("none"),
            current.revision_event_id.as_deref().unwrap_or("none"),
            read_command(&channel_id, &document.scope)
        ));
    }

    let builder = buzz_sdk_pkg::build_set_world_view_bindings(
        uuid,
        expected_revision_event_id.as_deref(),
        &document,
    )
    .map_err(|error| error.to_string())?;
    let result = submit_event(builder, &state).await?;

    Ok(serde_json::json!({
        "ok": true,
        "revisionEventId": result.event_id,
        "nextReadCommand": read_command(&channel_id, &document.scope),
    }))
}

/// Resolve one bound local or hosted world view through the shared typed resolver.
#[tauri::command]
pub async fn resolve_world_view(
    request: WorldViewResolutionRequest,
) -> Result<ResolvedWorldView, String> {
    resolve_typed_world_view(request)
        .await
        .map_err(|error| error.to_string())
}
