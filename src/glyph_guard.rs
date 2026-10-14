//! Regression guard for the UI glyph set.
//!
//! egui 0.27 ships subsetted bundled fonts. Several visually handy characters
//! are NOT in those subsets, and egui then draws them as empty outlined squares
//! (tofu). That is what users saw on the roster row buttons and the picker
//! footer: a row of blank boxes with no hint of what they do.
//!
//! The characters below are therefore banned from the sources -- in strings AND
//! in comments, so a comment cannot smuggle one back into a copy-pasted label.
//! Glyphs that DO render and stay in use: the shiny star, the filled bullet,
//! the middle dot, the em dash, and the ellipsis.
//!
//! The deny list below is written with escapes on purpose: a file that names a
//! banned character literally would fail its own scan.

/// The characters egui 0.27's bundled fonts cannot draw, by code point:
/// pencil, multiplication cross, rightwards arrow, left/right pointing
/// triangles, upwards and downwards arrow.
const UNRENDERABLE: [u16; 7] = [
    0x270E, // pencil
    0x2715, // multiplication X
    0x2192, // rightwards arrow
    0x25C0, // left-pointing triangle
    0x25B6, // right-pointing triangle
    0x2191, // upwards arrow
    0x2193, // downwards arrow
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Every `.rs` file under this crate's `src/` must be free of the banned
    /// characters, comments included. Skipping is only allowed when the source
    /// tree itself cannot be read (a checkout without sources), never when a
    /// file is unreadable -- that is reported.
    #[test]
    fn no_unrenderable_glyph_anywhere_in_the_sources() {
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

        let banned: Vec<char> = UNRENDERABLE
            .iter()
            .filter_map(|&cp| char::from_u32(cp as u32))
            .collect();
        let mut hits: Vec<String> = Vec::new();
        for file in &files {
            let text = std::fs::read_to_string(file)
                .unwrap_or_else(|e| panic!("glyph guard cannot read {file:?}: {e}"));
            for (lineno, line) in text.lines().enumerate() {
                for (col, ch) in line.chars().enumerate() {
                    if banned.contains(&ch) {
                        hits.push(format!(
                            "{}:{}:{} U+{:04X}",
                            file.display(),
                            lineno + 1,
                            col + 1,
                            ch as u32
                        ));
                    }
                }
            }
        }
        assert!(
            hits.is_empty(),
            "{} source hit(s) of characters egui 0.27 cannot draw (they render as \
             empty boxes in the UI). Replace with an ASCII label and put the meaning \
             in `on_hover_text`:\n  {}",
            hits.len(),
            hits.join("\n  ")
        );
    }

    /// The characters that are known to render must not be "fixed" away by a
    /// future sweep: they are the star / bullet / middle dot / em dash / ellipsis
    /// the user confirmed seeing correctly.
    #[test]
    fn the_guard_list_itself_names_only_unrenderable_code_points() {
        assert_eq!(UNRENDERABLE.len(), 7);
        assert!(UNRENDERABLE.contains(&0x270E));
        assert!(UNRENDERABLE.contains(&0x2715));
        assert!(UNRENDERABLE.contains(&0x2192));
        assert!(UNRENDERABLE.contains(&0x25C0));
        assert!(UNRENDERABLE.contains(&0x25B6));
        assert!(UNRENDERABLE.contains(&0x2191));
        assert!(UNRENDERABLE.contains(&0x2193));
        // Renderable characters deliberately left in the UI.
        for renderable in [0x2605u16, 0x25CF, 0x00B7, 0x2014, 0x2026] {
            assert!(
                !UNRENDERABLE.contains(&renderable),
                "U+{renderable:04X} renders fine and must not be banned"
            );
        }
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
