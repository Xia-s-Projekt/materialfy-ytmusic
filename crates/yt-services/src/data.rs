//! Data helpers: `known` cache and `home_feed` stub (old sf-services layout).

pub mod known {
    use md3_model::TrackItem;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn cache() -> &'static Mutex<HashMap<String, TrackItem>> {
        static CACHE: OnceLock<Mutex<HashMap<String, TrackItem>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }
    pub fn get(uri: &str) -> Option<TrackItem> {
        cache().lock().ok()?.get(uri).cloned()
    }
    pub fn remember(items: impl Iterator<Item = TrackItem>) {
        if let Ok(mut c) = cache().lock() {
            for item in items {
                c.insert(item.play_uri(), item);
            }
        }
    }
}

pub mod home_feed {
    pub const DJ_URI: &str = "ytmusic:radio:system";
}
