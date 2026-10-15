//! Visual editors for the hub ("home base") - the tile map and the player's
//! house.
//!
//! The batch asked for "a visual editor for the home base menu". The old UI was
//! a grid of checkboxes plus x/y number fields, which is technically complete
//! but unusable for the thing players actually want: seeing where the buildings
//! are and moving one to another tile. These two panels draw the tile grid the
//! game uses and let the user click/drag on it.
//!
//! Everything here stays inside the semantics already documented for the save:
//! * `areaBuiltFlags` and `areaCoords` are INDEPENDENT bytes on disk. The
//!    game's
//! `RogueHub_BuildArea()` writes both, but live saves contain built areas and
//! stored coordinates that do not imply each other, so this editor never
//! couples them silently: building, unbuilding, and moving are separate
//! actions, each labelled as what it does.
//! * `areaCoords` is `s8 x, s8 y` plus two pad bytes (stride 4 on disk).
//! * `homeDecorations` is 192 slots: 0..160 outdoor, 160..192 indoor, each
//! `u8 x, u8 y, u8 decorVariant, u8 active:1`.
//! * Reachability rules (collision, no built neighbour, orphaning a neighbour)
//!    are
//! reported by `SaveFile::hub_coord_warnings` and shown before/while the move
//! is made, not after the fact.

use crate::domain::{DecorVariant, HubArea};
use crate::format::HOME_DECOR_OUTSIDE;
use crate::game_data::HUB_AREAS;
use crate::savefile::{RogueBlock, SaveFile};
use egui::{Color32, Pos2, Rect, Ui, Vec2};

/// Tile span the map view starts with. A v2.2.0 live save keeps every area
/// inside (-2..2, -1..1); this is that with room to place new buildings.
const VIEW_MIN: i32 = -8;
const VIEW_MAX: i32 = 9;
/// Tile size in points. Small enough that the whole default view fits a panel
/// without scrolling, big enough that a 9-byte area label still reads.
const CELL: f32 = 30.0;

/// Which area the map editor treats as "the one to move".
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MapSel {
    area: Option<usize>,
}

/// Convert a pointer position into a tile coordinate, or None outside the grid.
/// Kept free of egui state so the mapping can be unit-tested directly.
pub(crate) fn tile_at(
    pos: Pos2,
    origin: Pos2,
    cell: f32,
    x0: i32,
    y0: i32,
    cols: i32,
    rows: i32,
) -> Option<(i32, i32)> {
    if cell <= 0.0 {
        return None;
    }
    let tx = ((pos.x - origin.x) / cell).floor();
    let ty = ((pos.y - origin.y) / cell).floor();
    if !tx.is_finite() || !ty.is_finite() {
        return None;
    }
    let (tx, ty) = (tx as i32 + x0, ty as i32 + y0);
    if (x0..x0 + cols).contains(&tx) && (y0..y0 + rows).contains(&ty) {
        Some((tx, ty))
    } else {
        None
    }
}

/// Pixel rect of one tile, given the grid origin.
fn tile_rect(origin: Pos2, cell: f32, x0: i32, y0: i32, tx: i32, ty: i32) -> Rect {
    let x = origin.x + (tx - x0) as f32 * cell;
    let y = origin.y + (ty - y0) as f32 * cell;
    Rect::from_min_size(Pos2::new(x, y), Vec2::splat(cell))
}

/// Colour per area, so the map is readable at a glance without reading labels.
fn area_color(i: usize) -> Color32 {
    const PAL: [Color32; 8] = [
        Color32::from_rgb(90, 140, 200),
        Color32::from_rgb(200, 120, 90),
        Color32::from_rgb(110, 180, 120),
        Color32::from_rgb(190, 170, 90),
        Color32::from_rgb(150, 110, 190),
        Color32::from_rgb(90, 170, 180),
        Color32::from_rgb(200, 150, 110),
        Color32::from_rgb(130, 160, 150),
    ];
    PAL[i % PAL.len()]
}

