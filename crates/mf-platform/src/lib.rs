//! The platform layer: directories, settings, system preferences,
//! MPRIS (Linux) and "single instance" (Linux — D-Bus, Windows — a named pipe, macOS — a unix socket).
//! Spotify secrets are not stored here — that is sf-auth (keyring-core).
pub mod dirs;
pub mod kwin;
pub mod mpris;
pub mod settings;
pub mod single_instance;
pub mod system;

pub use dirs::CacheUsage;
pub use settings::{SettingKey, SettingType, SettingValue, SettingsStore};
pub use single_instance::InstanceListener;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("D-Bus: {0}")]
    DBus(String),
    #[error("not supported on this OS")]
    Unsupported,
}
