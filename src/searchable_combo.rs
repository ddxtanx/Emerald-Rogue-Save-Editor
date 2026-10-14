//! One searchable dropdown for every id picklist in the editor.
//!
//! ## Why not `egui::ComboBox`
//! `ComboBox` renders its popup but decides dismissal from the *trigger
//! button's* rect: `containers/popup.rs` closes whenever
//! `button_response.clicked_elsewhere()`, and `Response::clicked_elsewhere`
//! tests the press against the button's own `interact_rect`. A click on the
//! popup's search field is therefore "elsewhere", so the popup closes and eats
//! the click before the `TextEdit` ever sees it. Nothing in egui 0.27 exempts
//! popup contents from that rule, so the popup is built here on `egui::Area`
//! with our own dismissal: a press closes only when it lands outside the popup's
//! own frame, or on Escape, or when a row is picked.
//!
//! Two behaviours the stock widget cannot give us, both requested:
//!   * clicking the search field keeps the popup open and puts the caret there;
//!   * the field is focused the frame the popup opens, so you can type right
//!     away and the list filters as a case-insensitive name substring.
//!
//! Rows are virtualized (`show_rows`) - the species table is ~1500 rows and the
//! item table ~900 - and each row can carry an icon from `crate::icons`.
//!
//! Transient state (filter text, highlighted row, focus arming) lives in egui's
//! per-`Id` data store, so it survives the frame in which the popup loses focus
//! and is fresh again after the popup is closed.

use crate::icons::{self, IconKind, IconStore};
use egui::{
    Align2, Button, Color32, Context, Frame, Id, Key, LayerId, Order, PointerButton, Rect,
    ScrollArea, TextEdit, TextStyle, Ui, Vec2,
};

/// Popup geometry / content options.
pub struct Opts {
    /// Which icon namespace the ids belong to (`IconKind::None` = text only).
    pub icons: IconKind,
    /// Show the numeric id alongside each name.
    pub show_id: bool,
    pub min_width: f32,
    pub max_height: f32,
    /// Placeholder shown in the search field.
    pub hint: &'static str,
    /// When set, a row with id 0 and this label is offered (held item, ball…).
    pub none_label: Option<&'static str>,
    /// Label used if the current id is not in the table.
    pub unknown_label: &'static str,
    /// True when id 0 means "nothing here" in this namespace (species, item,
    /// move, held item). **False** for namespaces where 0 is a stored value:
    /// ability slot 0, or met location `MAPSEC_POKEMON_HUB`. Treating those as
    /// empty is how the ability picker started rendering every slot-0 mon as "-".
    pub zero_is_none: bool,
    /// Extra hover text for the trigger button. Names-only pickers hide the raw
    /// stored number, so this is where it stays discoverable.
    pub hover: Option<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            icons: IconKind::None,
            show_id: true,
            min_width: 320.0,
            max_height: 340.0,
            hint: "filter by name",
            none_label: None,
            unknown_label: "??????",
            zero_is_none: true,
            hover: None,
        }
    }
}

impl Opts {
    pub fn pokemon() -> Self {
        Opts {
            icons: IconKind::Pokemon,
            hint: "filter species",
            min_width: 360.0,
            ..Default::default()
        }
    }
    pub fn item() -> Self {
        Opts {
            icons: IconKind::Item,
            hint: "filter items",
            min_width: 340.0,
            ..Default::default()
        }
    }
    pub fn named() -> Self {
        Opts {
            show_id: true,
            hint: "filter",
            ..Default::default()
        }
    }
    pub fn with_none(mut self, label: &'static str) -> Self {
        self.none_label = Some(label);
        self
    }
    /// Rename the search-field hint for a narrow list (balls, styles...).
    pub fn with_hint(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }
    /// Drop the numeric id from rows and from the trigger label. For domains where
    /// the id carries no information (the 25 natures), showing it is noise.
    pub fn no_id(mut self) -> Self {
        self.show_id = false;
        self
    }
    /// The namespace numbers its values from 0, so 0 is a real choice and must be
    /// listed and labelled like any other (ability slots, met-location sections).
    pub fn positional(mut self) -> Self {
        self.zero_is_none = false;
        self
    }
    /// Hover text for the trigger, used to keep the raw stored value visible when
    /// `no_id()` hides it from the label.
    pub fn hover(mut self, text: impl Into<String>) -> Self {
        self.hover = Some(text.into());
        self
    }
    pub fn compact(mut self) -> Self {
        self.min_width = 200.0;
        self.max_height = 260.0;
        self
    }
}

