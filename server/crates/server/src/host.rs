//! In-game hosting for friends: an extra listener on `0.0.0.0:<HOST_PORT>` serving the
//! same router and the same game over the LAN, plus a best-effort UPnP port mapping
//! ("VOIDWRAP RUN", TCP HOST_PORT, 2 hours) so friends on the internet can join too.
//!
//! Also lists the host on the matchmaker once hosting settles (after the UPnP
//! attempt), heartbeats every 10 seconds, and delists on stop/shutdown. The
//! matchmaker is optional infrastructure: being unable to reach it never affects the game.

use crate::matchmaker_client::{
    GameStateHandle, HeartbeatOutcome, HeartbeatRequest, MatchmakerClient, MatchmakerConfig, MatchmakerSession,
    RegisterRequest,
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Default port for the hosting listener; override with the `VOIDWRAP_HOST_PORT` environment variable.
pub const DEFAULT_HOST_PORT: u16 = 7879;
/// How long the router is asked to keep the UPnP port mapping.
const UPNP_LEASE_SECS: u64 = 2 * 60 * 60;
/// How often an open UPnP mapping is renewed (well inside the 2 hour lease).
///
/// Lease-expiry choice: the mapping is re-asserted every 90 minutes (it never
/// silently lapses). If a re-assert fails, the UPnP state becomes `Failed` and the
/// next heartbeat carries `joinable: false` until the router is asked again.
const UPNP_RENEW_INTERVAL: Duration = Duration::from_secs(90 * 60);
/// How long to wait for the UPnP gateway to answer the gateway search.
const UPNP_SEARCH_TIMEOUT: Duration = Duration::from_secs(3);
/// How long to wait for the router to remove the UPnP mapping when hosting stops or the program exits.
const UPNP_REMOVE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for the hosting listener to close before aborting it.
const HOST_STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// Description attached to the UPnP port mapping (shown in the router's port-forwarding table).
const UPNP_DESCRIPTION: &str = "VOIDWRAP RUN";
/// Message used when the server itself listens on all interfaces (`VOIDWRAP_HOST=0.0.0.0`).
const FORCED_OPEN_MESSAGE: &str = "Hosting is on because VOIDWRAP_HOST=0.0.0.0 was set when the server started. Friends can join with the addresses below.";
/// How often the matchmaker is heartbeated while hosting.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// Display name reported to the matchmaker.
const MATCHMAKER_NAME: &str = "VOIDWRAP RUN";
/// Maximum players reported to the matchmaker (the game has no hard cap today).
const MAX_PLAYERS: u32 = 8;
/// Workspace game version reported to the matchmaker.
const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// UPnP state shown to the player: "off" | "trying" | "open" | "failed".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpnpStatus {
    /// Hosting is off, or UPnP has not been attempted.
    Off,
    /// Asking the router to open the port.
    Trying,
    /// Router opened the port; friends anywhere can join.
    Open,
    /// Router refused or was unreachable; LAN friends can still join.
    Failed,
}

impl UpnpStatus {
    /// Plain-words sentence for the player (for non-technical players).
    pub fn message(self) -> &'static str {
        match self {
            UpnpStatus::Off => "Hosting is off. Turn it on so friends can join.",
            UpnpStatus::Trying => "Asking your router to open the door for friends...",
            UpnpStatus::Open => "Friends anywhere can join with the 'Friends anywhere' address.",
            UpnpStatus::Failed => {
                "Your router did not allow automatic setup. Friends on your Wi-Fi can still join; for internet play see HOW-TO-PLAY.md."
            }
        }
    }
}

/// JSON body returned by `GET /api/host` and `POST /api/host`.
#[derive(Clone, Debug, Serialize)]
pub struct HostStatus {
    /// True while the hosting listener is active (or the server was started open).
    pub hosting: bool,
    /// Port friends connect to.
    pub port: u16,
    /// This machine's private IPv4 addresses (LAN), deduplicated and sorted.
    pub lan: Vec<String>,
    /// Public address friends on the internet can use, if the router reported one.
    pub public: Option<String>,
    /// UPnP state.
    pub upnp: UpnpStatus,
    /// Plain-words message for the player.
    pub message: String,
}

