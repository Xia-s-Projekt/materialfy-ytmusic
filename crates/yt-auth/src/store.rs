//! Headers-auth JSON persistence. File lives at `<state>/ytmusic/headers.json`
//! with `0600` on unix. No keyring dependency in this fork — the file is the
//! secret store (documented in README's "where data is stored").

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadersAuth {
    /// Normalized headers-auth JSON: `{"cookie":"..."}` (ytmusicapi derives the
    /// `authorization` header itself; `authorization` is only stored when the
    /// caller supplied it).
    pub json: String,
}

impl HeadersAuth {
    /// Accept pasted text in any of the usual formats and return a normalized
    /// headers-auth JSON string:
    ///
    /// 1. Already-normalized object: `{"cookie":"...","authorization":"..."}`
    ///    (used as-is; string values are re-serialized, other keys ignored).
    /// 2. Cookie-Editor / browser extension export: a JSON array of
    ///    `{"name":"...","value":"...","domain":".youtube.com", ...}` —
    ///    joined into a `Cookie` header string.
    ///
    /// Anything else is rejected with a human-readable error.
    pub fn normalize(pasted: &str) -> anyhow::Result<String> {
        let trimmed = pasted.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Nothing to save.");
        }
        // Not JSON at all: treat as a raw Cookie header string (what DevTools
        // shows under Request Headers -> Cookie, or a plain `a=1; b=2` paste).
        if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
            return Self::normalize_cookie_string(trimmed.to_string());
        }
        let v: serde_json::Value = serde_json::from_str(pasted)
            .map_err(|_| anyhow::anyhow!("Not valid JSON. Paste either: (a) headers-auth JSON object with a cookie field from DevTools request headers, (b) a cookie export JSON array with name/value entries (e.g. from the Cookie-Editor extension), or (c) a plain Cookie header string like `a=1; b=2` copied from DevTools Request Headers."))?;
        match &v {
            serde_json::Value::Object(o) => {
                let cookie = o.get("cookie").or_else(|| o.get("Cookie")).and_then(|c| c.as_str());
                let authorization = o.get("authorization").or_else(|| o.get("Authorization")).and_then(|c| c.as_str());
                if cookie.is_none() && authorization.is_none() {
                    anyhow::bail!("Not a ytmusicapi headers file — expected JSON with \"cookie\" or \"authorization\" (or paste a cookie export array instead).");
                }
                let mut map = serde_json::Map::new();
                if let Some(c) = cookie {
                    map.insert("cookie".into(), serde_json::Value::String(c.to_string()));
                }
                if let Some(a) = authorization {
                    map.insert("authorization".into(), serde_json::Value::String(a.to_string()));
                }
                Ok(serde_json::to_string(&serde_json::Value::Object(map))?)
            }
            serde_json::Value::Array(items) => {
                let mut cookie = String::new();
                for item in items {
                    let (Some(name), Some(value)) = (
                        item.get("name").and_then(|n| n.as_str()),
                        item.get("value").and_then(|v| v.as_str()),
                    ) else {
                        anyhow::bail!("Cookie export entries must have \"name\" and \"value\" string fields.");
                    };
                    if !name.is_empty() {
                        if !cookie.is_empty() {
                            cookie.push_str("; ");
                        }
                        cookie.push_str(name);
                        cookie.push('=');
                        cookie.push_str(value);
                    }
                }
                if cookie.contains("__Secure-3PAPISID") || cookie.contains("__Secure-1PAPISID") {
                    let map = serde_json::json!({ "cookie": cookie });
                    Ok(map.to_string())
                } else if cookie.is_empty() {
                    anyhow::bail!("Cookie export is empty.");
                } else {
                    anyhow::bail!("Cookie export does not contain __Secure-3PAPISID — export cookies for music.youtube.com (or copy the request headers directly).");
                }
            }
            _ => anyhow::bail!("Expected a headers-auth JSON object or a cookie export array."),
        }
    }

    /// Raw `Cookie: a=1; b=2` header string -> normalized `{"cookie":"a=1; b=2"}`.
    /// An optional leading `Cookie:` prefix is stripped.
    fn normalize_cookie_string(raw: String) -> anyhow::Result<String> {
        let without_prefix = raw
            .trim()
            .strip_prefix("Cookie:")
            .or_else(|| raw.trim().strip_prefix("cookie:"))
            .unwrap_or(raw.trim());
        let mut pairs: Vec<String> = Vec::new();
        for part in without_prefix.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.split_once('=') {
                Some((name, _value)) if !name.trim().is_empty() => pairs.push(part.to_string()),
                _ => anyhow::bail!("Not a valid Cookie header — expected `name=value; ...` pairs (or paste the headers-auth JSON / cookie export instead)."),
            }
        }
        let cookie = pairs.join("; ");
        if cookie.contains("__Secure-3PAPISID") || cookie.contains("__Secure-1PAPISID") {
            let map = serde_json::json!({ "cookie": cookie });
            Ok(map.to_string())
        } else if cookie.is_empty() {
            anyhow::bail!("Nothing to save.");
        } else {
            anyhow::bail!("Cookie string does not contain __Secure-3PAPISID — copy the full Cookie header from a music.youtube.com request in DevTools (or export cookies for that domain).");
        }
    }

    pub fn is_valid(&self) -> bool {
        Self::normalize(&self.json).is_ok()
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn file_path(&self) -> PathBuf {
        self.dir.join("ytmusic").join("headers.json")
    }

    pub fn load(&self) -> Option<HeadersAuth> {
        let json = std::fs::read_to_string(self.file_path()).ok()?;
        let auth = HeadersAuth { json };
        auth.is_valid().then_some(auth)
    }

    pub fn save(&self, json: &str) -> anyhow::Result<()> {
        let normalized = HeadersAuth::normalize(json)?;
        let path = self.file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        write_private(&tmp, normalized.as_bytes())?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.file_path());
    }

    pub fn has_auth(&self) -> bool {
        self.load().is_some()
    }
}

fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.flush()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedAccount {
    pub channel_id: String,
    pub name: String,
}
