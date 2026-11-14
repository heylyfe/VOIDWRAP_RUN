//! Build script for the `server` crate.
//!
//! On Windows this embeds the application icon and version metadata into the finished
//! executable. Everywhere else (and on a non-Windows host cross-compiling for Windows)
//! it only watches the icon file for changes, so Linux builds keep working.

/// Entry point for every build.
fn main() {
    println!("cargo:rerun-if-changed=../../../design/icon/icon.ico");
    embed_windows_resources();
}

/// Compile and link the Windows resources (icon + version strings).
///
/// Only meaningful when both the host and the target are Windows; on other hosts this
/// function does not exist, so `winresource` is never referenced where it is unavailable.
#[cfg(windows)]
fn embed_windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| String::from("0.0.0"));
    let mut resources = winresource::WindowsResource::new();
    // Absolute path: rc.exe runs in OUT_DIR, so a relative path would not resolve.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let icon = std::path::Path::new(&manifest).join("../../../design/icon/icon.ico");
    resources.set_icon(&icon.to_string_lossy());
    resources.set("ProductName", "VOIDWRAP RUN");
    resources.set("FileDescription", "VOIDWRAP RUN");
    resources.set("CompanyName", "VOIDWRAP RUN");
    resources.set("LegalCopyright", "VOIDWRAP RUN contributors");
    resources.set("OriginalFilename", "VOIDWRAP RUN.exe");
    resources.set("FileVersion", &version);
    resources.set("ProductVersion", &version);
    if let Err(error) = resources.compile() {
        // Never fail the build: version metadata is a nicety, and the lead merges what
        // compiles cleanly everywhere, not only on a perfectly set-up Windows box.
        println!("cargo:warning=could not embed the Windows resources: {error}");
    }
}

/// No-op on non-Windows hosts: the winresource crate only exists for Windows.
#[cfg(not(windows))]
fn embed_windows_resources() {}
