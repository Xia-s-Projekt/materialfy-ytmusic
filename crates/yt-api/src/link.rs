//! YouTube / YouTube Music link parsing (replaces Spotify link handling).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedLink {
    pub kind: String,
    pub id: String,
}

fn query_get(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return Some(v.to_string());
        }
    }
    None
}

pub fn parse(text: &str) -> Option<ParsedLink> {
    let text = text.trim();
    if text.chars().count() == 11 && !text.contains('/') && !text.contains('?') {
        return Some(ParsedLink { kind: "track".into(), id: text.to_string() });
    }
    let after = text.split("://").nth(1).unwrap_or(text);
    let (hostpath, query) = after.split_once('?').unwrap_or((after, ""));
    let (host, path) = match hostpath.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (hostpath, "/".to_string()),
    };
    if host == "youtu.be" || host.ends_with(".youtu.be") {
        let id = path.trim_start_matches('/').split('/').next().unwrap_or("");
        if !id.is_empty() {
            return Some(ParsedLink { kind: "track".into(), id: id.to_string() });
        }
        return None;
    }
    if !host.contains("youtube.com") {
        return None;
    }
    if let Some(v) = query_get(query, "v") {
        return Some(ParsedLink { kind: "track".into(), id: v });
    }
    if let Some(list) = query_get(query, "list") {
        if list.starts_with("MPREb_") {
            return Some(ParsedLink { kind: "album".into(), id: list });
        }
        return Some(ParsedLink { kind: "playlist".into(), id: list });
    }
    for prefix in ["/channel/", "/artist/", "/user/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            let id = rest.split('/').next().unwrap_or("");
            if !id.is_empty() {
                return Some(ParsedLink { kind: "artist".into(), id: id.to_string() });
            }
        }
    }
    if path.starts_with("/album/") || path.starts_with("/playlist/") {
        let id = path.rsplit('/').next().unwrap_or("");
        if !id.is_empty() {
            let kind = if path.starts_with("/album/") { "album" } else { "playlist" };
            return Some(ParsedLink { kind: kind.into(), id: id.to_string() });
        }
    }
    None
}

pub fn search_query(link: &ParsedLink) -> Option<String> {
    let _ = link;
    None
}

pub fn open_url(kind: &str, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    match kind {
        "track" => Some(format!("https://music.youtube.com/watch?v={id}")),
        "playlist" => Some(format!("https://music.youtube.com/playlist?list={id}")),
        "album" => Some(format!("https://music.youtube.com/watch?v=&list={id}")),
        "artist" | "channel" => Some(format!("https://music.youtube.com/channel/{id}")),
        _ => Some(format!("https://music.youtube.com/watch?v={id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_common_links() {
        assert_eq!(parse("https://music.youtube.com/watch?v=dQw4w9WgXcQ").unwrap().id, "dQw4w9WgXcQ");
        assert_eq!(parse("https://youtu.be/dQw4w9WgXcQ").unwrap().kind, "track");
        assert_eq!(parse("https://music.youtube.com/playlist?list=PLabc").unwrap().kind, "playlist");
        assert_eq!(parse("https://open.spotify.com/track/x"), None);
    }
}
