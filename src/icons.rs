//! On-disk icon art (PNG) with a lazy egui texture cache.
//!
//! Icons are NOT bundled in the binary and NOT extracted from a ROM: they are
//! downloaded by `tools/get_icons.py` from a public sprite repo into
//! `assets/icons/{pokemon,items}/<id>.png`, keyed by the editor's own species /
//! item ids. If that directory is absent the editor still works - every icon
//! slot falls back to a generated letter badge, and the top bar says so.
//!
//! The store is interior-mutable (`&self` methods) so icon lookups can happen
//! while the save file is borrowed immutably in the same frame.

use egui::{Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Which icon namespace an id belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconKind {
    None,
    Pokemon,
    Item,
}

impl IconKind {
    fn dir(self) -> Option<&'static str> {
        match self {
            IconKind::None => None,
            IconKind::Pokemon => Some("pokemon"),
            IconKind::Item => Some("items"),
        }
    }

    /// Arity of the tally array. `None` keeps a slot so the index of a kind is
    /// simply its discriminant.
    const COUNT: usize = 3;
    /// Position in the tally array (declaration order).
    fn index(self) -> usize {
        match self {
            IconKind::None => 0,
            IconKind::Pokemon => 1,
            IconKind::Item => 2,
        }
    }
}

/// What the store knows about one namespace so far. Reported per namespace
/// because the two are fetched by different scripts and go missing
/// independently: one pooled number hid "you have no item art at all".
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct IconTally {
    /// ids that resolved to a PNG and were uploaded as a texture
    pub resolved: usize,
    /// ids we asked for and found no art for (drawn as a letter badge)
    pub missing: usize,
}

pub struct IconStore {
    /// Directory holding `pokemon/` and `items/`, if we found one.
    root: Option<PathBuf>,
    /// id-key -> texture. A `None` entry means "looked, no art" so we do not
    /// re-stat a missing file every frame.
    cache: RefCell<HashMap<String, Option<TextureHandle>>>,
    /// One tally per namespace, indexed by `IconKind::index`.
    tallies: RefCell<[IconTally; IconKind::COUNT]>,
    /// Cached answer to "does `items/` hold any PNG on disk": -1 unknown, 0 no, 1
    /// yes. Probed once so the hint does not re-read a 900-entry directory every
    /// frame.
    items_probe: Cell<i8>,
    /// Cached gap report (which ids of the game's own id space have no PNG on disk),
    /// computed on first ask. `/assets/` is gitignored on purpose, so a fresh clone
    /// has no art at all; the old behaviour was a silent letter badge with no way to
    /// find out which id was missing art or what to do about it.
    gaps: RefCell<Option<Gaps>>,
}

/// Which ids of one namespace have no art on disk, and what to do about it.
#[derive(Clone, Debug)]
pub struct ArtGap {
    /// ids the editor can select in this namespace
    pub total: usize,
    /// how many of them have a PNG
    pub with_art: usize,
    /// a few missing ids with their names, so the message is actionable rather than
    /// "some art is missing"
    pub sample: Vec<(u16, &'static str)>,
}

/// Gap reports for both namespaces, computed once.
#[derive(Clone, Debug, Default)]
pub struct Gaps {
    pub pokemon: Option<ArtGap>,
    pub item: Option<ArtGap>,
}

impl Default for IconStore {
    fn default() -> Self {
        Self::new()
    }
}

impl IconStore {
    pub fn new() -> Self {
        IconStore {
            root: find_asset_root(),
            cache: RefCell::new(HashMap::new()),
            tallies: RefCell::new([IconTally::default(); IconKind::COUNT]),
            items_probe: Cell::new(-1),
            gaps: RefCell::new(None),
        }
    }