/// Short label drawn inside a tile. Area names are long ("Adventure Entrance"),
/// so the tile shows initials and the full name goes in the hover text.
fn initials(name: &str) -> String {
    let mut out = String::new();
    for w in name.split_whitespace() {
        if let Some(c) = w.chars().next() {
            out.push(c.to_ascii_uppercase());
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

impl super::Editor {
    /// The hub tile map: 11 buildings on a tile grid, click to select, click an
    /// empty tile to move the selected one, drag to move directly, and explicit
    /// build / unbuild buttons.
    pub(crate) fn hub_map_visual(&mut self, ui: &mut Ui, rb: &RogueBlock) {
        let built: Vec<bool> = {
            let sf: &SaveFile = self.file.as_ref().unwrap();
            (0..HUB_AREAS.len())
                .map(|i| sf.hub_area_built(rb, HubArea::from_raw(i as u8)))
                .collect()
        };
        let coords: Vec<(i8, i8)> = {
            let sf: &SaveFile = self.file.as_ref().unwrap();
            (0..HUB_AREAS.len())
                .map(|i| {
                    sf.hub_area_coord(rb, HubArea::from_raw(i as u8))
                        .unwrap_or((0, 0))
                })
                .collect()
        };

        let mut sel = ui
            .ctx()
            .data(|d| d.get_temp::<MapSel>(egui::Id::new("hub_map_sel")))
            .unwrap_or_default();
        let mut coord_edits: Vec<(usize, i8, i8)> = Vec::new();
        let mut built_edits: Vec<(usize, bool)> = Vec::new();

        let cols = VIEW_MAX - VIEW_MIN + 1;
        let rows = VIEW_MAX - VIEW_MIN + 1;
        // The map never asks for sideways room: the cell size adapts to the panel
        // width, so the whole grid is visible at any window width.
        let cell = CELL.min(ui.available_width() / cols as f32);
        let (_grid_rect, resp) = ui.allocate_exact_size(
            Vec2::new(cols as f32 * cell, rows as f32 * cell),
            egui::Sense::click_and_drag(),
        );
        let origin = resp.rect.min;
        // The grid's own rect is published (not recomputed by callers) so the click
        // mapping the user gets and the one a test drives are the same numbers.
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("hub_map_rect"), resp.rect));
        let painter = ui.painter();

        // Grid.
        for c in 0..=cols {
            let x = origin.x + c as f32 * cell;
            painter.line_segment(
                [
                    Pos2::new(x, origin.y),
                    Pos2::new(x, origin.y + rows as f32 * cell),
                ],
                egui::Stroke::new(0.6_f32, Color32::from_gray(55)),
            );
        }
        for r in 0..=rows {
            let y = origin.y + r as f32 * cell;
            painter.line_segment(
                [
                    Pos2::new(origin.x, y),
                    Pos2::new(origin.x + cols as f32 * cell, y),
                ],
                egui::Stroke::new(0.6_f32, Color32::from_gray(55)),
            );
        }
        // The (0,0) tile is the game's anchor; mark it so a move is not guessed blind.
        {
            let r0 = tile_rect(origin, cell, VIEW_MIN, VIEW_MIN, 0, 0);
            painter.rect_stroke(
                r0.shrink(1.5),
                0.0,
                egui::Stroke::new(1.2_f32, Color32::from_gray(130)),
            );
        }

        // Buildings (built areas only: an unbuilt area's coordinates are inert, so
        // drawing it as a building would misrepresent the map the player sees).
        for i in 0..HUB_AREAS.len() {
            if !built.get(i).copied().unwrap_or(false) {
                continue;
            }
            let (x, y) = coords.get(i).copied().unwrap_or((0, 0));
            let r = tile_rect(origin, cell, VIEW_MIN, VIEW_MIN, x as i32, y as i32);
            painter.rect_filled(r.shrink(2.0), 3.0, area_color(i).linear_multiply(0.75));
            let selected = sel.area == Some(i);
            if selected {
                painter.rect_stroke(
                    r.shrink(1.0),
                    3.0,
                    egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 210, 90)),
                );
            }
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                initials(HUB_AREAS[i]),
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
        }

        // Click: select a building, or move the selected one to the clicked tile.
        let click_tile = if resp.clicked() {
            resp.interact_pointer_pos()
                .and_then(|p| tile_at(p, origin, cell, VIEW_MIN, VIEW_MIN, cols, rows))
        } else {
            None
        };
        if let Some(p) = click_tile {
            let hit = (0..HUB_AREAS.len()).find(|i| {
                built.get(*i).copied().unwrap_or(false)
                    && coords
                        .get(*i)
                        .copied()
                        .map(|(x, y)| (x as i32, y as i32) == p)
                        == Some(true)
            });
            match (hit, sel.area) {
                (Some(i), _) => sel.area = Some(i),
                (None, Some(mv))
                    if coords.get(mv).copied().map(|(x, y)| (x as i32, y as i32)) != Some(p) =>
                {
                    coord_edits.push((mv, p.0 as i8, p.1 as i8));
                }
                (None, _) => {}
            }
        }

        // Drag: start on a building, drop on any tile.
        if resp.drag_started() {
            if let Some(p) = resp
                .interact_pointer_pos()
                .and_then(|p| tile_at(p, origin, cell, VIEW_MIN, VIEW_MIN, cols, rows))
            {
                if let Some(i) = (0..HUB_AREAS.len()).find(|i| {
                    built.get(*i).copied().unwrap_or(false)
                        && coords
                            .get(*i)
                            .copied()
                            .map(|(x, y)| (x as i32, y as i32) == p)
                            == Some(true)
                }) {
                    sel.area = Some(i);
                }
            }
        }
        if resp.drag_stopped() {
            if let (Some(mv), Some(p)) = (
                sel.area,
                resp.interact_pointer_pos()
                    .and_then(|p| tile_at(p, origin, cell, VIEW_MIN, VIEW_MIN, cols, rows)),
            ) {
                if coords.get(mv).copied().map(|(x, y)| (x as i32, y as i32)) != Some(p) {
                    coord_edits.push((mv, p.0 as i8, p.1 as i8));
                }
            }
        }

        // Hover tooltip: what is under the pointer, and what a click there would do.
        if let Some(hp) = resp
            .hover_pos()
            .and_then(|p| tile_at(p, origin, cell, VIEW_MIN, VIEW_MIN, cols, rows))
        {
            let what = (0..HUB_AREAS.len()).find(|i| {
                built.get(*i).copied().unwrap_or(false)
                    && coords
                        .get(*i)
                        .copied()
                        .map(|(x, y)| (x as i32, y as i32) == hp)
                        == Some(true)
            });
            let text = match (what, sel.area) {
                (Some(i), _) => format!(
                    "{} - built at ({},{}). Click to select it for moving.",
                    HUB_AREAS[i], hp.0, hp.1
                ),
                (None, Some(mv)) => format!(
                    "empty tile ({},{}). Click moves {} here.",
                    hp.0, hp.1, HUB_AREAS[mv]
                ),
                (None, None) => format!("empty tile ({},{}). Select a building first.", hp.0, hp.1),
            };
            // `on_hover_text` consumes the Response, so this block runs last.
            resp.on_hover_text(text);
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(
                    "click a building to select it, click an empty tile to move it there (or drag it); (0,0) is the game's anchor tile",
                )
                .small()
                .weak(),
            );
        });

        // Selected-area controls: build/unbuild are deliberately separate from moving.
        if let Some(i) = sel.area {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.strong(HUB_AREAS[i]);
                let (x, y) = coords.get(i).copied().unwrap_or((0, 0));
                ui.label(format!("at ({x},{y})"));
                let on = built.get(i).copied().unwrap_or(false);
                if on {
                    if ui.button("unbuild (clears the built flag only)").clicked() {
                        built_edits.push((i, false));
                    }
                } else {
                    if ui
                        .button("build here (sets the built flag; keeps these coords)")
                        .clicked()
                    {
                        built_edits.push((i, true));
                    }
                    ui.label(
                        egui::RichText::new(
                            "unbuilt: these coordinates are stored but the game does not place it",
                        )
                        .small()
                        .color(Color32::from_rgb(220, 150, 60)),
                    );
                }
            });
        }

        // Warnings for the pending moves, before the map is committed.
        if !coord_edits.is_empty() {
            let sf: &SaveFile = self.file.as_ref().unwrap();
            for &(i, x, y) in &coord_edits {
                for w in sf.hub_coord_warnings(rb, HubArea::from_raw(i as u8), x, y) {
                    ui.label(
                        egui::RichText::new(format!("moving {} to ({x},{y}): {w}", HUB_AREAS[i]))
                            .color(Color32::from_rgb(220, 150, 60)),
                    );
                }
            }
        }

        if !coord_edits.is_empty() || !built_edits.is_empty() {
            if let Some(sf) = self.file.as_mut() {
                for (i, x, y) in coord_edits {
                    sf.set_hub_area_coord(HubArea::from_raw(i as u8), x, y);
                }
                for (i, on) in built_edits {
                    sf.set_hub_area(HubArea::from_raw(i as u8), on);
                }
            }
            self.dirty = true;
        }

        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("hub_map_sel"), sel));
    }

    /// The player's house: outdoor and indoor tile grids with the placed props
    /// drawn on them. Click an empty tile to drop the chosen prop there, click a
    /// prop to select it, and use the buttons to change or clear the selected prop.
    pub(crate) fn house_visual(&mut self, ui: &mut Ui, rb: &RogueBlock) {
        let snap: Vec<(u8, u8, crate::domain::DecorVariant, bool)> = {
            let sf: &SaveFile = self.file.as_ref().unwrap();
            (0..rb.decor_f.n)
                .map(|k| {
                    sf.hub_decoration(rb, k).unwrap_or((
                        0,
                        0,
                        crate::domain::DecorVariant::from_raw(0),
                        false,
                    ))
                })
                .collect()
        };
        let slots = snap.len();
        let outside_end = HOME_DECOR_OUTSIDE.min(slots);

        // The coordinate space comes from the game's own region table
        // (sHomeRegionCoords, src/rogue_hub.c:123-128): the garden accepts props on x/y
        // 4..31, the interior on 1..10, and the house footprint itself is x 15..19 / y
        // 14..19. The viewport here used to be a guessed 24x24, which pushed a prop at
        // (25,15) - the statue in a real save - outside the drawn grid and into the
        // margin, where it read as a stray floating icon that could not even be hovered
        // for its name.
        struct Region {
            x0: i32,
            y0: i32,
            x1: i32,
            y1: i32,
        }
        const GARDEN: Region = Region {
            x0: 4,
            y0: 4,
            x1: 31,
            y1: 31,
        };
        const INTERIOR: Region = Region {
            x0: 1,
            y0: 1,
            x1: 10,
            y1: 10,
        };
        const HOUSE: Region = Region {
            x0: 15,
            y0: 14,
            x1: 19,
            y1: 19,
        };

        let mut sel = ui
            .ctx()
            .data(|d| d.get_temp::<DecorSel>(egui::Id::new("house_decor_sel")))
            .unwrap_or_default();
        let variant_table = decor_variant_table();
        let mut mut_edits: Vec<(usize, u8, u8, DecorVariant, bool)> = Vec::new();

        ui.label(
            egui::RichText::new(
                "props are stored in two ranges: outdoor (garden) then indoor (rooms). \
                 Click a placed prop to select it. Clicking empty ground only marks the \
                 target tile - nothing is written until you press \"place prop here\".",
            )
            .small()
            .weak(),
        );
        let views: [(&'static str, std::ops::Range<usize>, f32, i32, Region); 2] = [
            (
                "Outdoor",
                0usize..outside_end,
                16.0_f32,
                GARDEN.x1 + 1,
                GARDEN,
            ),
            (
                "Indoor",
                outside_end..slots,
                16.0_f32,
                INTERIOR.x1 + 1,
                INTERIOR,
            ),
        ];
        ui.horizontal_wrapped(|ui| {
            ui.label("prop to place");
            if let Some(v) = self.combo(
                ui,
                "house_decor_variant",
                variant_table,
                sel.variant.raw() as u16,
                &crate::searchable_combo::Opts::named().no_id().compact(),
            ) {
                sel.variant = DecorVariant::from_raw(v as u8);
            }
            // Placement is deliberately two-step: pick the prop, mark the tile, then
            // commit. A single click that wrote a prop put furniture down every time the
            // user was only trying to find out where a slot was.
            let armed = sel.pending_tile.map(|(px, py, _)| (px, py));
            let place = ui.add_enabled(armed.is_some(), egui::Button::new("place prop here"));
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("house_place_button"), place.rect));
            if place.clicked() {
                let Some((px, py, which)) = sel.pending_tile else {
                    unreachable!("the button is disabled with no armed tile")
                };
                let (label, range, _, _, _) = &views[which as usize];
                match range.clone().find(|i| !snap[*i].3) {
                    Some(i) => {
                        mut_edits.push((i, px as u8, py as u8, sel.variant, true));
                        sel.slot = Some(i);
                        sel.pending_tile = None;
                        self.status = format!(
                            "placed {} at ({px},{py}) in {label} slot {i} (unsaved)",
                            prop_name(variant_table, sel.variant)
                        );
                    }
                    None => {
                        self.status =
                            format!("no free {label} decoration slot (all {} used)", range.len());
                    }
                }
            }
            if ui.button("clear selected slot").clicked() {
                if let Some(i) = sel.slot {
                    let (x, y, v, _) = snap.get(i).copied().unwrap_or((
                        0,
                        0,
                        crate::domain::DecorVariant::from_raw(0),
                        false,
                    ));
                    mut_edits.push((i, x, y, v, false));
                }
            }
        });

        for (which, (label, range, cell, span, place)) in views.into_iter().enumerate() {
            if range.is_empty() {
                continue;
            }
            ui.add_space(6.0);
            ui.strong(format!(
                "{label} - slots {}..{}, {} placed, placeable x {}..{} y {}..{}",
                range.start,
                range.end,
                snap[range.clone()].iter().filter(|(_, _, _, a)| *a).count(),
                place.x0,
                place.x1,
                place.y0,
                place.y1
            ));
            // Cells shrink to the width on offer rather than pushing the page sideways: the
            // body area scrolls vertically only.
            let cell = cell.min(ui.available_width() / span as f32);
            let (_grid_rect, resp) = ui.allocate_exact_size(
                Vec2::new(span as f32 * cell, span as f32 * cell),
                egui::Sense::click(),
            );
            let origin = resp.rect.min;
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new(format!("house_grid_rect_{label}")), resp.rect)
            });
            let painter = ui.painter();
            for c in 0..=span {
                let x = origin.x + c as f32 * cell;
                painter.line_segment(
                    [
                        Pos2::new(x, origin.y),
                        Pos2::new(x, origin.y + span as f32 * cell),
                    ],
                    egui::Stroke::new(0.4_f32, Color32::from_gray(50)),
                );
                let y = origin.y + c as f32 * cell;
                painter.line_segment(
                    [
                        Pos2::new(origin.x, y),
                        Pos2::new(origin.x + span as f32 * cell, y),
                    ],
                    egui::Stroke::new(0.4_f32, Color32::from_gray(50)),
                );
            }
            // The band the game marks placeable, plus the house footprint it reserves: an
            // anonymous field makes a prop next to the wall impossible to reason about, and
            // the band is what tells you a prop sits off-map.
            let band_rect = tile_rect(origin, cell, 0, 0, place.x0, place.y0)
                .union(tile_rect(origin, cell, 0, 0, place.x1, place.y1));
            painter.rect_stroke(
                band_rect,
                0.0,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(90, 150, 210)),
            );
            if which == 0 {
                let foot = tile_rect(origin, cell, 0, 0, HOUSE.x0, HOUSE.y0)
                    .union(tile_rect(origin, cell, 0, 0, HOUSE.x1, HOUSE.y1));
                painter.rect_filled(foot, 0.0, Color32::from_gray(40));
                painter.rect_stroke(
                    foot,
                    0.0,
                    egui::Stroke::new(1.0_f32, Color32::from_gray(130)),
                );
                painter.text(
                    foot.center(),
                    egui::Align2::CENTER_CENTER,
                    "house",
                    egui::FontId::proportional(10.0_f32),
                    Color32::from_gray(130),
                );
            }
            // Placed props in this range. `on_hover_text` consumes the Response, so the
            // label is chosen here and attached once, after the grid.
            let mut hover: Option<String> = None;
            for i in range.clone() {
                let (x, y, v, act) = snap[i];
                if !act {
                    continue;
                }
                let r = tile_rect(origin, cell, 0, 0, x as i32, y as i32);
                // Placeholder, not game art: a flat blue square read as "a floating blue icon"
                // with no clue which prop it is. Hue comes from the variant id, and the name's
                // initials go on the tile itself, so the grid is legible before you hover.
                let name = prop_name(variant_table, v);
                let base = crate::icons::hue_for(v.raw() as u16);
                painter.rect_filled(r.shrink(1.0), 2.0, base);
                let initials: String = name
                    .split(|c: char| !c.is_alphanumeric())
                    .filter_map(|w| w.chars().next())
                    .take(2)
                    .flat_map(|c| c.to_uppercase())
                    .collect();
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    &initials,
                    egui::FontId::proportional((cell * 0.45).min(11.0)),
                    Color32::from_gray(15),
                );
                if sel.slot == Some(i) {
                    painter.rect_stroke(
                        r.shrink(0.5),
                        2.0,
                        egui::Stroke::new(1.6_f32, Color32::from_rgb(255, 210, 90)),
                    );
                }
                if resp.hover_pos().map_or(false, |p| r.contains(p)) {
                    hover = Some(format!(
                        "slot {i}: {} at ({x},{y})",
                        variant_table
                            .iter()
                            .find(|(id, _)| *id as u8 == v.raw())
                            .map(|(_, n)| *n)
                            .unwrap_or("(unknown prop)")
                    ));
                }
            }
            let click_tile = if resp.clicked() {
                resp.interact_pointer_pos()
                    .and_then(|p| tile_at(p, origin, cell, 0, 0, span, span))
            } else {
                None
            };
            let _hovered = if let Some(t) = hover {
                resp.on_hover_text(t)
            } else {
                resp.clone()
            };
            let _ = _hovered;
            if let Some(p) = click_tile {
                let hit = range
                    .clone()
                    .find(|i| snap[*i].3 && (snap[*i].0 as i32, snap[*i].1 as i32) == p);
                match hit {
                    Some(i) => {
                        sel.slot = Some(i);
                        sel.pending_tile = None;
                    }
                    // Marking a target is not an edit: no slot is touched and `dirty` stays false
                    // until the place button commits it.
                    None => {
                        sel.pending_tile = Some((p.0, p.1, which as u8));
                        // A warning, not a veto: the region table is where the game blits placeable
                        // tiles, which is not proof that it rejects a prop outside that band, so
                        // the user is told and left to choose.
                        if !(place.x0..=place.x1).contains(&p.0)
                            || !(place.y0..=place.y1).contains(&p.1)
                        {
                            self.status = format!(
                                "{label} ({},{}): outside the region the game marks \
                                 placeable (x {}..{}, y {}..{}) - it may not appear in-game",
                                p.0, p.1, place.x0, place.x1, place.y0, place.y1
                            );
                        }
                    }
                }
            }
            // Show the armed tile so the two-step flow has visible state.
            if let Some((px, py, w)) = sel.pending_tile {
                if w as usize == which {
                    let r = tile_rect(origin, cell, 0, 0, px, py);
                    painter.rect_stroke(
                        r.shrink(1.0),
                        2.0,
                        egui::Stroke::new(1.4_f32, Color32::from_rgb(120, 220, 140)),
                    );
                }
            }
        }

        if !mut_edits.is_empty() {
            if let Some(sf) = self.file.as_mut() {
                for (i, x, y, v, a) in mut_edits {
                    sf.set_hub_decoration(i, x, y, v, a);
                }
            }
            self.dirty = true;
        }
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("house_decor_sel"), sel));
    }
}

