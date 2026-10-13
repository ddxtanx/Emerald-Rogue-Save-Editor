//! Central Pokemon editor pane (fields of the selected mon).

use super::*;
use crate::basestats::base_stats;
use crate::tables::{MOVES, NATURES};

impl Editor {
    pub(crate) fn mon_editor_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut m) = self.selected_mon() else {
            ui.heading("No Pokémon selected");
            return;
        };
        let mut changed = false;

        ui.heading(m.display_name());
        let ck = m.checksum_ok();
        ui.label(format!(
            "PID {:08X}  OTID {:08X}  checksum {} ({})  {}",
            m.pid(),
            m.otid(),
            m.checksum_stored(),
            if ck { "OK" } else { "CORRUPT" },
            if m.is_bad_egg() { "⚠ BAD EGG" } else { "" }
        ));
        ui.separator();

        egui::Grid::new("mongrid")
            .num_columns(4)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Species");
                Self::filtered_combo(
                    ui,
                    "species",
                    &mut self.species_filter,
                    all_species(),
                    m.species(),
                    &mut |id| {
                        m.set_species(id);
                        changed = true;
                    },
                );
                ui.label("Held item");
                Self::filtered_combo(
                    ui,
                    "item",
                    &mut self.item_filter,
                    all_items(),
                    m.held_item(),
                    &mut |id| {
                        m.set_held_item(id);
                        changed = true;
                    },
                );
                ui.end_row();

                ui.label("Nickname");
                let mut nick = m.nickname();
                if ui.add(egui::TextEdit::singleline(&mut nick)).changed() {
                    m.set_nickname(&nick);
                    changed = true;
                }
                ui.label("OT");
                ui.label(m.ot_name());
                ui.label("Ball");
                let mut ball = m.ball();
                if ui
                    .add(egui::DragValue::new(&mut ball).clamp_range(0..=31))
                    .changed()
                {
                    m.set_ball(ball);
                    changed = true;
                }
                ui.end_row();

                // Level is derived from exp for every mon (box structs have no level
                // byte at all). Editing level writes exp = level*300, exactly like the
                // game does when it spawns a mon at a level.
                let derived = m.level().unwrap_or(0);
                ui.label("Level");
                let mut lvl = derived;
                if ui
                    .add(egui::DragValue::new(&mut lvl).clamp_range(1..=MAX_LEVEL))
                    .changed()
                {
                    m.set_level(lvl);
                    changed = true;
                }
                if let Some(cached) = m.level_cached() {
                    if cached != derived {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 160, 60),
                            format!(
                                "cached byte {cached} != exp-derived {derived} (game uses {derived})"
                            ),
                        );
                    }
                }
                ui.label("Experience");
                let mut exp = m.exp();
                if ui
                    .add(egui::DragValue::new(&mut exp).clamp_range(0..=0x1FFFFF))
                    .changed()
                {
                    m.set_exp(exp);
                    changed = true;
                }
                ui.label("Friendship");
                let mut fr = m.friendship();
                if ui
                    .add(egui::DragValue::new(&mut fr).clamp_range(0..=255))
                    .changed()
                {
                    m.set_friendship(fr);
                    changed = true;
                }
                ui.label("PP Ups");
                let mut ppb = m.pp_bonuses();
                if ui
                    .add(egui::DragValue::new(&mut ppb).clamp_range(0..=3))
                    .changed()
                {
                    m.set_pp_bonuses(ppb);
                    changed = true;
                }
                ui.end_row();

                ui.label("Nature");
                egui::ComboBox::from_id_source("nature")
                    .selected_text(NATURES[m.nature() as usize])
                    .show_ui(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(260.0)
                            .show(ui, |ui| {
                                for (i, n) in NATURES.iter().enumerate() {
                                    if ui.selectable_label(m.nature() as usize == i, *n).clicked() {
                                        m.set_nature(i as u8);
                                        changed = true;
                                        ui.close_menu();
                                    }
                                }
                            });
                    });
                ui.label("Shiny");
                let mut sh = m.is_shiny();
                if ui.checkbox(&mut sh, "").changed() {
                    m.set_shiny(sh);
                    changed = true;
                }
                // Gender: `genderFlag` is only consulted for variable-ratio
                // species. The game returns the species' fixed value otherwise,
                // so printing the raw bit claimed Metagross (flag 7, ratio
                // MON_GENDERLESS) was "Female (7)".
                let sp = m.species();
                let gflag = m.gender_flag();
                let derived = crate::lookup::gender_for_species(sp, gflag);
                let shown = derived.unwrap_or(match gflag {
                    0 => crate::lookup::Gender::Male,
                    _ => crate::lookup::Gender::Female,
                });
                let fixed = crate::lookup::gender_is_fixed(sp) == Some(true);
                ui.label("Gender");
                egui::ComboBox::from_id_source("gender")
                    .selected_text(shown.label())
                    .show_ui(ui, |ui| {
                        if fixed {
                            // Editing the flag cannot change what the game shows.
                            ui.label(format!(
                                "fixed by species; stored flag {gflag} is ignored by the game"
                            ));
                        } else {
                            if derived.is_none() {
                                ui.label(format!(
                                    "species {sp} not in the gender table -- showing the raw flag ({gflag})"
                                ));
                            }
                            for (val, g) in [
                                (0u8, crate::lookup::Gender::Male),
                                (1u8, crate::lookup::Gender::Female),
                            ] {
                                if ui
                                    .selectable_label(shown == g, format!("{} ({})", g.label(), val))
                                    .clicked()
                                {
                                    m.set_gender_flag(val);
                                    changed = true;
                                }
                            }
                        }
                    });
                // `abilityNum` is a slot index into the species' ability list,
                // so it is meaningless without the species. NUM_ABILITY_SLOTS is
                // 3 in this build (2 normal + 1 hidden); the old loop ran
                // 0..2, which left hidden abilities unselectable.
                ui.label("Ability");
                egui::ComboBox::from_id_source("ability")
                    .selected_text(format!(
                        "{} (slot {})",
                        crate::lookup::ability_label(sp, m.ability_num()),
                        m.ability_num()
                    ))
                    .show_ui(ui, |ui| {
                        for a in 0..=crate::lookup::MAX_ABILITY_SLOT {
                            if ui
                                .selectable_label(
                                    m.ability_num() == a,
                                    format!("{} (slot {a})", crate::lookup::ability_label(sp, a)),
                                )
                                .clicked()
                            {
                                m.set_ability_num(a);
                                changed = true;
                            }
                        }
                    });
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("IVs");
            let names = ["HP", "Atk", "Def", "Spe", "SpA", "SpD"];
            for (i, nm) in names.iter().enumerate() {
                let mut v = m.ivs()[i];
                if ui
                    .add(
                        egui::DragValue::new(&mut v)
                            .clamp_range(0..=31)
                            .prefix(format!("{nm}: ")),
                    )
                    .changed()
                {
                    m.set_iv(i, v);
                    changed = true;
                }
            }
            if ui.button("31 all").clicked() {
                for i in 0..6 {
                    m.set_iv(i, 31);
                }
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("EVs");
            let names = ["HP", "Atk", "Def", "Spe", "SpA", "SpD"];
            for (i, nm) in names.iter().enumerate() {
                let mut v = m.evs()[i];
                if ui
                    .add(
                        egui::DragValue::new(&mut v)
                            .clamp_range(0..=255)
                            .prefix(format!("{nm}: ")),
                    )
                    .changed()
                {
                    m.set_ev(i, v);
                    changed = true;
                }
            }
            let evsum: u32 = m.evs().iter().map(|&v| v as u32).sum();
            ui.label(format!("total {evsum}"));
        });

        ui.add_space(6.0);
        ui.strong("Moves");
        egui::Grid::new("moves")
            .num_columns(4)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                for s in 0..4 {
                    let id = m.moves()[s];
                    Self::filtered_combo(
                        ui,
                        &format!("move{s}"),
                        &mut self.species_filter,
                        MOVES,
                        id,
                        &mut |mid| {
                            m.set_move(s, mid);
                            changed = true;
                        },
                    );
                    let mut pp = m.pps()[s];
                    ui.add(
                        egui::DragValue::new(&mut pp)
                            .clamp_range(0..=255)
                            .suffix(" pp"),
                    );
                    if pp != m.pps()[s] {
                        m.set_pp(s, pp);
                        changed = true;
                    }
                }
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(format!(
                "Met lvl {}  loc {}  egg={}",
                m.met_level(),
                m.met_location(),
                m.is_egg()
            ));
            if ui.checkbox(&mut { m.is_egg() }, "is egg").clicked() {}
        });

        if let Some((hp, max, st)) = m.cached_stats() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(format!("stats {hp}/{max}"));
                let labels = ["Atk", "Def", "Spe", "SpA", "SpD"];
                for (i, lb) in labels.iter().enumerate() {
                    let mut v = st[i];
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .clamp_range(1..=999)
                                .prefix(format!("{lb}: ")),
                        )
                        .changed()
                    {
                        m.set_cached_stat(i, v);
                        changed = true;
                    }
                }
            });
            // Gen 3 party stats are *cached in the save*: an IV/EV/level change
            // does nothing in-game until the mon is recalculated by battle or
            // level-up. This button does what the game would do, immediately.
            ui.horizontal(|ui| {
                if ui
                    .button("recalculate stats from IV/EV/level/nature")
                    .clicked()
                {
                    let lvl = m.level().unwrap_or(0) as u32;
                    let nat = m.nature() as usize;
                    match base_stats(m.species()) {
                        Some(bs) => {
                            let evs = m.evs();
                            let ivs = m.ivs();
                            let hp =
                                calc_stat(bs[0] as u32, ivs[0] as u32, evs[0] as u32, lvl, nat, 0);
                            m.set_cached_hp(hp as u16);
                            for i in 1..6 {
                                let v = calc_stat(
                                    bs[i] as u32,
                                    ivs[i] as u32,
                                    evs[i] as u32,
                                    lvl,
                                    nat,
                                    i,
                                );
                                m.set_cached_stat(i - 1, v as u16);
                            }
                            self.mon_note = Some(format!("stats recalculated at level {lvl}"));
                            changed = true;
                        }
                        None => {
                            self.mon_note = Some(format!(
                                "no base stats in table for species {} - stats NOT recalculated",
                                m.species()
                            ));
                        }
                    }
                }
                if let Some(note) = &self.mon_note {
                    ui.colored_label(egui::Color32::GOLD, note);
                }
            });
        }

        if changed {
            m.seal();
            self.apply_mon(m);
        }
    }
}
