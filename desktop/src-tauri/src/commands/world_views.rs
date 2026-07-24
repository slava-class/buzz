use std::{ffi::OsString, process::Command};

use buzz_core_pkg::{
    kind::KIND_WORLD_VIEW_BINDINGS,
    world_view::{
        WorldViewBinding, WorldViewBindingsDocument, WorldViewReference,
        WORLD_VIEW_BINDINGS_VERSION,
    },
};
use tauri::State;

use crate::{
    app_state::AppState,
    relay::{query_relay, submit_event},
};

/// Read the latest channel-scoped Shivai world view bindings document.
#[tauri::command]
pub async fn get_world_view_bindings(
    channel_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let events = query_relay(
        &state,
        &[serde_json::json!({
            "kinds": [KIND_WORLD_VIEW_BINDINGS],
            "#h": [channel_id],
            "limit": 1
        })],
    )
    .await?;

    let Some(event) = events.first() else {
        return Ok(serde_json::json!({
            "document": {
                "version": WORLD_VIEW_BINDINGS_VERSION,
                "bindings": [],
            },
            "event_id": null,
            "updated_at": null,
            "author": null,
        }));
    };

    let document: WorldViewBindingsDocument = serde_json::from_str(&event.content)
        .map_err(|error| format!("invalid world view bindings event content: {error}"))?;
    document.validate()?;

    Ok(serde_json::json!({
        "document": document,
        "event_id": event.id.to_hex(),
        "updated_at": event.created_at.as_secs(),
        "author": event.pubkey.to_hex(),
    }))
}

/// Publish a complete replacement for a channel's ordered world view bindings.
#[tauri::command]
pub async fn set_world_view_bindings(
    channel_id: String,
    document: WorldViewBindingsDocument,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let uuid = uuid::Uuid::parse_str(&channel_id)
        .map_err(|_| format!("invalid channel UUID: {channel_id}"))?;
    document.validate()?;
    let builder = buzz_sdk_pkg::build_set_world_view_bindings(uuid, &document)
        .map_err(|error| error.to_string())?;
    let result = submit_event(builder, &state).await?;

    Ok(serde_json::json!({
        "ok": true,
        "event_id": result.event_id,
    }))
}

/// Resolve one bound local or hosted world view through the canonical `world` CLI.
#[tauri::command]
pub async fn resolve_world_view(binding: WorldViewBinding) -> Result<serde_json::Value, String> {
    WorldViewBindingsDocument {
        version: WORLD_VIEW_BINDINGS_VERSION,
        bindings: vec![binding.clone()],
    }
    .validate()?;

    let binary = std::env::var_os("SHIVAI_WORLD_BIN").unwrap_or_else(|| OsString::from("world"));
    let sensitive_token = match &binding.reference {
        WorldViewReference::HostedWorldViewExport { share_token, .. } => Some(share_token.clone()),
        WorldViewReference::LocalWorldMirrorLatest { .. } => None,
    };
    let args = binding.world_cli_args();

    let output =
        tauri::async_runtime::spawn_blocking(move || Command::new(binary).args(args).output())
            .await
            .map_err(|error| format!("world view resolver task failed: {error}"))?
            .map_err(|error| format!("could not launch the Shivai world resolver: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let diagnostics = sensitive_token
            .as_ref()
            .map_or(stderr.clone(), |token| stderr.replace(token, "<redacted>"));
        return Err(if diagnostics.is_empty() {
            "Shivai world view resolution failed without diagnostics".into()
        } else {
            diagnostics
        });
    }

    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Shivai world resolver returned invalid JSON: {error}"))?;
    if envelope.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err("Shivai world resolver returned a non-success envelope".into());
    }
    let result = envelope
        .get("result")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Shivai world resolver omitted its result".to_string())?;
    let presentation = result
        .get("presentation")
        .cloned()
        .ok_or_else(|| "Shivai world resolver omitted normalized presentation data".to_string())?;
    let revision = result
        .get("revision")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Shivai world resolver omitted its revision".to_string())?;
    let realm = result
        .get("realm")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Shivai world resolver omitted its realm readback".to_string())?;
    let view = result
        .get("view")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Shivai world resolver omitted its view readback".to_string())?;

    Ok(serde_json::json!({
        "binding_id": binding.id,
        "presentation": presentation,
        "resolved_at": chrono::Utc::now().to_rfc3339(),
        "revision": revision,
        "realm": realm,
        "view": view,
    }))
}
