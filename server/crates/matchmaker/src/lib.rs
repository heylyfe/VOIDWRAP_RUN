//! VOIDWRAP RUN matchmaker: the tiny lobby registry behind `GET /matches`.
//!
//! Every registration lives in memory and disappears [`TTL`] after its last heartbeat;
//! a background task sweeps expired entries every [`SWEEP_INTERVAL`]. The secret handed
//! out by `POST /matches` is the only authority over a registration — there are no
//! accounts. The HTTP contract is frozen (see `tickets/T-064.md`).
//!
//! Every registry method takes the caller's clock reading (`now`) instead of calling
//! [`Instant::now`] itself, so expiry is deterministic in tests.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

/// How long a registration stays listed after its last heartbeat.
pub const TTL: Duration = Duration::from_secs(30);

/// How often the background sweeper drops expired registrations.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(10);

/// Port used when `PORT` is unset or is not a port number.
pub const DEFAULT_PORT: u16 = 8080;

/// Interface the service binds; hosting platforms route to it from outside.
pub const BIND_ADDRESS: [u8; 4] = [0, 0, 0, 0];

/// Bytes of randomness in a match id (hex-encoded, so 32 characters).
const ID_BYTES: usize = 16;

/// Bytes of randomness in a match secret (hex-encoded, so 64 characters).
const SECRET_BYTES: usize = 32;

/// Lower-case hex digits used to encode ids and secrets.
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Request body of `POST /matches`.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateRequest {
    /// Lobby name shown to players.
    pub name: String,
    /// Game version; `GET /matches` filters on an exact match.
    pub version: String,
    /// Port the game server listens on. Accepted for contract compatibility; the frozen
    /// `GET /matches` payload does not expose it.
    pub port: u16,
    /// Players currently in the lobby.
    pub players: u32,
    /// Lobby capacity.
    pub max_players: u32,
    /// Opaque level identifier, stored and echoed verbatim.
    pub level: String,
    /// Opaque phase identifier, stored and echoed verbatim.
    pub phase: String,
    /// Whether new players may join.
    pub joinable: bool,
    /// Address to advertise. When `null` or absent, the request source IP is used.
    pub address: Option<String>,
}

/// Response body of `POST /matches`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateResponse {
    /// Opaque match id, used in the heartbeat and delete URLs.
    pub id: String,
    /// Shared secret; the only authority over this registration.
    pub secret: String,
    /// Address clients should connect to.
    pub address: String,
}

/// Request body of `POST /matches/{id}`.
#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatRequest {
    /// Secret returned by `POST /matches`.
    pub secret: String,
    /// Players currently in the lobby.
    pub players: u32,
    /// Opaque level identifier, stored and echoed verbatim.
    pub level: String,
    /// Opaque phase identifier, stored and echoed verbatim.
    pub phase: String,
    /// Whether new players may join.
    pub joinable: bool,
}

/// Request body of `DELETE /matches/{id}`.
#[derive(Debug, Clone, Deserialize)]
pub struct SecretRequest {
    /// Secret returned by `POST /matches`.
    pub secret: String,
}

/// Query string of `GET /matches`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListQuery {
    /// Only registrations with exactly this version are returned.
    pub version: String,
}

/// One element of the `GET /matches` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListedMatch {
    /// Lobby name.
    pub name: String,
    /// Address clients should connect to.
    pub address: String,
    /// Players currently in the lobby.
    pub players: u32,
    /// Lobby capacity.
    pub max_players: u32,
    /// Opaque level identifier.
    pub level: String,
    /// Opaque phase identifier.
    pub phase: String,
    /// Whether new players may join.
    pub joinable: bool,
    /// Game version.
    pub version: String,
}

/// Why a heartbeat or a delete was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// No live registration with that id: unknown, deleted or expired.
    NotFound,
    /// The supplied secret does not match the registration's secret.
    Forbidden,
}

impl IntoResponse for Reject {
    fn into_response(self) -> Response {
        let status = match self {
            Reject::NotFound => StatusCode::NOT_FOUND,
            Reject::Forbidden => StatusCode::FORBIDDEN,
        };
        status.into_response()
    }
}

