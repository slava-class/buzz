use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

use buzz_core_pkg::world_view::{
    LocalWorldAuthority, LocalWorldAuthorityRegistry, LOCAL_WORLD_AUTHORITY_REGISTRY_FILE_NAME,
};
use serde::Deserialize;

use crate::managed_agents::nest_dir;

const SHIVAI_LOCAL_MIRROR_BINDING_PATH: &str = ".shivai/local-world-mirror-binding.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShivaiLocalMirrorBinding {
    source_root: String,
    world_ref: ShivaiLocalMirrorRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShivaiLocalMirrorRef {
    kind: String,
    mirror_id: String,
}

/// Register private mutable authority for a public local-world mirror binding.
///
/// The absolute source path is validated against Shivai's package-local mirror
/// binding and written only to the machine-local Buzz nest. It is never included
/// in the Nostr world-view binding event.
#[tauri::command]
pub fn register_local_world_authority(
    origin: String,
    mirror_id: String,
    source_root: String,
) -> Result<serde_json::Value, String> {
    let nest = nest_dir().ok_or_else(|| "could not resolve the Buzz nest directory".to_string())?;
    let registry_path = nest.join(LOCAL_WORLD_AUTHORITY_REGISTRY_FILE_NAME);
    let authority = register_local_world_authority_at(
        &registry_path,
        LocalWorldAuthority {
            origin,
            mirror_id,
            source_root,
        },
    )?;

    Ok(serde_json::json!({
        "authority": authority,
        "requiresAgentRestart": true,
    }))
}

fn register_local_world_authority_at(
    registry_path: &Path,
    mut authority: LocalWorldAuthority,
) -> Result<LocalWorldAuthority, String> {
    let source_root = fs::canonicalize(&authority.source_root)
        .map_err(|error| format!("could not resolve local world source root: {error}"))?;
    if !source_root.is_dir() {
        return Err("local world source root must be a directory".into());
    }
    authority.source_root = source_root.to_string_lossy().into_owned();

    let binding_path = source_root.join(SHIVAI_LOCAL_MIRROR_BINDING_PATH);
    let binding_text = fs::read_to_string(&binding_path).map_err(|error| {
        format!(
            "could not read Shivai local mirror binding {}: {error}",
            binding_path.display()
        )
    })?;
    let binding: ShivaiLocalMirrorBinding = serde_json::from_str(&binding_text)
        .map_err(|error| format!("invalid Shivai local mirror binding: {error}"))?;
    if binding.world_ref.kind != "local-world-mirror" {
        return Err("Shivai binding does not reference a local-world mirror".into());
    }
    if binding.world_ref.mirror_id != authority.mirror_id {
        return Err(format!(
            "local world mirror mismatch: source is bound to {}, not {}",
            binding.world_ref.mirror_id, authority.mirror_id
        ));
    }
    let bound_root = fs::canonicalize(&binding.source_root)
        .map_err(|error| format!("could not resolve Shivai binding sourceRoot: {error}"))?;
    if bound_root != source_root {
        return Err(format!(
            "Shivai binding sourceRoot {} does not match selected root {}",
            bound_root.display(),
            source_root.display()
        ));
    }

    static REGISTRY_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    let _guard = REGISTRY_WRITE_LOCK
        .lock()
        .map_err(|_| "local world authority registry lock poisoned".to_string())?;
    let mut registry = read_registry(registry_path)?;
    registry.upsert(authority.clone())?;
    write_registry(registry_path, &registry)?;
    Ok(authority)
}

fn read_registry(path: &Path) -> Result<LocalWorldAuthorityRegistry, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LocalWorldAuthorityRegistry::default());
        }
        Err(error) => {
            return Err(format!(
                "could not read local world authority registry: {error}"
            ))
        }
    };
    let registry: LocalWorldAuthorityRegistry = serde_json::from_str(&text)
        .map_err(|error| format!("invalid local world authority registry: {error}"))?;
    registry.validate()?;
    Ok(registry)
}

fn write_registry(path: &Path, registry: &LocalWorldAuthorityRegistry) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "local world authority registry path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create local world authority directory: {error}"))?;
    let temp_path = temporary_registry_path(path);
    let write_result = (|| -> Result<(), String> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp_path)
            .map_err(|error| format!("could not create local world authority registry: {error}"))?;
        let mut bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| format!("could not encode local world authority registry: {error}"))?;
        bytes.push(b'\n');
        file.write_all(&bytes)
            .map_err(|error| format!("could not write local world authority registry: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync local world authority registry: {error}"))?;
        fs::rename(&temp_path, path).map_err(|error| {
            format!("could not replace local world authority registry: {error}")
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn temporary_registry_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_only_the_mirror_bound_to_the_selected_source() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("demo.world");
        fs::create_dir_all(source_root.join(".shivai")).unwrap();
        fs::write(
            source_root.join(SHIVAI_LOCAL_MIRROR_BINDING_PATH),
            serde_json::json!({
                "sourceRoot": source_root,
                "version": 1,
                "worldRef": {
                    "kind": "local-world-mirror",
                    "mirrorId": "mirror-1",
                    "packageRevision": "package-1",
                    "revisionId": "revision-1"
                }
            })
            .to_string(),
        )
        .unwrap();
        let registry_path = temp.path().join("world-authorities.json");

        let registered = register_local_world_authority_at(
            &registry_path,
            LocalWorldAuthority {
                origin: "https://manifest.shivai.space".into(),
                mirror_id: "mirror-1".into(),
                source_root: source_root.to_string_lossy().into_owned(),
            },
        )
        .unwrap();

        assert_eq!(
            registered.source_root,
            fs::canonicalize(source_root).unwrap().to_string_lossy()
        );
        let registry = read_registry(&registry_path).unwrap();
        assert_eq!(registry.authorities, vec![registered]);
    }

    #[test]
    fn rejects_a_source_bound_to_another_mirror() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("demo.world");
        fs::create_dir_all(source_root.join(".shivai")).unwrap();
        fs::write(
            source_root.join(SHIVAI_LOCAL_MIRROR_BINDING_PATH),
            serde_json::json!({
                "sourceRoot": source_root,
                "worldRef": { "kind": "local-world-mirror", "mirrorId": "mirror-2" }
            })
            .to_string(),
        )
        .unwrap();

        let error = register_local_world_authority_at(
            &temp.path().join("world-authorities.json"),
            LocalWorldAuthority {
                origin: "https://manifest.shivai.space".into(),
                mirror_id: "mirror-1".into(),
                source_root: source_root.to_string_lossy().into_owned(),
            },
        )
        .unwrap_err();

        assert!(error.contains("source is bound to mirror-2"));
    }
}
