//! Client for the VOIDWRAP RUN matchmaker (frozen contract: `POST /matches`,
//! `POST /matches/{id}`, `DELETE /matches/{id}`, `GET /matches?version=..`).
//!
//! The matchmaker is optional infrastructure. Nothing in this module may make the
//! game fail: every request has a short timeout, unreachable or unhelpful answers are
//! logged at debug and reported as plain outcomes, and callers retry on the next
//! heartbeat. Delisting on shutdown is best effort and never blocks the game.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default matchmaker base URL when `MATCHMAKER_URL` is not set.
pub const DEFAULT_MATCHMAKER_URL: &str = "http://localhost:8080";

/// Short timeout for matchmaker requests, connect and total alike. The game must never wait on the matchmaker.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Body of `POST /matches`: field-for-field the frozen contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RegisterRequest {
    /// Display name of the listing.
    pub name: String,
    /// Workspace game version.
    pub version: String,
    /// Port friends connect to.
    pub port: u16,
    /// Current players.
    pub players: u32,
    /// Maximum players.
    pub max_players: u32,
    /// Current level, as an opaque string (the contract stores and echoes it verbatim).
    pub level: String,
    /// Current phase (short lowercase word).
    pub phase: String,
    /// True when the match accepts new players.
    pub joinable: bool,
    /// Address friends may use, or null when unknown.
    pub address: Option<String>,
}

/// Body of the `POST /matches/{id}` heartbeat: field-for-field the frozen contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HeartbeatRequest {
    /// Secret returned by registration.
    pub secret: String,
    /// Current players.
    pub players: u32,
    /// Current level, as an opaque string (the contract stores and echoes it verbatim).
    pub level: String,
    /// Current phase.
    pub phase: String,
    /// True when the match accepts new players.
    pub joinable: bool,
}

/// Body of `DELETE /matches/{id}`: field-for-field the frozen contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct DelistRequest {
    /// Secret returned by registration.
    secret: String,
}

/// Response of `POST /matches`. A missing `address` decodes as `None`.
#[derive(Clone, Debug, Deserialize)]
pub struct RegisterResponse {
    /// Opaque match id.
    pub id: String,
    /// Secret used for the heartbeat and the delist.
    pub secret: String,
    /// Address the matchmaker recorded, if it reports one.
    #[serde(default)]
    pub address: Option<String>,
}

/// An active matchmaker listing: the id and secret returned by registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchmakerSession {
    /// Opaque match id.
    pub id: String,
    /// Secret used for the heartbeat and the delist.
    pub secret: String,
    /// Address the matchmaker recorded (its echo wins on mismatch).
    pub address: Option<String>,
}

/// Outcome of a heartbeat, so the caller can re-register when the entry is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatOutcome {
    /// The matchmaker accepted the heartbeat (200).
    Ok,
    /// The entry expired (404) or the secret was rejected (403); register again.
    Expired,
    /// Unreachable, timed out or answered something unexpected; try again next beat.
    Unreachable,
}

/// Best-effort snapshot of the running game, sent with every heartbeat.
///
/// The game task publishes it through [`GameStateHandle`] once per tick; the heartbeat
/// reads it without ever blocking on the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSnapshot {
    /// Current number of connected players.
    pub players: u32,
    /// Level of the current run.
    pub level: u32,
    /// Phase of the game ("unknown" until the game task reports).
    pub phase: String,
}

impl Default for GameSnapshot {
    /// Zero players, level 1, phase "unknown".
    fn default() -> Self {
        Self {
            players: 0,
            level: 1,
            phase: "unknown".to_string(),
        }
    }
}

/// Lock-free live game state shared between the game task and the heartbeat task.
///
/// The game task calls [`GameStateHandle::publish`] once per tick; the heartbeat calls
/// [`GameStateHandle::snapshot`]. No lock is ever held across an await.
#[derive(Debug)]
pub struct GameStateHandle {
    players: std::sync::atomic::AtomicU32,
    level: std::sync::atomic::AtomicU32,
    phase: std::sync::atomic::AtomicU8,
}

