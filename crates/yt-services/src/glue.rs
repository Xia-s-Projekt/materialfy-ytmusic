//! Glue: settings → player, media keys stubs.

use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::broadcast;

use mf_platform::{SettingKey, SettingsStore};
use yt_player::PlayerHandle;

use crate::AppRequest;

pub struct MediaStop;

impl MediaStop {
    pub fn stop(&self, _timeout: Duration) {}
}

pub fn spawn_settings_glue(rt: &Handle, settings: Arc<SettingsStore>, player: PlayerHandle) {
    let mut changes = settings.subscribe();
    rt.spawn(async move {
        loop {
            match changes.recv().await {
                Ok(SettingKey::Volume) => {
                    let v = settings.get_f64(SettingKey::Volume).clamp(0.0, 1.0);
                    player.set_volume(v);
                }
                Ok(SettingKey::Autoplay) => {
                    player.set_autoplay(settings.get_bool(SettingKey::Autoplay));
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub fn start_media_controls(
    _rt: &Handle,
    _settings: Arc<SettingsStore>,
    _player: PlayerHandle,
    _app_requests: broadcast::Sender<AppRequest>,
) -> Option<MediaStop> {
    // Media keys / SMTC / MPRIS are owned by materialfy's Qt side in this fork;
    // returning None keeps Services::shutdown happy.
    None
}