/// JSON body accepted by `POST /api/host`.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct HostRequest {
    /// True to start hosting, false to stop.
    pub enable: bool,
}

/// Parse the hosting port from `VOIDWRAP_HOST_PORT`.
/// Missing, empty, zero or unparseable values fall back to [`DEFAULT_HOST_PORT`].
pub fn parse_host_port(raw: Option<&str>) -> u16 {
    raw.and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|&p| p != 0)
        .unwrap_or(DEFAULT_HOST_PORT)
}

/// True when the peer is on this machine (127.0.0.0/8 or `[::1]`).
/// Used to keep `/api/host` local-only, so friends can never toggle hosting.
pub fn is_loopback_peer(peer: &SocketAddr) -> bool {
    peer.ip().is_loopback()
}

/// True for RFC 1918 private IPv4 addresses (10/8, 172.16/12, 192.168/16).
pub fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
}

/// Filter to private addresses, deduplicate and sort them (stable output, testable).
pub fn private_ipv4_strings(addrs: &[Ipv4Addr]) -> Vec<String> {
    let mut list: Vec<String> = addrs
        .iter()
        .filter(|ip| is_private_ipv4(**ip))
        .map(|ip| ip.to_string())
        .collect();
    list.sort();
    list.dedup();
    list
}

/// Local IPv4 address this machine would use to reach `target`
/// (a UDP `connect` selects the route; no packet is ever sent).
fn outbound_ipv4(target: Ipv4Addr) -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((target, 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(_) => None,
    }
}

/// This machine's private IPv4 addresses, best effort. Sends no traffic.
pub fn lan_ipv4_addresses() -> Vec<String> {
    // Two well-known public resolvers usually route via the primary interface; the
    // connect only picks a route, it does not send a packet.
    let probes = [Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(1, 1, 1, 1)];
    let found: Vec<Ipv4Addr> = probes.into_iter().filter_map(outbound_ipv4).collect();
    private_ipv4_strings(&found)
}

/// Compute the `joinable` flag reported to the matchmaker.
///
/// A forced-open host (the Render deployment) is always joinable. A normal host is
/// joinable only while its UPnP mapping is open; waiting on the router or a lapsed
/// mapping reports `joinable: false`.
fn matchmaker_joinable(forced_open: bool, upnp: UpnpStatus) -> bool {
    forced_open || upnp == UpnpStatus::Open
}

/// Cross-check the address we send with the matchmaker's echo.
///
/// On a mismatch the matchmaker's address wins (it may know how the world reaches us,
/// for example when the router reports no public address) and a warning is logged.
/// The sent address is kept only when the matchmaker reports none.
fn reconcile_echo(sent: Option<&str>, echo: Option<&str>) -> Option<String> {
    match (sent, echo) {
        (Some(sent), Some(echo)) => {
            if sent != echo {
                tracing::warn!("matchmaker reports address {echo}, we sent {sent}; trusting the matchmaker");
            }
            Some(echo.to_string())
        }
        (None, Some(echo)) => Some(echo.to_string()),
        (Some(_), None) => {
            tracing::debug!("matchmaker did not report the address it recorded; keeping {sent:?}");
            sent.map(str::to_string)
        }
        (None, None) => None,
    }
}

/// Shared hosting state for the HTTP routes and the server. Clone into `Arc`s.
pub struct HostManager {
    main_port: u16,
    host_port: u16,
    forced_open: bool,
    inner: Mutex<HostInner>,
    /// Matchmaker connection settings (`MATCHMAKER_URL`, `PUBLIC_ORIGIN`).
    matchmaker: MatchmakerConfig,
    /// Matchmaker HTTP client shared with the heartbeat task.
    matchmaker_client: MatchmakerClient,
    /// Live game state published by the game task (players/level/phase).
    game_state: Arc<GameStateHandle>,
}

