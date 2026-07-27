use std::{
    fs::{self, OpenOptions},
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};

use buzz_core_pkg::world_view::{
    HostedWorldAuthority, LocalWorldAuthority, WorldAuthorityRegistry, WorldViewReference,
    WORLD_AUTHORITY_REGISTRY_FILE_NAME, WORLD_AUTHORITY_REGISTRY_VERSION,
    WORLD_AUTHORITY_SECRET_DIRECTORY,
};
use buzz_world_view_resolver_pkg::HostedEditShareInspection;
use serde::Deserialize;

use crate::managed_agents::nest_dir;

const SHIVAI_LOCAL_MIRROR_BINDING_PATH: &str = ".shivai/local-world-mirror-binding.json";
const SHIVAI_LOCAL_MIRROR_BINDING_VERSION: u8 = 2;
static REGISTRY_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShivaiLocalMirrorBinding {
    origin: String,
    source_root: String,
    version: u8,
    world_ref: ShivaiLocalMirrorRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShivaiLocalMirrorRef {
    kind: String,
    mirror_id: String,
    package_revision: String,
    revision_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorldAuthorityRegistryV1 {
    version: u8,
    local_authorities: Vec<LocalWorldAuthority>,
}

fn validate_world_origin(origin: &str) -> Result<(), String> {
    WorldViewReference::HostedWorldLatest {
        origin: origin.to_owned(),
        hosted_world_id: "origin-validation".into(),
    }
    .validate()
}

/// List credential-free world sources already connected to this Buzz client.
#[tauri::command]
pub fn list_world_authorities() -> Result<serde_json::Value, String> {
    let registry = load_world_authority_registry()?;
    let local = registry.local_authorities.into_iter().map(|authority| {
        serde_json::json!({
            "kind": "local-world-mirror-latest",
            "origin": authority.origin,
            "mirrorId": authority.mirror_id,
            "sourceRoot": authority.source_root,
        })
    });
    let hosted = registry.hosted_authorities.into_iter().map(|authority| {
        serde_json::json!({
            "kind": "hosted-world-latest",
            "origin": authority.origin,
            "hostedWorldId": authority.hosted_world_id,
        })
    });
    Ok(serde_json::json!({
        "authorities": local.chain(hosted).collect::<Vec<_>>(),
    }))
}

/// Connect a published local `.world` package without asking for its mirror id.
#[tauri::command]
pub fn connect_local_world_authority(source_root: String) -> Result<serde_json::Value, String> {
    let registry_path = world_authority_registry_path()?;
    let authority = connect_local_world_authority_at(&registry_path, Path::new(&source_root))?;
    let origin = authority.origin.clone();
    let mirror_id = authority.mirror_id.clone();

    Ok(serde_json::json!({
        "authority": authority,
        "worldRef": {
            "kind": "local-world-mirror-latest",
            "origin": origin,
            "mirrorId": mirror_id,
        },
    }))
}

/// Register a private hosted edit-share capability for one public hosted world.
///
/// The bearer capability is written to an owner-only local file, inspected by
/// the canonical `world hosted latest` boundary, and never crosses into Nostr.
#[tauri::command]
pub async fn register_hosted_world_authority(
    origin: String,
    credential: String,
) -> Result<serde_json::Value, String> {
    let registry_path = world_authority_registry_path()?;
    let (authority, inspection) = register_hosted_world_authority_with_inspector(
        &registry_path,
        origin,
        &credential,
        |origin, credential_file| async move {
            buzz_world_view_resolver_pkg::inspect_hosted_edit_share(&origin, credential_file)
                .await
                .map_err(|error| error.to_string())
        },
    )
    .await?;
    let world_origin = authority.origin.clone();
    let hosted_world_id = inspection.hosted_world_id.clone();

    Ok(serde_json::json!({
        "authority": authority,
        "revision": inspection.revision,
        "worldRef": {
            "kind": "hosted-world-latest",
            "origin": world_origin,
            "hostedWorldId": hosted_world_id,
        },
    }))
}

async fn register_hosted_world_authority_with_inspector<Inspect, InspectFuture>(
    registry_path: &Path,
    origin: String,
    credential: &str,
    inspect: Inspect,
) -> Result<(HostedWorldAuthority, HostedEditShareInspection), String>
where
    Inspect: FnOnce(String, PathBuf) -> InspectFuture,
    InspectFuture: Future<Output = Result<HostedEditShareInspection, String>>,
{
    validate_world_origin(&origin)?;

    let credential = credential.trim();
    if credential.is_empty() {
        return Err("hosted edit-share token or URL must not be blank".into());
    }
    if credential.len() > 8192 {
        return Err("hosted edit-share token or URL exceeds 8192 bytes".into());
    }

    let nest = registry_path
        .parent()
        .ok_or_else(|| "world authority registry path has no parent".to_string())?;
    let credential_directory = nest.join(WORLD_AUTHORITY_SECRET_DIRECTORY);
    ensure_private_directory(&credential_directory)?;
    let credential_file = credential_directory.join(format!("{}.edit-share", uuid::Uuid::new_v4()));
    write_private_credential(&credential_file, credential)?;

    let inspection = match inspect(origin.clone(), credential_file.clone()).await {
        Ok(inspection) => inspection,
        Err(error) => {
            let _ = fs::remove_file(&credential_file);
            return Err(error);
        }
    };
    let authority = HostedWorldAuthority {
        origin,
        hosted_world_id: inspection.hosted_world_id.clone(),
        credential_file: credential_file.to_string_lossy().into_owned(),
    };
    if let Err(error) = register_hosted_world_authority_at(registry_path, authority.clone()) {
        let _ = fs::remove_file(&credential_file);
        return Err(error);
    }

    Ok((authority, inspection))
}

fn connect_local_world_authority_at(
    registry_path: &Path,
    selected_root: &Path,
) -> Result<LocalWorldAuthority, String> {
    let source_root = fs::canonicalize(selected_root)
        .map_err(|error| format!("could not resolve local world source root: {error}"))?;
    if !source_root.is_dir() {
        return Err("local world source root must be a directory".into());
    }

    let binding_path = source_root.join(SHIVAI_LOCAL_MIRROR_BINDING_PATH);
    let binding_text = fs::read_to_string(&binding_path).map_err(|error| {
        format!(
            "could not read Shivai local mirror binding {}: {error}",
            binding_path.display()
        )
    })?;
    let binding: ShivaiLocalMirrorBinding = serde_json::from_str(&binding_text)
        .map_err(|error| format!("invalid Shivai local mirror binding: {error}"))?;
    if binding.version != SHIVAI_LOCAL_MIRROR_BINDING_VERSION {
        return Err(format!(
            "unsupported Shivai local mirror binding version: {}",
            binding.version
        ));
    }
    if binding.world_ref.kind != "local-world-mirror" {
        return Err("Shivai binding does not reference a local-world mirror".into());
    }
    if binding.world_ref.package_revision.trim().is_empty() {
        return Err("Shivai binding packageRevision must not be blank".into());
    }
    if binding.world_ref.revision_id.trim().is_empty() {
        return Err("Shivai binding revisionId must not be blank".into());
    }
    WorldViewReference::LocalWorldMirrorLatest {
        origin: binding.origin.clone(),
        mirror_id: binding.world_ref.mirror_id.clone(),
    }
    .validate()?;

    let bound_root = fs::canonicalize(&binding.source_root)
        .map_err(|error| format!("could not resolve Shivai binding sourceRoot: {error}"))?;
    if bound_root != source_root {
        return Err(format!(
            "Shivai binding sourceRoot {} does not match selected root {}",
            bound_root.display(),
            source_root.display()
        ));
    }

    let authority = LocalWorldAuthority {
        origin: binding.origin,
        mirror_id: binding.world_ref.mirror_id,
        source_root: source_root.to_string_lossy().into_owned(),
    };
    let _guard = REGISTRY_WRITE_LOCK
        .lock()
        .map_err(|_| "world authority registry lock poisoned".to_string())?;
    let mut registry = read_registry_unlocked(registry_path)?;
    registry.upsert_local(authority.clone())?;
    write_registry(registry_path, &registry)?;
    Ok(authority)
}

fn register_hosted_world_authority_at(
    registry_path: &Path,
    authority: HostedWorldAuthority,
) -> Result<HostedWorldAuthority, String> {
    let _guard = REGISTRY_WRITE_LOCK
        .lock()
        .map_err(|_| "world authority registry lock poisoned".to_string())?;
    let mut registry = read_registry_unlocked(registry_path)?;
    let replaced = registry
        .resolve_hosted(&authority.origin, &authority.hosted_world_id)
        .cloned();
    registry.upsert_hosted(authority.clone())?;
    write_registry(registry_path, &registry)?;

    if let Some(replaced) = replaced {
        if replaced.credential_file != authority.credential_file {
            if let Err(error) = fs::remove_file(&replaced.credential_file) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "buzz-desktop: could not remove replaced hosted world credential {}: {error}",
                        replaced.credential_file,
                    );
                }
            }
        }
    }
    Ok(authority)
}

