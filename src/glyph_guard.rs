//! Regression guard for the UI glyph set -- a font-verified ALLOW-list.
//!
//! egui 0.27 ships subsetted bundled fonts. Characters outside those subsets
//! draw as empty outlined squares (tofu). Two user-visible bugs came from
//! assuming a glyph was safe: the row-button icons (pencil / cross / arrows),
//! and the genderless marker U+2205, which is present in Hack (the MONO font
//! chain) but NOT in Ubuntu-Light (the PROPORTIONAL chain labels use) -- so
//! only genderless mons showed an empty box while ♂/♀ looked fine.
//!
//! The old guard was a deny-list: it could never catch the next unverified
//! character. This guard is the inverse: every non-ASCII code point anywhere
//! under src/ (comments included, so a comment cannot smuggle one back into a
//! copy-pasted label) must appear on the explicit allow-list below.
//!
//! The allow-list is NOT a guess. Every entry was verified against the cmap
//! tables of the exact .ttf files egui/epaint 0.27.2 bundles by
//! tools/check_glyphs.py, which parses the fonts themselves and reports, per
//! character, coverage in the proportional chain (Ubuntu-Light, NotoEmoji,
//! emoji-icon) and the monospace chain (Hack, NotoEmoji, emoji-icon). That tool
//! is the only sanctioned way to add or remove entries: run it, do not re-guess
//! coverage from memory.

/// Every non-ASCII code point permitted in the sources, by code point. Each one
/// is covered by the PROPORTIONAL font chain (the chain that draws labels); the
/// bracket comment names the verification from tools/check_glyphs.py.
const ALLOWED_NON_ASCII: [u32; 10] = [
    0x00B7, // middle dot (both chains)
    0x00C9, // Latin capital E with acute (both chains; Pokemon -> Pokémon)
    0x00E9, // Latin small e with acute (both chains)
    0x00D8, // Latin capital O with stroke (both chains; genderless marker)
    0x2026, // horizontal ellipsis (proportional chain)
    0x2212, // minus sign (both chains; "-1 season" button)
    0x25CB, // white circle (both chains; selected-row mark)
    0x2605, // black star (both chains; shiny mark)
    0x2640, // female sign (both chains)
    0x2642, // male sign (both chains)
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Every `.rs` file under this crate's `src/` may contain only ASCII or a code
    /// point on the allow-list, comments included. Skipping is only allowed when
    /// the source tree itself cannot be read (a checkout without sources), never
    /// when a file is unreadable -- that is reported.
    #[test]
    fn no_non_ascii_glyph_outside_the_font_verified_allow_list() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        if std::fs::read_dir(&src).is_err() {
            eprintln!("glyph guard: cannot read {src:?}, skipping");
            return;
        }

        let mut files: Vec<PathBuf> = Vec::new();
        collect_rs_files(&src, &mut files);
        assert!(
            !files.is_empty(),
            "glyph guard walked {src:?} and found no .rs files"
        );

        let mut hits: Vec<String> = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(file)
                .unwrap_or_else(|e| panic!("glyph guard cannot read {file:?}: {e}"));
            for (lineno, line) in text.lines().enumerate() {
                for (col, ch) in line.chars().enumerate() {
                    let cp = ch as u32;
                    if cp < 0x80 || ALLOWED_NON_ASCII.contains(&cp) {
                        continue;
                    }
                    hits.push(format!(
                        "{}:{}:{} U+{:04X} ({})",
                        file.display(),
                        lineno + 1,
                        col + 1,
                        cp,
                        if char::from_u32(cp).is_some() && !ch.is_ascii() {
                            "not on the allow-list: run tools/check_glyphs.py before adding it"
                        } else {
                            "control"
                        }
                    ));
                }
            }
        }
        assert!(
            hits.is_empty(),
            "{} source hit(s) of characters that are not on the font-verified \
             allow-list. egui 0.27's bundled subset fonts draw them as empty \
             boxes (the old genderless U+2205 bug). Use an ASCII label or a \
             listed code point, and verify any new one with tools/check_glyphs.py:\n  {}",
            hits.len(),
            hits.join("\n  ")
        );
    }

    /// The anti-regression lock: the characters that were verified to be missing
    /// from the proportional chain (and are therefore banned by the allow-list)
    /// must stay off it. If someone "helpfully" re-adds one, this test fails even
    /// before the source scan does.
    #[test]
    fn the_allow_list_excludes_every_unverified_glyph() {
        for banned in [
            0x2205u32, // empty set: Hack only -> tofu in labels (the genderless bug)
            0x2014,    // em dash
            0x201C,    // left double quotation mark
            0x201D,    // right double quotation mark
            0x25CF,    // black circle
            0x270E,    // pencil
            0x2715,    // multiplication X
            0x2192,    // rightwards arrow
            0x25C0,    // left-pointing triangle
            0x25B6,    // right-pointing triangle
            0x2191,    // upwards arrow
            0x2193,    // downwards arrow
        ] {
            assert!(
                !ALLOWED_NON_ASCII.contains(&banned),
                "U+{banned:04X} is not covered by the proportional font chain \
                 (verified by tools/check_glyphs.py); it must not return to the allow-list"
            );
        }
    }

    /// The allow-list is exactly the verified set: if a character is added to the
    /// sources, the same commit must extend this list AND keep the entry justified
    /// by tools/check_glyphs.py.
    #[test]
    fn the_allow_list_is_the_documented_verified_set() {
        let expected: Vec<u32> = vec![
            0x00B7, 0x00C9, 0x00D8, 0x00E9, 0x2026, 0x2212, 0x25CB, 0x2605, 0x2640, 0x2642,
        ];
        let mut got: Vec<u32> = ALLOWED_NON_ASCII.to_vec();
        got.sort_unstable();
        assert_eq!(got, expected, "allow-list drifted from the verified set");
    }

    fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return; // a subdirectory we cannot read is skipped, like the root
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}
