//! Functional tests for the searchable dropdown: real egui frames driven by
//! synthetic pointer and key events, checking what the user reported - the
//! popup must survive a press on its search field, take the caret when it
//! opens, filter as you type, and return the id you pick.
//!
//! Press and release are delivered on *separate* frames, like a real mouse
//! click. That matters: `show()` decides dismissal from `pointer.any_pressed()`,
//! which is only true on the frame of the press, so a same-frame press+release
//! would model a click that egui itself does not produce.

use super::*;
use crate::icons::IconStore;
use egui::{Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

fn tiny_table() -> Vec<(u16, &'static str)> {
    vec![
        (1, "applum"),
        (2, "oran berry"),
        (3, "poke ball"),
        (4, "great ball"),
        (5, "master ball"),
        (6, "tm40 aerial ace"),
    ]
}

fn move_to(pos: Pos2) -> Vec<Event> {
    vec![Event::PointerMoved(pos)]
}

fn press(pos: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        },
    ]
}

fn release(pos: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        },
    ]
}

fn key(k: Key) -> Vec<Event> {
    vec![Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    }]
}

fn type_str(s: &str) -> Vec<Event> {
    s.chars().map(|c| Event::Text(c.to_string())).collect()
}

struct Harness {
    ctx: egui::Context,
    store: IconStore,
    table: Vec<(u16, &'static str)>,
    opts: Opts,
    last: Option<u16>,
    frames: usize,
}

impl Harness {
    fn new() -> Self {
        Harness {
            ctx: egui::Context::default(),
            store: IconStore::default(),
            table: tiny_table(),
            // Text-only rows: no asset files needed for a unit test.
            opts: Opts::named().with_none("none"),
            last: None,
            frames: 0,
        }
    }

    /// One egui frame, with one warm-up frame up front: a widget cannot be
    /// clicked in the same frame it is first laid out, exactly as in a running
    /// app where the trigger has been on screen since before the click.
    fn frame(&mut self, events: Vec<Event>) {
        if self.frames == 0 {
            self.frames += 1;
            self.run_once(vec![]);
        }
        self.frames += 1;
        self.run_once(events);
    }

    /// One egui frame. The trigger is the first widget of an Area pinned at
    /// (100,100), min_width 320 -> the button spans x in [100,420], y in
    /// [100,~122], and the popup opens directly below it.
    fn run_once(&mut self, events: Vec<Event>) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        let mut picked = None;
        let table = std::mem::take(&mut self.table);
        let opts = &self.opts;
        let store = &self.store;
        let _ = self.ctx.run(input, |ctx| {
            egui::Area::new(Id::new("root"))
                .fixed_pos(Pos2::new(100.0, 100.0))
                .show(ctx, |ui| {
                    if let Some(id) = show(ui, "probe", &table, 0, opts, store) {
                        picked = Some(id);
                    }
                });
        });
        self.table = table;
        self.last = picked;
    }

    /// A full click, three frames: hover, press, release - the way a real
    /// pointer arrives. egui registers click interest only once the pointer has
    /// been over the widget for a frame, so skipping the hover frame makes the
    /// synthetic click land nowhere.
    fn click(&mut self, pos: Pos2) {
        self.frame(move_to(pos));
        self.frame(press(pos));
        self.frame(release(pos));
    }

    /// Ask egui directly rather than recomputing the widget's internal popup
    /// id: the Area's child ui derives its id from a private child path, so a
    /// hand-mirrored id would silently be wrong.
    fn popup_open(&self) -> bool {
        self.ctx.memory(|m| m.any_popup_open())
    }

    fn focused(&self) -> Option<Id> {
        self.ctx.memory(|m| m.focused())
    }
}

// Trigger button: x in [100,420], y in [100,122].
const BUTTON: Pos2 = Pos2::new(150.0, 110.0);
// Popup opens under the button; its search field is the first row inside.
const FIELD: Pos2 = Pos2::new(140.0, 132.0);
// Well outside both button and popup.
const FAR: Pos2 = Pos2::new(900.0, 800.0);

#[test]
fn popup_starts_closed_and_a_full_click_opens_it() {
    let mut h = Harness::new();
    h.frame(vec![]);
    assert!(!h.popup_open(), "popup must start closed");
    h.frame(move_to(BUTTON));
    assert!(!h.popup_open(), "hovering must not open the popup");
    h.frame(press(BUTTON));
    h.frame(release(BUTTON));
    assert!(h.popup_open(), "clicking the trigger must open the popup");
}

#[test]
fn opening_the_popup_moves_the_caret_to_the_search_field() {
    let mut h = Harness::new();
    h.click(BUTTON);
    assert!(
        h.focused().is_some(),
        "the popup must grab the keyboard for its search field"
    );
}

