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
    /// The caller is currently an active channel member.
    AccessChannel,
    /// The caller currently holds the channel owner or admin role.
    ManageChannel,
}

/// Request body for [`check_capability`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CheckCapabilityRequest {
    /// Authorization request contract version.
    pub schema_version: u8,
    /// Exact channel whose current role is checked.
    pub channel_id: Uuid,
    /// Side-effect-free capability requested by the caller.
    pub capability: AuthorizationCapability,
    /// Optional exact thread root whose existence is checked inside the channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_event_id: Option<String>,
}

fn is_supported_capability_request(request: &CheckCapabilityRequest) -> bool {
    request.schema_version == AUTHORIZATION_SCHEMA_VERSION
        && (request.capability != AuthorizationCapability::ManageChannel
            || request.thread_root_event_id.is_none())
}

/// Current authoritative channel role returned by a successful capability check.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelRole {
    /// Current channel owner.
    Owner,
    /// Current channel administrator.
    Admin,
    /// Current standard member.
    Member,
    /// Current read-only guest member.
    Guest,
    /// Current bot member.
    Bot,
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
    pub role: ChannelRole,
    /// Exact thread root checked for an access capability, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_root_event_id: Option<String>,
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
    let subject = pubkey.to_bytes();
    let auth_tag = headers
        .get("x-auth-tag")
        .and_then(|value| value.to_str().ok());
    super::relay_members::enforce_relay_membership(&state, tenant.community(), &subject, auth_tag)
        .await?;

    let request: CheckCapabilityRequest = serde_json::from_slice(&body)
        .map_err(|_| api_error(StatusCode::BAD_REQUEST, "invalid capability request"))?;
    if !is_supported_capability_request(&request) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "unsupported capability request shape",
        ));
    }
    let current_role = state
        .db
        .get_member_role(tenant.community(), request.channel_id, &subject)
        .await
        .map_err(|error| internal_error(&format!("current channel role lookup failed: {error}")))?;

    let role = match current_role.as_deref() {
        Some("owner") => ChannelRole::Owner,
        Some("admin") => ChannelRole::Admin,
        Some("member") => ChannelRole::Member,
        Some("guest") => ChannelRole::Guest,
        Some("bot") => ChannelRole::Bot,
        _ => {
            return Err(api_error(StatusCode::FORBIDDEN, "authorization denied"));
        }
    };
    if request.capability == AuthorizationCapability::ManageChannel
        && !matches!(role, ChannelRole::Owner | ChannelRole::Admin)
    {
        return Err(api_error(StatusCode::FORBIDDEN, "authorization denied"));
    }

    if let Some(thread_root_event_id) = request.thread_root_event_id.as_deref() {
        let event_id = hex::decode(thread_root_event_id)
            .ok()
            .filter(|bytes| bytes.len() == 32)
            .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "invalid thread root event id"))?;
        let (event_result, metadata_result) = tokio::join!(
            state.db.get_event_by_id(tenant.community(), &event_id),
            state
                .db
                .get_thread_metadata_by_event(tenant.community(), &event_id),
        );
        let event = event_result
            .map_err(|error| internal_error(&format!("thread root event lookup failed: {error}")))?
            .ok_or_else(|| api_error(StatusCode::FORBIDDEN, "authorization denied"))?;
        let metadata = metadata_result.map_err(|error| {
            internal_error(&format!("thread root metadata lookup failed: {error}"))
        })?;
        let is_message_root =
            buzz_core::kind::is_thread_root_kind(u32::from(event.event.kind.as_u16()));
        let has_no_ancestry = metadata
            .as_ref()
            .map(|metadata| {
                metadata.channel_id == request.channel_id
                    && metadata.depth == 0
                    && metadata.parent_event_id.is_none()
                    && metadata.root_event_id.is_none()
            })
            .unwrap_or(true);
        if event.channel_id != Some(request.channel_id) || !is_message_root || !has_no_ancestry {
            return Err(api_error(StatusCode::FORBIDDEN, "authorization denied"));
        }
    }

    Ok(Json(CheckCapabilityResponse {
        schema_version: AUTHORIZATION_SCHEMA_VERSION,
        subject_pubkey: pubkey.to_hex(),
        channel_id: request.channel_id,
        capability: request.capability,
        decision: CapabilityDecision::Allow,
        role,
        thread_root_event_id: request.thread_root_event_id,
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
    use buzz_core::{
        kind::{
            KIND_FORUM_POST, KIND_NIP29_GROUP_ADMINS, KIND_STREAM_MESSAGE, KIND_SYSTEM_MESSAGE,
        },
        TenantContext,
    };
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
        config.require_relay_membership = true;

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

    fn signed_event(keys: &Keys, kind: Kind, content: &str) -> nostr::Event {
        EventBuilder::new(kind, content)
            .sign_with_keys(keys)
            .expect("sign event")
    }

    fn access_request_for_root(
        channel_id: Uuid,
        thread_root_event: &nostr::Event,
    ) -> CheckCapabilityRequest {
        CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION,
            channel_id,
            capability: AuthorizationCapability::AccessChannel,
            thread_root_event_id: Some(thread_root_event.id.to_hex()),
        }
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
    fn capability_contract_has_explicit_wire_values() {
        let manage = CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION,
            channel_id: Uuid::nil(),
            capability: AuthorizationCapability::ManageChannel,
            thread_root_event_id: None,
        };
        assert_eq!(
            serde_json::to_value(manage).expect("serialize manage request"),
            serde_json::json!({
                "schema_version": AUTHORIZATION_SCHEMA_VERSION,
                "channel_id": Uuid::nil(),
                "capability": "manage-channel",
            })
        );
        let access = CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION,
            channel_id: Uuid::nil(),
            capability: AuthorizationCapability::AccessChannel,
            thread_root_event_id: Some("ab".repeat(32)),
        };
        assert_eq!(
            serde_json::to_value(access).expect("serialize access request"),
            serde_json::json!({
                "schema_version": AUTHORIZATION_SCHEMA_VERSION,
                "channel_id": Uuid::nil(),
                "capability": "access-channel",
                "thread_root_event_id": "ab".repeat(32),
            })
        );
        assert!(
            serde_json::from_value::<CheckCapabilityRequest>(serde_json::json!({
                "schema_version": AUTHORIZATION_SCHEMA_VERSION,
                "channel_id": Uuid::nil(),
                "capability": "access-channel",
                "unexpected": true,
            }))
            .is_err()
        );
        assert!(!is_supported_capability_request(&CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION + 1,
            channel_id: Uuid::nil(),
            capability: AuthorizationCapability::AccessChannel,
            thread_root_event_id: None,
        }));
        assert!(!is_supported_capability_request(&CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION,
            channel_id: Uuid::nil(),
            capability: AuthorizationCapability::ManageChannel,
            thread_root_event_id: Some("ab".repeat(32)),
        }));
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
        assert!(state
            .db
            .add_relay_member(community, &owner_a.public_key().to_hex(), "member", None,)
            .await
            .expect("add first owner to closed relay"));
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

        let outsider = Keys::generate();
        let outsider_bytes = outsider.public_key().to_bytes();
        state
            .db
            .ensure_user(community, &outsider_bytes)
            .await
            .expect("ensure prospective member");
        assert!(state
            .db
            .add_relay_member(community, &outsider.public_key().to_hex(), "member", None,)
            .await
            .expect("add prospective member to closed relay"));
        let access_request = CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION,
            channel_id: channel.id,
            capability: AuthorizationCapability::AccessChannel,
            thread_root_event_id: None,
        };
        let nonmember = post_capability(state.clone(), &host, &outsider, &access_request).await;
        assert_eq!(nonmember.status(), StatusCode::FORBIDDEN);
        state
            .db
            .add_member(
                community,
                channel.id,
                &outsider_bytes,
                buzz_db::channel::MemberRole::Guest,
                Some(&owner_a_bytes),
            )
            .await
            .expect("add guest member");
        let current_member =
            post_capability(state.clone(), &host, &outsider, &access_request).await;
        assert_eq!(current_member.status(), StatusCode::OK);
        let member_body = to_bytes(current_member.into_body(), 64 * 1024)
            .await
            .expect("read member response");
        let member: CheckCapabilityResponse =
            serde_json::from_slice(&member_body).expect("decode member response");
        assert_eq!(member.role, ChannelRole::Guest);

        assert_eq!(
            state
                .db
                .remove_relay_member(community, &outsider.public_key().to_hex())
                .await
                .expect("remove guest from closed relay"),
            buzz_db::relay_members::RemoveResult::Removed
        );
        assert_eq!(
            state
                .db
                .get_member_role(community, channel.id, &outsider_bytes)
                .await
                .expect("read retained channel membership")
                .as_deref(),
            Some("guest")
        );
        let removed_relay_member =
            post_capability(state.clone(), &host, &outsider, &access_request).await;
        assert_eq!(removed_relay_member.status(), StatusCode::FORBIDDEN);

        let request = CheckCapabilityRequest {
            schema_version: AUTHORIZATION_SCHEMA_VERSION,
            channel_id: channel.id,
            capability: AuthorizationCapability::ManageChannel,
            thread_root_event_id: None,
        };
        let allowed = post_capability(state.clone(), &host, &owner_a, &request).await;
        assert_eq!(allowed.status(), StatusCode::OK);
        let allowed_body = to_bytes(allowed.into_body(), 64 * 1024)
            .await
            .expect("read allowed response");
        let allowed: CheckCapabilityResponse =
            serde_json::from_slice(&allowed_body).expect("decode allowed response");
        assert_eq!(allowed.subject_pubkey, owner_a.public_key().to_hex());
        assert_eq!(allowed.role, ChannelRole::Owner);

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

    #[tokio::test]
    #[ignore = "requires Postgres and Redis"]
    async fn capability_accepts_only_live_canonical_roots_in_the_requested_channel() {
        let host = format!("capability-root-test-{}.local", Uuid::new_v4().simple());
        let (state, community) = test_state(&host)
            .await
            .expect("local Postgres and Redis must be reachable");
        let member = Keys::generate();
        let member_bytes = member.public_key().to_bytes();
        state
            .db
            .ensure_user(community, &member_bytes)
            .await
            .expect("ensure member");
        assert!(state
            .db
            .add_relay_member(community, &member.public_key().to_hex(), "member", None,)
            .await
            .expect("add member to closed relay"));
        let channel = state
            .db
            .create_channel(
                community,
                "capability-root-test",
                buzz_db::channel::ChannelType::Stream,
                buzz_db::channel::ChannelVisibility::Open,
                None,
                &member_bytes,
                None,
            )
            .await
            .expect("create requested channel");
        let other_channel = state
            .db
            .create_channel(
                community,
                "capability-root-test-other",
                buzz_db::channel::ChannelType::Stream,
                buzz_db::channel::ChannelVisibility::Open,
                None,
                &member_bytes,
                None,
            )
            .await
            .expect("create other channel");

        let root = signed_event(
            &member,
            Kind::Custom(KIND_STREAM_MESSAGE as u16),
            "valid root",
        );
        state
            .db
            .insert_event_with_thread_metadata(community, &root, Some(channel.id), None)
            .await
            .expect("insert valid root");
        let valid = post_capability(
            state.clone(),
            &host,
            &member,
            &access_request_for_root(channel.id, &root),
        )
        .await;
        assert_eq!(valid.status(), StatusCode::OK);

        let forum_root = signed_event(
            &member,
            Kind::Custom(KIND_FORUM_POST as u16),
            "valid forum root",
        );
        state
            .db
            .insert_event_with_thread_metadata(community, &forum_root, Some(channel.id), None)
            .await
            .expect("insert valid forum root");
        let valid_forum = post_capability(
            state.clone(),
            &host,
            &member,
            &access_request_for_root(channel.id, &forum_root),
        )
        .await;
        assert_eq!(valid_forum.status(), StatusCode::OK);

        let reply = signed_event(&member, Kind::Custom(KIND_STREAM_MESSAGE as u16), "reply");
        let root_created_at = chrono::DateTime::from_timestamp(root.created_at.as_secs() as i64, 0)
            .expect("valid root timestamp");
        let reply_created_at =
            chrono::DateTime::from_timestamp(reply.created_at.as_secs() as i64, 0)
                .expect("valid reply timestamp");
        state
            .db
            .insert_event_with_thread_metadata(
                community,
                &reply,
                Some(channel.id),
                Some(buzz_db::event::ThreadMetadataParams {
                    event_id: reply.id.as_bytes(),
                    event_created_at: reply_created_at,
                    channel_id: channel.id,
                    parent_event_id: Some(root.id.as_bytes()),
                    parent_event_created_at: Some(root_created_at),
                    root_event_id: Some(root.id.as_bytes()),
                    root_event_created_at: Some(root_created_at),
                    depth: 1,
                    broadcast: false,
                }),
            )
            .await
            .expect("insert reply");

        let reaction = signed_event(&member, Kind::Reaction, "reaction");
        state
            .db
            .insert_event_with_thread_metadata(community, &reaction, Some(channel.id), None)
            .await
            .expect("insert reaction");
        let system_event = signed_event(
            &member,
            Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
            "system event",
        );
        state
            .db
            .insert_event_with_thread_metadata(community, &system_event, Some(channel.id), None)
            .await
            .expect("insert system event");
        let wrong_channel_root = signed_event(
            &member,
            Kind::Custom(KIND_STREAM_MESSAGE as u16),
            "wrong channel root",
        );
        state
            .db
            .insert_event_with_thread_metadata(
                community,
                &wrong_channel_root,
                Some(other_channel.id),
                None,
            )
            .await
            .expect("insert wrong-channel root");
        let deleted_root = signed_event(
            &member,
            Kind::Custom(KIND_STREAM_MESSAGE as u16),
            "deleted root",
        );
        state
            .db
            .insert_event_with_thread_metadata(community, &deleted_root, Some(channel.id), None)
            .await
            .expect("insert root to delete");
        assert!(state
            .db
            .soft_delete_event(community, deleted_root.id.as_bytes())
            .await
            .expect("soft-delete root"));
        let nonexistent_root = signed_event(
            &member,
            Kind::Custom(KIND_STREAM_MESSAGE as u16),
            "never inserted",
        );

        for (case, denied_root) in [
            ("reply", &reply),
            ("reaction", &reaction),
            ("system event", &system_event),
            ("wrong-channel root", &wrong_channel_root),
            ("deleted root", &deleted_root),
            ("nonexistent root", &nonexistent_root),
        ] {
            let denied = post_capability(
                state.clone(),
                &host,
                &member,
                &access_request_for_root(channel.id, denied_root),
            )
            .await;
            assert_eq!(
                denied.status(),
                StatusCode::FORBIDDEN,
                "{case} must not be accepted as a thread root"
            );
        }
    }
}