pub(crate) fn load_world_authority_registry() -> Result<WorldAuthorityRegistry, String> {
    read_registry(&world_authority_registry_path()?)
}

fn world_authority_registry_path() -> Result<PathBuf, String> {
    let nest = nest_dir().ok_or_else(|| "could not resolve the Buzz nest directory".to_string())?;
    Ok(nest.join(WORLD_AUTHORITY_REGISTRY_FILE_NAME))
}

fn read_registry(path: &Path) -> Result<WorldAuthorityRegistry, String> {
    let _guard = REGISTRY_WRITE_LOCK
        .lock()
        .map_err(|_| "world authority registry lock poisoned".to_string())?;
    read_registry_unlocked(path)
}

fn read_registry_unlocked(path: &Path) -> Result<WorldAuthorityRegistry, String> {
    read_registry_with_migration_writer(path, write_registry)
}

fn read_registry_with_migration_writer(
    path: &Path,
    mut write_migration: impl FnMut(&Path, &WorldAuthorityRegistry) -> Result<(), String>,
) -> Result<WorldAuthorityRegistry, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(WorldAuthorityRegistry::default());
        }
        Err(error) => return Err(format!("could not read world authority registry: {error}")),
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("invalid world authority registry: {error}"))?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            "invalid world authority registry: version must be an unsigned integer".to_string()
        })?;

    match version {
        1 => {
            let legacy: WorldAuthorityRegistryV1 = serde_json::from_value(value)
                .map_err(|error| format!("invalid v1 world authority registry: {error}"))?;
            if legacy.version != 1 {
                return Err(format!(
                    "unsupported world authority registry version: {}",
                    legacy.version
                ));
            }
            let registry = WorldAuthorityRegistry {
                version: WORLD_AUTHORITY_REGISTRY_VERSION,
                local_authorities: legacy.local_authorities,
                hosted_authorities: Vec::new(),
            };
            registry.validate()?;
            write_migration(path, &registry)?;
            Ok(registry)
        }
        current if current == u64::from(WORLD_AUTHORITY_REGISTRY_VERSION) => {
            let registry: WorldAuthorityRegistry = serde_json::from_value(value)
                .map_err(|error| format!("invalid world authority registry: {error}"))?;
            registry.validate()?;
            Ok(registry)
        }
        unsupported => Err(format!(
            "unsupported world authority registry version: {unsupported}"
        )),
    }
}

