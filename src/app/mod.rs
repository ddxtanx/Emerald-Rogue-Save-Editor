//! The eframe application: layout, tab dispatch, selection model, roster
//! management (add / delete / move mons), and file open/save plumbing.
//! Per-tab UIs live in the sibling modules as `impl Editor` blocks.
//!
//! Layout notes, both from explicit requests:
//!   * There is no "Party"/"Boxes" tab. The roster is always in the left panel,
//!     so a tab that showed the same thing did nothing; selecting a mon instead
//!     opens a PKHeX-style popup editor window.
//!   * Every id picklist (species, item, ball, move, quest, decoration variant)
//!     goes through `crate::searchable_combo`, which is the only place that
//!     knows how to keep a popup open while you type in its search field.

use crate::deco_variants;
use crate::format::*;
use crate::game_data::*;
use crate::icons::{self, IconKind, IconStore};
use crate::mon::*;
use crate::savefile::*;
use crate::searchable_combo::{self, Opts};

// per-tab UIs, each an `impl Editor` block in its own file
mod items_ui;
mod mon_ui;
mod rogue_ui;
mod trainer_ui;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Items,
    Trainer,
    Rogue,
}

#[derive(Clone, PartialEq, Debug)]
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
    hide_empty_items: bool,
    show_empty_safari: bool,
    dirty: bool,
    // bag add-row state (persists across frames so the combo/search survives)
    bag_add_id: u16,
    bag_add_qty: u16,
    bag_add_stash: bool,
    // add-mon rows: species picked per roster group
    add_party_species: u16,
    add_box_species: [u16; ACTIVE_BOXES],
    // the PKHeX-style mon editor window
    mon_popup: bool,
    // PP picker popup (per move slot, opened from the mon editor)
    // icon textures + on-disk art
    icons: IconStore,
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
            tab: Tab::Items,
            status: "Open a .sav file to begin.".into(),
            hide_empty_items: true,
            show_empty_safari: false,
            bag_add_id: 1,
            bag_add_qty: 1,
            bag_add_stash: false,
            add_party_species: 4, // Poké Ball holders expect a real species; 4 = Squirtle-line id
            add_box_species: [4; ACTIVE_BOXES],
            mon_popup: false,
            icons: IconStore::new(),
            mon_note: None,
            dirty: false,
        }
    }
}

