//! Headers-auth JSON persistence. File lives at `<state>/ytmusic/headers.json`
//! with `0600` on unix. No keyring dependency in this fork — the file is the
//! secret store (documented in README's "where data is stored").

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadersAuth {
    /// Raw headers-auth JSON as produced by `ytmusicapi` / browser extension.
    pub json: String,
}

impl HeadersAuth {
    pub fn is_valid(&self) -> bool {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&self.json) else {
            return false;
        };
        let obj = v.as_object();
        match obj {
            Some(o) => o.contains_key("cookie") || o.contains_key("authorization"),
            None => false,
        }
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
        let auth = HeadersAuth { json: json.to_string() };
        if !auth.is_valid() {
            anyhow::bail!(
                "Not a ytmusicapi headers file — expected JSON with \"cookie\" or \"authorization\""
            );
        }
        let path = self.file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        write_private(&tmp, json.as_bytes())?;
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