fn write_registry(path: &Path, registry: &WorldAuthorityRegistry) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "world authority registry path has no parent".to_string())?;
    ensure_private_directory(parent)?;
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
            .map_err(|error| format!("could not create world authority registry: {error}"))?;
        let mut bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| format!("could not encode world authority registry: {error}"))?;
        bytes.push(b'\n');
        file.write_all(&bytes)
            .map_err(|error| format!("could not write world authority registry: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not sync world authority registry: {error}"))?;
        fs::rename(&temp_path, path)
            .map_err(|error| format!("could not replace world authority registry: {error}"))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("could not create world authority directory: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("could not secure world authority directory: {error}"))?;
    }
    Ok(())
}

fn write_private_credential(path: &Path, credential: &str) -> Result<(), String> {
    write_private_credential_with(path, |file| {
        file.write_all(credential.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()
    })
}

fn write_private_credential_with(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("could not create hosted world credential: {error}"))?;
    let write_result = write(&mut file);
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(path);
        return Err(format!("could not write hosted world credential: {error}"));
    }
    Ok(())
}

fn temporary_registry_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        io::Write as _,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
    };

    use super::*;

    fn write_local_binding(source_root: &Path, origin: &str) {
        fs::create_dir_all(source_root.join(".shivai")).unwrap();
        fs::write(
            source_root.join(SHIVAI_LOCAL_MIRROR_BINDING_PATH),
            serde_json::json!({
                "origin": origin,
                "sourceRoot": source_root,
                "version": SHIVAI_LOCAL_MIRROR_BINDING_VERSION,
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
    }

    #[test]
    fn discovers_and_preserves_the_mirror_publishing_origin() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("demo.world");
        let custom_origin = "http://127.0.0.1:8787";
        write_local_binding(&source_root, custom_origin);
        let registry_path = temp.path().join("world-authorities.json");

        let registered = connect_local_world_authority_at(&registry_path, &source_root).unwrap();

        assert_eq!(registered.origin, custom_origin);
        assert_eq!(
            registered.source_root,
            fs::canonicalize(source_root).unwrap().to_string_lossy()
        );
        let registry = read_registry(&registry_path).unwrap();
        assert_eq!(registry.local_authorities, vec![registered]);
    }

    #[test]
    fn rejects_legacy_local_bindings_without_an_origin() {
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

        let error = connect_local_world_authority_at(
            &temp.path().join("world-authorities.json"),
            &source_root,
        )
        .unwrap_err();

        assert!(error.contains("invalid Shivai local mirror binding"));
    }

    #[tokio::test]
    async fn invalid_hosted_origins_do_not_write_credentials_or_invoke_the_inspector() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("world-authorities.json");
        let inspector_invoked = Arc::new(AtomicBool::new(false));
        let invoked = Arc::clone(&inspector_invoked);

        let error = register_hosted_world_authority_with_inspector(
            &registry_path,
            "http://hosted.example".into(),
            "private-token",
            move |_, _| {
                invoked.store(true, Ordering::SeqCst);
                async {
                    Ok(HostedEditShareInspection {
                        hosted_world_id: "hosted-1".into(),
                        revision: "revision-1".into(),
                    })
                }
            },
        )
        .await
        .unwrap_err();

        assert!(error.contains("must use https"));
        assert!(!inspector_invoked.load(Ordering::SeqCst));
        assert!(!temp.path().join(WORLD_AUTHORITY_SECRET_DIRECTORY).exists());
        assert!(!registry_path.exists());
    }

    #[test]
    fn canonical_origin_validation_allows_https_and_loopback_only() {
        assert!(validate_world_origin("https://hosted.example").is_ok());
        assert!(validate_world_origin("http://localhost:8787").is_ok());
        assert!(validate_world_origin("http://127.0.0.1:8787").is_ok());
        assert!(validate_world_origin("http://hosted.example").is_err());
    }

    #[test]
    fn partial_credential_write_errors_remove_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let credential = temp.path().join("secret.edit-share");

        let error = write_private_credential_with(&credential, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        })
        .unwrap_err();

        assert!(error.contains("injected write failure"));
        assert!(!credential.exists());
    }

    #[tokio::test]
    async fn inspector_failures_remove_the_written_credential() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("world-authorities.json");
        let (credential_path_sender, credential_path_receiver) = mpsc::channel();

        let error = register_hosted_world_authority_with_inspector(
            &registry_path,
            "https://hosted.example".into(),
            "private-token",
            move |_, path| {
                credential_path_sender.send(path).unwrap();
                async { Err("injected inspection failure".into()) }
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error, "injected inspection failure");
        assert!(!credential_path_receiver.recv().unwrap().exists());
        assert!(!registry_path.exists());
    }

    #[test]
    fn migrates_an_exact_v1_registry_to_v2_once() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("world-authorities.json");
        let local_authority = LocalWorldAuthority {
            origin: "https://hosted.example".into(),
            mirror_id: "mirror-1".into(),
            source_root: temp
                .path()
                .join("demo.world")
                .to_string_lossy()
                .into_owned(),
        };
        fs::write(
            &registry_path,
            serde_json::json!({
                "version": 1,
                "localAuthorities": [local_authority.clone()]
            })
            .to_string(),
        )
        .unwrap();
        let migration_writes = Cell::new(0);

        let migrated = read_registry_with_migration_writer(&registry_path, |path, registry| {
            migration_writes.set(migration_writes.get() + 1);
            write_registry(path, registry)
        })
        .unwrap();
        let first_v2_bytes = fs::read(&registry_path).unwrap();
        let loaded_again = read_registry_with_migration_writer(&registry_path, |path, registry| {
            migration_writes.set(migration_writes.get() + 1);
            write_registry(path, registry)
        })
        .unwrap();

        assert_eq!(migration_writes.get(), 1);
        assert_eq!(migrated.version, WORLD_AUTHORITY_REGISTRY_VERSION);
        assert_eq!(migrated.local_authorities, vec![local_authority]);
        assert!(migrated.hosted_authorities.is_empty());
        assert_eq!(loaded_again, migrated);
        assert_eq!(fs::read(registry_path).unwrap(), first_v2_bytes);
    }

    #[test]
    fn rejects_unknown_registry_versions_and_non_exact_v1_shapes() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("world-authorities.json");
        fs::write(
            &registry_path,
            serde_json::json!({
                "version": 99,
                "localAuthorities": [],
                "hostedAuthorities": []
            })
            .to_string(),
        )
        .unwrap();

        let version_error = read_registry(&registry_path).unwrap_err();
        assert!(version_error.contains("unsupported world authority registry version: 99"));

        fs::write(
            &registry_path,
            serde_json::json!({
                "version": 1,
                "localAuthorities": [],
                "hostedAuthorities": []
            })
            .to_string(),
        )
        .unwrap();
        let shape_error = read_registry(&registry_path).unwrap_err();
        assert!(shape_error.contains("invalid v1 world authority registry"));
    }

    #[test]
    fn replaces_hosted_authority_and_removes_the_superseded_credential() {
        let temp = tempfile::tempdir().unwrap();
        let registry_path = temp.path().join("world-authorities.json");
        let old_credential = temp.path().join("old.edit-share");
        let new_credential = temp.path().join("new.edit-share");
        write_private_credential(&old_credential, "old-token").unwrap();
        write_private_credential(&new_credential, "new-token").unwrap();
        let authority = |credential_file: &Path| HostedWorldAuthority {
            origin: "https://manifest.shivai.space".into(),
            hosted_world_id: "hosted-1".into(),
            credential_file: credential_file.to_string_lossy().into_owned(),
        };

        register_hosted_world_authority_at(&registry_path, authority(&old_credential)).unwrap();
        let registered =
            register_hosted_world_authority_at(&registry_path, authority(&new_credential)).unwrap();

        assert!(!old_credential.exists());
        assert!(new_credential.exists());
        assert_eq!(
            read_registry(&registry_path).unwrap().hosted_authorities,
            vec![registered]
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_hosted_credentials_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let credential = temp.path().join("secret.edit-share");
        write_private_credential(&credential, "secret-token").unwrap();

        assert_eq!(
            fs::metadata(credential).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
