//! Central Pokemon editor pane (fields of the selected mon).

use super::*;
use crate::basestats::base_stats;
use crate::searchable_combo::Opts;
use crate::tables::MOVES;

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
            "PID {:08X}  OTID {:08X}  checksum {} ({})",
            m.pid(),
            m.otid(),
            m.checksum_stored(),
            if ck { "OK" } else { "CORRUPT" },
        ));
        // Flag byte (0x13 in the party struct): bad-egg / has-species / is-egg.
        // They are shown rather than edited because the game only ever sets them
        // as a side effect of hatching or receiving a mon.
        ui.label(format!(
            "flags: {}has-species={} is-egg={}",
            if m.is_bad_egg() { "BAD EGG, " } else { "" },
            m.has_species(),
            m.is_egg_boxflag(),
        ));
        ui.separator();

        egui::Grid::new("mongrid")
            .num_columns(4)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Species");
                if let Some(id) =
                    self.combo(ui, "species", all_species(), m.species(), &Opts::pokemon())
                {
                    m.set_species(id);
                    changed = true;
                }
                ui.label("Held item");
                if let Some(id) = self.combo(
                    ui,
                    "item",
                    all_items(),
                    m.held_item(),
                    &Opts::item().with_none("(none)"),
                ) {
                    m.set_held_item(id);
                    changed = true;
                }
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
                ui.horizontal(|ui| {
                    // Name, not id, and searchable — the request was that the ball
                    // menu say which ball the mon was caught in, using the same
                    // picker class as every other list. Ball ids live in
                    // include/constants/items.h (a ball IS an item to the game).
                    if let Some(id) = self.combo(
                        ui,
                        "ball",
                        crate::game_data::BALLS,
                        ball as u16,
                        &Opts::named().compact().with_none("(no ball)"),
                    ) {
                        ball = id as u8;
                        m.set_ball(ball);
                        changed = true;
                    }
                    // Raw value stays editable: Emerald Rogue uses ball ids the
                    // stock list does not cover.
                    if ui
                        .add(egui::DragValue::new(&mut ball).clamp_range(0..=31))
                        .on_hover_text(format!(
                            "raw ball id (the game's own field). Known name for {ball}: {}",
                            crate::game_data::ball_name(ball)
                        ))
                        .changed()
                    {
                        m.set_ball(ball);
                        changed = true;
                    }
                });
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
                ui.label("Nature");
                // 25 values, no useful ids: same picker as everything else, with
                // the numbers hidden (nature 7 means nothing; "Relaxed" does).
                if let Some(n) = self.combo(
                    ui,
                    "nature",
                    crate::tables::NATURE_IDS,
                    m.nature() as u16,
                    &Opts::named().no_id().compact(),
                ) {
                    m.set_nature(n as u8);
                    changed = true;
                }
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
                                (
                                    crate::lookup::flag_for_gender(crate::lookup::Gender::Male),
                                    crate::lookup::Gender::Male,
                                ),
                                (
                                    crate::lookup::flag_for_gender(crate::lookup::Gender::Female),
                                    crate::lookup::Gender::Female,
                                ),
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
        ui.label(
            "PP ups are per-slot (0..=3). The game's cap is base + base/5 * ups, so \"max\" \
             fills PP to that cap; the PP field itself stays free-form.",
        );
        egui::Grid::new("moves")
            .num_columns(5)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                for s in 0..4 {
                    let id = m.moves()[s];
                    if let Some(mid) =
                        self.combo(ui, &format!("move{s}"), MOVES, id, &Opts::named())
                    {
                        m.set_move(s, mid);
                        changed = true;
                    }
                    let base = crate::move_pp::base_pp(id);
                    let ladder = crate::move_pp::pp_ladder(id);
                    let tips: Vec<String> = ladder
                        .iter()
                        .map(|(u, p)| format!("{u} ups -> {p}"))
                        .collect();
                    let pp_now = m.pps()[s];
                    let mut pp = pp_now;
                    if ui
                        .add(
                            egui::DragValue::new(&mut pp)
                                .clamp_range(0..=255)
                                .prefix(format!("PP {pp_now}/"))
                                .suffix(format!("{base}")),
                        )
                        .changed()
                    {
                        m.set_pp(s, pp);
                        changed = true;
                    }
                    // Per-slot PP Ups. The whole-byte setter used to mask with
                    // `& 3`, which wiped the other three slots' ups.
                    let mut ups = m.pp_bonus(s) as usize;
                    if ui
                        .add(
                            egui::DragValue::new(&mut ups)
                                .clamp_range(0..=3)
                                .prefix("ups "),
                        )
                        .on_hover_text(tips.join("\n"))
                        .changed()
                    {
                        m.set_pp_bonus(s, ups as u8);
                        changed = true;
                    }
                    if ui
                        .button("max PP")
                        .on_hover_text("set current PP to the cap for the current ups")
                        .clicked()
                    {
                        let cap = ladder[(m.pp_bonus(s) as usize).min(3)].1;
                        m.set_pp(s, cap);
                        changed = true;
                    }
                    // The PP picker: the PP-Up steps the game knows, each shown with
                    // the PP it unlocks, plus a free-form entry (the request asked
                    // for both). This is an `Area` popup rather than a ComboBox, so
                    // its text field can actually be clicked — the bug that made
                    // ComboBox popups vanish the moment you touched their field.
                    let trig = ui
                        .small_button("PP…")
                        .on_hover_text("PP ladder (0-3 PP Ups) and custom PP entry");
                    let opening = trig.clicked();
                    let custom_id = egui::Id::new(format!("pp_custom{s}"));
                    let mut custom = ui
                        .ctx()
                        .data(|d| d.get_temp::<String>(custom_id))
                        .unwrap_or_default();
                    let picked = crate::area_popup::anchored(
                        ui,
                        &format!("pp_pop{s}"),
                        &trig,
                        opening,
                        |ui| {
                            let mut out: Option<(Option<u8>, u8)> = None;
                            ui.strong(format!("PP — base {base}"));
                            for (ups, val) in ladder.iter() {
                                let lbl = format!(
                                    "{} PP up{} → {val}",
                                    ups,
                                    if *ups == 1 { "" } else { "s" }
                                );
                                let cur = m.pp_bonus(s) == *ups && m.pps()[s] == *val;
                                if ui.selectable_label(cur, lbl).clicked() {
                                    out = Some((Some(*ups), *val));
                                }
                            }
                            ui.separator();
                            let te = egui::TextEdit::singleline(&mut custom)
                                .desired_width(90.0)
                                .hint_text("custom PP");
                            // Focus on the opening frame only, so typing works at
                            // once without stealing the caret back every frame.
                            let resp = ui.add(te);
                            if opening {
                                resp.request_focus();
                            }
                            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if (resp.lost_focus() && enter) || ui.button("set").clicked() {
                                if let Ok(v) = custom.trim().parse::<u8>() {
                                    out = Some((None, v));
                                }
                            }
                            out
                        },
                    );
                    if let Some(Some((ups, pp))) = picked {
                        if let Some(u) = ups {
                            m.set_pp_bonus(s, u as u8);
                        }
                        m.set_pp(s, pp);
                        changed = true;
                        custom = String::new();
                    }
                    ui.ctx().data_mut(|d| d.insert_temp(custom_id, custom));
                }
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // Met level / met location are both stored (substruct 3): met level
            // is 7 bits alongside other flags, met location is a byte id.
            let mut ml = m.met_level() as i32;
            if ui
                .add(
                    egui::DragValue::new(&mut ml)
                        .clamp_range(0..=127)
                        .prefix("met lvl "),
                )
                .changed()
            {
                m.set_met_level(ml as u16);
                changed = true;
            }
            let mut loc = m.met_location() as i32;
            if ui
                .add(
                    egui::DragValue::new(&mut loc)
                        .clamp_range(0..=255)
                        .prefix("loc "),
                )
                .on_hover_text("met-location id; the game only prints it in the summary")
                .changed()
            {
                m.set_met_location(loc as u8);
                changed = true;
            }
            // The bit exists in the struct (substruct 3 bit 31) even though a
            // GBA-gen game has no Gigantamax forms; shown so a mon imported from
            // elsewhere does not look silently different.
            let mut gmax = m.gigantamax_factor();
            if ui
                .checkbox(&mut gmax, "gmax flag")
                .on_hover_text("substruct-3 bit 31; unused by this ROM, kept for imports")
                .changed()
            {
                m.set_gigantamax_factor(gmax);
                changed = true;
            }
            // Egg is a real flag (hatch steps live in the egg data block); the
            // old checkbox wrote to a temporary and did nothing.
            let mut egg = m.is_egg();
            if ui.checkbox(&mut egg, "is egg").changed() {
                m.set_is_egg(egg);
                changed = true;
            }
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