struct HostInner {
    /// Shutdown signal for the hosting listener, present while it runs.
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    /// Handle of the hosting listener task, present while it runs.
    listener: Option<tokio::task::JoinHandle<()>>,
    /// UPnP gateway connection, kept so the mapping can be removed on stop/exit.
    gateway: Option<igd_next::aio::Gateway<igd_next::aio::tokio::Tokio>>,
    /// Background task that renews the UPnP lease while the mapping is open.
    renewal: Option<tokio::task::JoinHandle<()>>,
    /// Current UPnP state.
    upnp: UpnpStatus,
    /// Public IP as reported by the router, if any.
    public: Option<IpAddr>,
    /// Matchmaker listing session, present while registered; taken on stop/delist.
    matchmaker: Option<MatchmakerSession>,
    /// Background registration/heartbeat task, spawned once hosting settles; aborted on stop.
    matchmaker_task: Option<tokio::task::JoinHandle<()>>,
}

impl HostManager {
    /// Create a manager. `forced_open` must be true when the server itself already
    /// listens on all interfaces (`VOIDWRAP_HOST=0.0.0.0` or `PORT` set).
    ///
    /// The matchmaker is configured from `MATCHMAKER_URL` (default
    /// `http://localhost:8080`) and `PUBLIC_ORIGIN`.
    pub fn new(main_port: u16, host_port: u16, forced_open: bool) -> Self {
        Self::with_matchmaker(main_port, host_port, forced_open, MatchmakerConfig::from_env())
    }

    /// Create a manager with explicit matchmaker settings (used by tests).
    pub fn with_matchmaker(main_port: u16, host_port: u16, forced_open: bool, matchmaker: MatchmakerConfig) -> Self {
        let matchmaker_client = MatchmakerClient::new(&matchmaker.url);
        Self {
            main_port,
            host_port,
            forced_open,
            inner: Mutex::new(HostInner {
                shutdown: None,
                listener: None,
                gateway: None,
                renewal: None,
                upnp: UpnpStatus::Off,
                public: None,
                matchmaker: None,
                matchmaker_task: None,
            }),
            matchmaker,
            matchmaker_client,
            game_state: Arc::new(GameStateHandle::new()),
        }
    }

    /// Share the game task's live state with the heartbeat (call once at startup).
    pub fn set_game_state(&mut self, state: Arc<GameStateHandle>) {
        self.game_state = state;
    }

    /// Port reported to friends.
    fn reported_port(&self) -> u16 {
        if self.forced_open {
            self.main_port
        } else {
            self.host_port
        }
    }

    /// Address to register: forced-open hosts advertise `PUBLIC_ORIGIN`; everyone else
    /// prefers the router-reported public address (`inner.public`).
    fn registration_address(&self, inner: &HostInner) -> Option<String> {
        if self.forced_open {
            return self.matchmaker.public_origin.clone();
        }
        inner.public.map(|ip| ip.to_string())
    }

    /// Build the JSON status from a locked inner state (synchronous).
    fn snapshot(&self, inner: &HostInner) -> HostStatus {
        let hosting = self.forced_open || inner.listener.is_some();
        let upnp = if hosting { inner.upnp } else { UpnpStatus::Off };
        let message = if self.forced_open {
            FORCED_OPEN_MESSAGE.to_string()
        } else {
            upnp.message().to_string()
        };
        HostStatus {
            hosting,
            port: self.reported_port(),
            lan: lan_ipv4_addresses(),
            public: if hosting {
                inner.public.map(|ip| ip.to_string())
            } else {
                None
            },
            upnp,
            message,
        }
    }

    /// Build the JSON status on failure to open the hosting port.
    fn bind_failure(&self, e: &std::io::Error) -> HostStatus {
        HostStatus {
            hosting: false,
            port: self.host_port,
            lan: lan_ipv4_addresses(),
            public: None,
            upnp: UpnpStatus::Off,
            message: format!("Could not open port {} so friends can join ({e}).", self.host_port),
        }
    }

    /// Current hosting status.
    pub async fn status(&self) -> HostStatus {
        let inner = self.inner.lock().await;
        self.snapshot(&inner)
    }

    /// Register with the matchmaker when hosting was forced open at startup (the Render
    /// deployment): a forced-open server never calls [`HostManager::start_hosting`], so
    /// this is the hook that lists it (`joinable: true`, address from `PUBLIC_ORIGIN`).
    /// A no-op otherwise; never fails the server.
    pub async fn register_if_forced_open(self: &Arc<Self>) {
        if self.forced_open {
            self.matchmaker_settle().await;
        }
    }

