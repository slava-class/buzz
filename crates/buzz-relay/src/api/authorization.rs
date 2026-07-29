//! Side-effect-free authorization capability checks for external integrations.
//!
//! The endpoint in this module exposes a narrow, authenticated read of current
//! relay authority. Callers sign the exact request with NIP-98; the relay binds
//! the request host to one tenant, replays are rejected, and the decision reads
//! the primary channel-membership store rather than relay-signed projection
//! events.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

use super::bridge::{
    check_nip98_replay, enforce_http_admission, nip98_expected_url, verify_bridge_auth_with_options,
};
use super::{api_error, internal_error};

const CHECK_CAPABILITY_PATH: &str = "/authorization/capability";
const AUTHORIZATION_SCHEMA_VERSION: u8 = 1;

/// A side-effect-free capability understood by the relay.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorizationCapability {
    /// The caller currently holds the channel owner or admin role.
    ManageChannel,
}

/// Request body for [`check_capability`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CheckCapabilityRequest {
    /// Exact channel whose current role is checked.
    pub channel_id: Uuid,
    /// Side-effect-free capability requested by the caller.
    pub capability: AuthorizationCapability,
}

/// Current channel roles that satisfy [`AuthorizationCapability::ManageChannel`].
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelManagementRole {
    /// Current channel owner.
    Owner,
    /// Current channel administrator.
    Admin,
}

/// Successful current-authority readback.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CheckCapabilityResponse {
    /// Authorization response contract version.
    pub schema_version: u8,
    /// Authenticated Nostr public key, encoded as lowercase hex.
    pub subject_pubkey: String,
    /// Exact channel whose current role was checked.
    pub channel_id: Uuid,
    /// Capability that was authorized.
    pub capability: AuthorizationCapability,
    /// Explicit allow decision.
    pub decision: CapabilityDecision,
    /// Current authoritative role that satisfied the capability.
    pub role: ChannelManagementRole,
}

/// A successful response is always an explicit allow; denials use HTTP 403.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityDecision {
    /// The authenticated caller currently satisfies the requested capability.
    Allow,
}

/// Check one current relay capability without mutating relay state.
///
/// This endpoint intentionally requires a NIP-98 payload tag even when the
/// relay permits the development-only `X-Pubkey` bridge fallback elsewhere.
/// The exact body therefore remains covered by the caller's signature.
pub async fn check_capability(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<CheckCapabilityResponse>, (StatusCode, Json<serde_json::Value>)> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::NOT_FOUND,
                "relay: no community is configured for this host",
            )
        })?;

    let url = nip98_expected_url(&state.config.relay_url, &tenant, CHECK_CAPABILITY_PATH);
    let (pubkey, event_id_bytes) =
        verify_bridge_auth_with_options(&headers, "POST", &url, Some(body.as_ref()), true, true)?;
    enforce_http_admission(&state, &tenant, &pubkey).await?;
    check_nip98_replay(&state, &tenant, event_id_bytes).await?;

    let request: CheckCapabilityRequest = serde_json::from_slice(&body)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid capability request"))?;
    let subject = pubkey.to_bytes();
    let current_role = state
        .db
        .get_member_role(tenant.community(), request.channel_id, &subject)
        .await
        .map_err(|error| internal_error(&format!("current channel role lookup failed: {error}")))?;

    let role = match (request.capability, current_role.as_deref()) {
        (AuthorizationCapability::ManageChannel, Some("owner")) => ChannelManagementRole::Owner,
        (AuthorizationCapability::ManageChannel, Some("admin")) => ChannelManagementRole::Admin,
        _ => {
            return Err(api_error(StatusCode::FORBIDDEN, "authorization denied"));
        }
    };

    Ok(Json(CheckCapabilityResponse {
        schema_version: AUTHORIZATION_SCHEMA_VERSION,
        subject_pubkey: pubkey.to_hex(),
        channel_id: request.channel_id,
        capability: request.capability,
        decision: CapabilityDecision::Allow,
        role,
    }))
}

