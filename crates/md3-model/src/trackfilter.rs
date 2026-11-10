//! Search inside a list of tracks (like trackfilter.py in yamusic-md3) — pure functions for the QML TrackFilter type.
//! The query is split into words; each must be found in title, version, artists or album
//! (case-insensitive — str.casefold, 'ё' = 'е').
use serde_json::{Map, Value};

/// casefold + 'ё'→'е'. For full casefold parity: to_lowercase and replacing 'ß' → "ss".
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ß' => out.push_str("ss"),
            // casefold does not distinguish the final sigma
            'ς' => out.push('σ'),
            'ё' => out.push('е'),
            c => out.push(c),
        }
    }
    out
}

/// str(item.get(k) or ""): empty/false values — "", numbers and bool as in Python.
fn field_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(a)) if a.is_empty() => String::new(),
        Some(Value::Object(o)) if o.is_empty() => String::new(),
        Some(other) => other.to_string(),
    }
}

/// The search string for a track: normalize(" ".join(title, version, artists, album)).
pub fn haystack(row: &Map<String, Value>) -> String {
    let parts: Vec<String> = ["title", "version", "artists", "album"].iter().map(|k| field_text(row.get(*k))).collect();
    normalize(&parts.join(" "))
}

/// Query words (normalize + split on whitespace); empty — the filter is inactive.
pub fn query_words(query: &str) -> Vec<String> {
    normalize(query).split_whitespace().map(str::to_owned).collect()
}

pub fn matches(haystack: &str, words: &[String]) -> bool {
    words.iter().all(|w| haystack.contains(w.as_str()))
}

/// How to apply a new filter result to the model (_refresh(reset=False) in yamusic-md3):
/// usually the source changes in one piece — a page load, a new like at the top, a removed like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshPlan {
    /// nothing changed
    Keep,
    /// insert found[at..at + count]
    Insert { at: usize, count: usize },
    Remove { at: usize, count: usize },
    Reset,
}

/// `same` — identity of rows (in the model — `Arc::ptr_eq`).
pub fn refresh_plan<T>(old: &[T], found: &[T], same: impl Fn(&T, &T) -> bool) -> RefreshPlan {
    let common = old.len().min(found.len());
    let mut head = 0;
    while head < common && same(&found[head], &old[head]) {
        head += 1;
    }
    let mut tail = 0;
    while tail < common - head && same(&found[found.len() - 1 - tail], &old[old.len() - 1 - tail]) {
        tail += 1;
    }
    if head + tail == old.len() && found.len() > old.len() {
        RefreshPlan::Insert { at: head, count: found.len() - tail - head }
    } else if head + tail == found.len() && old.len() > found.len() {
        RefreshPlan::Remove { at: head, count: old.len() - found.len() }
    } else if found.len() != old.len() || head != old.len() {
        RefreshPlan::Reset
    } else {
        RefreshPlan::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn normalize_like_casefold() {
        assert_eq!(normalize("HEDGEHOG Straße"), "hedgehog strasse");
        assert_eq!(normalize("ΣΟΦΟΣ"), "σοφοσ");
        assert_eq!(normalize("ё"), "е");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn haystack_fields() {
        let r = row(json!({"title": "Stars", "version": "Remix", "artists": "A, B", "album": "E", "trackId": "1"}));
        assert_eq!(haystack(&r), "stars remix a, b e");
        let r = row(json!({"title": "X", "version": null}));
        assert_eq!(haystack(&r), "x   ");
        let r = row(json!({"title": 0, "version": 12, "artists": true}));
        assert_eq!(haystack(&r), " 12 true ");
    }

    #[test]
    fn query_and_match() {
        assert!(query_words("   ").is_empty());
        let words = query_words("  STARS  remix ");
        assert_eq!(words, ["stars", "remix"]);
        assert!(matches("stars remix a, b e", &words));
        assert!(!matches("stars a", &words));
        assert!(matches("anything", &[]));
    }

    #[test]
    fn plans() {
        let same = |a: &i32, b: &i32| a == b;
        assert_eq!(refresh_plan(&[1, 2, 3], &[1, 2, 3], same), RefreshPlan::Keep);
        assert_eq!(refresh_plan(&[1, 2], &[1, 2, 3, 4], same), RefreshPlan::Insert { at: 2, count: 2 });
        assert_eq!(refresh_plan(&[1, 2], &[0, 1, 2], same), RefreshPlan::Insert { at: 0, count: 1 });
        assert_eq!(refresh_plan(&[1, 2, 3], &[1, 3], same), RefreshPlan::Remove { at: 1, count: 1 });
        assert_eq!(refresh_plan(&[1, 2, 3], &[], same), RefreshPlan::Remove { at: 0, count: 3 });
        assert_eq!(refresh_plan(&[], &[1], same), RefreshPlan::Insert { at: 0, count: 1 });
        assert_eq!(refresh_plan(&[1, 2, 3], &[3, 2, 1], same), RefreshPlan::Reset);
        assert_eq!(refresh_plan(&[1, 2], &[3, 4, 5], same), RefreshPlan::Reset);
        assert_eq!(refresh_plan::<i32>(&[], &[], same), RefreshPlan::Keep);
    }
}
