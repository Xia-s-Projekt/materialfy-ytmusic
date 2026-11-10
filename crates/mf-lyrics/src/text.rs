//! Normalization and matching of titles (lyrics_sources.py: clean_title, primary_artist, norm, similar,
//! version_tags, is_placeholder). The regular expressions are the same (crate regex, the (?i) flag).
//!
//! The \w/\s classes differ between Python and regex (in Python \w is isalnum or "_", \s is isspace), so where
//! that matters they are written explicitly via \p{L}/\p{N} and `py_isspace`.
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[\(\[\{（【].*?[\)\]\}）】]").unwrap());
static FEAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s+(feat\.?|ft\.?|featuring|при уч\.?)\s+.*$").unwrap());
static DASH_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s+[-–—]\s+(.*(remaster|version|edit|live|mix|mono|stereo|ремастер|версия).*)$").unwrap()
});
static ARTIST_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(?:,|&|/|;| feat\.? | ft\.? | x | и )\s*").unwrap());
/// [\W_]+ in the Python sense: everything except letters and numbers
static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+").unwrap());

static META_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^\s*(作词|作曲|编曲|制作人|制作|混音|母带|和声|吉他|贝斯|鼓|录音|监制|出品|发行|词|曲|",
        r"lyrics|lyricist|composer|composed|producer|produced|arranger|written|mixed|mastered|vocals?)\s*[:：]",
    ))
    .unwrap()
});

// Instead of "there is no text", NetEase returns a placeholder line: "纯音乐，请欣赏" (instrumental, pleasant
// listening), "暂无歌词" (no lyrics yet), "此歌曲为没有填词的纯音乐" and so on.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("纯音乐|純音樂|暂无歌词|暫無歌詞|没有填词|沒有填詞|无歌词|無歌詞|请欣赏|請欣賞|歌词贡献者|翻译贡献者|暂时没有歌词|此歌曲为")
        .unwrap()
});
/// ^[\s\W\d一-鿿㐀-䶿]+$ in the Python sense
static CJK_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[\s\x1c-\x1f]|[^\p{L}\p{N}_]|\p{Nd}|[一-鿿㐀-䶿])+$").unwrap()
});

const VERSION_WORDS: [&str; 18] = [
    "remix", "live", "acoustic", "instrumental", "karaoke", "cover", "sped up", "slowed", "nightcore", "demo", "edit",
    "version", "mix", "ремикс", "лайв", "акустика", "минус", "伴奏",
];

/// Python's str.isspace(): Unicode White_Space and the \x1c–\x1f separators
pub(crate) fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// str.strip() with no arguments
pub(crate) fn py_strip(s: &str) -> &str {
    s.trim_matches(py_isspace)
}

/// str.splitlines(): \n, \r, \r\n, \v, \f, \x1c–\x1e, \x85,  ,
pub(crate) fn py_splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n') {
                chars.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// A Chinese character/kana/hangul at the end of a syllable is a word boundary ([぀-ヿ㐀-鿿가-힯]$)
pub(crate) fn ends_with_cjk(s: &str) -> bool {
    s.chars()
        .last()
        .is_some_and(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}'))
}

pub fn clean_title(title: &str) -> String {
    let t = DASH_SUFFIX.replace_all(title, "");
    let t = PAREN.replace_all(&t, "");
    let t = FEAT.replace_all(&t, "");
    let stripped = py_strip(&t);
    if stripped.is_empty() { t.into_owned() } else { stripped.to_owned() }
}

pub fn primary_artist(artist: &str) -> String {
    let first = ARTIST_SPLIT.split(artist).next().unwrap_or("");
    let first = py_strip(first);
    if first.is_empty() { artist.to_owned() } else { first.to_owned() }
}

/// NFKD, no diacritics, ё→е, letters/digits only, lower case.
pub fn norm(text: &str) -> String {
    let s: String = text.to_lowercase().nfkd().filter(|&c| canonical_combining_class(c) == 0).collect();
    let s = s.replace('ё', "е");
    NON_WORD.replace_all(&s, "").into_owned()
}

pub fn similar(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

pub fn version_tags(text: &str) -> Vec<&'static str> {
    let s = text.to_lowercase();
    VERSION_WORDS.iter().copied().filter(|w| s.contains(w)).collect()
}

/// Lines are a NetEase "no lyrics" placeholder (纯音乐，请欣赏 and so on).
pub fn is_placeholder<'a>(texts: impl IntoIterator<Item = &'a str>) -> bool {
    let texts: Vec<&str> = texts.into_iter().map(py_strip).filter(|t| !t.is_empty()).collect();
    if texts.is_empty() {
        return true;
    }
    let marked = texts.iter().filter(|t| PLACEHOLDER.is_match(t)).count();
    if marked > 0 {
        // a placeholder is usually one or two lines; in real Chinese text such words are rare
        return texts.len() <= 4 || marked * 2 >= texts.len();
    }
    // one or two lines made entirely of Chinese characters are also a placeholder, not a song
    texts.len() <= 2 && texts.iter().all(|t| CJK_ONLY.is_match(t))
}

/// A meta line (作词/composer/…:) is dropped from the text.
pub fn is_meta_line(text: &str) -> bool {
    META_LINE.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("Song (Remastered 2011)"), "Song");
        assert_eq!(clean_title("Song - 2011 Remaster"), "Song");
        assert_eq!(clean_title("Song feat. Someone"), "Song");
        assert_eq!(clean_title("Song [Live] ft. Someone"), "Song");
        assert_eq!(clean_title("(Intro)"), "");
        assert_eq!(clean_title("  "), "  ");
    }

    #[test]
    fn primary_artist_is_first() {
        assert_eq!(primary_artist("Band, Some Singer"), "Band");
        assert_eq!(primary_artist("A feat. B"), "A");
        assert_eq!(primary_artist("Max, Dasha"), "Max");
        assert_eq!(primary_artist("AC/DC"), "AC");
        assert_eq!(primary_artist(", B"), ", B");
    }

    #[test]
    fn norm_and_similar() {
        assert_eq!(norm("Fir — Café!"), "fircafe");
        assert!(similar("Song", "song (remastered)"));
        assert!(!similar("", "x"));
        assert!(!similar("abc", "xyz"));
    }

    #[test]
    fn version_words() {
        assert_eq!(version_tags("Song (Live Remix)"), vec!["remix", "live", "mix"]);
        assert!(version_tags("Song").is_empty());
    }

    #[test]
    fn placeholders() {
        assert!(is_placeholder(["纯音乐，请欣赏"]));
        assert!(is_placeholder(["暂无歌词"]));
        assert!(is_placeholder(std::iter::empty()));
        assert!(is_placeholder(["  ", ""]));
        assert!(is_placeholder(["你好世界"]));
        assert!(!is_placeholder(["White snow, gray ice", "On the cracked ground"]));
        assert!(!is_placeholder(["Hello darkness, my old friend"]));
        assert!(!is_placeholder(["你好", "世界", "再见"]));
    }

    #[test]
    fn meta_lines() {
        assert!(is_meta_line("作词 : 某人"));
        assert!(is_meta_line("  Lyrics: someone"));
        assert!(is_meta_line("VOCALS："));
        assert!(!is_meta_line("lyrics of my life"));
    }

    #[test]
    fn splitlines_like_python() {
        assert_eq!(py_splitlines("a\r\nb\rc\n\nd\n"), vec!["a", "b", "c", "", "d"]);
        assert_eq!(py_splitlines(""), Vec::<&str>::new());
    }
}
