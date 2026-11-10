//! On-disk image cache for QML (images.py): QQmlNetworkAccessManagerFactory in C++ (cpp/mf_shell.cpp),
//! each NAM has its own QNetworkDiskCache on the shared directory dirs::cache_subdir("images"), 512 MiB;
//! HTTP/1.1 only (Http2AllowedAttribute = false: over HTTP/2 the cover server answers REFUSED_STREAM).
//! Proxy - the one in effect for the services (AuthService::proxy / watch_proxy), the same as for Web API requests.
use cxx_qt_lib::{QQmlApplicationEngine, QString};
use mf_platform::SettingKey;
use yt_services::{ProxyConfig, Services};
use std::pin::Pin;

pub const CACHE_BYTES: i64 = 512 * 1024 * 1024;

/// Install the factory on the engine (before load) and watch the proxy.
pub fn install_caching_nam(engine: Pin<&mut QQmlApplicationEngine>, rt: &tokio::runtime::Handle, services: &Services) {
    let dir = mf_platform::dirs::cache_subdir("images");
    let dir = QString::from(dir.to_string_lossy().as_ref());
    crate::qt::ffi::ffi::engine_install_caching_nam(engine, &dir, CACHE_BYTES);

    let mut proxy = services.auth.watch_proxy();
    let settings = services.settings.clone();
    let bypass = move || settings.get_bool(SettingKey::CoversBypassProxy);
    apply_proxy(&proxy.borrow_and_update(), bypass());
    let mut changes = services.settings.subscribe();
    // set_image_proxy is thread-safe - from a tokio task, without the GUI thread
    rt.spawn(async move {
        loop {
            tokio::select! {
                changed = proxy.changed() => if changed.is_err() { break },
                key = changes.recv() => match key {
                    Ok(SettingKey::CoversBypassProxy) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
            let config = proxy.borrow_and_update().clone();
            apply_proxy(&config, bypass());
        }
    });
}

/// Modes from cpp/mf_shell.h: 0 - no proxy, 1 - system, 2 - HTTP, 3 - SOCKS5, 4 - block.
/// `bypass` is "Covers bypassing the proxy": images go direct, except for invalid settings (then block, like everything).
fn apply_proxy(config: &ProxyConfig, bypass: bool) {
    let none = QString::default();
    let (mode, manual) = match config {
        // with invalid settings the services block requests - images do not go direct either
        ProxyConfig::Invalid(_) => (4, None),
        _ if bypass => (0, None),
        ProxyConfig::Off => (0, None),
        ProxyConfig::System => (1, None),
        ProxyConfig::Http(manual) => (2, Some(manual)),
        ProxyConfig::Socks(manual) => (3, Some(manual)),
    };
    match manual {
        Some(manual) => crate::qt::ffi::ffi::set_image_proxy(
            mode,
            &QString::from(manual.host()),
            i32::from(manual.port()),
            &QString::from(manual.username()),
            &QString::from(manual.password()),
        ),
        None => crate::qt::ffi::ffi::set_image_proxy(mode, &none, 0, &none, &none),
    }
}