/// One live registration.
#[derive(Debug, Clone)]
struct Entry {
    /// Secret that authorises heartbeats and deletes.
    secret: String,
    /// Lobby name.
    name: String,
    /// Game version.
    version: String,
    /// Players currently in the lobby.
    players: u32,
    /// Lobby capacity.
    max_players: u32,
    /// Opaque level identifier.
    level: String,
    /// Opaque phase identifier.
    phase: String,
    /// Whether new players may join.
    joinable: bool,
    /// Address clients should connect to.
    address: String,
    /// Clock reading of the last create or heartbeat.
    last_seen: Instant,
}

/// Shared in-memory registry of live matches.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    /// Every registration, keyed by match id.
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

impl Registry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a match and return its id, secret and advertised address.
    ///
    /// `address` is the already-derived address to advertise (see [`derive_address`]);
    /// `now` is the caller's clock reading.
    pub fn create(&self, request: CreateRequest, address: String, now: Instant) -> CreateResponse {
        let id = random_token(ID_BYTES);
        let secret = random_token(SECRET_BYTES);
        let entry = Entry {
            secret: secret.clone(),
            name: request.name,
            version: request.version,
            players: request.players,
            max_players: request.max_players,
            level: request.level,
            phase: request.phase,
            joinable: request.joinable,
            address: address.clone(),
            last_seen: now,
        };
        self.lock().insert(id.clone(), entry);
        CreateResponse { id, secret, address }
    }

    /// Apply a heartbeat, refreshing the entry's [`TTL`].
    ///
    /// Returns [`Reject::NotFound`] when the id is unknown or already expired, and
    /// [`Reject::Forbidden`] when the secret does not match. `now` is the caller's
    /// clock reading.
    pub fn heartbeat(&self, id: &str, request: HeartbeatRequest, now: Instant) -> Result<(), Reject> {
        let mut entries = self.lock();
        let entry = live_entry(&mut entries, id, now)?;
        if entry.secret != request.secret {
            return Err(Reject::Forbidden);
        }
        entry.players = request.players;
        entry.level = request.level;
        entry.phase = request.phase;
        entry.joinable = request.joinable;
        entry.last_seen = now;
        Ok(())
    }

    /// Remove a registration.
    ///
    /// Returns [`Reject::NotFound`] when the id is unknown or already expired, and
    /// [`Reject::Forbidden`] when the secret does not match. `now` is the caller's
    /// clock reading.
    pub fn delete(&self, id: &str, secret: &str, now: Instant) -> Result<(), Reject> {
        let mut entries = self.lock();
        let entry = live_entry(&mut entries, id, now)?;
        if entry.secret != secret {
            return Err(Reject::Forbidden);
        }
        entries.remove(id);
        Ok(())
    }

    /// List live registrations with exactly `version`, sorted by name then address.
    ///
    /// `now` is the caller's clock reading; entries last seen [`TTL`] ago or earlier are
    /// left out even when the sweeper has not run yet.
    pub fn list(&self, version: &str, now: Instant) -> Vec<ListedMatch> {
        let entries = self.lock();
        let mut listed: Vec<ListedMatch> = entries
            .values()
            .filter(|entry| entry.version == version)
            .filter(|entry| is_live(entry, now))
            .map(|entry| ListedMatch {
                name: entry.name.clone(),
                address: entry.address.clone(),
                players: entry.players,
                max_players: entry.max_players,
                level: entry.level.clone(),
                phase: entry.phase.clone(),
                joinable: entry.joinable,
                version: entry.version.clone(),
            })
            .collect();
        listed.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.address.cmp(&right.address))
        });
        listed
    }

    /// Drop every registration older than [`TTL`] and return how many were removed.
    ///
    /// `now` is the caller's clock reading.
    pub fn sweep(&self, now: Instant) -> usize {
        let mut entries = self.lock();
        let before = entries.len();
        entries.retain(|_, entry| is_live(entry, now));
        before - entries.len()
    }

    /// Lock the map, recovering from a poisoned mutex: a panic while serving one request
    /// must not take the whole registry down.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// True when `entry` was last seen less than [`TTL`] before `now`.