#[cfg(test)]
mod tests {
    use std::{future::Future, pin::Pin, sync::Arc};

    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use base64::Engine;
    use buzz_auth::Nip98ReplayGuard;
    use buzz_core::{kind::KIND_NIP29_GROUP_ADMINS, TenantContext};
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;

    use super::*;
    use crate::router::build_router;

    const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

    struct AlwaysFreshReplayGuard;

    impl Nip98ReplayGuard for AlwaysFreshReplayGuard {
        fn try_mark_in_scope<'a>(
            &'a self,
            _scope: &'a str,
            _event_id: &'a nostr::EventId,
            _ttl_secs: u64,
        ) -> Pin<Box<dyn Future<Output = Result<bool, buzz_auth::AuthError>> + Send + 'a>> {
            Box::pin(async { Ok(true) })
        }
    }

    fn nip98_auth_header(keys: &Keys, url: &str, body: &[u8]) -> String {
        let hash: [u8; 32] = Sha256::digest(body).into();
        let tags = vec![
            Tag::parse(["u", url]).expect("u tag"),
            Tag::parse(["method", "POST"]).expect("method tag"),
            Tag::parse(["payload", hex::encode(hash).as_str()]).expect("payload tag"),
        ];
        let event = EventBuilder::new(Kind::HttpAuth, "")
            .tags(tags)
            .sign_with_keys(keys)
            .expect("sign NIP-98 event");
        let json = serde_json::to_string(&event).expect("serialize NIP-98 event");
        let encoded = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());
        format!("Nostr {encoded}")
    }

    async fn test_state(host: &str) -> Option<(Arc<AppState>, buzz_core::CommunityId)> {
        let mut config = crate::config::Config::from_env().ok()?;
        config.database_url = TEST_DB_URL.to_string();
        config.redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        config.relay_url = format!("wss://{host}");
        config.require_auth_token = true;
        config.require_relay_membership = false;

        let pool = sqlx::PgPool::connect(TEST_DB_URL).await.ok()?;
        let db = buzz_db::Db::from_pool(pool.clone());
        let community = db.ensure_configured_community(host).await.ok()?.id;
        let redis_pool = deadpool_redis::Config::from_url(&config.redis_url)
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .ok()?;
        let pubsub = Arc::new(
            buzz_pubsub::PubSubManager::new(&config.redis_url, redis_pool.clone())
                .await
                .ok()?,
        );
        let audit = buzz_audit::AuditService::new(pool.clone());
        let auth = buzz_auth::AuthService::new(config.auth.clone());
        let search = buzz_search::SearchService::new(pool.clone());
        let workflow_engine = Arc::new(buzz_workflow::WorkflowEngine::new(
            db.clone(),
            buzz_workflow::WorkflowConfig::default(),
        ));
        let media_storage = buzz_media::MediaStorage::new(&config.media).ok()?;
        let (mut state, _audit_shutdown) = AppState::new(
            config,
            db,
            redis_pool,
            audit,
            pubsub,
            auth,
            search,
            workflow_engine,
            Keys::generate(),
            media_storage,
        );
        state.nip98_replay = Arc::new(AlwaysFreshReplayGuard);
        Some((Arc::new(state), community))
    }

    async fn post_capability(
        state: Arc<AppState>,
        host: &str,
        keys: &Keys,
        request: &CheckCapabilityRequest,
    ) -> axum::response::Response {
        let body = serde_json::to_vec(request).expect("serialize request");
        let auth = nip98_auth_header(
            keys,
            &format!("https://{host}{CHECK_CAPABILITY_PATH}"),
            &body,
        );
        build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(CHECK_CAPABILITY_PATH)
                    .header(header::HOST, host)
                    .header(header::AUTHORIZATION, auth)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response")
    }

    async fn latest_admin_projection(
        state: &AppState,
        community: buzz_core::CommunityId,
        channel_id: Uuid,
    ) -> nostr::Event {
        state
            .db
            .query_events(&buzz_db::event::EventQuery {
                kinds: Some(vec![KIND_NIP29_GROUP_ADMINS as i32]),
                channel_id: Some(channel_id),
                limit: Some(1),
                ..buzz_db::event::EventQuery::for_community(community)
            })
            .await
            .expect("query group-admin projection")
            .into_iter()
            .next()
            .expect("group-admin projection")
            .event
    }

    fn projection_has_role(event: &nostr::Event, pubkey_hex: &str, role: &str) -> bool {
        event.tags.iter().any(|tag| {
            let parts = tag.as_slice();
            parts.len() == 3 && parts[0] == "p" && parts[1] == pubkey_hex && parts[2] == role
        })
    }

    #[test]
    fn capability_contract_has_one_explicit_wire_value() {
        let request = CheckCapabilityRequest {
            channel_id: Uuid::nil(),
            capability: AuthorizationCapability::ManageChannel,
        };
        assert_eq!(
            serde_json::to_value(request).expect("serialize request"),
            serde_json::json!({
                "channel_id": Uuid::nil(),
                "capability": "manage-channel",
            })
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres and Redis"]
    async fn demoted_owner_is_denied_while_stale_signed_projection_claims_owner() {
        let host = format!("capability-test-{}.local", Uuid::new_v4().simple());
        let (state, community) = test_state(&host)
            .await
            .expect("local Postgres and Redis must be reachable");
        let owner_a = Keys::generate();
        let owner_b = Keys::generate();
        let owner_a_bytes = owner_a.public_key().to_bytes();
        let owner_b_bytes = owner_b.public_key().to_bytes();
        state
            .db
            .ensure_user(community, &owner_a_bytes)
            .await
            .expect("ensure first owner");
        state
            .db
            .ensure_user(community, &owner_b_bytes)
            .await
            .expect("ensure second owner");
        let channel = state
            .db
            .create_channel(
                community,
                "capability-test",
                buzz_db::channel::ChannelType::Stream,
                buzz_db::channel::ChannelVisibility::Open,
                None,
                &owner_a_bytes,
                None,
            )
            .await
            .expect("create channel");
        state
            .db
            .add_member(
                community,
                channel.id,
                &owner_b_bytes,
                buzz_db::channel::MemberRole::Owner,
                Some(&owner_a_bytes),
            )
            .await
            .expect("add second owner");

        let request = CheckCapabilityRequest {
            channel_id: channel.id,
            capability: AuthorizationCapability::ManageChannel,
        };
        let allowed = post_capability(state.clone(), &host, &owner_a, &request).await;
        assert_eq!(allowed.status(), StatusCode::OK);
        let allowed_body = to_bytes(allowed.into_body(), 64 * 1024)
            .await
            .expect("read allowed response");
        let allowed: CheckCapabilityResponse =
            serde_json::from_slice(&allowed_body).expect("decode allowed response");
        assert_eq!(allowed.subject_pubkey, owner_a.public_key().to_hex());
        assert_eq!(allowed.role, ChannelManagementRole::Owner);

        let tenant = TenantContext::resolved(community, host.clone());
        crate::handlers::side_effects::emit_group_discovery_events(&tenant, &state, channel.id)
            .await
            .expect("emit current group projection");
        let stale_projection = latest_admin_projection(&state, community, channel.id).await;
        stale_projection.verify().expect("projection signature");
        assert!(projection_has_role(
            &stale_projection,
            &owner_a.public_key().to_hex(),
            "owner"
        ));

        state
            .db
            .add_member(
                community,
                channel.id,
                &owner_a_bytes,
                buzz_db::channel::MemberRole::Member,
                Some(&owner_b_bytes),
            )
            .await
            .expect("demote first owner");

        let still_stale = latest_admin_projection(&state, community, channel.id).await;
        assert_eq!(still_stale.id, stale_projection.id);
        assert!(projection_has_role(
            &still_stale,
            &owner_a.public_key().to_hex(),
            "owner"
        ));

        let denied = post_capability(state, &host, &owner_a, &request).await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
}
