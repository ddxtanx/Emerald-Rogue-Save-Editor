#!/usr/bin/env python3
"""Publish the place button's rect (test handle, same pattern as the grid rects)
and rewrite the house placement test for the two-step flow."""
import pathlib
import re

p = pathlib.Path("src/app/hub_visual.rs")
s = p.read_text()

# Publish the button rect right after it is created.
old = '''            if place.clicked() {'''
new = '''            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new("house_place_button"), place.rect)
            });
            if place.clicked() {'''
assert old in s
s = s.replace(old, new, 1)

# Rewrite the placement test.
start = s.index('''    #[test]
    fn clicking_an_empty_house_tile_places_a_prop_in_a_free_slot() {''')
end = s.rindex("}\n")  # final closing brace of mod tests
new_test = '''    #[test]
    fn a_marked_tile_writes_nothing_until_the_place_button_is_pressed() {
        let Some(path) = save_path() else { return };
        let disk_before = std::fs::read(&path).unwrap();
        let mut p = Panel::open(&path);
        p.house(vec![]);
        let rect = p.rect("house_grid_rect_Outdoor");
        let btn = p.rect("house_place_button");

        let count = |app: &super::super::Editor| {
            let sf = app.file.as_ref().unwrap();
            let rb = sf.rogue_block().unwrap();
            (0..rb.decor_f.n)
                .filter(|i| sf.hub_decoration(&rb, *i).map(|d| d.3).unwrap_or(false))
                .count()
        };
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
        let probe = |p: &Panel| {
            // A disabled button swallows clicks: the count must not move.
            let before = {
                let sf = p.app.file.as_ref().unwrap();
                let rb = sf.rogue_block().unwrap();
                (0..rb.decor_f.n)
                    .filter(|i| sf.hub_decoration(&rb, *i).map(|d| d.3).unwrap_or(false))
                    .count()
            };
            p.click("house", btn.center());
            let after = {
                let sf = p.app.file.as_ref().unwrap();
                let rb = sf.rogue_block().unwrap();
                (0..rb.decor_f.n)
                    .filter(|i| sf.hub_decoration(&rb, *i).map(|d| d.3).unwrap_or(false))
                    .count()
            };
            (before, after)
        };
        let (before, after) = probe(&p);
        assert_eq!(
            before, after,
            "with no tile marked, pressing the place button changes nothing"
        );
    }
'''
s = s[:start] + new_test + "}\n"
p.write_text(s)
print("test rewritten")