    /// Start the hosting listener (no-op when already hosting or when forced open).
    /// `app` must serve the same router as the main listener, with the same game.
    pub async fn start_hosting(self: &Arc<Self>, app: axum::Router) -> HostStatus {
        if self.forced_open {
            return self.status().await;
        }
        {
            let inner = self.inner.lock().await;
            if inner.listener.is_some() {
                return self.snapshot(&inner);
            }
        }
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), self.host_port);
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(e) => {
                tracing::error!("cannot start hosting on {addr}: {e}");
                return self.bind_failure(&e);
            }
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let server = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                });
            if let Err(e) = server.await {
                tracing::error!("hosting listener error: {e}");
            }
        });
        {
            let mut inner = self.inner.lock().await;
            inner.shutdown = Some(shutdown_tx);
            inner.listener = Some(task);
            inner.upnp = UpnpStatus::Trying;
            inner.public = None;
        }
        tracing::info!(
            "hosting listener on 0.0.0.0:{} (LAN); asking the router for a UPnP mapping",
            self.host_port
        );
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            manager.upnp_try().await;
        });
        self.status().await
    }

    /// Stop hosting: close the listener, remove the UPnP mapping (best effort) and delist
    /// from the matchmaker (best effort; the request is time-bounded and shutdown is never blocked).
    pub async fn stop_hosting(&self) -> HostStatus {
        if self.forced_open {
            // Keep today's behaviour: the main listener stays open; only the report changes.
            return self.status().await;
        }
        let (shutdown, task, gateway, matchmaker_session) = {
            let mut inner = self.inner.lock().await;
            if let Some(renewal) = inner.renewal.take() {
                renewal.abort();
            }
            if let Some(matchmaker_task) = inner.matchmaker_task.take() {
                matchmaker_task.abort();
            }
            inner.upnp = UpnpStatus::Off;
            inner.public = None;
            let session = inner.matchmaker.take();
            (
                inner.shutdown.take(),
                inner.listener.take(),
                inner.gateway.take(),
                session,
            )
        };
        if let Some(tx) = shutdown {
            let _ = tx.send(());
        }
        if let Some(mut handle) = task
            && tokio::time::timeout(HOST_STOP_TIMEOUT, &mut handle).await.is_err()
        {
            // A friend still has an open connection; close anyway.
            handle.abort();
            let _ = handle.await;
        }
        if let Some(gateway) = gateway {
            let _ = tokio::time::timeout(UPNP_REMOVE_TIMEOUT, upnp_close(gateway, self.host_port)).await;
        }
        if let Some(session) = matchmaker_session
            && !self.matchmaker_client.delist(&session.id, &session.secret).await
        {
            tracing::debug!("matchmaker: delist did not confirm; the entry expires on its own");
        }
        self.status().await
    }

    /// Ask the router to forward our hosting port to this machine's LAN address.
    /// Runs in the background; on success friends anywhere can join with `public`.
    async fn upnp_try(self: Arc<Self>) {
        let port = self.host_port;
        let result = upnp_open(port).await;
        let mut inner = self.inner.lock().await;
        if inner.listener.is_none() {
            // Hosting was stopped while we were asking the router; undo the mapping.
            drop(inner);
            if let Ok((_, gateway)) = result {
                let _ = tokio::time::timeout(UPNP_REMOVE_TIMEOUT, upnp_close(gateway, port)).await;
            }
            return;
        }
        match result {
            Ok((external, gateway)) => {
                inner.upnp = UpnpStatus::Open;
                inner.public = Some(external);
                inner.gateway = Some(gateway);
                if let Some(old) = inner.renewal.take() {
                    old.abort();
                }
                let manager = Arc::clone(&self);
                inner.renewal = Some(tokio::spawn(async move {
                    manager.upnp_renew_loop().await;
                }));
            }
            Err(e) => {
                tracing::warn!("UPnP setup failed: {e}");
                inner.upnp = UpnpStatus::Failed;
                inner.public = None;
            }
        }
        // Hosting has settled (the router answered, or not): register with the matchmaker.
        drop(inner);
        self.matchmaker_settle().await;
    }

    /// The router only keeps the mapping for the lease (2 hours): re-open it every
    /// [`UPNP_RENEW_INTERVAL`] while it is open. A failed renewal marks UPnP as failed
    /// (the status message then tells the player) and flips the next heartbeat's
    /// `joinable` to false; LAN friends keep working.
    async fn upnp_renew_loop(self: Arc<Self>) {
        loop {
            tokio::time::sleep(UPNP_RENEW_INTERVAL).await;
            if self.inner.lock().await.listener.is_none() {
                return;
            }
            let result = upnp_open(self.host_port).await;
            let mut inner = self.inner.lock().await;
            if inner.listener.is_none() {
                return;
            }
            match result {
                Ok((external, gateway)) => {
                    tracing::info!("UPnP mapping renewed");
                    inner.upnp = UpnpStatus::Open;
                    inner.public = Some(external);
                    inner.gateway = Some(gateway);
                }
                Err(e) => {
                    tracing::warn!("UPnP renewal failed: {e}");
                    inner.upnp = UpnpStatus::Failed;
                    inner.public = None;
                    inner.renewal = None;
                    return;
                }
            }
        }
    }

    /// Spawn the registration/heartbeat task once, now that hosting is active (called when
    /// hosting settles, i.e. after the UPnP attempt for normal hosts, and at startup for
    /// forced-open hosts). A no-op when the task already runs or hosting is off.
    async fn matchmaker_settle(self: &Arc<Self>) {
        {
            let inner = self.inner.lock().await;
            if !self.matchmaker_active(&inner) || inner.matchmaker_task.is_some() {
                return;
            }
        }
        let mut inner = self.inner.lock().await;
        if !self.matchmaker_active(&inner) || inner.matchmaker_task.is_some() {
            return;
        }
        let manager = Arc::clone(self);
        inner.matchmaker_task = Some(tokio::spawn(async move {
            manager.matchmaker_loop().await;
        }));
    }

    /// True while this host should be listed on the matchmaker.
    fn matchmaker_active(&self, inner: &HostInner) -> bool {
        self.forced_open || inner.listener.is_some()
    }

    /// Registration/heartbeat loop. Registers first when there is no session, then heartbeats
    /// every [`HEARTBEAT_INTERVAL`]. An unreachable matchmaker is logged at debug and retried
    /// on the next tick; a 404/403 response drops the session so the next tick re-registers.
    /// Returns (and is never re-spawned by itself) once hosting is no longer active.
    async fn matchmaker_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        loop {
            interval.tick().await;
            let (players, level, phase, joinable, port, address) = {
                let inner = self.inner.lock().await;
                if !self.matchmaker_active(&inner) {
                    return;
                }
                let game = self.game_state.snapshot();
                (
                    game.players,
                    game.level,
                    game.phase,
                    matchmaker_joinable(self.forced_open, inner.upnp),
                    self.reported_port(),
                    self.registration_address(&inner),
                )
            };
            let session = self.inner.lock().await.matchmaker.clone();
            match session {
                None => {
                    let body = RegisterRequest {
                        name: MATCHMAKER_NAME.to_string(),
                        version: GAME_VERSION.to_string(),
                        port,
                        players,
                        max_players: MAX_PLAYERS,
                        level: level.to_string(),
                        phase,
                        joinable,
                        address: address.clone(),
                    };
                    match self.matchmaker_client.register(&body).await {
                        Some(response) => {
                            let address = reconcile_echo(address.as_deref(), response.address.as_deref());
                            let mut inner = self.inner.lock().await;
                            inner.matchmaker = Some(MatchmakerSession {
                                id: response.id,
                                secret: response.secret,
                                address,
                            });
                            tracing::info!("registered with the matchmaker (joinable: {joinable})");
                        }
                        None => {
                            // Debug details were logged inside the client; the next beat registers again.
                        }
                    }
                }
                Some(session) => {
                    let body = HeartbeatRequest {
                        secret: session.secret.clone(),
                        players,
                        level: level.to_string(),
                        phase,
                        joinable,
                    };
                    match self.matchmaker_client.heartbeat(&session.id, &body).await {
                        HeartbeatOutcome::Ok => {}
                        HeartbeatOutcome::Expired => {
                            tracing::debug!(
                                "matchmaker: entry expired or rejected; registering again on the next beat"
                            );
                            self.inner.lock().await.matchmaker = None;
                        }
                        HeartbeatOutcome::Unreachable => {}
                    }
                }
            }
        }
    }
}