/// Per-popup transient state (kept in `ctx.data()`, not in the caller).
#[derive(Clone, Default)]
struct PopupState {
    filter: String,
    highlighted: usize,
    /// We have already requested the caret for this open session of the popup.
    focus_armed: bool,
}

/// Draw the trigger button; if its popup is open, draw the list and return the
/// id the user chose this frame (click, Enter, or double-click on a row).
pub fn show(
    ui: &mut Ui,
    id_src: &str,
    table: &[(u16, &str)],
    current: u16,
    opts: &Opts,
    icons_store: &IconStore,
) -> Option<u16> {
    let button_id = ui.id().with("scombo").with(id_src);
    let popup_id = button_id.with("popup");
    let ctx = ui.ctx().clone();

    // If the popup is closed (we just closed it, or another widget took the
    // popup stack), disarm the caret request so the next open focuses again.
    if !ui.memory(|m| m.is_popup_open(popup_id)) {
        if let Some(mut st) = ui.ctx().data_mut(|d| d.get_temp::<PopupState>(popup_id)) {
            if st.focus_armed {
                st.focus_armed = false;
                ui.ctx().data_mut(|d| d.insert_temp(popup_id, st));
            }
        }
    }

    let icon_pad = if opts.icons == IconKind::None {
        0.0
    } else {
        ICON_SIZE + ui.spacing().item_spacing.x
    };

    // Width the line had when the trigger was allocated; the popup uses it so it
    // covers the row behind it instead of floating narrowly over a wider one.
    let anchor_width = ui.available_width();

    // ---- trigger button -----------------------------------------------------
    let cur_name = lookup(table, current, opts);
    let show_cur_id = opts.show_id && !(opts.zero_is_none && current == 0);
    let text = if show_cur_id {
        format!("{current} · {cur_name}")
    } else {
        cur_name.to_string()
    };

    // Rows are as wide as the popup, so the button is a comfortable click target.
    let btn_response = {
        let galley = ui.painter().layout_no_wrap(
            text.clone(),
            TextStyle::Button.resolve(ui.style()),
            Color32::PLACEHOLDER,
        );
        let size = Vec2::new(
            (icon_pad + galley.size().x + 22.0).max(opts.min_width * 0.5),
            galley.size().y + ui.spacing().button_padding.y * 2.0,
        );
        let resp = ui.add_sized(size, Button::new("").min_size(size));
        if ui.is_rect_visible(resp.rect) {
            let rect = resp.rect;
            if icon_pad > 0.0 {
                let icon_rect = trigger_icon_rect(rect, icon_pad);
                icons::paint_icon(
                    ui,
                    icons_store,
                    &ctx,
                    opts.icons,
                    current,
                    &cur_name,
                    ICON_SIZE,
                    icon_rect,
                );
            }
            let text_x = rect.left() + if icon_pad > 0.0 { icon_pad + 4.0 } else { 6.0 };
            ui.painter().galley(
                egui::pos2(text_x, rect.center().y - galley.size().y * 0.5),
                galley,
                ui.visuals().text_color(),
            );
            // Down-pointing triangle like ComboBox's default icon.
            let tri_w = 8.0;
            let tri = egui::Rect::from_min_size(
                egui::pos2(rect.right() - tri_w - 6.0, rect.center().y - 2.0),
                egui::vec2(tri_w, 5.0),
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
                ui.visuals().weak_text_color(),
                egui::Stroke::NONE,
            ));
        }
        resp
    };

    // Names-only pickers hide the stored number from the label, so the raw value
    // (and any "what this byte is" note) lives on hover instead.
    let btn_response = match opts.hover.as_deref() {
        Some(h) => btn_response.on_hover_text(h.to_string()),
        None => btn_response,
    };

    if btn_response.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut picked: Option<u16> = None;
    let open = ui.memory(|m| m.is_popup_open(popup_id));

    if open {
        // The popup must cover the whole line the button sits on, not just the
        // button: a picker as narrow as its label floats over a much wider roster
        // row, and the row sticking out on both sides of it reads as the pokemon
        // being cut off. `anchor_width` is what the button had to work with.
        let popup_width = (opts
            .min_width
            .max(anchor_width)
            .max(btn_response.rect.width()))
        .min(ctx.screen_rect().width() - 16.0);
        // Place below the button, flipping above when it would clip the screen.
        let below = btn_response.rect.bottom() + opts.max_height < ctx.screen_rect().bottom();
        let pos = if below {
            btn_response.rect.left_bottom()
        } else {
            btn_response.rect.left_top()
        };
        let pivot = if below {
            Align2::LEFT_TOP
        } else {
            Align2::LEFT_BOTTOM
        };

        // The frame geometry is published under a key derived from the call-site
        // name (not the egui id path, which a test cannot reconstruct) so the
        // width/anchor contract below is checkable without a screen.
        let rect_key = Id::new(format!("scombo_frame_rect:{id_src}"));
        let frame_rect = AreaPopup {
            popup_id,
            pos,
            pivot,
            min_width: popup_width,
            max_height: opts.max_height,
        }
        .draw(&ctx, |ui| {
            body(
                ui,
                &ctx,
                popup_id,
                table,
                current,
                opts,
                icons_store,
                &mut picked,
            )
        });
        ctx.data_mut(|d| d.insert_temp(rect_key, frame_rect));

        // ---- dismissal ------------------------------------------------------
        // A press outside our popup frame closes it. A press inside the frame -
        // including on the search field, the scrollbar, or empty padding - does
        // not, which is the whole point of this module.
        let press = ctx.input(|i| {
            if i.pointer.any_pressed() {
                i.pointer.interact_pos()
            } else {
                None
            }
        });
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let outside = press.map_or(false, |p| !frame_rect.contains(p));
        // A press that landed on a widget belonging to another layer (i.e. not
        // our popup) also closes us, even if it overlaps our rect.
        let foreign = press.map_or(false, |p| match ui.memory(|m| m.layer_id_at(p)) {
            Some(layer) => layer != LayerId::new(Order::Foreground, popup_id),
            None => false,
        });
        if escape || picked.is_some() || outside || foreign {
            ui.memory_mut(|m| m.close_popup());
            // Closing also drops the search text: reopening a 900- or
            // 1500-row list showing only the previous needle is how a picker
            // gets mistaken for "the item is missing".
            ui.ctx().data_mut(|d| {
                if let Some(mut st) = d.get_temp::<PopupState>(popup_id) {
                    st.focus_armed = false;
                    st.filter.clear();
                    st.highlighted = 0;
                    d.insert_temp(popup_id, st);
                }
            });
        }
    }

    picked
}