fn is_live(entry: &Entry, now: Instant) -> bool {
    now.saturating_duration_since(entry.last_seen) < TTL
}

/// Borrow the live entry `id`, dropping it first when it has already expired.
fn live_entry<'a>(entries: &'a mut HashMap<String, Entry>, id: &str, now: Instant) -> Result<&'a mut Entry, Reject> {
    let expired = match entries.get(id) {
        None => return Err(Reject::NotFound),
        Some(entry) => !is_live(entry, now),
    };
    if expired {
        entries.remove(id);
        return Err(Reject::NotFound);
    }
    entries.get_mut(id).ok_or(Reject::NotFound)
}

/// Hex-encode `bytes` bytes of randomness taken from the thread RNG.
fn random_token(bytes: usize) -> String {
    let mut token = String::with_capacity(bytes * 2);
    for _ in 0..bytes {
        let byte = rand::random::<u8>();
        token.push(HEX_DIGITS[usize::from(byte >> 4)] as char);
        token.push(HEX_DIGITS[usize::from(byte & 0x0f)] as char);
    }
    token
}

/// Work out the address to advertise for a new registration.
///
/// A client-supplied address wins verbatim. Otherwise the first `X-Forwarded-For` hop is
/// used (Render fronts the service with a proxy, so the socket peer is the proxy), and
/// the socket peer IP is the last resort.
pub fn derive_address(supplied: Option<&str>, headers: &HeaderMap, peer: SocketAddr) -> String {
    if let Some(address) = supplied {
        return address.to_string();
    }
    let forwarded = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|hop| !hop.is_empty());
    if let Some(hop) = forwarded {
        return hop.to_string();
    }
    peer.ip().to_string()
}

/// `GET /health` — liveness probe for the hosting platform.
async fn health() -> StatusCode {
    StatusCode::OK
}

/// `POST /matches` — register a match.
async fn create_match(
    State(registry): State<Registry>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<CreateRequest>,
) -> Response {
    let address = derive_address(request.address.as_deref(), &headers, peer);
    let created = registry.create(request, address, Instant::now());
    (StatusCode::OK, Json(created)).into_response()
}

/// `POST /matches/{id}` — heartbeat.
async fn heartbeat_match(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Json(request): Json<HeartbeatRequest>,
) -> Response {
    match registry.heartbeat(&id, request, Instant::now()) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(reject) => reject.into_response(),
    }
}

/// `DELETE /matches/{id}` — remove a registration.
async fn delete_match(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    Json(request): Json<SecretRequest>,
) -> Response {
    match registry.delete(&id, &request.secret, Instant::now()) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(reject) => reject.into_response(),
    }
}

/// `GET /matches?version=` — list live matches.
///
/// This is the only endpoint a browser calls cross-origin, so it is the only one that
/// carries `Access-Control-Allow-Origin`.
async fn list_matches(State(registry): State<Registry>, Query(query): Query<ListQuery>) -> Response {
    let listed = registry.list(&query.version, Instant::now());
    let mut response = Json(listed).into_response();
    response
        .headers_mut()
        .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}

/// Build the HTTP application.
pub fn router(registry: Registry) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/matches", get(list_matches).post(create_match))
        .route("/matches/{id}", post(heartbeat_match).delete(delete_match))
        .with_state(registry)
}

/// Read `PORT`, falling back to [`DEFAULT_PORT`] when it is unset or not a port number.
pub fn port_from_env() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Spawn the background task that drops expired registrations every [`SWEEP_INTERVAL`].
pub fn spawn_sweeper(registry: Registry) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            registry.sweep(Instant::now());
        }
    })
}