/// One UPnP setup: find the gateway (3 s timeout), map TCP `port` for 2 hours
/// ("VOIDWRAP RUN") and read our public IP. The router maps the port to the device
/// that asked for the mapping, which is this machine's LAN address (the address used
/// to reach the gateway). Isolated in one function so the igd-next calls are easy to audit.
async fn upnp_open(port: u16) -> Result<(IpAddr, igd_next::aio::Gateway<igd_next::aio::tokio::Tokio>), String> {
    let mut options = igd_next::SearchOptions::default();
    options.timeout = Some(UPNP_SEARCH_TIMEOUT);
    let gateway = igd_next::aio::tokio::search_gateway(options)
        .await
        .map_err(|e| format!("gateway search failed: {e}"))?;
    let external = gateway
        .get_external_ip()
        .await
        .map_err(|e| format!("could not read the external IP: {e}"))?;
    // The router forwards to this computer's LAN address (the one used to reach the internet).
    let lan: std::net::Ipv4Addr = lan_ipv4_addresses()
        .first()
        .and_then(|a| a.parse().ok())
        .ok_or_else(|| "no local network address found".to_string())?;
    let lease = u32::try_from(UPNP_LEASE_SECS).unwrap_or(u32::MAX);
    gateway
        .add_port(
            igd_next::PortMappingProtocol::TCP,
            port,
            std::net::SocketAddr::from((lan, port)),
            lease,
            UPNP_DESCRIPTION,
        )
        .await
        .map_err(|e| format!("router refused the port mapping: {e}"))?;
    Ok((external, gateway))
}

