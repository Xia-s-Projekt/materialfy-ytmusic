//! End-to-end playback probe: resolve an InnerTube stream, play it through the
//! real audio device, seek, and confirm the position moves.
//!
//! `cargo run -p yt-player --example play_test -- [videoId]`

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use md3_model::PlayContext;
use yt_api::client::YtClient;
use yt_player::{PlayerConfig, PlayerHandle};

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|info| eprintln!("PANIC: {info}")));
    let id = std::env::args().nth(1).unwrap_or_else(|| "dQw4w9WgXcQ".to_string());
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
    player.play_uris(vec![format!("ytmusic:track:{id}")], 0, PlayContext::new("tracks"));

    let mut played = false;
    for i in 0..12 {
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
        if s.position_ms > 3000 {
            played = true;
            break;
        }
    }
    if !played {
        println!("FAIL: never reached 3s of playback");
        player.shutdown();
        return;
    }

    // Seek into the middle of the track and confirm the clock jumps there.
    player.seek_ms(60_000);
    for i in 0..4 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let s = player.snapshot();
        println!(
            "seek t={}s playing={} err={:?} pos={}ms",
            i + 1,
            s.playing,
            s.error_text,
            s.position_ms
        );
    }
    let s = player.snapshot();
    if s.position_ms >= 60_000 && s.error_text.is_empty() {
        println!("OK: playback streams and seeks");
    } else {
        println!("FAIL: seek did not land (pos={}ms err={:?})", s.position_ms, s.error_text);
    }
    player.shutdown();
}