/// Bind `0.0.0.0:$PORT` (default [`DEFAULT_PORT`]) and serve until the process stops.
pub async fn serve(registry: Registry) -> std::io::Result<()> {
    let address = SocketAddr::from((BIND_ADDRESS, port_from_env()));
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("matchmaker listening on http://{address}");
    axum::serve(
        listener,
        router(registry).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// A registration body with everything filled in.
    fn create_request(name: &str, version: &str, address: Option<&str>) -> CreateRequest {
        CreateRequest {
            name: name.to_string(),
            version: version.to_string(),
            port: 7878,
            players: 1,
            max_players: 8,
            level: "level-1".to_string(),
            phase: "lobby".to_string(),
            joinable: true,
            address: address.map(str::to_string),
        }
    }

    /// A heartbeat body carrying `secret`.
    fn heartbeat_request(secret: &str) -> HeartbeatRequest {
        HeartbeatRequest {
            secret: secret.to_string(),
            players: 2,
            level: "level-2".to_string(),
            phase: "playing".to_string(),
            joinable: false,
        }
    }

    /// The socket peer used by the handler tests.
    fn peer() -> SocketAddr {
        SocketAddr::from(([198, 51, 100, 9], 40_000))
    }

    /// Decode a JSON response body.
    async fn json_body<T: serde::de::DeserializeOwned>(response: Response) -> T {
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.expect("response body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[test]
    fn entries_expire_thirty_seconds_after_the_last_heartbeat() {
        let registry = Registry::new();
        let start = Instant::now();
        let created = registry.create(create_request("alpha", "0.4.0", None), "203.0.113.7".to_string(), start);

        assert_eq!(registry.list("0.4.0", start).len(), 1);
        assert_eq!(registry.list("0.4.0", start + TTL - Duration::from_millis(1)).len(), 1);
        assert!(registry.list("0.4.0", start + TTL).is_empty());

        // A heartbeat refreshes the entry, so it survives another full TTL.
        let refreshed = start + Duration::from_secs(20);
        assert_eq!(
            registry.heartbeat(&created.id, heartbeat_request(&created.secret), refreshed),
            Ok(())
        );
        assert_eq!(
            registry.list("0.4.0", refreshed + TTL - Duration::from_millis(1)).len(),
            1
        );
        assert!(registry.list("0.4.0", refreshed + TTL).is_empty());

        // An expired entry is refused even before the sweeper runs.
        let stale = registry.create(create_request("stale", "0.4.0", None), "203.0.113.8".to_string(), start);
        assert_eq!(
            registry.heartbeat(&stale.id, heartbeat_request(&stale.secret), start + TTL),
            Err(Reject::NotFound)
        );
        // The stale entry is gone; the refreshed one is still live at this instant.
        let listed = registry.list("0.4.0", start + TTL);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "alpha");

        // The sweeper drops what the listing already hides.
        assert_eq!(registry.sweep(refreshed + TTL), 1);
        assert_eq!(registry.sweep(refreshed + TTL), 0);
    }

    #[test]
    fn listing_filters_on_an_exact_version_and_echoes_the_heartbeat() {
        let registry = Registry::new();
        let now = Instant::now();
        let alpha = registry.create(create_request("alpha", "0.4.0", None), "203.0.113.1".to_string(), now);
        registry.create(create_request("beta", "0.4.0", None), "203.0.113.2".to_string(), now);
        registry.create(create_request("gamma", "0.5.0", None), "203.0.113.3".to_string(), now);

        let listed = registry.list("0.4.0", now);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "alpha");
        assert_eq!(listed[1].name, "beta");
        assert!(listed.iter().all(|entry| entry.version == "0.4.0"));

        // Exact match only: "0.4" is not "0.4.0".
        assert!(registry.list("0.4", now).is_empty());
        assert_eq!(registry.list("0.5.0", now).len(), 1);

        // level/phase are opaque: stored and echoed verbatim.
        assert_eq!(
            registry.heartbeat(&alpha.id, heartbeat_request(&alpha.secret), now),
            Ok(())
        );
        let listed = registry.list("0.4.0", now);
        assert_eq!(listed[0].level, "level-2");
        assert_eq!(listed[0].phase, "playing");
        assert_eq!(listed[0].players, 2);
        assert!(!listed[0].joinable);
    }

    #[test]
    fn a_supplied_address_is_used_verbatim() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
        assert_eq!(
            derive_address(Some("example.test:7878"), &headers, peer()),
            "example.test:7878"
        );
    }

    #[test]
    fn the_first_forwarded_hop_wins_over_the_socket_peer() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7, 10.0.0.1"));
        assert_eq!(derive_address(None, &headers, peer()), "203.0.113.7");
    }

    #[test]
    fn without_a_forwarded_header_the_socket_peer_is_used() {
        assert_eq!(derive_address(None, &HeaderMap::new(), peer()), "198.51.100.9");
    }

    #[tokio::test]
    async fn health_is_an_empty_200() {
        assert_eq!(health().await, StatusCode::OK);
    }

    #[tokio::test]
    async fn create_uses_the_forwarded_address_when_none_is_supplied() {
        let registry = Registry::new();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7, 10.0.0.1"));
        let response = create_match(
            State(registry.clone()),
            ConnectInfo(peer()),
            headers,
            Json(create_request("alpha", "0.4.0", None)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let created: CreateResponse = json_body(response).await;
        assert_eq!(created.address, "203.0.113.7");
        assert_eq!(created.id.len(), ID_BYTES * 2);
        assert_eq!(created.secret.len(), SECRET_BYTES * 2);
    }

    #[tokio::test]
    async fn wrong_secret_is_forbidden_and_the_right_one_is_accepted() {
        let registry = Registry::new();
        let created = registry.create(
            create_request("alpha", "0.4.0", None),
            "203.0.113.7".to_string(),
            Instant::now(),
        );

        let forbidden = heartbeat_match(
            State(registry.clone()),
            Path(created.id.clone()),
            Json(heartbeat_request("not-the-secret")),
        )
        .await;
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let accepted = heartbeat_match(
            State(registry.clone()),
            Path(created.id.clone()),
            Json(heartbeat_request(&created.secret)),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::OK);

        let unknown = heartbeat_match(
            State(registry.clone()),
            Path("no-such-match".to_string()),
            Json(heartbeat_request(&created.secret)),
        )
        .await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

        let delete_forbidden = delete_match(
            State(registry.clone()),
            Path(created.id.clone()),
            Json(SecretRequest {
                secret: "not-the-secret".to_string(),
            }),
        )
        .await;
        assert_eq!(delete_forbidden.status(), StatusCode::FORBIDDEN);

        let delete_accepted = delete_match(
            State(registry.clone()),
            Path(created.id.clone()),
            Json(SecretRequest {
                secret: created.secret.clone(),
            }),
        )
        .await;
        assert_eq!(delete_accepted.status(), StatusCode::OK);

        let delete_again = delete_match(
            State(registry.clone()),
            Path(created.id),
            Json(SecretRequest { secret: created.secret }),
        )
        .await;
        assert_eq!(delete_again.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_the_listing_carries_the_cors_header() {
        let registry = Registry::new();
        let created = registry.create(
            create_request("alpha", "0.4.0", None),
            "203.0.113.7".to_string(),
            Instant::now(),
        );

        let listing = list_matches(
            State(registry.clone()),
            Query(ListQuery {
                version: "0.4.0".to_string(),
            }),
        )
        .await;
        assert_eq!(listing.status(), StatusCode::OK);
        assert_eq!(
            listing
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .map(HeaderValue::as_bytes),
            Some(&b"*"[..])
        );
        let listed: Vec<ListedMatch> = json_body(listing).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "alpha");
        assert_eq!(listed[0].address, "203.0.113.7");

        let created_response = create_match(
            State(registry.clone()),
            ConnectInfo(peer()),
            HeaderMap::new(),
            Json(create_request("beta", "0.4.0", None)),
        )
        .await;
        assert!(
            created_response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );

        let heartbeat = heartbeat_match(
            State(registry.clone()),
            Path(created.id.clone()),
            Json(heartbeat_request(&created.secret)),
        )
        .await;
        assert!(heartbeat.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

        let deleted = delete_match(
            State(registry.clone()),
            Path(created.id),
            Json(SecretRequest { secret: created.secret }),
        )
        .await;
        assert!(deleted.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }
}
