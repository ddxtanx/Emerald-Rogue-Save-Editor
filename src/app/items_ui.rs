//! Items tab: bag/PC/stash editing, sort, add row.

use super::*;
use crate::searchable_combo::Opts;

impl Editor {
    pub(crate) fn items_ui(&mut self, ui: &mut egui::Ui) {
        // Read everything out of the save first, then drop the borrow, so the
        // widgets below can mutate `self` (filter / add-row state) freely.
        //
        // Bag header: the bag is one flat array the game splits into contiguous
        // per-pocket runs on load (`UpdateBagItemsPointers`), and it does NOT
        // re-sort after loading — so quantity edits go through
        // set_bag_slot_qty (re-compacts, keeps runs intact) and new ids only
        // ever arrive via add_bag_item, which inserts in sorted position.
        let (bag_items, pc_items, cap, mut cap_up, warnings, sort_mode) = match &self.file {
            Some(sf) => (
                sf.bag_items(),
                sf.pc_items(),
                sf.bag_capacity(),
                sf.bag_capacity_upgrades(),
                sf.bag_warnings(false),
                sf.bag_sort_mode(),
            ),
            None => return,
        };
        let used = bag_items.iter().take_while(|(id, _)| *id != 0).count();
        ui.checkbox(&mut self.hide_empty_items, "hide empty slots");
        let mut edits: Vec<(bool, usize, u16, u16)> = Vec::new();

        // The saved sort mode decides how the GAME orders items inside a pocket
        // run; the editor's own sort button uses "type" order. Editable, because
        // the game's own sort menu writes this byte (item_menu.c:3247).
        let mut sort_changed = false;
        let mut sort_mode = sort_mode as usize;
        let (cap_bumped, do_sort) = ui
            .horizontal(|ui| {
                ui.label(format!("bag {used}/{cap} slots"));
                egui::ComboBox::from_id_source("bag_sort_mode")
                    .selected_text(format!(
                        "sort mode: {}",
                        crate::format::BAG_SORT_MODES
                            .get(sort_mode)
                            .copied()
                            .unwrap_or("unknown")
                    ))
                    .show_ui(ui, |ui| {
                        for (i, name) in crate::format::BAG_SORT_MODES.iter().enumerate() {
                            if ui.selectable_label(sort_mode == i, *name).clicked() {
                                sort_mode = i;
                                sort_changed = true;
                                ui.close_menu();
                            }
                        }
                    });
                let r = ui.add(
                    egui::DragValue::new(&mut cap_up)
                        .clamp_range(0..=37)
                        .suffix(" upgrades"),
                );
                let sort = ui
                    .button("sort bag (pocket, evo-first, id)")
                    .on_hover_text(
                        "the game's canonical bag order: pocket runs, evolution items \
                         grouped first inside each run, then ascending id",
                    )
                    .clicked();
                (r.changed(), sort)
            })
            .inner;
        if sort_changed {
            if let Some(sf) = self.file.as_mut() {
                sf.set_bag_sort_mode(sort_mode as u8);
                self.status = format!(
                    "bag sort mode set to {} (unsaved — press Save to write)",
                    crate::format::BAG_SORT_MODES[sort_mode]
                );
            }
            self.dirty = true;
        }
        for w in warnings {
            ui.colored_label(
                egui::Color32::from_rgb(210, 130, 40),
                format!("warning: {w}"),
            );
        }

        // Add / remove row: one searchable item picker, one quantity, and the
        // same stash toggle for both directions.
        let mut do_add = false;
        let mut do_remove = false;
        ui.horizontal(|ui| {
            ui.label("add to bag:");
            if let Some(id) = self.combo(
                ui,
                "bag_add_item",
                all_items(),
                self.bag_add_id,
                &Opts::item(),
            ) {
                self.bag_add_id = id;
            }
            ui.add(
                egui::DragValue::new(&mut self.bag_add_qty)
                    .clamp_range(1..=999)
                    .prefix("x"),
            );
            ui.checkbox(&mut self.bag_add_stash, "stashed (hub) bag");
            do_remove = ui.button("Remove").clicked();
            if ui
                .button(format!(
                    "Add {} x{}",
                    item_name(self.bag_add_id),
                    self.bag_add_qty
                ))
                .clicked()
            {
                do_add = true;
            }
        });

        if do_remove {
            let id = self.bag_add_id;
            let qty = self.bag_add_qty;
            let stash = self.bag_add_stash;
            if let Some(sf) = self.file.as_mut() {
                match sf.remove_bag_item(id, qty, stash) {
                    Ok(msg) => {
                        self.status = format!("{msg} (unsaved — press Save to write)");
                        self.dirty = true;
                    }
                    Err(e) => self.status = format!("remove failed: {e}"),
                }
            }
        }

        for (bag, label) in [(true, "Bag"), (false, "PC (x256 box)")] {
            ui.strong(label);
            let items: Vec<(u16, u16)> = if bag {
                bag_items.clone()
            } else {
                pc_items.clone()
            };
            egui::Grid::new(label)
                .num_columns(if bag { 5 } else { 4 })
                .striped(true)
                .show(ui, |ui| {
                    let mut shown = 0;
                    for (i, (id, qty)) in items.into_iter().enumerate() {
                        if id == 0 && self.hide_empty_items {
                            continue;
                        }
                        shown += 1;
                        ui.monospace(format!("{i:3}"));
                        let name = item_name(id);
                        if bag {
                            let p = item_pocket(id);
                            ui.label(pocket_name(p));
                        } else {
                            ui.label("");
                        }
                        ui.label(format!("{id:5} {name}"));
                        let mut q = qty;
                        if ui
                            .add(egui::DragValue::new(&mut q).clamp_range(0..=999))
                            .changed()
                        {
                            edits.push((bag, i, id, q));
                        }
                        if shown % 2 == 0 {
                            ui.end_row();
                        }
                    }
                    if shown % 2 != 0 {
                        ui.end_row();
                    }
                });
        }
        for (bag, i, id, q) in edits {
            if let Some(sf) = self.file.as_mut() {
                if bag {
                    let _ = sf.set_bag_slot_qty(i, q, false);
                } else {
                    sf.set_item(false, i, id, q);
                }
            }
            self.dirty = true;
        }
        if cap_bumped {
            if let Some(sf) = self.file.as_mut() {
                sf.set_bag_capacity_upgrades(cap_up);
            }
            self.dirty = true;
        }
        if do_sort {
            if let Some(sf) = self.file.as_mut() {
                self.status = sf
                    .sort_bag(false)
                    .unwrap_or_else(|e| format!("sort failed: {e}"));
            }
            self.dirty = true;
        }
        if do_add {
            let (id, qty, stash) = (self.bag_add_id, self.bag_add_qty, self.bag_add_stash);
            if let Some(sf) = self.file.as_mut() {
                self.status = match sf.add_bag_item(id, qty, stash) {
                    Ok(m) => m,
                    Err(e) => format!("add refused: {e}"),
                };
            }
            self.dirty = true;
        }
    }
}
