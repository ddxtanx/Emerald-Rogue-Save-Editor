#!/usr/bin/env python3
"""House editor: an empty tile no longer places a prop on click.

User rule: "it should not place anything unless i have selected the decoration i
want to place and clicked a set button." So a click on empty ground only ARMS a
target tile (drawn as an outline); the write happens on `place prop here`.
Clicking a placed prop still just selects it.
"""
import pathlib

p = pathlib.Path("src/app/hub_visual.rs")
s = p.read_text()

old_help = '''        ui.label(
            egui::RichText::new(
                "props are stored in two ranges: outdoor (garden) then indoor (rooms). Click a tile to place the chosen prop there; click a placed prop to select it.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
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
            if ui.button("clear selected slot").clicked() {'''
new_help = '''        ui.label(
            egui::RichText::new(
                "props are stored in two ranges: outdoor (garden) then indoor (rooms). \\
                 Click a placed prop to select it. Clicking empty ground only marks the \\
                 target tile - nothing is written until you press \\"place prop here\\".",
            )
            .small()
            .weak(),
        );
        let views: [(&'static str, std::ops::Range<usize>, f32, i32); 2] = [
            ("Outdoor", 0usize..outside_end, 16.0_f32, 24),
            ("Indoor", outside_end..slots, 16.0_f32, 24),
        ];
        ui.horizontal(|ui| {
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
            // commit. A single click that wrote a prop put furniture down every time
            // the user was only trying to find out where a slot was.
            let armed = sel.pending_tile.map(|(px, py, _)| (px, py));
            let place = ui.add_enabled(
                armed.is_some(),
                egui::Button::new(
                    "place prop here",
                ),
            );
            if place.clicked() {
                let Some((px, py, which)) = sel.pending_tile else {
                    unreachable!("the button is disabled with no armed tile")
                };
                let (label, range, _, _) = &views[which as usize];
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
                        self.status = format!(
                            "no free {label} decoration slot (all {} used)",
                            range.len()
                        );
                    }
                }
            }
            if ui.button("clear selected slot").clicked() {'''
assert old_help in s
s = s.replace(old_help, new_help, 1)

old_loop = '''        for (label, range, cell, span) in [
            ("Outdoor", 0usize..outside_end, 16.0f32, 24i32),
            ("Indoor", outside_end..slots, 16.0f32, 24i32),
        ] {'''
new_loop = '''        for (which, (label, range, cell, span)) in views.into_iter().enumerate() {'''
assert old_loop in s
s = s.replace(old_loop, new_loop, 1)

old_click = '''            if let Some(p) = click_tile {
                let hit = range
                    .clone()
                    .find(|i| snap[*i].3 && (snap[*i].0 as i32, snap[*i].1 as i32) == p);
                match hit {
                    Some(i) => sel.slot = Some(i),
                    None => {
                        // First free slot in this range takes the placement; the game
                        // itself stores one prop per slot and scans the range.
                        if let Some(i) = range.clone().find(|i| !snap[*i].3) {
                            mut_edits.push((i, p.0 as u8, p.1 as u8, sel.variant, true));
                            sel.slot = Some(i);
                        } else {
                            self.status = format!(
                                "no free {label} decoration slot (all {} used)",
                                range.len()
                            );
                        }
                    }
                }
            }'''
new_click = '''            if let Some(p) = click_tile {
                let hit = range
                    .clone()
                    .find(|i| snap[*i].3 && (snap[*i].0 as i32, snap[*i].1 as i32) == p);
                match hit {
                    Some(i) => {
                        sel.slot = Some(i);
                        sel.pending_tile = None;
                    }
                    // Marking a target is not an edit: no slot is touched and `dirty`
                    // stays false until the place button commits it.
                    None => sel.pending_tile = Some((p.0, p.1, which as u8)),
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
            }'''
assert old_click in s
s = s.replace(old_click, new_click, 1)

old_sel = '''struct DecorSel {'''
assert old_sel in s
# find the struct body and add the pending field
import re

m = re.search(r"struct DecorSel \{[^}]*\}", s)
body = m.group(0)
assert "pending_tile" not in body
new_body = body.rstrip("}") + "    /// Tile the user has marked for the next placement (x, y, view).\n    pending_tile: Option<(i32, i32, u8)>,\n}"
s = s.replace(body, new_body, 1)

# helper for status text
s = s.replace(
    "#[derive(Clone, Copy, Default)]",
    '''/// Display name for a stored variant id, for status messages.
fn prop_name(table: &[(u16, &str)], v: crate::domain::DecorVariant) -> &'static str {
    table
        .iter()
        .find(|(id, _)| *id as u8 == v.raw())
        .map(|(_, n)| *n)
        .unwrap_or("(unknown prop)")
}

#[derive(Clone, Copy, Default)]''',
    1,
)

p.write_text(s)
print("patched hub_visual.rs")
