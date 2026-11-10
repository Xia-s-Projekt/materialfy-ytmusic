//! Spotify images: several CDNs serve the same /image/<id> path. The main one, i.scdn.co, does not open in Russia
//! or answers in 8+ s, so we download only from mirrors (Fastly, Akamai, image-cdn, i2o — the last one recompresses
//! JPEG, same picture); i.scdn.co addresses from Spotify data are rewritten. The same list is in cpp/mf_shell.cpp.
pub const SPOTIFY_IMAGE_HOSTS: [&str; 4] =
    ["image-cdn-fa.spotifycdn.com", "image-cdn-ak.spotifycdn.com", "image-cdn.spotifycdn.com", "i2o.scdn.co"];
/// Hosts whose /image/<id> addresses are rewritten to mirrors.
const KNOWN_HOSTS: [&str; 5] =
    ["i.scdn.co", "image-cdn-fa.spotifycdn.com", "image-cdn-ak.spotifycdn.com", "image-cdn.spotifycdn.com", "i2o.scdn.co"];

/// Addresses worth trying for a picture: for a Spotify image — each mirror in order,
/// for any other — the address itself.
pub fn mirrors(url: &str) -> Vec<String> {
    for host in KNOWN_HOSTS {
        for scheme in ["https://", "http://"] {
            if let Some(path) = url.strip_prefix(scheme).and_then(|rest| rest.strip_prefix(host)) {
                if path.starts_with("/image/") {
                    return SPOTIFY_IMAGE_HOSTS.iter().map(|mirror| format!("https://{mirror}{path}")).collect();
                }
            }
        }
    }
    vec![url.to_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_images_go_through_every_cdn_mirror_first() {
        assert_eq!(
            mirrors("https://i.scdn.co/image/ab67616d0000b273"),
            [
                "https://image-cdn-fa.spotifycdn.com/image/ab67616d0000b273",
                "https://image-cdn-ak.spotifycdn.com/image/ab67616d0000b273",
                "https://image-cdn.spotifycdn.com/image/ab67616d0000b273",
                "https://i2o.scdn.co/image/ab67616d0000b273",
            ]
        );
        assert_eq!(mirrors("https://image-cdn-ak.spotifycdn.com/image/x").len(), 4);
    }

    #[test]
    fn other_addresses_stay_as_they_are() {
        assert_eq!(mirrors("https://mosaic.scdn.co/640/abc"), ["https://mosaic.scdn.co/640/abc"]);
        assert_eq!(mirrors("https://i.scdn.co.evil.example/image/x"), ["https://i.scdn.co.evil.example/image/x"]);
        assert_eq!(mirrors("https://i.scdn.co/other"), ["https://i.scdn.co/other"]);
    }
}