/// Display name for a stored variant id, for status messages.
fn prop_name(table: &[(u16, &'static str)], v: crate::domain::DecorVariant) -> &'static str {
    table
        .iter()
        .find(|(id, _)| *id as u8 == v.raw())
        .map(|(_, n)| *n)
        .unwrap_or("(unknown prop)")
}

#[derive(Clone, Copy, Default)]
struct DecorSel {
    slot: Option<usize>,
    variant: DecorVariant,
    /// Tile the user has marked for the next placement (x, y, view).
    pending_tile: Option<(i32, i32, u8)>,
}

/// `(id, name)` table for the searchable prop picker. `decorVariant` indexes
/// `sDecorationVariants[]`, so the enum order IS the id; the game's own
/// "remove" code (id `DECOR_VARIANT_REMOVE`) is a menu return value, never a
/// stored variant, so it is not offered.
fn decor_variant_table() -> &'static [(u16, &'static str)] {
    static TABLE: std::sync::OnceLock<Vec<(u16, &'static str)>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        crate::deco_variants::DECOR_VARIANTS
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != crate::deco_variants::DECOR_VARIANT_REMOVE)
            .map(|(i, n)| (i as u16, *n))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_mapping_is_exact_at_every_corner() {
        let o = Pos2::new(100.0, 50.0);
        // VIEW_MIN..VIEW_MAX grid, cell 30.
        let cols = VIEW_MAX - VIEW_MIN + 1;
        let rows = VIEW_MAX - VIEW_MIN + 1;
        assert_eq!(
            tile_at(o, o, CELL, VIEW_MIN, VIEW_MIN, cols, rows),
            Some((VIEW_MIN, VIEW_MIN))
        );
        // Inside the first cell.
        assert_eq!(
            tile_at(
                Pos2::new(o.x + 29.0, o.y + 29.0),
                o,
                CELL,
                VIEW_MIN,
                VIEW_MIN,
                cols,
                rows
            ),
            Some((VIEW_MIN, VIEW_MIN))
        );
        // Next cell over.
        assert_eq!(
            tile_at(
                Pos2::new(o.x + 30.0, o.y + 60.0),
                o,
                CELL,
                VIEW_MIN,
                VIEW_MIN,
                cols,
                rows
            ),
            Some((VIEW_MIN + 1, VIEW_MIN + 2))
        );
        // Just outside the grid: None, both sides.
        assert_eq!(
            tile_at(
                Pos2::new(o.x - 0.5, o.y),
                o,
                CELL,
                VIEW_MIN,
                VIEW_MIN,
                cols,
                rows
            ),
            None
        );
        assert_eq!(
            tile_at(
                Pos2::new(o.x + cols as f32 * CELL, o.y),
                o,
                CELL,
                VIEW_MIN,
                VIEW_MIN,
                cols,
                rows
            ),
            None
        );
        // A negative tile coordinate must survive the round trip.
        let (tx, ty) = tile_at(o, o, CELL, -8, -8, 18, 18).unwrap();
        assert_eq!((tx, ty), (-8, -8));
    }

    #[test]
    fn initials_stay_short_and_readable() {
        assert_eq!(initials("Town Square"), "TS");
        assert_eq!(initials("Adventure Entrance"), "AE");
        assert_eq!(initials("Home"), "H");
        assert_eq!(initials("Challenge Frontier").len(), 2);
    }

    #[test]
    fn prop_table_excludes_the_remove_action_code() {
        let t = decor_variant_table();
        assert!(t.len() >= 100, "the game has 170+ prop variants");
        assert!(
            !t.iter()
                .any(|(id, _)| *id as usize == crate::deco_variants::DECOR_VARIANT_REMOVE),
            "the in-game 'remove' code is a menu result, never a stored variant"
        );
        // The wardrobe the game seeds in a fresh save must be pickable by name.
        assert!(t.iter().any(|(id, _)| *id == 119));
    }
}

#[cfg(test)]
mod functional {
    // ---- functional tests: real egui frames, synthetic clicks -----------------
    //
    // The point of these is the claim the panel makes: that clicking a tile does
    // what the panel says it does. Compiling proves nothing about the click
    // mapping, so the grid publishes its own rect and the test clicks the centre of
    // a tile the same way the widget computes it.

    use super::*;
    use egui::{Event, Id, Modifiers, PointerButton, RawInput};

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

    /// A live save, or None when this machine has none (CI never does).
    fn save_path() -> Option<String> {
        let p =
            std::env::var("ER_TEST_SAVE").unwrap_or_else(|_| "/workspace/hermes/er2.sav".into());
        if std::path::Path::new(&p).exists() {
            Some(p)
        } else {
            eprintln!("hub_visual functional tests: no save at {p}, skipping");
            None
        }
    }

    struct Panel {
        app: super::super::Editor,
        ctx: egui::Context,
        warmed: bool,
    }

    impl Panel {
        fn open(path: &str) -> Panel {
            let mut app = super::super::Editor::default();
            app.open_path(path.to_string());
            assert!(app.file.is_some(), "the test save must parse");
            Panel {
                app,
                ctx: egui::Context::default(),
                warmed: false,
            }
        }

        fn run(&mut self, which: &str, events: Vec<Event>) {
            if !self.warmed {
                self.warmed = true;
                self.run(which, vec![]);
            }
            let rb = match self.app.file.as_ref().and_then(|sf| sf.rogue_block()) {
                Some(rb) => rb,
                None => return,
            };
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_max(
                    Pos2::ZERO,
                    Pos2::new(1500.0, 1200.0),
                )),
                focused: true,
                events,
                ..Default::default()
            };
            let _ = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| match which {
                    "hub" => self.app.hub_map_visual(ui, &rb),
                    _ => self.app.house_visual(ui, &rb),
                });
            });
        }

        fn hub(&mut self, events: Vec<Event>) {
            self.run("hub", events);
        }

        fn house(&mut self, events: Vec<Event>) {
            self.run("house", events);
        }

        fn rect(&self, key: &str) -> egui::Rect {
            self.ctx
                .data(|d| d.get_temp::<egui::Rect>(Id::new(key.to_string())))
                .expect("the panel publishes its grid rect")
        }

        fn click(&mut self, which: &str, pos: Pos2) {
            self.run(which, move_to(pos));
            self.run(which, press(pos));
            self.run(which, release(pos));
        }
    }

    fn coord(app: &super::super::Editor, area: usize) -> (i8, i8) {
        let sf = app.file.as_ref().unwrap();
        let rb = sf.rogue_block().unwrap();
        sf.hub_area_coord(&rb, crate::domain::HubArea::from_raw(area as u8))
            .unwrap_or((0, 0))
    }

    fn built(app: &super::super::Editor, area: usize) -> bool {
        let sf = app.file.as_ref().unwrap();
        let rb = sf.rogue_block().unwrap();
        sf.hub_area_built(&rb, crate::domain::HubArea::from_raw(area as u8))
    }

    /// A tile inside the view that holds no built building.
    fn empty_tile(app: &super::super::Editor, tx: i32, ty: i32) -> (i32, i32) {
        for dy in 0..12 {
            for dx in 0..12 {
                let (x, y) = (tx + dx, ty + dy);
                if !(VIEW_MIN..=VIEW_MAX).contains(&x) || !(VIEW_MIN..=VIEW_MAX).contains(&y) {
                    continue;
                }
                let taken = (0..HUB_AREAS.len())
                    .any(|i| built(app, i) && coord(app, i) == (x as i8, y as i8));
                if !taken {
                    return (x, y);
                }
            }
        }
        panic!("no empty tile in the view");
    }

    #[test]
    fn clicking_a_building_selects_it_and_clicking_a_tile_moves_it_there() {
        let Some(path) = save_path() else { return };
        let disk_before = std::fs::read(&path).unwrap();
        let mut p = Panel::open(&path);
        p.hub(vec![]);
        let rect = p.rect("hub_map_rect");
        let centre = |tx: i32, ty: i32| {
            rect.min
                + Vec2::new(
                    ((tx - VIEW_MIN) as f32 + 0.5) * CELL,
                    ((ty - VIEW_MIN) as f32 + 0.5) * CELL,
                )
        };

        // A built area is clickable and becomes the "move this one" target.
        let area = (0..HUB_AREAS.len())
            .find(|i| built(&p.app, *i))
            .expect("this save has built hub areas");
        let from = coord(&p.app, area);
        p.click("hub", centre(from.0 as i32, from.1 as i32));
        let sel: MapSel = p
            .ctx
            .data(|d| d.get_temp::<MapSel>(Id::new("hub_map_sel")))
            .unwrap_or_default();
        assert_eq!(
            sel.area,
            Some(area),
            "clicking the tile selects the building"
        );

        // Clicking an empty tile then moves that area onto it.
        let target = empty_tile(&p.app, VIEW_MIN + 1, VIEW_MAX - 4);
        p.click("hub", centre(target.0, target.1));
        assert_eq!(
            coord(&p.app, area),
            (target.0 as i8, target.1 as i8),
            "the click must move the selected building to the clicked tile"
        );
        assert!(
            built(&p.app, area),
            "moving never touches the built flag (independent bytes on disk)"
        );
        // And nothing was written to disk: editing in memory is not consent.
        assert_eq!(
            disk_before,
            std::fs::read(&path).unwrap(),
            "the editor must not write the save unprompted"
        );
    }

    #[test]
    fn a_marked_tile_writes_nothing_until_the_place_button_is_pressed() {
        let Some(path) = save_path() else { return };
        let disk_before = std::fs::read(&path).unwrap();
        let mut p = Panel::open(&path);
        p.house(vec![]);
        let rect = p.rect("house_grid_rect_Outdoor");
        let btn = p.rect("house_place_button");

        let at = |app: &super::super::Editor, tx: i32, ty: i32| {
            let sf = app.file.as_ref().unwrap();
            let rb = sf.rogue_block().unwrap();
            (0..rb.decor_f.n).any(|i| {
                sf.hub_decoration(&rb, i)
                    .map(|(x, y, _, act)| act && (x as i32, y as i32) == (tx, ty))
                    .unwrap_or(false)
            })
        };
        let before = count(&p.app);

        let (tx, ty) = (3, 7);
        let cell = 16.0f32;
        let tile = rect.min + Vec2::new((tx as f32 + 0.5) * cell, (ty as f32 + 0.5) * cell);
        p.click("house", tile);

        // Step one marks the tile. It must not touch the save, not even in memory.
        assert!(!at(&p.app, tx, ty), "a bare click must not place a prop");
        assert_eq!(count(&p.app), before, "no prop added by a bare click");
        assert!(!p.app.dirty, "marking a target tile is not an edit");

        // Step two commits it.
        p.click("house", btn.center());
        assert!(at(&p.app, tx, ty), "the place button writes the prop");
        assert_eq!(count(&p.app), before + 1, "exactly one prop was placed");
        assert!(p.app.dirty, "a real edit marks the save dirty");

        assert_eq!(
            disk_before,
            std::fs::read(&path).unwrap(),
            "the editor must not write the save unprompted"
        );
    }

    #[test]
    fn the_place_button_is_disabled_until_a_tile_is_marked() {
        let Some(path) = save_path() else { return };
        let mut p = Panel::open(&path);
        p.house(vec![]);
        let btn = p.rect("house_place_button");
        // A disabled button swallows clicks: nothing may change.
        let before = count(&p.app);
        p.click("house", btn.center());
        assert_eq!(
            before,
            count(&p.app),
            "with no tile marked, the place button must change nothing"
        );
    }

    #[test]
    fn the_garden_grid_reaches_every_tile_the_game_can_place_a_prop_on() {
        let Some(path) = save_path() else { return };
        let mut p = Panel::open(&path);
        p.house(vec![]);
        let rect = p.rect("house_grid_rect_Outdoor");
        let cell = 16.0f32;
        // sHomeRegionCoords puts the garden band at 4..31, and a real save keeps its
        // statue at (25,15). Under the old guessed 24-tile viewport that tile fell
        // outside the grid, into the margin, where it read as a stray floating icon.
        assert!(
            rect.width() >= 32.0 * cell && rect.height() >= 32.0 * cell,
            "the outdoor grid must cover tiles 0..31, not 0..23 (got {} x {})",
            rect.width(),
            rect.height()
        );
        let statue_tile = rect.min + Vec2::new(25.5 * cell, 15.5 * cell);
        assert!(
            rect.contains(statue_tile),
            "the tile (25,15) must sit inside the drawn grid so it can be clicked/hovered"
        );
    }

    fn count(app: &super::super::Editor) -> usize {
        let sf = app.file.as_ref().unwrap();
        let rb = sf.rogue_block().unwrap();
        (0..rb.decor_f.n)
            .filter(|i| sf.hub_decoration(&rb, *i).map(|d| d.3).unwrap_or(false))
            .count()
    }
}