#[test]
fn a_press_on_the_search_field_keeps_the_popup_open() {
    let mut h = Harness::new();
    h.click(BUTTON);
    assert!(h.popup_open());
    // The exact interaction that egui's stock ComboBox kills: pressing inside
    // the popup, on its own search field, must not count as "clicked elsewhere".
    h.frame(press(FIELD));
    assert!(
        h.popup_open(),
        "a press on the search field must not dismiss the popup"
    );
    h.frame(release(FIELD));
    assert!(
        h.popup_open(),
        "releasing on the search field must not either"
    );
    // The caret stays claimed by the popup (the typing test below proves it):
    // what this test guards is that the *dismissal* rule ignores presses that
    // land inside the popup frame.
}

#[test]
fn typing_filters_and_enter_picks_the_highlighted_row() {
    let mut h = Harness::new();
    h.click(BUTTON);
    h.frame(type_str("great"));
    h.frame(key(Key::Enter));
    // "great" matches exactly one row (id 4). Unfiltered, the top row is the
    // "none" entry (id 0), so this also proves the keystrokes reached the field.
    assert_eq!(h.last, Some(4), "Enter must pick the filtered row");
    assert!(!h.popup_open(), "picking a row closes the popup");
}

#[test]
fn filter_survives_a_click_on_the_field() {
    let mut h = Harness::new();
    h.click(BUTTON);
    h.frame(type_str("ball"));
    h.frame(press(FIELD));
    h.frame(release(FIELD));
    h.frame(key(Key::Enter));
    // poke/great/master ball match; the first is id 3.
    assert_eq!(
        h.last,
        Some(3),
        "the filter must survive clicking the field"
    );
}

#[test]
fn typing_a_bare_id_matches_it() {
    let mut h = Harness::new();
    h.click(BUTTON);
    h.frame(type_str("6"));
    h.frame(key(Key::Enter));
    assert_eq!(h.last, Some(6), "a bare id must match exactly");
}

#[test]
fn a_press_outside_dismisses_the_popup() {
    let mut h = Harness::new();
    h.click(BUTTON);
    assert!(h.popup_open());
    h.frame(press(FAR));
    assert!(!h.popup_open(), "a press outside the popup must close it");
}

#[test]
fn escape_dismisses() {
    let mut h = Harness::new();
    h.click(BUTTON);
    assert!(h.popup_open());
    h.frame(key(Key::Escape));
    assert!(!h.popup_open(), "Escape must close the popup");
}

#[test]
fn arrow_keys_move_the_selection() {
    let mut h = Harness::new();
    h.click(BUTTON);
    // Rows: none(0), 1, 2, ... Two downs from the top -> id 2.
    h.frame(key(Key::ArrowDown));
    h.frame(key(Key::ArrowDown));
    h.frame(key(Key::Enter));
    assert_eq!(h.last, Some(2), "arrow keys must move the highlighted row");
}

#[test]
fn clicking_a_row_picks_it() {
    let mut h = Harness::new();
    h.click(BUTTON);
    // Measured from the widget's own layout in this style: rows start at
    // y=148 and are ~20px tall, so the second band (id 1) is centred ~170.
    let row = Pos2::new(200.0, 170.0);
    h.frame(move_to(row));
    h.frame(press(row));
    h.frame(release(row));
    assert_eq!(
        h.last,
        Some(1),
        "clicking the second row band must return that row's id"
    );
    assert!(!h.popup_open(), "clicking a row closes the popup");
}

#[test]
fn reopening_resets_the_filter_so_the_full_list_shows_again() {
    let mut h = Harness::new();
    h.click(BUTTON);
    h.frame(type_str("great"));
    h.frame(key(Key::Escape));
    assert!(!h.popup_open());
    h.click(BUTTON);
    h.frame(key(Key::Enter));
    // With the filter cleared, the top row is the "none" entry.
    assert_eq!(h.last, Some(0), "a reopened popup must show the full list");
}

/// The rows must actually ask the icon store for sprites: a decoded file on disk
/// proves nothing if the painted row never resolves a texture, and a Rogue-only
/// id with no public art must fall back to its badge without derailing the row.
#[test]
fn rows_resolve_sprite_textures_and_badges() {
    if crate::icons::find_asset_root().is_none() {
        eprintln!("no assets/icons tree present; skipping");
        return;
    }
    let mut h = Harness::new();
    h.opts = Opts::pokemon();
    h.table = vec![(25, "pikachu"), (1522, "rogue start")];
    h.click(BUTTON);
    assert!(h.popup_open());
    // The list paints on the frame *after* the popup opens (egui cannot lay out
    // an Area in the same frame it is requested), so idle one frame first.
    h.frame(vec![]);
    assert!(
        h.store.tally(IconKind::Pokemon).resolved >= 1,
        "a row with art on disk must resolve a texture"
    );
    assert!(
        h.store.tally(IconKind::Pokemon).missing >= 1,
        "the art-less id must be recorded as absent (badge fallback)"
    );
}

