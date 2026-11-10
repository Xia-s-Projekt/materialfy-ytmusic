//! Play a real YouTube Music track through the app's own path
//! (`PlayerHandle::play_items` → queue actor → InnerTube → rodio).
//!
//! `cargo run -p yt-player --example play_music -- [videoId]`

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use md3_model::{PlayContext, TrackItem};
use yt_api::client::YtClient;
use yt_player::{PlayerConfig, PlayerHandle};

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|info| eprintln!("PANIC: {info}")));
    let id = std::env::args().nth(1).unwrap_or_else(|| "Jb6gcoR266U".to_string());
    let headers = std::env::var_os("MATERIALFY_HEADERS").map(PathBuf::from);
    let api = Arc::new(YtClient::new(headers));
    let cfg = PlayerConfig {
        cover_dir: std::env::temp_dir(),
        session_file: None,
        volume: 0.35,
        http: reqwest::Client::new(),
    };
    let rt = tokio::runtime::Handle::current();
    let player = PlayerHandle::start(&rt, cfg, api).expect("start");
    let _rx = player.watch();

    let track = TrackItem {
        track_id: id.clone(),
        uri: format!("ytmusic:track:{id}"),
        title: "Around the World".into(),
        duration_ms: 429_000,
        ..Default::default()
    };
    player.play_items(vec![track], 0, PlayContext::new("tracks"));

    let mut ok = false;
    for i in 0..15 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let s = player.snapshot();
        println!(
            "t={}s playing={} buffering={} err={:?} pos={}ms codec={:?}",
            i + 1,
            s.playing,
            s.buffering,
            s.error_text,
            s.position_ms,
            s.codec
        );
        if !s.error_text.is_empty() && !s.buffering {
            player.shutdown();
            return;
        }
        if s.position_ms > 5000 {
            ok = true;
            break;
        }
    }
    println!("{}", if ok { "OK: music streams from InnerTube" } else { "FAIL: no playback" });
    player.shutdown();
}
