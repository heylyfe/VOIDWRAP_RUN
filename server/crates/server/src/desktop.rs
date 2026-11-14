//! Native desktop window for the game (enabled by the `desktop` cargo feature).
//!
//! The server runs on a tokio runtime in a background thread; the main thread owns the
//! tao event loop and a wry WebView loading `http://localhost:<port>/`. Closing the
//! window asks the server to shut down (best effort) and then exits.

#![cfg(feature = "desktop")]

use std::time::{Duration, Instant};

/// Title of the native window.
pub const WINDOW_TITLE: &str = "VOIDWRAP RUN";
/// Window width in logical pixels.
const WINDOW_WIDTH: f64 = 1280.0;
/// Window height in logical pixels.
const WINDOW_HEIGHT: f64 = 800.0;

/// Decide whether to run in a native window. The window is disabled by `--browser` and
/// `--headless`, and when `PORT` is set (hosting platforms, background agents).
pub fn should_open_window(args: &[String], port_env_set: bool) -> bool {
    if port_env_set {
        return false;
    }
    !args.iter().any(|a| a == "--browser" || a == "--headless")
}

/// Wait until the server accepts TCP connections on `port`, or `timeout` passes
/// (in which case the window is opened anyway).
pub fn wait_for_server(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
            return;
        }
        if Instant::now() >= deadline {
            tracing::warn!(
                "server did not accept connections within {} s; opening the window anyway",
                timeout.as_secs()
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Open the native window and block until the user closes it.
/// Sends `shutdown` to the server when the window closes (best effort).
pub fn open_window(port: u16, shutdown: tokio::sync::oneshot::Sender<()>, server_done: std::sync::mpsc::Receiver<()>) {
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;

    let event_loop = EventLoop::<()>::new();
    let window = match WindowBuilder::new()
        .with_title(WINDOW_TITLE)
        .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .with_resizable(true)
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(e) => {
            tracing::error!("could not create the window: {e}");
            let _ = shutdown.send(());
            return;
        }
    };
    let url = format!("http://localhost:{port}/");
    // `window` and `_webview` must stay alive for the whole event loop.
    let _webview = match wry::WebViewBuilder::new().with_url(&url).build(&window) {
        Ok(webview) => webview,
        Err(e) => {
            tracing::error!("could not create the web view ({e}); open {url} in a browser instead");
            let _ = shutdown.send(());
            return;
        }
    };
    let mut shutdown = Some(shutdown);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            if let Some(tx) = shutdown.take() {
                let _ = tx.send(());
                // Give the server time to close the hosting listener and remove the UPnP mapping.
                let _ = server_done.recv_timeout(Duration::from_secs(8));
            }
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::should_open_window;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn window_opens_by_default() {
        assert!(should_open_window(&[], false));
        assert!(should_open_window(&args(&["--something-else"]), false));
    }

    #[test]
    fn browser_and_headless_flags_disable_the_window() {
        assert!(!should_open_window(&args(&["--browser"]), false));
        assert!(!should_open_window(&args(&["--headless"]), false));
        assert!(!should_open_window(&args(&["--browser", "--headless"]), false));
    }

    #[test]
    fn port_env_disables_the_window() {
        assert!(!should_open_window(&[], true));
        assert!(!should_open_window(&args(&["--browser", "--headless"]), true));
    }
}
