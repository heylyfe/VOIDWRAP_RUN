//! In-game hosting for friends: an extra listener on `0.0.0.0:<HOST_PORT>` serving the
//! same router and the same game over the LAN, plus a best-effort UPnP port mapping
//! ("VOIDWRAP RUN", TCP HOST_PORT, 2 hours) so friends on the internet can join too.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Default port for the hosting listener; override with the `VOIDWRAP_HOST_PORT` environment variable.
pub const DEFAULT_HOST_PORT: u16 = 7879;
/// How long the router is asked to keep the UPnP port mapping.
const UPNP_LEASE_SECS: u64 = 2 * 60 * 60;
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
            UpnpStatus::Open => "Friends anywhere can join with the address below.",
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

/// Shared hosting state for the HTTP routes and the server. Clone into `Arc`s.
pub struct HostManager {
    main_port: u16,
    host_port: u16,
    forced_open: bool,
    inner: Mutex<HostInner>,
}

struct HostInner {
    /// Shutdown signal for the hosting listener, present while it runs.
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    /// Handle of the hosting listener task, present while it runs.
    listener: Option<tokio::task::JoinHandle<()>>,
    /// UPnP gateway connection, kept so the mapping can be removed on stop/exit.
    gateway: Option<igd_next::aio::Gateway<igd_next::aio::tokio::Tokio>>,
    /// Current UPnP state.
    upnp: UpnpStatus,
    /// Public IP as reported by the router, if any.
    public: Option<IpAddr>,
}

impl HostManager {
    /// Create a manager. `forced_open` must be true when the server itself already
    /// listens on all interfaces (`VOIDWRAP_HOST=0.0.0.0` or `PORT` set).
    pub fn new(main_port: u16, host_port: u16, forced_open: bool) -> Self {
        Self {
            main_port,
            host_port,
            forced_open,
            inner: Mutex::new(HostInner {
                shutdown: None,
                listener: None,
                gateway: None,
                upnp: UpnpStatus::Off,
                public: None,
            }),
        }
    }

    /// Port reported to friends.
    fn reported_port(&self) -> u16 {
        if self.forced_open {
            self.main_port
        } else {
            self.host_port
        }
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

    /// Stop hosting: close the listener and remove the UPnP mapping (best effort).
    pub async fn stop_hosting(&self) -> HostStatus {
        if self.forced_open {
            // Keep today's behaviour: the main listener stays open; only the report changes.
            return self.status().await;
        }
        let (shutdown, task, gateway) = {
            let mut inner = self.inner.lock().await;
            inner.upnp = UpnpStatus::Off;
            inner.public = None;
            (inner.shutdown.take(), inner.listener.take(), inner.gateway.take())
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
            }
            Err(e) => {
                tracing::warn!("UPnP setup failed: {e}");
                inner.upnp = UpnpStatus::Failed;
                inner.public = None;
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
            "Friends anywhere can join with the address below."
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
}