const ICON_SIZE: f32 = 24.0;

/// Where a trigger button draws its icon. The button reserves `icon_pad` on its
/// left, and the icon has to sit **inside** that reservation: drawing it at
/// `left_center() - icon_pad/2` placed it left of the button itself, so a picker
/// sitting at a panel's left edge had its pokemon sprite sliced off by that edge.
fn trigger_icon_rect(rect: Rect, icon_pad: f32) -> Rect {
    let side = ICON_SIZE.min(rect.height()).min(icon_pad);
    Rect::from_min_size(
        rect.left_top() + egui::vec2(4.0, (rect.height() - side) * 0.5),
        Vec2::splat(side),
    )
}

/// The popup itself: an `egui::Area` on the foreground layer, returning its
/// frame rect so the caller can decide about dismissal.
struct AreaPopup {
    popup_id: Id,
    pos: egui::Pos2,
    pivot: Align2,
    min_width: f32,
    max_height: f32,
}

impl AreaPopup {
    fn draw<R>(self, ctx: &Context, contents: impl FnOnce(&mut Ui) -> R) -> Rect {
        egui::Area::new(self.popup_id)
            .order(Order::Foreground)
            .constrain(true)
            .fixed_pos(self.pos)
            .pivot(self.pivot)
            .show(ctx, |ui| {
                let frame = Frame::popup(ui.style());
                let inner = frame.show(ui, |ui| {
                    ui.set_min_width(self.min_width - frame.total_margin().sum().x);
                    ui.set_max_height(self.max_height);
                    contents(ui)
                });
                inner.response.rect
            })
            .inner
    }
}

