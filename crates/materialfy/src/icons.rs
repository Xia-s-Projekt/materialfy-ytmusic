//! IconMetrics (icons.py): optical centering of Material Symbols icons.
//! The font is trimmed to the used icons (tools/subset_fonts.py), so an icon is drawn as a character by code
//! from assets/fonts/icons.json, not as a ligature - glyph(name). ink(name, size) measures the glyph by pixels
//! (C++: icon_measure in cpp/mf_shell.cpp), cached by (name, round(size)).
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::OnceLock;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QRectF, QString};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qrectf.h");
        type QRectF = cxx_qt_lib::QRectF;
    }

    unsafe extern "RustQt" {
        #[qobject]
        type IconMetrics = super::IconMetricsRust;

        #[qinvokable]
        fn glyph(self: &IconMetrics, name: &QString) -> QString;

        /// x, y - the offset of the glyph outline center from the center of its text block (advance x height);
        /// width, height - the outline size, in pixels. In QML: glyph.x -= x, glyph.y -= y.
        #[qinvokable]
        fn ink(self: Pin<&mut IconMetrics>, name: &QString, size: f64) -> QRectF;
    }
}

#[derive(Default)]
pub struct IconMetricsRust {
    cache: HashMap<(String, i64), QRectF>,
}

fn codepoints() -> &'static HashMap<String, u32> {
    static CODEPOINTS: OnceLock<HashMap<String, u32>> = OnceLock::new();
    CODEPOINTS.get_or_init(|| {
        let path = crate::engine::assets_dir().join("fonts").join("icons.json");
        match std::fs::read_to_string(&path).map(|text| serde_json::from_str(&text)) {
            Ok(Ok(map)) => map,
            Ok(Err(e)) => {
                eprintln!("could not parse {}: {e}", path.display());
                HashMap::new()
            }
            Err(e) => {
                eprintln!("could not read {}: {e}", path.display());
                HashMap::new()
            }
        }
    })
}

fn glyph_text(name: &str) -> String {
    match codepoints().get(name).and_then(|&code| char::from_u32(code)) {
        Some(c) => c.to_string(),
        None => {
            if !name.is_empty() {
                eprintln!("icon \"{name}\" is missing from the subset font — run tools/subset_fonts.py");
            }
            String::new()
        }
    }
}

impl qobject::IconMetrics {
    fn glyph(&self, name: &QString) -> QString {
        QString::from(glyph_text(&name.to_string()).as_str())
    }

    fn ink(mut self: Pin<&mut Self>, name: &QString, size: f64) -> QRectF {
        let name = name.to_string();
        // Python round() - to even
        let size = size.round_ties_even() as i64;
        let key = (name, size);
        if let Some(cached) = self.rust().cache.get(&key) {
            return cached.clone();
        }
        let result = if !key.0.is_empty() && size > 0 {
            let text = glyph_text(&key.0);
            if text.is_empty() {
                QRectF::default()
            } else {
                crate::qt::ffi::ffi::icon_measure(&QString::from(text.as_str()), size.min(i32::MAX as i64) as i32)
            }
        } else {
            QRectF::default()
        };
        self.as_mut().rust_mut().cache.insert(key, result.clone());
        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Every icon name QML asks for must be in the subset font (assets/fonts/icons.json), otherwise it
    /// renders as nothing. After adding an icon run `python tools/subset_fonts.py`.
    #[test]
    fn every_icon_used_in_qml_is_in_the_font() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/materialfy");
        let json = std::fs::read_to_string(root.join("assets/fonts/icons.json")).unwrap();
        let font: std::collections::HashMap<String, serde_json::Value> = serde_json::from_str(&json).unwrap();
        let mut missing = Vec::new();
        let mut stack = vec![root.join("qml")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "qml") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (number, line) in text.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    let Some(at) = code.find("icon:").or_else(|| code.find("Icon:")) else { continue };
                    // property bindings like `icon: "name"` or `icon: cond ? "a" : "b"`, up to the next property
                    let binding = code[at..].split([';', '}']).next().unwrap_or("");
                    let parts: Vec<&str> = binding.split('"').collect();
                    for (index, name) in parts.iter().enumerate().skip(1).step_by(2) {
                        // operands of comparisons and call arguments are not icons: `x === "one" ? …`
                        let before = parts[index - 1].trim_end();
                        if before.ends_with('=') || before.ends_with('(') || before.ends_with(',') {
                            continue;
                        }
                        let looks_like_icon = !name.is_empty()
                            && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                        if looks_like_icon && !font.contains_key(*name) {
                            missing.push(format!("{}:{}: {name}", path.display(), number + 1));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "icons missing from the subset font (run tools/subset_fonts.py):\n{}", missing.join("\n"));
    }
}
