//! Resolve an InnerTube audio URL without downloading anything.
//!
//! `cargo run -p yt-api --example dl_test -- [videoId]`

use yt_api::client::YtClient;

#[tokio::main]
async fn main() {
    let id = std::env::args().nth(1).unwrap_or_else(|| "07Qzp6RlybE".to_string());
    let headers = std::env::var_os("MATERIALFY_HEADERS").map(std::path::PathBuf::from);
    let client = YtClient::new(headers);
    let t0 = std::time::Instant::now();
    match client.get_stream_url(&id).await {
        Ok(v) => {
            let url = v.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let bitrate = v.get("bitrate").and_then(|b| b.as_u64()).unwrap_or(0);
            let mime = v.get("mime").and_then(|m| m.as_str()).unwrap_or("");
            println!("OK {mime} {bitrate}bps elapsed={:?}", t0.elapsed());
            println!("{url}");
        }
        Err(e) => println!("ERR {e:?} elapsed={:?}", t0.elapsed()),
    }
}
