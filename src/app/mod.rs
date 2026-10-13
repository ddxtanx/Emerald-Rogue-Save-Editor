//! The eframe application: layout, tab dispatch, selection model, and file
//! open/save plumbing. Per-tab UIs live in the sibling modules as `impl Editor`
//! blocks — edit those files, not this one, for tab-specific changes.

use crate::basestats::base_stats;
use crate::deco_variants;
use crate::format::*;
use crate::game_data::*;
use crate::mon::*;
use crate::savefile::*;

// per-tab UIs, each an `impl Editor` block in its own file
mod items_ui;
mod mon_ui;
mod rogue_ui;
mod trainer_ui;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Party,
    Boxes,
    Items,
    Trainer,
    Rogue,
}

#[derive(Clone)]
enum Sel {
    Party(usize),
    Box(usize, usize),
    Fusion(usize),       // fusion storage (struct Pokemon fusions[4] at storage+0x8432)
    StashParty(usize),   // hub-state party, stashed in the Rogue block while adventuring
    BlockDaycare(usize), // daycare as the game currently sees it
    StashDaycare(usize), // daycare stashed for the hub state
}

pub(crate) struct Editor {
    file: Option<SaveFile>,
    path: Option<String>,
    sel: Option<Sel>,
    tab: Tab,
    status: String,
    species_filter: String,
    item_filter: String,
    quest_filter: String,
    hide_empty_items: bool,
    show_empty_safari: bool,
    dirty: bool,
    // bag add-row state (persists across frames so the combo/search survives)
    bag_add_id: u16,
    bag_add_qty: u16,
    bag_add_stash: bool,
    // one-line feedback from mon-editor actions that cannot self-correct
    // (e.g. "no base stats in table for this species")
    mon_note: Option<String>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor {
            file: None,
            path: None,
            sel: None,
            tab: Tab::Party,
            status: "Open a .sav file to begin.".into(),
            species_filter: String::new(),
            item_filter: String::new(),
            quest_filter: String::new(),
            hide_empty_items: true,
            show_empty_safari: false,
            bag_add_id: 1,
            bag_add_qty: 1,
            bag_add_stash: false,
            mon_note: None,
            dirty: false,
        }
    }
}