impl Editor {
    pub(crate) fn open_path(&mut self, ps: String) {
        match SaveFile::open(&ps) {
            Ok(sf) => {
                let n = sf.active_slot / SLOT_SECTORS + 1;
                // The backup slot is only a fallback: if the active slot's
                // sector checksums are bad the save is already damaged, and the
                // editor should say so before anyone edits it.
                let intact = sf.slot_valid(sf.active_slot);
                self.status = format!(
                    "Opened {ps} (active save slot {n}, sector checksums {})",
                    if intact { "OK" } else { "MISMATCH" }
                );
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

    // ---- searchable picklists ---------------------------------------------
    // The single entry point for every dropdown in the editor. All of them get
    // the same behaviour: search field keeps the popup open, auto-focus, ↑↓ +
    // Enter, and icons where the id namespace has art.
    fn combo(
        &self,
        ui: &mut egui::Ui,
        id: &str,
        table: &[(u16, &str)],
        current: u16,
        opts: &Opts,
    ) -> Option<u16> {
        searchable_combo::show(ui, id, table, current, opts, &self.icons)
    }

    // ---- selection ---------------------------------------------------------
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

    /// Human name for the current selection, used as the popup window title.
    fn sel_label(&self) -> String {
        let Some(sf) = self.file.as_ref() else {
            return "Pokémon".into();
        };
        match self.sel.clone() {
            Some(Sel::Party(i)) => format!("Party slot {}", i + 1),
            Some(Sel::Box(bx, s)) => {
                let bn = sf.box_name(bx);
                if bn.is_empty() {
                    format!("Box {} slot {}", bx + 1, s + 1)
                } else {
                    format!("Box {} ({bn}) slot {}", bx + 1, s + 1)
                }
            }
            Some(Sel::Fusion(f)) => format!("Fusion storage slot {}", f + 1),
            Some(Sel::StashParty(i)) => format!("Stashed (hub) party slot {}", i + 1),
            Some(Sel::BlockDaycare(i)) => format!("Daycare slot {}", i + 1),
            Some(Sel::StashDaycare(i)) => format!("Stashed (hub) daycare slot {}", i + 1),
            None => "No Pokémon selected".into(),
        }
    }

    /// Where a mon can be moved to: the other side of party <-> box.
    fn first_empty_box_slot(&self, bx: usize) -> Option<usize> {
        let sf = self.file.as_ref()?;
        (0..BOX_SLOTS).find(|&s| sf.box_mon(bx, s).species() == 0)
    }

    fn first_empty_party_slot(&self) -> Option<usize> {
        let sf = self.file.as_ref()?;
        if sf.party_count() < 6 {
            Some(sf.party_count())
        } else {
            None
        }
    }

    // ---- roster mutation ---------------------------------------------------
    /// Add a fresh mon of `species` to the party (needs a free slot).
    fn add_to_party(&mut self, species: u16) {
        if species == 0 {
            self.status = "Pick a species first.".into();
            return;
        }
        let Some(sf) = self.file.as_mut() else { return };
        let ot_name = sf.player_name();
        let blank = Mon::blank(sf.enc_key(), &ot_name, species, 5);
        let Some((slot, _otid)) = sf.add_party_mon(&blank) else {
            self.status = "Party is full (6). Move one to the PC first.".into();
            return;
        };
        self.dirty = true;
        self.sel = Some(Sel::Party(slot));
        self.mon_popup = true;
        self.status = format!(
            "Added species {species} to party slot {} (unsaved — press Save to write)",
            slot + 1
        );
    }

    /// Add a fresh mon of `species` to a box (needs a free slot).
    fn add_to_box(&mut self, bx: usize, species: u16) {
        if species == 0 {
            self.status = "Pick a species first.".into();
            return;
        }
        let Some(slot) = self.first_empty_box_slot(bx) else {
            self.status = format!("Box {} is full (30).", bx + 1);
            return;
        };
        let Some(sf) = self.file.as_mut() else { return };
        let ot_name = sf.player_name();
        let m = Mon::blank(sf.enc_key(), &ot_name, species, 5);
        sf.set_box_mon(bx, slot, &m);
        self.dirty = true;
        self.sel = Some(Sel::Box(bx, slot));
        self.mon_popup = true;
        self.status = format!(
            "Added species {species} to box {} slot {} (unsaved — press Save to write)",
            bx + 1,
            slot + 1
        );
    }

    /// Clear the selected mon's slot. Party slots are closed up so the party
    /// never has a gap in the middle (the game reads `partyCount` + 6 slots).
    fn delete_selected(&mut self) {
        let Some(sel) = self.sel.clone() else { return };
        let Some(sf) = self.file.as_mut() else { return };
        match sel {
            Sel::Party(i) => {
                let n = sf.delete_party_mon(i);
                self.dirty = true;
                self.sel = Some(Sel::Party(i.min(n.saturating_sub(1))));
                self.status = format!("Deleted party slot {} (party now {n})", i + 1);
            }
            Sel::Box(bx, s) => {
                sf.clear_box_slot(bx, s);
                self.dirty = true;
                self.status = format!("Cleared box {} slot {}", bx + 1, s + 1);
            }
            Sel::Fusion(f) => {
                sf.set_fusion_mon(f, &Mon::from_bytes(&vec![0u8; MONSZ]));
                self.dirty = true;
                self.status = format!("Cleared fusion slot {}", f + 1);
            }
            other => {
                self.status = format!(
                    "This slot ({other:?}) is part of the Rogue block; delete is not offered here."
                );
            }
        }
        self.mon_popup = false;
    }

    /// Party <-> box transfer. Moving to the PC closes up the party slot;
    /// moving to the party needs a free party slot and an empty target isn't
    /// needed (the box slot is cleared).
    fn move_between_party_and_box(&mut self) {
        let Some(sel) = self.sel.clone() else { return };
        // Resolve destinations BEFORE the save is mutably borrowed: the
        // "find a free slot" helpers take &self.
        let dest_box =
            (0..ACTIVE_BOXES).find_map(|bx| self.first_empty_box_slot(bx).map(|s| (bx, s)));
        let dest_party = self.first_empty_party_slot();
        let Some(sf) = self.file.as_mut() else { return };
        match sel {
            Sel::Party(i) if i < sf.party_count() => {
                let Some((bx, slot)) = dest_box else {
                    self.status = "No empty box slot to move to.".into();
                    return;
                };
                let m = sf.party()[i].clone();
                sf.set_box_mon(bx, slot, &m);
                let n = sf.delete_party_mon(i);
                self.dirty = true;
                self.sel = Some(Sel::Box(bx, slot));
                self.status = format!(
                    "Moved party slot {} to box {} slot {} (party now {n})",
                    i + 1,
                    bx + 1,
                    slot + 1
                );
            }
            Sel::Box(bx, s) => {
                let m = sf.box_mon(bx, s);
                if m.species() == 0 {
                    self.status = "That box slot is empty.".into();
                    return;
                }
                let Some(pslot) = dest_party else {
                    self.status = "Party is full (6).".into();
                    return;
                };
                // The box copy is 80 B (BoxPokemon); the party copy is 104 B
                // (Pokemon) with a cached-stat block the game recomputes on
                // load. Pad the tail with zeros: CalculateMonStats overwrites it.
                let mut raw = m.raw.clone();
                raw.resize(MONSZ, 0);
                let pm = Mon::from_bytes(&raw);
                sf.set_party_mon(pslot, &pm);
                sf.set_party_count(pslot + 1);
                sf.clear_box_slot(bx, s);
                self.dirty = true;
                self.sel = Some(Sel::Party(pslot));
                self.status = format!(
                    "Moved box {} slot {} to party slot {} (run 'recalculate stats' if it looks wrong)",
                    bx + 1,
                    s + 1,
                    pslot + 1
                );
            }
            _ => self.status = "Select a party or box slot to move it.".into(),
        }
        self.mon_note = None;
    }

    // ---- roster panel ------------------------------------------------------
    fn roster_ui(&mut self, ui: &mut egui::Ui) {
        // Snapshot everything the panel draws. The save must not stay borrowed
        // across the row helpers (edit / move / delete), which need `&mut self`:
        // with the old shared borrow held, egui rows could not mutate anything.
        let (party, fusions, boxes) = match self.file.as_ref() {
            None => return,
            Some(sf) => {
                let party: Vec<(usize, Mon)> = sf.party().into_iter().enumerate().collect();
                let fusions: Vec<(usize, Mon)> = (0..FUSION_COUNT)
                    .map(|f| (f, sf.fusion_mon(f)))
                    .filter(|(_, m)| m.species() != 0)
                    .collect();
                let boxes: Vec<(usize, String, Vec<(usize, Mon)>)> = (0..ACTIVE_BOXES)
                    .map(|bx| {
                        let mons: Vec<(usize, Mon)> = (0..BOX_SLOTS)
                            .map(|s| (s, sf.box_mon(bx, s)))
                            .filter(|(_, m)| m.species() != 0)
                            .collect();
                        (bx, sf.box_name(bx), mons)
                    })
                    .collect();
                (party, fusions, boxes)
            }
        };

        // ---- party ---------------------------------------------------------
        ui.horizontal(|ui| {
            ui.strong(format!("Party — {}", party.len()));
            if ui
                .add_enabled(party.len() < 6, |ui: &mut egui::Ui| ui.button("+ add"))
                .clicked()
            {
                self.mon_note = Some("pick a species, then press Add".into());
            }
        });
        ui.horizontal(|ui| {
            let picked = self.combo(
                ui,
                "add_party_species",
                all_species(),
                self.add_party_species,
                &Opts::pokemon(),
            );
            if let Some(id) = picked {
                self.add_party_species = id;
            }
            if ui
                .add_enabled(party.len() < 6, |ui: &mut egui::Ui| ui.button("to party"))
                .clicked()
            {
                let sp = self.add_party_species;
                self.add_to_party(sp);
            }
        });
        for (i, m) in party {
            self.roster_row(ui, Sel::Party(i), &m, i + 1);
        }

        // ---- fusion storage ------------------------------------------------
        if !fusions.is_empty() {
            ui.add_space(8.0);
            ui.strong(format!("Fusion storage — {}", fusions.len()));
            for (f, m) in fusions {
                self.roster_row(ui, Sel::Fusion(f), &m, f + 1);
            }
        }

        // ---- boxes ---------------------------------------------------------
        for (bx, name, mons) in boxes {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.strong(format!("Box {}", bx + 1));
                // BOX_NAME_LEN is 9 bytes: 8 characters plus the 0xFF
                // terminator, and the charset is the Gen 3 one, so the field is
                // clamped to 8 characters here.
                let mut nm = name.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut nm)
                            .desired_width(90.0)
                            .char_limit(8),
                    )
                    .on_hover_text("box name (8 chars, the game's PC rename uses the same)")
                    .changed()
                {
                    if let Some(sf) = self.file.as_mut() {
                        sf.set_box_name(bx, &nm);
                    }
                    self.dirty = true;
                }
                ui.label(format!("{} mons", mons.len()));
            });
            ui.horizontal(|ui| {
                let picked = self.combo(
                    ui,
                    &format!("add_box_species_{bx}"),
                    all_species(),
                    self.add_box_species[bx],
                    &Opts::pokemon().compact(),
                );
                if let Some(id) = picked {
                    self.add_box_species[bx] = id;
                }
                if ui
                    .add_enabled(mons.len() < BOX_SLOTS, |ui: &mut egui::Ui| {
                        ui.button("to box")
                    })
                    .clicked()
                {
                    let sp = self.add_box_species[bx];
                    self.add_to_box(bx, sp);
                }
            });
            for (s, m) in mons {
                self.roster_row(ui, Sel::Box(bx, s), &m, s + 1);
            }
        }
    }

    /// One roster row: icon, level/gender, nickname, and the row actions
    /// (edit popup, move party<->box, delete). Double-click opens the editor.
    fn roster_row(&mut self, ui: &mut egui::Ui, sel: Sel, m: &Mon, slot_no: usize) {
        let selected = self.sel.as_ref() == Some(&sel);
        let sp = m.species();
        let name = m.nickname();
        let name = if name.is_empty() {
            species_name(sp).to_string()
        } else {
            name
        };
        let lvl = m.level().unwrap_or(0);
        let g = crate::lookup::gender_for_species(sp, m.gender_flag())
            .map(|x| x.glyph())
            .unwrap_or('?');
        let shiny = if m.is_shiny() { "★" } else { "" };

        // `icons` is borrowed for the whole row, so the row may not touch the
        // rest of self: it just reports which button was pressed and the
        // mutations happen afterwards, below.
        let (dbl, edit, mv, del) = {
            let icons = &self.icons;
            ui.horizontal(|ui| {
                let ctx = ui.ctx().clone();
                icons::draw_icon(ui, icons, &ctx, IconKind::Pokemon, sp, &name, 22.0);
                let label = format!("{slot_no:>2}. {name} L{lvl} {g}{shiny}");
                let r = ui.selectable_label(selected, label);
                let edit = ui.small_button("✎").on_hover_text("edit in popup");
                let mv = match &sel {
                    Sel::Party(_) => Some(
                        ui.small_button("→PC")
                            .on_hover_text("move this mon to the first empty box slot"),
                    ),
                    Sel::Box(..) => Some(
                        ui.small_button("→PT")
                            .on_hover_text("move this mon into the party"),
                    ),
                    _ => None,
                };
                let del = match &sel {
                    Sel::Party(..) | Sel::Box(..) | Sel::Fusion(..) => Some(
                        ui.small_button("✕")
                            .on_hover_text("delete this mon (clears the slot)"),
                    ),
                    _ => None,
                };
                (
                    r.double_clicked(),
                    edit.clicked(),
                    mv.map(|r| r.clicked()),
                    del.map(|r| r.clicked()),
                )
            })
            .inner
        };

        if dbl || edit {
            self.sel = Some(sel.clone());
            self.mon_popup = true;
        }
        if mv == Some(true) {
            self.sel = Some(sel.clone());
            self.move_between_party_and_box();
        }
        if del == Some(true) {
            self.sel = Some(sel);
            self.delete_selected();
        }
    }

    // ---- the mon editor window --------------------------------------------
    fn mon_window(&mut self, ctx: &egui::Context) {
        if !self.mon_popup || self.file.is_none() || self.selected_mon().is_none() {
            return;
        }
        let title = self.sel_label();
        let mut open = self.mon_popup;
        let resp = egui::Window::new(title)
            .id(egui::Id::new("mon_editor_window"))
            .open(&mut open)
            .default_width(720.0)
            .max_width(900.0)
            .resizable(true)
            .show(ctx, |ui| {
                // The window content is scrollable: with every dropdown open
                // the fields no longer fit, which was the reported complaint.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let can_move =
                            matches!(self.sel, Some(Sel::Party(..)) | Some(Sel::Box(..)));
                        ui.horizontal(|ui| {
                            if can_move && ui.button("move party <-> box").clicked() {
                                self.move_between_party_and_box();
                            }
                            if ui.button("delete mon").clicked() {
                                self.delete_selected();
                            }
                            if ui.button("31 IVs / max PP").clicked() {
                                if let Some(mut m) = self.selected_mon() {
                                    m.set_all_ivs(31);
                                    for s in 0..4 {
                                        m.set_pp_bonus(s, 3);
                                        // cap = base + base/5 * 3, exactly what the
                                        // game allows after three PP Ups
                                        let cap = crate::move_pp::max_pp(m.moves()[s], 3);
                                        m.set_pp(s, cap);
                                    }
                                    m.seal();
                                    self.apply_mon(m);
                                }
                            }
                        });
                        ui.separator();
                        self.mon_editor_ui(ui);
                    });
            });
        self.mon_popup = open && resp.is_some();
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
                } else {
                    ui.label("no file open");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(self.icons.status());
                });
            });
            if self.file.is_some() {
                ui.horizontal(|ui| {
                    // No Party/Boxes tab: the roster is always in the left panel.
                    ui.selectable_value(&mut self.tab, Tab::Items, "Items");
                    ui.selectable_value(&mut self.tab, Tab::Trainer, "Trainer");
                    ui.selectable_value(&mut self.tab, Tab::Rogue, "Rogue");
                    ui.separator();
                    if ui.button("Edit selected mon").clicked() {
                        self.mon_popup = self.selected_mon().is_some();
                        if !self.mon_popup {
                            self.status = "Select a Pokémon in the left panel first.".into();
                        }
                    }
                });
            }
        });
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.label(&self.status);
            ui.label(self.icons.status());
        });

        if self.file.is_some() {
            egui::SidePanel::left("mons")
                .default_width(320.0)
                .min_width(260.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(note) = &self.mon_note {
                                ui.colored_label(egui::Color32::GOLD, note);
                            }
                            self.roster_ui(ui);
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
                ui.add_space(8.0);
                ui.weak(self.icons.status());
                return;
            }
            // Everything in the centre is scrollable, including the Rogue tab
            // with a dozen collapsing sections open at once.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    Tab::Items => self.items_ui(ui),
                    Tab::Trainer => self.trainer_ui(ui),
                    Tab::Rogue => self.rogue_ui(ui),
                });
        });

        self.mon_window(ctx);
    }
}

// The stat formula lives in crate::mon (mirroring CalStat); mon_ui sees it
// through `use super::*` -> the glob re-export of crate::mon::* below.