    /// A store pointed at an explicit root (or no root at all). Test-only: it lets
    /// the counters and the status line be pinned without the real, gitignored
    /// `assets/` tree being present.
    #[cfg(test)]
    pub(crate) fn at_root(root: Option<PathBuf>) -> Self {
        IconStore {
            root,
            ..IconStore::new()
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// One-line description for the status bar, honest about missing art, and per
    /// namespace so "items are all badges" can never read as intentional.
    pub fn status(&self) -> String {
        match self.root() {
            None => "icons: no assets/icons dir - run `python3 tools/get_icons.py`".into(),
            Some(r) => {
                let p = self.tally(IconKind::Pokemon);
                let i = self.tally(IconKind::Item);
                let gaps = self.art_gaps();
                let mut out = format!(
                    "icons: pokemon {} shown / {} missing, items {} shown / {} missing \
                     (root {})",
                    p.resolved,
                    p.missing,
                    i.resolved,
                    i.missing,
                    r.display()
                );
                if let Some(g) = gaps.pokemon.filter(|g| g.with_art < g.total) {
                    let sample = g
                        .sample
                        .iter()
                        .map(|(id, n)| format!("{id} {n}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        " | pokemon: {} of {} ids have no art (e.g. {sample}); \
                         fetch with `python3 tools/get_icons.py`",
                        g.total - g.with_art,
                        g.total
                    ));
                }
                if let Some(g) = gaps.item.filter(|g| g.with_art < g.total) {
                    out.push_str(&format!(
                        " | items: {} of {} ids have no art; fetch with `python3 \
                         tools/get_icons_from_source.py`",
                        g.total - g.with_art,
                        g.total
                    ));
                }
                out
            }
        }
    }

    /// Which ids the editor offers but has no PNG for, per namespace.
    ///
    /// Compared against the game's own id space (the species / item tables), not
    /// against the ids some picker happened to request, so "you are missing the art
    /// for Xerneas" is true on the first frame instead of only after you look for
    /// Xerneas. One directory read per namespace, then cached.
    pub fn art_gaps(&self) -> Gaps {
        if let Some(g) = self.gaps.borrow().as_ref() {
            return g.clone();
        }
        let gaps = Gaps {
            pokemon: self.gap_for(IconKind::Pokemon, crate::game_data::all_species()),
            item: self.gap_for(IconKind::Item, crate::game_data::all_items()),
        };
        *self.gaps.borrow_mut() = Some(gaps.clone());
        gaps
    }

    fn gap_for(&self, kind: IconKind, ids: &[(u16, &'static str)]) -> Option<ArtGap> {
        let root = self.root()?;
        let dir = {
            let d = kind.dir()?;
            root.join(d)
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Some(ArtGap {
                total: ids.len(),
                with_art: 0,
                sample: ids.iter().take(4).copied().collect(),
            });
        };
        let mut on_disk: HashSet<u32> = HashSet::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".png") {
                if let Ok(id) = stem.parse::<u32>() {
                    on_disk.insert(id);
                }
            }
        }
        let with_art = ids
            .iter()
            .filter(|(id, _)| on_disk.contains(&(*id as u32)))
            .count();
        let sample = ids
            .iter()
            .filter(|(id, _)| !on_disk.contains(&(*id as u32)))
            .take(4)
            .copied()
            .collect();
        Some(ArtGap {
            total: ids.len(),
            with_art,
            sample,
        })
    }

    /// A note for one species id when its sprite is not on disk: the label, the id,
    /// and the command that fetches it. This is what makes a letter badge explain
    /// itself in the place the user noticed it.
    pub fn species_art_note(&self, species: u16) -> Option<String> {
        // No assets root at all: the status line already says to run the script, and
        // repeating it under every species field would be noise.
        self.root()?;
        if self.dir_has_pokemon(species) {
            return None;
        }
        let label = crate::game_data::species_name(species);
        Some(format!(
            "no sprite on disk for {species} ({label}); fetch art with \
             `python3 tools/get_icons.py`"
        ))
    }

    /// Direct on-disk check for one pokemon id (cheap: one `Path::exists`).
    fn dir_has_pokemon(&self, id: u16) -> bool {
        match self.root() {
            Some(r) => r.join("pokemon").join(format!("{id}.png")).is_file(),
            None => false,
        }
    }

    /// A fixed-length summary for the top bar: two counters, never a sentence. The
    /// long form - which ids are missing, and the command that fetches them - lives in
    /// the Diagnostics pane, because a strip of that length pushed the file name off
    /// the bar.
    pub fn art_summary(&self) -> String {
        let gaps = self.art_gaps();
        match (gaps.pokemon, gaps.item) {
            (Some(p), Some(i)) => format!(
                "icons {}/{} | items {}/{}",
                p.with_art, p.total, i.with_art, i.total
            ),
            _ => "icons: none on disk".to_string(),
        }
    }

    /// Counters for one namespace: how many ids resolved, how many had no art.
    pub fn tally(&self, kind: IconKind) -> IconTally {
        self.tallies.borrow()[kind.index()]
    }

    /// True when the assets root exists but the item namespace has no art at all,
    /// so every item row is a generated letter badge. The on-disk probe makes this
    /// right on the very first frame, before any item id has been requested by a
    /// picker.
    pub fn items_art_absent(&self) -> bool {
        // No root at all: the status line already names the command to run.
        if self.root.is_none() || self.tally(IconKind::Item).resolved > 0 {
            return false;
        }
        if self.items_probe.get() < 0 {
            let on_disk = self
                .root
                .as_ref()
                .and_then(|r| {
                    r.join(IconKind::Item.dir().unwrap_or("items"))
                        .read_dir()
                        .ok()
                })
                .map(|mut entries| {
                    entries.any(|e| {
                        e.ok()
                            .and_then(|e| e.path().extension().map(|x| x == "png"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            self.items_probe.set(on_disk as i8);
        }
        self.items_probe.get() == 0
    }

    fn bump(&self, kind: IconKind, resolved: bool) {
        let mut tallies = self.tallies.borrow_mut();
        let slot = &mut tallies[kind.index()];
        if resolved {
            slot.resolved += 1;
        } else {
            slot.missing += 1;
        }
    }

    fn key(kind: IconKind, id: u16) -> String {
        let dir = kind.dir().unwrap_or("none");
        format!("{dir}/{id}.png")
    }

    /// Resolve an icon to a texture, decoding + uploading it on first use. `None`
    /// means "no art for this id" (Rogue-only items, fake forms).
    pub fn get(&self, ctx: &Context, kind: IconKind, id: u16) -> Option<TextureHandle> {
        if kind == IconKind::None || id == 0 {
            return None;
        }
        let key = Self::key(kind, id);
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let tex = self.root.as_ref().and_then(|root| {
            let path = root.join(&key);
            let bytes = std::fs::read(&path).ok()?;
            let (w, h, rgba) = decode_png(&bytes)?;
            let img = ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            Some(ctx.load_texture(&key, img, TextureOptions::LINEAR))
        });
        match &tex {
            Some(_) => self.bump(kind, true),
            None => self.bump(kind, false),
        }
        self.cache.borrow_mut().insert(key, tex.clone());
        tex
    }
}

/// Look for an `assets/icons` directory, in a deliberate order:
/// `$ER_EDITOR_ASSETS`, next to the executable, repo root when running from
/// `cargo run`, then the current directory.
pub(crate) fn find_asset_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("ER_EDITOR_ASSETS") {
        candidates.push(PathBuf::from(env));
    }
    if let Ok(exe) = std::env::current_exe() {
        for up in 0..4 {
            let mut p = exe.clone();
            for _ in 0..up {
                p = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
            }
            candidates.push(p.join("assets"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets"));
    }
    candidates
        .into_iter()
        .map(|p| p.join("icons"))
        .find(|p| p.is_dir())
}

/// Decode PNG bytes to RGBA8, normalising every colour type the sprite repos
/// actually ship (RGBA8, RGB8, palette-expanded, gray, and their 16-bit forms).
fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // The crate defaults to `Transformations::IDENTITY`, i.e. exactly what is on
    // disk. Every sprite in the public repos is a 4-bit *paletted* PNG (some with
    // tRNS transparency), and paletted indices are useless here - without this the
    // whole icon set silently renders as fallback badges. `ALPHA | EXPAND |
    // STRIP_16` (the crate's `normalize_to_color8` plus forced alpha) yields 8-bit
    // RGBA for every input colour type, transparent background included. Must be
    // set on the Decoder, before `read_info`, so the buffer size matches.
    decoder.set_transformations(
        png::Transformations::ALPHA | png::Transformations::EXPAND | png::Transformations::STRIP_16,
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let w = info.width as usize;
    let h = info.height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    // png 0.17 keeps bit depth separate from colour type: 16-bit sources use the
    // same ColourType with two bytes per channel, and the high byte is the value.
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        // The transformations above leave no indexed or sub-8-bit output behind.
        _ => return None,
    };
    let bytes_per_channel = if info.bit_depth == png::BitDepth::Sixteen {
        2
    } else {
        1
    };
    let expected = w * h * channels * bytes_per_channel;
    if buf.len() < expected {
        return None;
    }
    let stride = channels * bytes_per_channel;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in buf[..expected].chunks_exact(stride) {
        let px = |i: usize| row[i * bytes_per_channel];
        match channels {
            4 => rgba.extend_from_slice(&[px(0), px(1), px(2), px(3)]),
            3 => rgba.extend_from_slice(&[px(0), px(1), px(2), 255]),
            2 => rgba.extend_from_slice(&[px(0), px(0), px(0), px(1)]),
            _ => rgba.extend_from_slice(&[px(0), px(0), px(0), 255]),
        }
    }
    Some((w, h, rgba))
}

/// Deterministic hue per id, so a missing icon still groups visually with its
/// own kind instead of being a grey box everywhere.
pub(crate) fn hue_for(id: u16) -> Color32 {
    let h = (id.wrapping_mul(47)) as f32 / 256.0 * 360.0;
    hsv_to_rgb(h, 0.45, 0.75)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Initials for a fallback badge, skipping a leading form marker like "MEGA ".
fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    match words.len() {
        0 => "?".into(),
        1 => words[0].chars().take(2).collect(),
        _ => format!(
            "{}{}",
            words[0].chars().next().unwrap_or('?'),
            words[1].chars().next().unwrap_or('?')
        ),
    }
}

/// Reserve `size` x `size` in the current layout and draw the icon there, or a
/// generated badge when the art is missing. Returns the rect used.
pub fn draw_icon(
    ui: &mut Ui,
    icons: &IconStore,
    ctx: &Context,
    kind: IconKind,
    id: u16,
    name: &str,
    size: f32,
) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_icon(ui, icons, ctx, kind, id, name, size, rect);
    rect
}

/// Paint an icon into an explicit rect (used where the caller already allocated
/// the row, e.g. inside virtualized combo rows and roster lists).
// The parameter list is the operation: store, context, namespace, id, where to
// draw and how big. Bundling them into a struct would move the meaning away from
// the call site without shortening anything a reader has to hold.
#[allow(clippy::too_many_arguments)]
pub fn paint_icon(
    ui: &mut Ui,
    icons: &IconStore,
    ctx: &Context,
    kind: IconKind,
    id: u16,
    name: &str,
    size: f32,
    rect: Rect,
) {
    if kind == IconKind::None {
        return;
    }
    let square = Rect::from_center_size(rect.center(), Vec2::splat(size));
    match icons.get(ctx, kind, id) {
        Some(tex) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            // egui 0.27's painter takes a TextureId, not a handle.
            ui.painter().image(tex.id(), square, uv, Color32::WHITE);
        }
        None => {
            let fill = hue_for(id);
            ui.painter()
                .rect_filled(square, 3.0, fill.gamma_multiply(0.35));
            ui.painter()
                .rect_stroke(square, 3.0, egui::Stroke::new(1.0_f32, fill));
            let text = initials(name);
            let galley = ui.painter().layout_no_wrap(
                text,
                egui::TextStyle::Small.resolve(ui.style()),
                Color32::WHITE,
            );
            ui.painter().galley(
                square.center() - galley.size() * 0.5,
                galley,
                Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-skipping: a fresh clone has no art tree (it is fetched, not committed).
    /// Locally this pins the canonical layout AND proves a real PNG decodes into a
    /// texture, which is the part that silently no-ops when the paths are wrong.
    #[test]
    fn asset_tree_resolves_and_real_sprites_load() {
        let store = IconStore::new();
        let Some(root) = store.root.clone() else {
            eprintln!("no assets/icons tree present (run `python3 tools/get_icons.py`); skipping");
            return;
        };
        assert!(
            root.join("pokemon/25.png").is_file(),
            "root must be the icons dir itself; got {root:?}"
        );
        let ctx = egui::Context::default();
        assert!(
            store.get(&ctx, IconKind::Pokemon, 25).is_some(),
            "species art must load"
        );
        assert!(
            store.get(&ctx, IconKind::Item, 2).is_some(),
            "item art must load"
        );
        // Rogue homebrew ids have no public art: fallback badge, and counted once.
        assert!(store.get(&ctx, IconKind::Pokemon, 1522).is_none());
        assert_eq!(store.tally(IconKind::Pokemon).missing, 1);
        assert_eq!(store.tally(IconKind::Pokemon).resolved, 1);
        assert_eq!(store.tally(IconKind::Item).resolved, 1);
    }

    /// A root that exists but has no art fetched: the counters must be per
    /// namespace and the status line must name both. One pooled number used to
    /// print "0 with no art" while every item row was a letter badge, which is
    /// exactly what read as a deliberate design choice.
    #[test]
    fn per_namespace_counters_and_status_name_pokemon_and_items_separately() {
        let root = temp_root("empty");
        // An empty root: the assets dir was found, but nothing was fetched.
        let store = IconStore::at_root(Some(root.clone()));
        let ctx = egui::Context::default();
        assert!(store.get(&ctx, IconKind::Pokemon, 25).is_none());
        assert!(store.get(&ctx, IconKind::Item, 2).is_none());
        assert!(store.get(&ctx, IconKind::Item, 7).is_none());
        assert!(
            store.get(&ctx, IconKind::Item, 2).is_none(),
            "a cached miss must stay a miss"
        );
        assert_eq!(
            store.tally(IconKind::Pokemon),
            IconTally {
                resolved: 0,
                missing: 1
            }
        );
        assert_eq!(
            store.tally(IconKind::Item),
            IconTally {
                resolved: 0,
                missing: 2
            },
            "the repeat lookup must not be counted twice"
        );
        assert_eq!(store.tally(IconKind::None), IconTally::default());

        let line = store.status();
        assert!(line.starts_with("icons: pokemon "), "{line}");
        assert!(line.contains("pokemon 0 shown / 1 missing"), "{line}");
        assert!(line.contains("items 0 shown / 2 missing"), "{line}");
        assert!(
            line.contains(&format!("(root {})", root.display())),
            "{line}"
        );
        // The state the Items tab has to explain out loud instead of leaving as a badge
        // that looks intentional.
        assert!(store.items_art_absent(), "an empty root has no item art");

        // The same store with item art on disk must NOT claim it is missing.
        let stocked_root = temp_root("stocked");
        std::fs::create_dir_all(stocked_root.join("items")).unwrap();
        std::fs::write(
            stocked_root.join("items/68.png"),
            b"not a png, only the name counts",
        )
        .unwrap();
        let stocked = IconStore::at_root(Some(stocked_root.clone()));
        assert!(
            !stocked.items_art_absent(),
            "item art installed: no hint, even before an id resolved"
        );
        assert!(stocked.status().contains("items 0 shown / 0 missing"));

        // No root at all: the line keeps naming the fetcher, and the in-list hint stays
        // off because the status line already says what to run.
        let bare = IconStore::at_root(None);
        let line = bare.status();
        assert!(line.contains("no assets/icons dir"), "{line}");
        assert!(line.contains("python3 tools/get_icons.py"), "{line}");
        assert!(!bare.items_art_absent());

        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&stocked_root).unwrap();
    }

    /// Unique scratch root under the temp dir (tests run in parallel threads).
    fn temp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "er-save-editor-icons-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    /// Every file the fetcher put on disk must actually decode. This is the check
    /// that caught `Transformations::IDENTITY` silently rejecting 4-bit palettes.
    #[test]
    fn every_fetched_png_decodes() {
        let Some(root) = find_asset_root() else {
            eprintln!("no assets/icons tree present; skipping");
            return;
        };
        let mut checked = 0usize;
        let mut bad = Vec::new();
        for kind in ["pokemon", "items"] {
            let dir = root.join(kind);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                bad.push(format!("missing dir {dir:?}"));
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("png") {
                    continue;
                }
                let bytes = std::fs::read(&path).unwrap_or_default();
                if decode_png(&bytes).is_none() {
                    bad.push(path.file_name().unwrap().to_string_lossy().into_owned());
                }
                checked += 1;
            }
        }
        assert!(
            checked > 2000,
            "only {checked} icons on disk - re-run tools/get_icons.py"
        );
        assert!(
            bad.is_empty(),
            "{} undecodable icons: {:?}",
            bad.len(),
            &bad[..bad.len().min(10)]
        );
    }
    /// The status line is the only place a fresh clone is told why every row is a
    /// letter badge, so it has to name the gap and the command that fixes it.
    /// The status line is the only place a fresh clone learns why every row is a letter
    /// badge, so it has to name the gap and the command that fixes it - on a fixture, so
    /// it also runs where no art was ever fetched.
    #[test]
    fn the_status_line_names_the_gap_and_the_command_that_fixes_it() {
        let root = temp_root("status-gaps");
        std::fs::create_dir_all(root.join("pokemon")).unwrap();
        std::fs::create_dir_all(root.join("items")).unwrap();
        std::fs::write(root.join("items/68.png"), b"stub").unwrap();
        let line = IconStore::at_root(Some(root.clone())).status();
        assert!(line.contains("no art"), "{line}");
        assert!(line.contains("tools/get_icons.py"), "{line}");
        assert!(line.contains("tools/get_icons_from_source.py"), "{line}");
        std::fs::remove_dir_all(&root).unwrap();

        let bare = IconStore::at_root(None);
        assert!(
            bare.status().contains("tools/get_icons.py"),
            "{}",
            bare.status()
        );
    }

    /// Locally, when the fetched art tree is present, pin how complete it is: the only
    /// species ids without art should be the ones the game itself declares with no
    /// `species_info` entry. Self-skipping, because CI has no art tree.
    #[test]
    fn the_real_asset_tree_is_nearly_complete() {
        let store = IconStore::new();
        let Some(_) = store.root() else {
            eprintln!("no assets/icons tree present (run `python3 tools/get_icons.py`); skipping");
            return;
        };
        let g = store.art_gaps().pokemon.expect("root present");
        assert!(g.with_art > 1400, "resolved art: {}", g.with_art);
        let absent = g.total - g.with_art;
        // Scatterbug cloaks, Mothim cloaks, Totem forms, Shadow Lugia and friends.
        assert!((50..=90).contains(&absent), "ids without art: {absent}");
    }

    /// The gap report answers "which ids have no art" from the game's id space, not
    /// from whatever a picker happened to request, so a fresh clone is told to fetch
    /// art instead of quietly showing letter badges. Measured against a two-file
    /// fixture here because the real tree is gitignored: CI has never fetched it, and a
    /// test that assumes it exists passes locally while breaking the build - which is
    /// exactly how the first version of this test failed. The real tree is pinned
    /// separately by `the_real_asset_tree_is_nearly_complete`.
    #[test]
    fn the_gap_report_counts_the_games_id_space_against_the_files() {
        let root = temp_root("gaps");
        std::fs::create_dir_all(root.join("pokemon")).unwrap();
        std::fs::create_dir_all(root.join("items")).unwrap();
        std::fs::write(root.join("pokemon/25.png"), b"stub").unwrap();
        let store = IconStore::at_root(Some(root.clone()));

        let gaps = store.art_gaps();
        let g = gaps.pokemon.expect("a root means a gap report");
        assert_eq!(
            g.total,
            crate::game_data::all_species().len(),
            "the gap is measured against the game's id space, not the file count"
        );
        assert_eq!(g.with_art, 1, "the fixture holds one sprite");
        assert_eq!(
            g.sample.len(),
            4,
            "a few named examples is what makes it actionable"
        );
        for (id, name) in &g.sample {
            assert!(!store.dir_has_pokemon(*id), "{id} is in the missing sample");
            assert!(
                !name.is_empty(),
                "a sampled gap needs its label to be useful"
            );
        }

        // The per-row note: silent where art exists, explicit where it does not, and it
        // names the command either way.
        assert!(
            store.species_art_note(25).is_none(),
            "25 has art in the fixture"
        );
        let gap_id = g.sample[0].0;
        let note = store
            .species_art_note(gap_id)
            .expect("a gap id must explain itself");
        assert!(note.contains("tools/get_icons.py"), "{note}");
        assert!(note.contains(&gap_id.to_string()), "{note}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