/// Search field + virtualized filtered list + keyboard navigation.
fn body(
    ui: &mut Ui,
    ctx: &Context,
    popup_id: Id,
    table: &[(u16, &str)],
    current: u16,
    opts: &Opts,
    icons_store: &IconStore,
    picked: &mut Option<u16>,
) {
    let mut st = ctx.data_mut(|d| d.get_temp_mut_or_default::<PopupState>(popup_id).clone());

    // ---- search field (keeps the popup open; auto-focused on open) ---------
    let mut field_rect = Rect::NOTHING;
    let search_resp = ui
        .horizontal(|ui| {
            let te = TextEdit::singleline(&mut st.filter)
                .hint_text(opts.hint)
                .desired_width(opts.min_width - 120.0);
            let resp = ui.add(te);
            field_rect = resp.rect;
            resp
        })
        .inner;
    if !st.focus_armed {
        st.focus_armed = true;
        search_resp.request_focus();
    }
    // The field has focus, so egui would hand it our arrow keys: a focused
    // TextEdit installs a focus-lock filter (text_edit/builder.rs:589) and
    // `Memory::begin_frame` DROPS any event that filter claims
    // (memory.rs:386-396) - `vertical_arrows: true` by default
    // (builder.rs:128). Without releasing it, the first Up/Down after the popup
    // opens is consumed as caret movement and never reaches the list, so the
    // keyboard walk is off by one press. Applied after `ui.add`, this overrides
    // the field's own filter for the next frame.
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            search_resp.id,
            egui::EventFilter {
                horizontal_arrows: true, // caret stays in the filter text
                vertical_arrows: false,  // Up/Down drive the row highlight
                tab: false,              // Tab moves focus, as usual
                escape: false,           // Escape closes the popup
            },
        )
    });
    if !st.filter.is_empty() && ui.button("clear").clicked() {
        st.filter.clear();
        st.highlighted = 0;
        search_resp.request_focus();
    }

    // ---- filter (case-insensitive name substring; also matches on id) ------
    let needle = st.filter.trim().to_lowercase();
    let mut matches: Vec<u16> = Vec::new();
    if let Some(nl) = opts.none_label {
        if needle.is_empty() || nl.to_lowercase().contains(&needle) {
            matches.push(0);
        }
    }
    for (id, name) in table {
        if *id == 0 && opts.zero_is_none {
            continue;
        }
        if needle.is_empty()
            || name.to_lowercase().contains(&needle)
            || (opts.show_id && id.to_string() == needle)
        {
            matches.push(*id);
        }
    }
    if !matches.is_empty() {
        st.highlighted = st.highlighted.min(matches.len() - 1);
    } else {
        st.highlighted = 0;
    }

    // ---- keyboard: arrows move the highlight, Enter picks ------------------
    let (up, down, enter) = ctx.input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
        )
    });
    if !matches.is_empty() {
        if down {
            st.highlighted = (st.highlighted + 1).min(matches.len() - 1);
        }
        if up {
            st.highlighted = st.highlighted.saturating_sub(1);
        }
        if enter {
            *picked = Some(matches[st.highlighted]);
        }
    }

    // ---- rows --------------------------------------------------------------
    let row_font = TextStyle::Button.resolve(ui.style());
    let row_height = ctx.fonts(|f| f.row_height(&row_font)) + ui.spacing().item_spacing.y;
    let used = field_rect.height() + ui.spacing().item_spacing.y;
    let view_h = (opts.max_height - used - 12.0).max(row_height * 4.0);

    let sa = ScrollArea::vertical()
        .max_height(view_h)
        .auto_shrink([false, true])
        .stick_to_bottom(false);

    let hi = st.highlighted;
    let out = sa.show_rows(ui, row_height, matches.len(), |ui, visible| {
        let mut hit: Option<u16> = None;
        let wide = ui.available_width();
        for r in visible {
            let id = matches[r];
            let name = if id == 0 && opts.zero_is_none {
                opts.none_label.unwrap_or("none")
            } else {
                lookup(table, id, opts)
            };
            let label = if opts.show_id && !(opts.zero_is_none && id == 0) {
                format!("{id:5}  {name}")
            } else {
                format!("  {name}")
            };
            let resp = ui.add_sized(
                Vec2::new(wide, row_height),
                Button::new(label).min_size(Vec2::new(wide, row_height)),
            );
            if ui.is_rect_visible(resp.rect) {
                let rect = resp.rect;
                if r == hi {
                    ui.painter().rect_filled(
                        rect.shrink(1.0),
                        3.0,
                        ui.visuals().selection.bg_fill.gamma_multiply(0.45),
                    );
                }
                if opts.icons != IconKind::None {
                    let icon_rect = Rect::from_center_size(
                        rect.left_center() + egui::vec2(4.0 + ICON_SIZE * 0.5, 0.0),
                        Vec2::splat(ICON_SIZE.min(row_height - 1.0)),
                    );
                    icons::paint_icon(
                        ui,
                        icons_store,
                        ctx,
                        opts.icons,
                        id,
                        name,
                        icon_rect.height(),
                        icon_rect,
                    );
                }
                if id == current {
                    // mark the currently-selected row
                    let galley = ui.painter().layout_no_wrap(
                        "○".to_owned(),
                        TextStyle::Small.resolve(ui.style()),
                        ui.visuals().strong_text_color(),
                    );
                    ui.painter().galley(
                        rect.right_center()
                            - egui::vec2(galley.size().x + 6.0, galley.size().y * 0.5),
                        galley,
                        ui.visuals().strong_text_color(),
                    );
                }
            }
            // `Response::clicked` also fires for keyboard/accesskit activation
            // (`fake_primary_click`, response.rs:137) - egui activates whatever
            // button is focusable when Enter arrives. Rows must answer only to
            // the pointer, or every Enter would pick a row out from under the
            // highlight the up/down walk just built. `clicked_by` is egui's own
            // pointer-only variant (response.rs:143-151).
            if resp.clicked_by(PointerButton::Primary)
                || resp.double_clicked_by(PointerButton::Primary)
            {
                hit = Some(id);
            }
        }
        hit
    });
    if let Some(id) = out.inner {
        *picked = Some(id);
    }

    // ---- footer: match count / empty state --------------------------------
    if matches.is_empty() {
        ui.weak(format!("nothing matches '{}'", st.filter.trim()));
    } else {
        footer_line(ui, matches.len(), listed_total(table, opts));
    }

    ctx.data_mut(|d| d.insert_temp(popup_id, st));
}

