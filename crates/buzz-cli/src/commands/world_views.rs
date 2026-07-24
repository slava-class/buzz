use std::ffi::OsString;

use buzz_core::kind::KIND_WORLD_VIEW_BINDINGS;
use buzz_core::world_view::{
    WorldViewBinding, WorldViewBindingsDocument, WorldViewReference, WORLD_VIEW_BINDINGS_VERSION,
};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::validate::{parse_uuid, read_or_stdin};
use crate::{CliError, WorldViewsCmd};

async fn fetch_document(
    client: &BuzzClient,
    channel_id: &str,
) -> Result<WorldViewBindingsDocument, CliError> {
    parse_uuid(channel_id)?;
    let filter = serde_json::json!({
        "kinds": [KIND_WORLD_VIEW_BINDINGS],
        "#h": [channel_id],
        "limit": 1
    });
    let response = client.query(&filter).await?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("decode world view bindings query: {error}")))?;
    let Some(event) = events.first() else {
        return Ok(WorldViewBindingsDocument {
            version: WORLD_VIEW_BINDINGS_VERSION,
            bindings: Vec::new(),
        });
    };
    let content = event
        .get("content")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CliError::Other("world view bindings event omitted content".into()))?;
    let document: WorldViewBindingsDocument = serde_json::from_str(content)
        .map_err(|error| CliError::Other(format!("decode world view bindings: {error}")))?;
    document
        .validate()
        .map_err(|error| CliError::Other(format!("invalid world view bindings: {error}")))?;
    Ok(document)
}

async fn cmd_get(client: &BuzzClient, channel_id: &str) -> Result<(), CliError> {
    let document = fetch_document(client, channel_id).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&document)
            .map_err(|error| CliError::Other(format!("encode world view bindings: {error}")))?
    );
    Ok(())
}

async fn cmd_set(
    client: &BuzzClient,
    channel_id: &str,
    document_json: &str,
) -> Result<(), CliError> {
    let channel_uuid = parse_uuid(channel_id)?;
    let document_text = read_or_stdin(document_json)?;
    let document: WorldViewBindingsDocument = serde_json::from_str(&document_text)
        .map_err(|error| CliError::Usage(format!("invalid world view bindings JSON: {error}")))?;
    document
        .validate()
        .map_err(|error| CliError::Usage(format!("invalid world view bindings: {error}")))?;
    let builder = buzz_sdk::build_set_world_view_bindings(channel_uuid, &document)
        .map_err(|error| CliError::Other(format!("build world view bindings event: {error}")))?;
    let event = client.sign_event(builder)?;
    let response = client.submit_event(event).await?;
    println!("{response}");
    Ok(())
}

async fn cmd_resolve(
    client: &BuzzClient,
    channel_id: &str,
    binding_id: Option<&str>,
) -> Result<(), CliError> {
    let document = fetch_document(client, channel_id).await?;
    let binding = select_binding(&document.bindings, binding_id)?;
    let binary = std::env::var_os("SHIVAI_WORLD_BIN").unwrap_or_else(|| OsString::from("world"));
    let output = tokio::process::Command::new(binary)
        .args(binding.world_cli_args())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| CliError::Other(format!("launch Shivai world resolver: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let diagnostics = match &binding.reference {
            WorldViewReference::HostedWorldViewExport { share_token, .. } => {
                stderr.replace(share_token, "<redacted>")
            }
            WorldViewReference::LocalWorldMirrorLatest { .. } => stderr,
        };
        return Err(CliError::Other(if diagnostics.is_empty() {
            "Shivai world view resolution failed without diagnostics".into()
        } else {
            diagnostics
        }));
    }

    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| CliError::Other(format!("decode Shivai world view result: {error}")))?;
    if envelope.get("ok").and_then(serde_json::Value::as_bool) != Some(true)
        || envelope
            .get("result")
            .and_then(|result| result.get("presentation"))
            .is_none()
    {
        return Err(CliError::Other(
            "Shivai world resolver omitted normalized presentation data".into(),
        ));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).map_err(|error| CliError::Other(format!(
            "encode Shivai world view result: {error}"
        )))?
    );
    Ok(())
}

fn select_binding<'a>(
    bindings: &'a [WorldViewBinding],
    binding_id: Option<&str>,
) -> Result<&'a WorldViewBinding, CliError> {
    if let Some(binding_id) = binding_id {
        let id = Uuid::parse_str(binding_id)
            .map_err(|_| CliError::Usage(format!("invalid binding UUID: {binding_id}")))?;
        return bindings
            .iter()
            .find(|binding| binding.id == id)
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
        WorldViewsCmd::Get { channel } => cmd_get(client, &channel).await,
        WorldViewsCmd::Set { channel, document } => cmd_set(client, &channel, &document).await,
        WorldViewsCmd::Resolve { channel, binding } => {
            cmd_resolve(client, &channel, binding.as_deref()).await
        }
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

    #[test]
    fn requires_an_id_when_multiple_bindings_exist() {
        let bindings = [binding(Uuid::nil()), binding(Uuid::new_v4())];
        let error = select_binding(&bindings, None).expect_err("ambiguous");
        assert!(error.to_string().contains("--binding"));
    }

    #[test]
    fn selects_one_binding_without_extra_choreography() {
        let expected = binding(Uuid::nil());
        assert_eq!(
            select_binding(std::slice::from_ref(&expected), None).unwrap(),
            &expected
        );
    }
}
