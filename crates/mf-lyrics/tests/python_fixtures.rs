//! Cross-check against the outputs of lyrics_sources.py from yamusic-md3 (tests/fixtures.json — frozen references; the Python code was removed).
//! Numbers are compared through their JSON text: equal f64 values give the same shortest representation.
use serde::Serialize;
use serde_json::Value;
use mf_lyrics::lrc::{add_gaps, build_from_timed_lines, lines_from_lrc, parse_yrc, round3};
use mf_lyrics::sources::{NE_LYRIC_PATH, ne_eapi_params, ne_eapi_params_text, ne_lyric_json};
use mf_lyrics::text::{clean_title, is_placeholder, norm, primary_artist, similar};
use mf_lyrics::{Line, cache_file_name_v};

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures.json")).unwrap()
}

/// Value → text → Value: this way both our result and the reference go through the same number parsing
fn canon<T: Serialize>(v: &T) -> Value {
    serde_json::from_str(&serde_json::to_string(v).unwrap()).unwrap()
}

fn cases<'a>(f: &'a Value, key: &str) -> &'a [Value] {
    f[key].as_array().unwrap()
}

fn pairs(v: &Value) -> Vec<(f64, String)> {
    serde_json::from_value(v.clone()).unwrap()
}

#[test]
fn lines_from_lrc_matches_python() {
    let f = fixtures();
    for c in cases(&f, "lines_from_lrc") {
        let lrc = c["lrc"].as_str().unwrap();
        assert_eq!(canon(&lines_from_lrc(lrc)), canon(&c["out"]), "{lrc:?}");
    }
}

#[test]
fn build_from_timed_lines_matches_python() {
    let f = fixtures();
    for c in cases(&f, "build_from_timed_lines") {
        let out = build_from_timed_lines(&pairs(&c["pairs"]), c["duration"].as_f64().unwrap());
        assert_eq!(canon(&out), canon(&c["out"]), "{}", c["pairs"]);
    }
}

#[test]
fn parse_yrc_matches_python() {
    let f = fixtures();
    for c in cases(&f, "parse_yrc") {
        let yrc = c["yrc"].as_str().unwrap();
        assert_eq!(canon(&parse_yrc(yrc)), canon(&c["out"]), "{yrc:?}");
    }
}

#[test]
fn add_gaps_matches_python() {
    let f = fixtures();
    for c in cases(&f, "add_gaps") {
        let lines: Vec<Line> = serde_json::from_value(c["lines"].clone()).unwrap();
        // parsing lines back and forth does not lose fields
        assert_eq!(canon(&lines), canon(&c["lines"]));
        assert_eq!(canon(&add_gaps(lines, 0.0)), canon(&c["out"]));
    }
}

#[test]
fn is_placeholder_matches_python() {
    let f = fixtures();
    for c in cases(&f, "is_placeholder") {
        let texts: Vec<&str> = c["texts"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
        assert_eq!(is_placeholder(texts.iter().copied()), c["out"].as_bool().unwrap(), "{texts:?}");
    }
}

#[test]
fn text_helpers_match_python() {
    let f = fixtures();
    for c in cases(&f, "clean_title") {
        assert_eq!(clean_title(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "{c}");
    }
    for c in cases(&f, "primary_artist") {
        assert_eq!(primary_artist(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "{c}");
    }
    for c in cases(&f, "norm") {
        assert_eq!(norm(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "{c}");
    }
    for c in cases(&f, "similar") {
        assert_eq!(similar(c[0].as_str().unwrap(), c[1].as_str().unwrap()), c[2].as_bool().unwrap(), "{c}");
    }
}

#[test]
fn round3_matches_python() {
    let f = fixtures();
    for c in cases(&f, "round3") {
        assert_eq!(canon(&round3(c[0].as_f64().unwrap())), canon(&c[1]), "{c}");
    }
}

#[test]
fn ne_eapi_params_matches_python() {
    let f = fixtures();
    for c in cases(&f, "ne_eapi_params") {
        assert_eq!(ne_eapi_params(c["path"].as_str().unwrap(), &c["params"]), c["out"].as_str().unwrap());
    }
    let lyric = &f["ne_lyric_params"];
    let json = ne_lyric_json(lyric["id"].as_str().unwrap());
    assert_eq!(json, lyric["json"].as_str().unwrap());
    assert_eq!(ne_eapi_params_text(NE_LYRIC_PATH, &json), lyric["out"].as_str().unwrap());
}

#[test]
fn cache_key_matches_python() {
    let f = fixtures();
    for c in cases(&f, "cache_key") {
        assert_eq!(cache_file_name_v(5, c[0].as_str().unwrap()), format!("{}.json", c[1].as_str().unwrap()));
    }
}