/// Remove the UPnP port mapping (best effort).
async fn upnp_close(gateway: igd_next::aio::Gateway<igd_next::aio::tokio::Tokio>, port: u16) {
    if let Err(e) = gateway.remove_port(igd_next::PortMappingProtocol::TCP, port).await {
        tracing::warn!("could not remove the UPnP mapping for port {port}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_parsing() {
        assert_eq!(parse_host_port(None), DEFAULT_HOST_PORT);
        assert_eq!(parse_host_port(Some("")), DEFAULT_HOST_PORT);
        assert_eq!(parse_host_port(Some("abc")), DEFAULT_HOST_PORT);
        assert_eq!(parse_host_port(Some("0")), DEFAULT_HOST_PORT);
        assert_eq!(parse_host_port(Some("65536")), DEFAULT_HOST_PORT);
        assert_eq!(parse_host_port(Some("5000")), 5000);
        assert_eq!(parse_host_port(Some("65535")), 65535);
        assert_eq!(parse_host_port(Some(" 7879 ")), 7879);
    }

    #[test]
    fn loopback_peers_are_accepted() {
        let v4: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let v4_high: SocketAddr = "127.0.0.2:1".parse().unwrap();
        let v6: SocketAddr = "[::1]:8080".parse().unwrap();
        assert!(is_loopback_peer(&v4));
        assert!(is_loopback_peer(&v4_high));
        assert!(is_loopback_peer(&v6));
    }

    #[test]
    fn non_loopback_peers_are_rejected() {
        for text in [
            "192.168.1.23:8080",
            "10.0.0.5:80",
            "172.16.0.9:9",
            "8.8.8.8:80",
            "0.0.0.0:80",
            "[::2]:80",
        ] {
            let peer: SocketAddr = text.parse().unwrap();
            assert!(!is_loopback_peer(&peer), "expected {text} to be rejected");
        }
    }

    #[test]
    fn private_ipv4_classification() {
        assert!(is_private_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(10, 255, 255, 255)));
        assert!(is_private_ipv4(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(172, 31, 255, 254)));
        assert!(!is_private_ipv4(Ipv4Addr::new(172, 15, 0, 1)));
        assert!(!is_private_ipv4(Ipv4Addr::new(172, 32, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(192, 168, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(192, 168, 1, 23)));
        assert!(!is_private_ipv4(Ipv4Addr::new(192, 167, 1, 1)));
        assert!(!is_private_ipv4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_private_ipv4(Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn private_ipv4_strings_filters_dedups_sorts() {
        let input = [
            Ipv4Addr::new(192, 168, 1, 2),
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(8, 8, 8, 8),
            Ipv4Addr::new(192, 168, 1, 2),
            Ipv4Addr::new(10, 0, 0, 1),
        ];
        let expected = vec!["10.0.0.1".to_string(), "192.168.1.2".to_string()];
        assert_eq!(private_ipv4_strings(&input), expected);
    }

    #[test]
    fn status_messages_are_plain_words() {
        assert_eq!(
            UpnpStatus::Open.message(),
            "Friends anywhere can join with the 'Friends anywhere' address."
        );
        assert_eq!(
            UpnpStatus::Failed.message(),
            "Your router did not allow automatic setup. Friends on your Wi-Fi can still join; for internet play see HOW-TO-PLAY.md."
        );
        assert!(!UpnpStatus::Off.message().is_empty());
        assert!(!UpnpStatus::Trying.message().is_empty());
        assert_ne!(UpnpStatus::Off.message(), UpnpStatus::Failed.message());
    }

    #[test]
    fn upnp_status_serializes_lowercase() {
        assert_eq!(serde_json::to_value(UpnpStatus::Off).unwrap(), serde_json::json!("off"));
        assert_eq!(
            serde_json::to_value(UpnpStatus::Trying).unwrap(),
            serde_json::json!("trying")
        );
        assert_eq!(
            serde_json::to_value(UpnpStatus::Open).unwrap(),
            serde_json::json!("open")
        );
        assert_eq!(
            serde_json::to_value(UpnpStatus::Failed).unwrap(),
            serde_json::json!("failed")
        );
    }

    #[test]
    fn host_status_json_shape() {
        let status = HostStatus {
            hosting: false,
            port: 7879,
            lan: vec!["192.168.1.23".to_string()],
            public: None,
            upnp: UpnpStatus::Off,
            message: UpnpStatus::Off.message().to_string(),
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["hosting"], serde_json::json!(false));
        assert_eq!(value["port"], serde_json::json!(7879));
        assert_eq!(value["lan"], serde_json::json!(["192.168.1.23"]));
        assert_eq!(value["public"], serde_json::Value::Null);
        assert_eq!(value["upnp"], serde_json::json!("off"));
        assert!(value["message"].is_string());
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["hosting", "lan", "message", "port", "public", "upnp"]);
    }

    #[test]
    fn host_status_json_public_address() {
        let status = HostStatus {
            hosting: true,
            port: 7879,
            lan: vec!["192.168.1.23".to_string()],
            public: Some("203.0.113.5".to_string()),
            upnp: UpnpStatus::Open,
            message: UpnpStatus::Open.message().to_string(),
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["public"], serde_json::json!("203.0.113.5"));
        assert_eq!(value["upnp"], serde_json::json!("open"));
        assert_eq!(value["hosting"], serde_json::json!(true));
    }

    #[test]
    fn host_request_deserializes() {
        let on: HostRequest = serde_json::from_str(r#"{"enable": true}"#).unwrap();
        let off: HostRequest = serde_json::from_str(r#"{"enable": false}"#).unwrap();
        assert!(on.enable);
        assert!(!off.enable);
    }

    #[test]
    fn joinable_reflects_forced_open_and_lease_lapse() {
        // Lease-expiry choice: the mapping is re-asserted every UPNP_RENEW_INTERVAL; if a
        // re-assert fails, UPnP becomes Failed and the next heartbeat reports joinable: false.
        assert!(matchmaker_joinable(true, UpnpStatus::Open));
        assert!(matchmaker_joinable(true, UpnpStatus::Failed));
        assert!(matchmaker_joinable(true, UpnpStatus::Trying));
        assert!(matchmaker_joinable(true, UpnpStatus::Off));
        assert!(matchmaker_joinable(false, UpnpStatus::Open));
        assert!(!matchmaker_joinable(false, UpnpStatus::Failed));
        assert!(!matchmaker_joinable(false, UpnpStatus::Trying));
        assert!(!matchmaker_joinable(false, UpnpStatus::Off));
    }

    #[test]
    fn echo_mismatch_trusts_the_matchmaker() {
        assert_eq!(
            reconcile_echo(Some("1.2.3.4"), Some("5.6.7.8")).as_deref(),
            Some("5.6.7.8")
        );
        assert_eq!(
            reconcile_echo(Some("1.2.3.4"), Some("1.2.3.4")).as_deref(),
            Some("1.2.3.4")
        );
        assert_eq!(reconcile_echo(None, Some("5.6.7.8")).as_deref(), Some("5.6.7.8"));
        assert_eq!(reconcile_echo(Some("1.2.3.4"), None).as_deref(), Some("1.2.3.4"));
        assert_eq!(reconcile_echo(None, None), None);
    }

    #[test]
    fn stops_and_reports_matchmaker_config_defaults() {
        let manager = HostManager::new(7878, 7879, false);
        assert_eq!(manager.matchmaker.url, crate::matchmaker_client::DEFAULT_MATCHMAKER_URL);
    }

    #[tokio::test]
    async fn stop_hosting_delists_best_effort_when_matchmaker_unreachable() {
        let config = MatchmakerConfig {
            url: "http://127.0.0.1:9".to_string(),
            public_origin: None,
        };
        let manager = HostManager::with_matchmaker(7878, 0, false, config);
        {
            let mut inner = manager.inner.lock().await;
            inner.matchmaker = Some(MatchmakerSession {
                id: "id".to_string(),
                secret: "secret".to_string(),
                address: None,
            });
        }
        // Must not panic, hang or become an error; the entry expires on its own at the matchmaker.
        let status = manager.stop_hosting().await;
        assert!(!status.hosting);
        assert!(manager.inner.lock().await.matchmaker.is_none());
    }

    #[tokio::test]
    async fn hosting_works_with_the_matchmaker_unreachable() {
        let config = MatchmakerConfig {
            url: "http://127.0.0.1:9".to_string(),
            public_origin: None,
        };
        let manager = Arc::new(HostManager::with_matchmaker(7878, 0, false, config));
        // Port 0 binds an ephemeral port; the UPnP attempt fails (invalid port) or
        // times out harmlessly in the background; the matchmaker at 127.0.0.1:9 refuses.
        let status = manager.start_hosting(axum::Router::new()).await;
        assert!(status.hosting);
        let status = manager.stop_hosting().await;
        assert!(!status.hosting);
    }

    #[tokio::test]
    async fn manager_starts_off_and_stop_is_noop_when_off() {
        let manager = HostManager::new(7878, 7879, false);
        let status = manager.status().await;
        assert!(!status.hosting);
        assert_eq!(status.port, 7879);
        assert_eq!(status.upnp, UpnpStatus::Off);
        let status = manager.stop_hosting().await;
        assert!(!status.hosting);
    }

    #[tokio::test]
    async fn forced_open_manager_reports_hosting() {
        let manager = HostManager::new(7878, 7879, true);
        let status = manager.status().await;
        assert!(status.hosting);
        assert_eq!(status.port, 7878);
        let status = manager.stop_hosting().await;
        assert!(status.hosting);
    }

    #[tokio::test]
    async fn forced_open_host_registers_with_public_origin() {
        let config = MatchmakerConfig {
            url: "http://127.0.0.1:9".to_string(),
            public_origin: Some("https://voidwrap.example.com".to_string()),
        };
        let manager = Arc::new(HostManager::with_matchmaker(7878, 7879, true, config));
        // The matchmaker is unreachable, so this returns without surfacing an error,
        // but it exercises the forced-open registration path.
        manager.register_if_forced_open().await;
        assert!(manager.inner.lock().await.matchmaker_task.is_some());
    }
}