impl Default for GameStateHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl GameStateHandle {
    /// A handle reporting zero players, level 1, phase "unknown".
    pub fn new() -> Self {
        Self {
            players: std::sync::atomic::AtomicU32::new(0),
            level: std::sync::atomic::AtomicU32::new(1),
            phase: std::sync::atomic::AtomicU8::new(0),
        }
    }

    /// Publish the current game state (called by the game task every tick).
    pub fn publish(&self, players: u32, level: u32, phase: u8) {
        use std::sync::atomic::Ordering::Relaxed;
        self.players.store(players, Relaxed);
        self.level.store(level, Relaxed);
        self.phase.store(phase, Relaxed);
    }

    /// Read the latest published state.
    pub fn snapshot(&self) -> GameSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        let phase = match self.phase.load(Relaxed) {
            protocol::PHASE_ENEMY_DRAFT => "draft",
            protocol::PHASE_PLAYING => "playing",
            protocol::PHASE_REWARD_DRAFT => "reward",
            protocol::PHASE_OVER => "over",
            _ => "unknown",
        };
        GameSnapshot {
            players: self.players.load(Relaxed),
            level: self.level.load(Relaxed),
            phase: phase.to_string(),
        }
    }
}

/// Configuration for the matchmaker connection, read from the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchmakerConfig {
    /// Base URL of the matchmaker, trailing slashes trimmed (`MATCHMAKER_URL`).
    pub url: String,
    /// Public origin advertised by forced-open hosts (`PUBLIC_ORIGIN`), if set.
    pub public_origin: Option<String>,
}

impl MatchmakerConfig {
    /// Read `MATCHMAKER_URL` (defaulting to `http://localhost:8080`) and `PUBLIC_ORIGIN`.
    ///
    /// Empty values are treated as unset.
    pub fn from_env() -> Self {
        let url = std::env::var("MATCHMAKER_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_MATCHMAKER_URL.to_string());
        let public_origin = std::env::var("PUBLIC_ORIGIN")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        Self { url, public_origin }
    }
}

impl Default for MatchmakerConfig {
    /// The default configuration: localhost matchmaker, no public origin.
    fn default() -> Self {
        Self {
            url: DEFAULT_MATCHMAKER_URL.to_string(),
            public_origin: None,
        }
    }
}

/// Matchmaker HTTP client. Cheap to clone; every method logs failures at debug and never panics.
#[derive(Clone)]
pub struct MatchmakerClient {
    http: reqwest::Client,
    base_url: String,
}

impl MatchmakerClient {
    /// Create a client for `base_url` (trailing slashes trimmed).
    pub fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        Self { http, base_url }
    }

    /// `POST /matches`: register this host. `None` (logged at debug) when anything fails; retry next beat.
    pub async fn register(&self, body: &RegisterRequest) -> Option<RegisterResponse> {
        let url = format!("{}/matches", self.base_url);
        let response = match self.http.post(&url).json(body).send().await {
            Ok(response) => response,
            Err(e) => {
                tracing::debug!("matchmaker unreachable: {e}");
                return None;
            }
        };
        if !response.status().is_success() {
            tracing::debug!("matchmaker refused the registration: HTTP {}", response.status());
            return None;
        }
        match response.json::<RegisterResponse>().await {
            Ok(parsed) => Some(parsed),
            Err(e) => {
                tracing::debug!("matchmaker answered with an unreadable registration: {e}");
                None
            }
        }
    }

