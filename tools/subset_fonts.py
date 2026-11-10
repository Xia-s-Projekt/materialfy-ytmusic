"""Subset fonts for the application: less memory (Qt keeps a copy of the font for every combination of axes).

- Google Sans: Latin, Cyrillic, punctuation — all axes and OpenType features are preserved.
- Material Symbols: only the icons whose names appear in the code (string literals in QML and Rust — the bridges
  return device icon names and so on) or in tools/icons-extra.txt, by their PUA codes; drawn as a character,
  rather than as a ligature (the name → code map is assets/fonts/icons.json).

The sources are in assets/fonts-src. Run it after adding new icons (fonttools is required: pip install fonttools):
    PYTHONIOENCODING=utf-8 python tools/subset_fonts.py
"""

from __future__ import annotations

import json
import re
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

REPO = Path(__file__).resolve().parents[1]
ROOT = REPO / "src" / "materialfy"
SRC = ROOT / "assets" / "fonts-src"
OUT = ROOT / "assets" / "fonts"
CRATES = REPO / "crates"

TEXT_UNICODES = [
    *range(0x20, 0x7F), *range(0xA0, 0x180),     # Latin, Latin-1, Latin Extended-A
    *range(0x400, 0x530),                          # Cyrillic (+ supplement)
    *range(0x2000, 0x2070),                        # punctuation: dashes, quotes, …, •
    0x20BD, 0x20AC, 0x2116, 0x2212, 0x2190, 0x2192, 0x2191, 0x2193, 0x00D7, 0x2026,
]


def subset_font(src: Path, dst: Path, unicodes: list[int]) -> None:
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    options.glyph_names = False
    options.hinting = True
    font = TTFont(src)
    sub = subset.Subsetter(options)
    sub.populate(unicodes=unicodes)
    sub.subset(font)
    font.save(dst)
    print(f"{dst.name}: {src.stat().st_size // 1024} KB → {dst.stat().st_size // 1024} KB")


def icon_ligatures(font: TTFont) -> dict[str, int]:
    """Icon name → PUA code (by GSUB ligatures and cmap)."""
    cmap = font.getBestCmap()
    by_glyph: dict[str, int] = {}
    char_of: dict[str, str] = {}
    for code, glyph in cmap.items():
        if 0xE000 <= code <= 0xF8FF or code >= 0xF0000:
            by_glyph.setdefault(glyph, code)
        elif code < 0x80:
            char_of[glyph] = chr(code)
    names: dict[str, int] = {}
    for lookup in font["GSUB"].table.LookupList.Lookup:
        for table in lookup.SubTable:
            table = getattr(table, "ExtSubTable", table)
            for first, ligs in getattr(table, "ligatures", {}).items():
                for lig in ligs:
                    glyphs = [first, *lig.Component]
                    if not all(g in char_of for g in glyphs) or lig.LigGlyph not in by_glyph:
                        continue
                    names["".join(char_of[g] for g in glyphs)] = by_glyph[lig.LigGlyph]
    return names


EXTRA = Path(__file__).resolve().parent / "icons-extra.txt"   # icons from the mockups that are not in QML yet


def used_icon_names(known: set[str]) -> set[str]:
    literal = re.compile(r'"([a-z0-9_]+)"')
    used: set[str] = set()
    for path in [*ROOT.rglob("*.qml"), *ROOT.rglob("*.js"), *CRATES.glob("*/src/**/*.rs")]:
        used |= {m for m in literal.findall(path.read_text(encoding="utf-8")) if m in known}
    if EXTRA.is_file():
        lines = EXTRA.read_text(encoding="utf-8").splitlines()
        used |= {line.strip() for line in lines if line.strip() in known}
    return used


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    subset_font(SRC / "GoogleSans.ttf", OUT / "GoogleSans.ttf", TEXT_UNICODES)

    icons = TTFont(SRC / "MaterialSymbolsRounded.ttf")
    names = icon_ligatures(icons)
    used = sorted(used_icon_names(set(names)))
    mapping = {name: names[name] for name in used}
    subset_font(SRC / "MaterialSymbolsRounded.ttf", OUT / "MaterialSymbolsRounded.ttf", sorted(set(mapping.values())))
    (OUT / "icons.json").write_text(json.dumps(mapping, indent=0, sort_keys=True) + "\n")
    print(f"icons: {len(mapping)} of {len(names)}")


if __name__ == "__main__":
    main()