/// How many options the list could show at all, which is what "N of M options"
/// compares against. It is not `table.len()`: a 0-means-empty namespace offers a
/// synthetic "none" row that the table does not contain, and hides a 0 row that
/// the table does contain. Counting the table alone printed "29 of 28 options"
/// in the ball picker, because that picker offers `none` plus 28 balls.
fn listed_total(table: &[(u16, &str)], opts: &Opts) -> usize {
    let hidden_zero = usize::from(opts.zero_is_none && table.iter().any(|(id, _)| *id == 0));
    let none_row = usize::from(opts.none_label.is_some());
    table.len() - hidden_zero + none_row
}

fn footer_line(ui: &mut Ui, shown: usize, total: usize) {
    ui.small(format!(
        "{shown} of {} options · type to filter, up/down + Enter",
        total
    ));
}

/// Name for an id, honouring the `none` label and unknown ids.
fn lookup<'a>(table: &'a [(u16, &'a str)], id: u16, opts: &Opts) -> &'a str {
    if opts.zero_is_none && id == 0 {
        return opts.none_label.unwrap_or("-");
    }
    table
        .iter()
        .find(|(v, _)| *v == id)
        .map(|(_, n)| *n)
        .unwrap_or(opts.unknown_label)
}

#[cfg(test)]
mod tests;