    /// `POST /matches/{id}`: heartbeat. 404/403 are reported as [`HeartbeatOutcome::Expired`].
    pub async fn heartbeat(&self, id: &str, body: &HeartbeatRequest) -> HeartbeatOutcome {
        let url = format!("{}/matches/{id}", self.base_url);
        let response = match self.http.post(&url).json(body).send().await {
            Ok(response) => response,
            Err(e) => {
                tracing::debug!("matchmaker unreachable: {e}");
                return HeartbeatOutcome::Unreachable;
            }
        };
        match response.status().as_u16() {
            200 => HeartbeatOutcome::Ok,
            404 | 403 => HeartbeatOutcome::Expired,
            status => {
                tracing::debug!("matchmaker answered the heartbeat with HTTP {status}");
                HeartbeatOutcome::Unreachable
            }
        }
    }

    /// `DELETE /matches/{id}`: delist (best effort). True when the entry is gone.
    ///
    /// A missing entry (404) counts as delisted: the goal is that no listing remains.
    pub async fn delist(&self, id: &str, secret: &str) -> bool {
        let url = format!("{}/matches/{id}", self.base_url);
        let body = DelistRequest {
            secret: secret.to_string(),
        };
        let response = match self.http.delete(&url).json(&body).send().await {
            Ok(response) => response,
            Err(e) => {
                tracing::debug!("matchmaker unreachable: {e}");
                return false;
            }
        };
        match response.status().as_u16() {
            200 | 404 => true,
            status => {
                tracing::debug!("matchmaker refused the delist: HTTP {status}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::{Path, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::post,
    };
    use std::sync::{Arc, Mutex};

    fn sample_register() -> RegisterRequest {
        RegisterRequest {
            name: "VOIDWRAP RUN".to_string(),
            version: "0.4.0-alpha.1".to_string(),
            port: 7879,
            players: 2,
            max_players: 8,
            level: "3".to_string(),
            phase: "playing".to_string(),
            joinable: true,
            address: Some("203.0.113.5".to_string()),
        }
    }

    #[test]
    fn register_body_matches_contract_field_for_field() {
        let value = serde_json::to_value(sample_register()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "name": "VOIDWRAP RUN",
                "version": "0.4.0-alpha.1",
                "port": 7879,
                "players": 2,
                "max_players": 8,
                "level": "3",
                "phase": "playing",
                "joinable": true,
                "address": "203.0.113.5",
            })
        );
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "address",
                "joinable",
                "level",
                "max_players",
                "name",
                "phase",
                "players",
                "port",
                "version"
            ]
        );
    }

    #[test]
    fn register_body_serializes_null_address() {
        let body = RegisterRequest {
            address: None,
            ..sample_register()
        };
        assert_eq!(serde_json::to_value(&body).unwrap()["address"], serde_json::Value::Null);
    }

    #[test]
    fn heartbeat_body_matches_contract_field_for_field() {
        let body = HeartbeatRequest {
            secret: "abc123".to_string(),
            players: 4,
            level: "7".to_string(),
            phase: "playing".to_string(),
            joinable: false,
        };
        let value = serde_json::to_value(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "secret": "abc123",
                "players": 4,
                "level": "7",
                "phase": "playing",
                "joinable": false,
            })
        );
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["joinable", "level", "phase", "players", "secret"]);
    }

    #[test]
    fn delist_body_matches_contract_field_for_field() {
        let body = DelistRequest {
            secret: "abc123".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({ "secret": "abc123" })
        );
    }

    #[test]
    fn game_snapshot_default_is_unknown() {
        assert_eq!(
            GameSnapshot::default(),
            GameSnapshot {
                players: 0,
                level: 1,
                phase: "unknown".to_string(),
            }
        );
    }

    #[test]
    fn matchmaker_config_defaults_to_localhost() {
        let config = MatchmakerConfig::default();
        assert_eq!(config.url, DEFAULT_MATCHMAKER_URL);
        assert_eq!(config.public_origin, None);
    }

    #[tokio::test]
    async fn client_reports_unreachable_matchmaker_without_failing() {
        let client = MatchmakerClient::new("http://127.0.0.1:9");
        assert!(client.register(&sample_register()).await.is_none());
        let heartbeat = HeartbeatRequest {
            secret: "s".to_string(),
            players: 0,
            level: "1".to_string(),
            phase: "unknown".to_string(),
            joinable: false,
        };
        assert_eq!(client.heartbeat("id", &heartbeat).await, HeartbeatOutcome::Unreachable);
        assert!(!client.delist("id", "s").await);
    }

    /// Inline contract stub: `/matches` implemented exactly as the frozen contract describes.
    #[derive(Clone, Default)]
    struct StubState {
        /// (method, path, body) for every request the stub answered.
        requests: Arc<Mutex<Vec<(String, String, serde_json::Value)>>>,
    }

    fn record(state: &StubState, method: &str, path: &str, body: &serde_json::Value) {
        state
            .requests
            .lock()
            .unwrap()
            .push((method.to_string(), path.to_string(), body.clone()));
    }

    async fn stub_register(
        State(state): State<StubState>,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        record(&state, "POST", "/matches", &body);
        Json(serde_json::json!({ "id": "stub-id", "secret": "stub-secret", "address": "203.0.113.5" }))
    }

    async fn stub_heartbeat(
        State(state): State<StubState>,
        Path(id): Path<String>,
        Json(body): Json<serde_json::Value>,
    ) -> Response {
        if id != "stub-id" {
            return StatusCode::NOT_FOUND.into_response();
        }
        record(&state, "POST", &format!("/matches/{id}"), &body);
        if body["secret"] == "stub-secret" {
            StatusCode::OK.into_response()
        } else {
            StatusCode::FORBIDDEN.into_response()
        }
    }

    async fn stub_delist(
        State(state): State<StubState>,
        Path(id): Path<String>,
        Json(body): Json<serde_json::Value>,
    ) -> Response {
        if id != "stub-id" {
            return StatusCode::NOT_FOUND.into_response();
        }
        record(&state, "DELETE", &format!("/matches/{id}"), &body);
        if body["secret"] == "stub-secret" {
            StatusCode::OK.into_response()
        } else {
            StatusCode::FORBIDDEN.into_response()
        }
    }

    #[tokio::test]
    async fn client_round_trips_against_contract_stub() {
        let state = StubState::default();
        let requests = Arc::clone(&state.requests);
        let app = Router::new()
            .route("/matches", post(stub_register))
            .route("/matches/{id}", post(stub_heartbeat).delete(stub_delist))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = MatchmakerClient::new(&format!("http://{addr}"));
        let register = sample_register();
        let response = client.register(&register).await.unwrap();
        assert_eq!(response.id, "stub-id");
        assert_eq!(response.secret, "stub-secret");
        assert_eq!(response.address.as_deref(), Some("203.0.113.5"));

        let heartbeat = HeartbeatRequest {
            secret: response.secret.clone(),
            players: 3,
            level: "4".to_string(),
            phase: "playing".to_string(),
            joinable: true,
        };
        assert_eq!(client.heartbeat(&response.id, &heartbeat).await, HeartbeatOutcome::Ok);

        let wrong_secret = HeartbeatRequest {
            secret: "not-the-secret".to_string(),
            ..heartbeat.clone()
        };
        assert_eq!(
            client.heartbeat(&response.id, &wrong_secret).await,
            HeartbeatOutcome::Expired
        );

        assert!(client.delist(&response.id, &response.secret).await);

        let recorded = {
            let guard = requests.lock().unwrap();
            guard.clone()
        };
        assert_eq!(recorded.len(), 4);
        assert_eq!(recorded[0].0, "POST");
        assert_eq!(recorded[0].1, "/matches");
        assert_eq!(recorded[0].2["port"], 7879);
        assert_eq!(recorded[0].2["max_players"], 8);
        assert_eq!(recorded[0].2["address"], "203.0.113.5");
        assert_eq!(recorded[1].1, "/matches/stub-id");
        assert_eq!(recorded[1].2["secret"], "stub-secret");
        assert_eq!(recorded[3].0, "DELETE");
        assert_eq!(recorded[3].2, serde_json::json!({ "secret": "stub-secret" }));
    }
}