/// User report: "the ball menu does not include the icons for the balls." A ball's
/// stored id IS the ball's item id (pokeball:5 + `ItemIdToBallId`), so the ball picker
/// must request the ITEM icon namespace; a name-only row hides which ball it is.
#[test]
fn ball_rows_request_the_item_icon_namespace() {
    if crate::icons::find_asset_root().is_none() {
        eprintln!("no assets/icons tree present; skipping");
        return;
    }
    let mut h = Harness::new();
    h.opts = Opts::item()
        .compact()
        .with_hint("filter balls")
        .with_none("(no ball)");
    h.table = crate::game_data::balls().to_vec();
    h.click(BUTTON);
    assert!(h.popup_open());
    h.frame(vec![]); // the list paints one frame after the Area is requested
    let items = h.store.tally(IconKind::Item);
    assert!(
        items.resolved >= 1,
        "ball rows must resolve the ball's own art (items/<id>.png, e.g. id 1 = Poké Ball)"
    );
    assert_eq!(
        h.store.tally(IconKind::Pokemon).resolved + h.store.tally(IconKind::Pokemon).missing,
        0,
        "a ball id must never be looked up in the species namespace"
    );
}

/// A picker narrower than the row it floats over leaves that row sticking out on
/// both sides, which reads as the pokemon next to the search bar being cut off.
/// The popup has to span the line its trigger sits on.
/// The sprite in a picker's own button used to be painted left of the button,
/// which read as "this pokemon is cut off" for every picker at a panel edge.
#[test]
fn trigger_icon_stays_inside_the_button() {
    let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(160.0, 24.0));
    let icon = trigger_icon_rect(rect, ICON_SIZE + 8.0);
    assert!(
        rect.contains(icon.min) && rect.contains(icon.max),
        "icon {icon:?} escapes the trigger {rect:?} - a sprite drawn outside the button is sliced by whatever edge the button sits against"
    );
}

#[test]
fn popup_spans_the_line_its_trigger_sits_on() {
    let ctx = egui::Context::default();
    let store = IconStore::default();
    let table = tiny_table();
    // A compact picker: its own min_width is small, so only the line-width rule
    // can make it wide enough to cover the row behind it.
    let opts = Opts::named().compact();
    let run = |events: Vec<Event>| {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::Area::new(Id::new("root"))
                .fixed_pos(Pos2::new(100.0, 100.0))
                .show(ctx, |ui| {
                    // A 400-px line, like the roster panel's header row.
                    ui.set_max_width(400.0);
                    let _ = show(ui, "wide_line", &table, 0, &opts, &store);
                });
        });
    };
    run(vec![]);
    run(move_to(Pos2::new(150.0, 110.0)));
    run(press(Pos2::new(150.0, 110.0)));
    run(release(Pos2::new(150.0, 110.0)));
    run(vec![]); // rows are laid out on the frame after the popup opens
    eprintln!(
        "DBG open={} frame={:?}",
        ctx.memory(|m| m.any_popup_open()),
        ctx.data(|d| d.get_temp::<Rect>(Id::new("scombo_frame_rect:wide_line")))
    );

    let frame: Option<Rect> =
        ctx.data(|d| d.get_temp::<Rect>(Id::new("scombo_frame_rect:wide_line")));
    let frame = frame.expect("the popup must publish its frame rect");
    assert!(
        frame.width() >= 380.0,
        "the popup must cover the 400-px line under its trigger; it is {:.1} wide \
         at {frame:?} - a narrow popup leaves the row poking out on both sides",
        frame.width()
    );
    assert!(
        frame.min.x <= 105.0,
        "the popup must start at the line's left edge, not inset from it: {frame:?}"
    );
}

/// Some id spaces number from 0 - ability slots, met-location sections - so id 0
/// has to be labelled like any other value. The `id == 0` means-empty convention
/// (right for species, items, moves, held item) is what made every mon with
/// `abilityNum` 0 show its ability as "-". The visible end of this is covered by
/// `the_mon_editor_names_the_met_location_and_the_ability_slot`; here the label
/// resolver itself is pinned so the two conventions cannot bleed into each other.
#[test]
fn a_positional_namespace_labels_zero_instead_of_calling_it_empty() {
    let table: Vec<(u16, &'static str)> =
        vec![(0, "clear body"), (1, "light metal"), (2, "slot empty")];
    assert_eq!(
        lookup(&table, 0, &Opts::named().positional()),
        "clear body",
        "id 0 of a positional namespace must resolve to its own name"
    );
    // Pickers whose 0 really means "nothing here" keep the placeholder, and id 0
    // is not offered as a row there (that path is exercised by `with_none`).
    assert_eq!(
        lookup(&table, 0, &Opts::named().with_none("none")),
        "none",
        "the default convention must stay 0 = empty"
    );
    assert_eq!(
        lookup(&table, 0, &Opts::named()),
        "-",
        "without a none label the placeholder is the dash that hid the ability"
    );
    // A positional namespace still resolves non-zero ids normally.
    assert_eq!(lookup(&table, 2, &Opts::named().positional()), "slot empty");
}