impl Editor {
    pub(crate) fn open_path(&mut self, ps: String) {
        match SaveFile::open(&ps) {
            Ok(sf) => {
                let n = sf.active_slot / 14 + 1;
                self.status = format!("Opened {ps} (active save slot {n})");
                self.sel = Some(Sel::Party(0));
                self.path = Some(ps);
                self.file = Some(sf);
                self.dirty = false;
            }
            Err(e) => self.status = format!("Failed to open: {e}"),
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("GBA save", &["sav", "srm", "gba"])
            .pick_file()
        {
            let ps = p.display().to_string();
            self.open_path(ps);
        }
        let _ = ctx;
    }

    fn do_save(&mut self, save_as: bool) {
        let path = if save_as {
            rfd::FileDialog::new()
                .add_filter("GBA save", &["sav"])
                .save_file()
                .map(|p| p.display().to_string())
        } else {
            self.path.clone()
        };
        let Some(path) = path else { return };
        let Some(sf) = self.file.as_mut() else { return };
        // seal every visible mon back into merged first (edits already flow through set_*)
        match sf.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {path} (checksums recomputed)");
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn selected_mon(&self) -> Option<Mon> {
        let sf = self.file.as_ref()?;
        match self.sel.clone()? {
            Sel::Party(i) if i < sf.party_count() => Some(sf.party()[i].clone()),
            Sel::Box(bx, s) => Some(sf.box_mon(bx, s)),
            Sel::Fusion(f) if f < FUSION_COUNT => Some(sf.fusion_mon(f)),
            Sel::StashParty(i) => {
                let rb = sf.rogue_block()?;
                rb.stash
                    .ok
                    .then(|| rb.stash.party.get(i).cloned())
                    .flatten()
            }
            Sel::BlockDaycare(i) => {
                let rb = sf.rogue_block()?;
                rb.ok.then_some(Mon::from_bytes(
                    &sf.merged[rb.daycare_f.data + i * DAYCARE_ELEMSZ
                        ..rb.daycare_f.data + i * DAYCARE_ELEMSZ + BOXSZ],
                ))
            }
            Sel::StashDaycare(i) => {
                let rb = sf.rogue_block()?;
                rb.stash
                    .ok
                    .then(|| rb.stash.daycare.get(i).cloned())
                    .flatten()
            }
            _ => None,
        }
    }

    fn apply_mon(&mut self, m: Mon) {
        if let Some(sf) = self.file.as_mut() {
            match self.sel {
                Some(Sel::Party(i)) => sf.set_party_mon(i, &m),
                Some(Sel::Box(bx, s)) => sf.set_box_mon(bx, s, &m),
                Some(Sel::Fusion(f)) => sf.set_fusion_mon(f, &m),
                Some(Sel::StashParty(i)) => sf.set_stash_mon(i, &m),
                Some(Sel::BlockDaycare(i)) => sf.set_block_daycare(i, &m),
                Some(Sel::StashDaycare(i)) => sf.set_stash_daycare(i, &m),
                _ => {}
            }
            self.dirty = true;
        }
    }

    fn filtered_combo(
        ui: &mut egui::Ui,
        label: &str,
        filter: &mut String,
        table: &[(u16, &str)],
        current: u16,
        on_change: &mut dyn FnMut(u16),
    ) {
        let cur_name = title_lookup(table, current);
        let resp = egui::ComboBox::from_id_source(label.to_string())
            .selected_text(format!("{current} — {cur_name}"))
            .show_ui(ui, |ui| {
                let mut f = filter.clone();
                ui.horizontal(|ui| {
                    ui.label("search:");
                    if ui.text_edit_singleline(&mut f).changed() {
                        *filter = f.clone();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (id, name) in table {
                            if !f.is_empty() && !name.to_lowercase().contains(&f.to_lowercase()) {
                                continue;
                            }
                            if ui
                                .selectable_label(*id == current, format!("{id:4} {name}"))
                                .clicked()
                            {
                                on_change(*id);
                                ui.close_menu();
                            }
                        }
                    });
            });
        let _ = resp;
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open").clicked() {
                    self.open_dialog(ctx);
                }
                let has = self.file.is_some();
                if ui
                    .add_enabled(has, |ui: &mut egui::Ui| ui.button("Save"))
                    .clicked()
                {
                    self.do_save(false);
                }
                if ui
                    .add_enabled(has, |ui: &mut egui::Ui| ui.button("Save As…"))
                    .clicked()
                {
                    self.do_save(true);
                }
                ui.separator();
                if let Some(p) = &self.path {
                    ui.label(p);
                    if self.dirty {
                        ui.colored_label(egui::Color32::GOLD, "● modified");
                    }
                }
            });
            if self.file.is_some() {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Party, "Party");
                    ui.selectable_value(&mut self.tab, Tab::Boxes, "Boxes");
                    ui.selectable_value(&mut self.tab, Tab::Items, "Items");
                    ui.selectable_value(&mut self.tab, Tab::Trainer, "Trainer");
                    ui.selectable_value(&mut self.tab, Tab::Rogue, "Rogue");
                });
            }
        });
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.label(&self.status);
        });

        if let Some(sf) = &self.file {
            egui::SidePanel::left("mons").default_width(230.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.strong("Party");
                    for (i, m) in sf.party().iter().enumerate() {
                        if ui.selectable_label(matches!(self.sel, Some(Sel::Party(x)) if x == i),
                                               m.display_name()).clicked() {
                            self.sel = Some(Sel::Party(i));
                        }
                    }
                    ui.add_space(8.0);
                    let fusions: Vec<(usize, Mon)> = (0..FUSION_COUNT)
                        .map(|f| (f, sf.fusion_mon(f)))
                        .filter(|(_, m)| m.species() != 0)
                        .collect();
                    if !fusions.is_empty() {
                        ui.strong(format!("Fusion storage — {}", fusions.len()));
                        for (f, m) in fusions {
                            let label = format!("{}. {}", f + 1, m.display_name());
                            if ui
                                .selectable_label(
                                    matches!(self.sel, Some(Sel::Fusion(x)) if x == f),
                                    label,
                                )
                                .clicked()
                            {
                                self.sel = Some(Sel::Fusion(f));
                            }
                        }
                        ui.add_space(8.0);
                    }
                    for bx in 0..ACTIVE_BOXES {
                        let cnt = (0..BOX_SLOTS).filter(|&s| sf.box_mon(bx, s).species() != 0).count();
                        if cnt == 0 && bx >= 10 { continue; }
                        let name = if bx < 10 { format!("Box {} ({})", bx + 1, sf.box_name(bx)) }
                                   else { format!("Extra box {}", bx - 9) };
                        ui.strong(format!("{name} — {cnt}"));
                        for s in 0..BOX_SLOTS {
                            let m = sf.box_mon(bx, s);
                            if m.species() == 0 { continue; }
                            let label = format!("{}. {}", s + 1, m.display_name());
                            if ui.selectable_label(matches!(self.sel, Some(Sel::Box(b, k)) if b == bx && k == s),
                                                   label).clicked() {
                                self.sel = Some(Sel::Box(bx, s));
                            }
                        }
                    }
                });
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.file.is_none() {
                ui.heading("Emerald Rogue Save Editor");
                ui.label("Open a Pokémon Emerald Rogue (v2.x / EX) .sav file.");
                ui.label(
                    "Works with mGBA saves (~/.config/mGBA/saves or the emulator Saves folder).",
                );
                return;
            }
            match self.tab {
                Tab::Party | Tab::Boxes => self.mon_editor_ui(ui),
                Tab::Items => self.items_ui(ui),
                Tab::Trainer => self.trainer_ui(ui),
                Tab::Rogue => self.rogue_ui(ui),
            }
        });
    }
}
