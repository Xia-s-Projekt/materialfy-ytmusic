//! Glue without QML (provider "services-extra"): settings → player (autoplay), building EngineConfig from settings,
//! media keys and the "Now playing" system panel through fastframe-now-playing: Windows — SMTC (its own thread and
//! a hidden window, no Qt HWND needed), macOS — Now Playing, Linux — MPRIS org.mpris.MediaPlayer2.materialfy.
use std::sync::Arc;
use std::time::Duration;

use sf_auth::ProxyConfig;
use sf_player::{EngineConfig, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use mf_platform::{SettingKey, SettingsStore};

use crate::AppRequest;

/// "Live" from settings → player: Autoplay, speed and crossfade (the audio output reads the last two itself on every
/// packet). Volume is passed to the player only by the Player bridge (the initial one — PlayerConfig.volume); it does not touch the engine
/// parameters (the restart flag is kept by AuthService).
pub fn spawn_settings_glue(rt: &Handle, settings: Arc<SettingsStore>, player: PlayerHandle) {
    apply_audio_fx(&settings);
    let mut changes = settings.subscribe();
    rt.spawn(async move {
        loop {
            match changes.recv().await {
                Ok(SettingKey::Autoplay) => player.set_autoplay(settings.get_bool(SettingKey::Autoplay)),
                Ok(SettingKey::PlaybackSpeed | SettingKey::Crossfade | SettingKey::DjVoice) => apply_audio_fx(&settings),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    player.set_autoplay(settings.get_bool(SettingKey::Autoplay));
                    apply_audio_fx(&settings);
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

/// Speed and crossfade from settings → audio output.
fn apply_audio_fx(settings: &SettingsStore) {
    use sf_player::engine::fx;
    fx::set_speed(settings.get_f64(SettingKey::PlaybackSpeed) as f32);
    let seconds = settings.get_int(SettingKey::Crossfade).clamp(0, mf_platform::settings::CROSSFADE_MAX_S) as u64;
    fx::set_crossfade(Duration::from_secs(seconds));
    fx::set_dj_voice(settings.get_bool(SettingKey::DjVoice));
}

/// EngineConfig from settings: device name (DeviceName or DevicesService::default_device_name), bitrate,
/// normalization, autoplay, gapless, output device, cache (AudioCache/AudioCacheMb → dirs::audio_cache_dir),
/// volume, proxy. tap — a new AudioTap (PlayerHandle replaces it with its own), eq — by default (there is no EQ UI).
pub fn engine_config(settings: &SettingsStore, proxy: ProxyConfig) -> EngineConfig {
    let volume = settings.get_f64(SettingKey::Volume).clamp(0.0, 1.0);
    let name = settings.get_str(SettingKey::DeviceName);
    let device_name = if name.trim().is_empty() { crate::devices::DevicesService::default_device_name() } else { name };
    let cache_mb = settings.get_int(SettingKey::AudioCacheMb).max(0) as u64;
    let device = settings.get_str(SettingKey::AudioDevice);
    EngineConfig {
        device_name,
        bitrate_kbps: match settings.get_int(SettingKey::Bitrate) {
            96 => 96,
            160 => 160,
            _ => 320,
        },
        normalisation: settings.get_bool(SettingKey::Normalisation),
        autoplay: settings.get_bool(SettingKey::Autoplay),
        gapless: settings.get_bool(SettingKey::Gapless),
        backend: None,
        audio_device: (!device.is_empty()).then_some(device),
        initial_volume: sf_player::volume_to_u16(volume),
        volume_dir: mf_platform::dirs::volume_dir(),
        audio_cache_dir: settings.get_bool(SettingKey::AudioCache).then(mf_platform::dirs::audio_cache_dir),
        audio_cache_limit: Some(cache_mb * 1024 * 1024),
        buffer_ms: sf_player::engine::sink::DEFAULT_BUFFER_MS,
        tap: sf_player::engine::vis::AudioTap::new(),
        eq: sf_player::engine::eq::shared(),
        proxy,
    }
}

/// Stopping the media integration on exit.
pub struct MediaStop {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    media: crate::extra::media::Media,
}

impl MediaStop {
    /// Take down the panel/bus and wait for the thread no longer than `timeout`.
    pub fn stop(self, timeout: Duration) {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        self.media.stop(timeout);
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        let _ = timeout;
    }
}

/// Media keys/SMTC (Windows), MPRIS (Linux), Now Playing (macOS): state from PlayerSnapshot,
/// commands → PlayerHandle; Raise/Quit → AppRequest. None — not supported/did not start.
pub fn start_media_controls(
    rt: &Handle,
    settings: Arc<SettingsStore>,
    player: PlayerHandle,
    requests: broadcast::Sender<AppRequest>,
) -> Option<MediaStop> {
    // the volume from the panel goes to the player; the Player bridge writes it to settings on volumeChanged
    let _ = settings;
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    {
        crate::extra::media::start(rt, player, requests).map(|media| MediaStop { media })
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (rt, player, requests);
        None
    }
}
