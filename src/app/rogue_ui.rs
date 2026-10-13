//! Rogue-tab UI: adventure state, hub map, quests, difficulty, daycare,
//! safari roster, unique mons.

use super::*;
use crate::tables::NATURES;
use crate::tables_extra;

impl Editor {
    pub(crate) fn rogue_ui(&mut self, ui: &mut egui::Ui) {
        let rb = match self.file.as_ref().and_then(|sf| sf.rogue_block()) {
            Some(rb) => rb,
            None => {
                ui.label("No Rogue save block found (not a v2.x Rogue save?).");
                return;
            }
        };
        if !rb.ok {
            ui.colored_label(
                egui::Color32::GOLD,
                format!(
                    "Rogue block header v{} found, but the stream layout does not match the \
                     expected v2.2.0 layout — showing header only; editing is disabled for this block.",
                    rb.version
                ),
            );
            return;
        }
        // Which state is live? SaveBlock1 always holds the *currently loaded* state.
        // If the stashed hub party is non-empty while SB1 shows more/different mons,
        // the player is mid-adventure.
        let stash_named = rb.stash.ok && rb.stash.party.iter().any(|m| m.species() != 0);
        ui.heading("Rogue adventure state");
        let mut tod = rb.time_of_day;
        let season = rb.season; // day counter, NOT a season index
        let season_idx = (season / DAYS_PER_SEASON) as usize % SEASON_COUNT;
        let mut weather = rb.weather;
        ui.horizontal(|ui| {
            ui.label(format!(
                "save stamp {} ({})",
                rb.version,
                rogue_version_name(rb.version)
            ));
            ui.label(format!("hub time {:02}:{:02}", tod / 60, tod % 60));
            if ui
                .add(
                    egui::DragValue::new(&mut tod)
                        .clamp_range(0..=1439)
                        .suffix(" min"),
                )
                .changed()
            {
                if let Some(sf) = self.file.as_mut() {
                    sf.set_hub_time(tod);
                }
                self.dirty = true;
            }
            egui::ComboBox::from_id_source("season")
                .selected_text(format!(
                    "{} (day {}/{})",
                    SEASONS[season_idx],
                    season % DAYS_PER_SEASON + 1,
                    DAYS_PER_SEASON
                ))
                .show_ui(ui, |ui| {
                    for (s, name) in SEASONS.iter().enumerate() {
                        if ui.selectable_label(season_idx == s, *name).clicked() {
                            if let Some(sf) = self.file.as_mut() {
                                sf.set_hub_season(s as u8);
                            }
                            self.dirty = true;
                        }
                    }
                });
            if ui
                .add(
                    egui::DragValue::new(&mut weather)
                        .clamp_range(0..=65535)
                        .prefix("weather "),
                )
                .changed()
            {
                if let Some(sf) = self.file.as_mut() {
                    sf.set_weather(weather);
                }
                self.dirty = true;
            }
            ui.label(format!("debug {}", rb.is_debug));
        });

        ui.collapsing("Quests", |ui| {
            let n = rb.quests_f.n;
            let base = rb.quests_f.data;
            let qn = tables_extra::QUEST_NAMES;
            // snapshot current values so the UI never borrows the file while editing it
            let snap: Vec<(u16, u8)> = {
                let sf = self.file.as_ref().unwrap();
                (0..n)
                    .map(|i| {
                        let w0 = le32(&sf.merged, base + i * 8);
                        ((w0 & 0xFFFF) as u16, ((w0 >> 16) & 7) as u8)
                    })
                    .collect()
            };
            let mut filt = self.quest_filter.clone();
            ui.horizontal(|ui| {
                ui.label("filter:");
                ui.text_edit_singleline(&mut filt);
                self.quest_filter = filt.clone();
                ui.label(format!(
                    "{} quest slots (names from quests.json, exporter order)",
                    n
                ));
            });
            let f = filt.to_lowercase();
            let mut edits: Vec<(usize, u16, u8)> = Vec::new();
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for i in 0..n {
                        let (flags, hc) = snap[i];
                        let name = qn.get(i).copied().unwrap_or("?");
                        if flags == 0 && hc == 0 {
                            continue; // untouched slot
                        }
                        if !f.is_empty() && !name.to_lowercase().contains(&f) {
                            continue;
                        }
                        let mut edit = [
                            flags & 1 != 0,
                            flags & 2 != 0,
                            flags & 4 != 0,
                            flags & 8 != 0,
                            flags & 0x10 != 0,
                            flags & 0x20 != 0,
                        ];
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{i:3}"));
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add_sized(
                                        [240.0, 0.0],
                                        egui::Label::new(name).truncate(true),
                                    );
                                },
                            );
                            let lbls = ["unlock", "active", "pin", "rwrd", "new", "done"];
                            let mut changed = false;
                            for (k, lb) in lbls.iter().enumerate() {
                                if ui.checkbox(&mut edit[k], *lb).changed() {
                                    changed = true;
                                }
                            }
                            let mut hcv = hc;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut hcv)
                                        .clamp_range(0..=7)
                                        .prefix("maxDiff "),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                            if changed {
                                let mut nf = 0u16;
                                for k in 0..6 {
                                    if edit[k] {
                                        nf |= 1 << k;
                                    }
                                }
                                edits.push((i, nf, hcv));
                            }
                        });
                    }
                });
            if !edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (i, nf, hcv) in edits {
                        sf.set_quest_state(i, nf, hcv);
                    }
                }
                self.dirty = true;
            }
        });

        ui.collapsing("Campaigns", |ui| {
            let snap: Vec<u32> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.campaigns_f.n)
                    .map(|i| le32(&sf.merged, rb.campaigns_f.data + i * 4))
                    .collect()
            };
            let mut edits: Vec<(usize, bool, bool, u16)> = Vec::new();
            for i in 0..rb.campaigns_f.n {
                let v = snap[i];
                let name = CAMPAIGN_NAMES.get(i).copied().unwrap_or("?");
                let mut unlocked = v & 1 != 0;
                let mut completed = (v >> 1) & 1 != 0;
                let mut best = (v >> 16) as u16;
                ui.horizontal(|ui| {
                    ui.strong(format!("{i}"));
                    ui.add_sized([170.0, 0.0], egui::Label::new(name).truncate(true));
                    let mut changed = false;
                    if ui.checkbox(&mut unlocked, "unlocked").changed() {
                        changed = true;
                    }
                    if ui.checkbox(&mut completed, "completed").changed() {
                        changed = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut best)
                                .clamp_range(0..=65535)
                                .prefix("best "),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if changed {
                        edits.push((i, unlocked, completed, best));
                    }
                });
            }
            if !edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (i, u, c, b) in edits {
                        sf.set_campaign(i, u, c, b);
                    }
                }
                self.dirty = true;
            }
        });

        ui.collapsing("Hub map (areas, upgrades, coordinates)", |ui| {
            let area_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.built_f.n)
                    .map(|k| sf.merged[rb.built_f.data + k])
                    .collect()
            };
            let up_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.upgrades_f.n)
                    .map(|k| sf.merged[rb.upgrades_f.data + k])
                    .collect()
            };
            let coord_snap: Vec<(i8, i8)> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.coords_f.n)
                    .map(|k| sf.hub_area_coord(&rb, k).unwrap_or((0, 0)))
                    .collect()
            };
            let built_snap: Vec<bool> = {
                let sf = self.file.as_ref().unwrap();
                (0..HUB_AREAS.len())
                    .map(|k| sf.hub_area_built(&rb, k))
                    .collect()
            };
            let mut area_edits: Vec<(usize, bool)> = Vec::new();
            let mut up_edits: Vec<(usize, bool)> = Vec::new();
            let mut coord_edits: Vec<(usize, i8, i8)> = Vec::new();
            ui.horizontal(|ui| {
                ui.strong("Areas built");
                ui.label(
                    egui::RichText::new("(built = present on the map; coordinates are separate)")
                        .small()
                        .weak(),
                );
            });
            egui::Grid::new("hubareas")
                .num_columns(4)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..HUB_AREAS.len() {
                        let byte = area_snap.get(i / 8).copied().unwrap_or(0);
                        let mut on = byte >> (i % 8) & 1 != 0;
                        if ui.checkbox(&mut on, HUB_AREAS[i]).changed() {
                            area_edits.push((i, on));
                        }
                        // editable position for this area
                        let (cx, cy) = coord_snap.get(i).copied().unwrap_or((0, 0));
                        let mut x = cx as i32;
                        let mut y = cy as i32;
                        let xw = ui
                            .add(
                                egui::DragValue::new(&mut x)
                                    .clamp_range(-128..=127)
                                    .speed(1),
                            )
                            .on_hover_text("area x (s8, hub-map tile)");
                        let yw = ui
                            .add(
                                egui::DragValue::new(&mut y)
                                    .clamp_range(-128..=127)
                                    .speed(1),
                            )
                            .on_hover_text("area y (s8, hub-map tile)");
                        if (xw.changed() || yw.changed())
                            && (-128..=127).contains(&x)
                            && (-128..=127).contains(&y)
                        {
                            coord_edits.push((i, x as i8, y as i8));
                        }
                        let (ox, oy) = coord_snap.get(i).copied().unwrap_or((0, 0));
                        let adj = if built_snap.get(i).copied().unwrap_or(false) {
                            format!("({ox},{oy})")
                        } else {
                            format!("({ox},{oy}) unbuilt")
                        };
                        ui.label(egui::RichText::new(adj).monospace().small());
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
            if !coord_edits.is_empty() {
                let sf = self.file.as_ref().unwrap();
                for &(i, x, y) in &coord_edits {
                    for w in sf.hub_coord_warnings(&rb, i, x, y) {
                        ui.label(
                            egui::RichText::new(format!("area {i} {}: {w}", HUB_AREAS[i]))
                                .color(egui::Color32::from_rgb(220, 150, 60)),
                        );
                    }
                }
            }
            ui.add_space(6.0);
            ui.strong("Upgrades");
            egui::Grid::new("hubupgrades")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..HUB_UPGRADES.len() {
                        let byte = up_snap.get(i / 8).copied().unwrap_or(0);
                        let mut on = byte >> (i % 8) & 1 != 0;
                        if ui.checkbox(&mut on, HUB_UPGRADES[i]).changed() {
                            up_edits.push((i, on));
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            if !area_edits.is_empty() || !up_edits.is_empty() || !coord_edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (i, on) in area_edits {
                        sf.set_hub_area(i, on);
                    }
                    for (i, on) in up_edits {
                        sf.set_hub_upgrade(i, on);
                    }
                    for (i, x, y) in coord_edits {
                        sf.set_hub_area_coord(i, x, y);
                    }
                }
                self.dirty = true;
            }
        });

        ui.collapsing("House (decorations, styles, follow-mons, weather)", |ui| {
            let style_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.styles_f.n)
                    .map(|k| sf.hub_style(&rb, k).unwrap_or(0))
                    .collect()
            };
            let wander_snap: Vec<(u16, bool)> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.wander_f.n)
                    .map(|k| sf.hub_wander_mon(&rb, k).unwrap_or((0, false)))
                    .collect()
            };
            let weather_snap = rb.weather;
            let mut style_edits: Vec<(usize, u8)> = Vec::new();
            let mut wander_edits: Vec<(usize, u16, bool)> = Vec::new();
            let mut weather_edit: Option<u16> = None;

            ui.strong("Home styles");
            egui::Grid::new("hubstyles")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for (i, names) in [
                        ("Exterior", HOME_STYLE_EXTERIOR),
                        ("Interior", HOME_STYLE_INTERIOR),
                        ("Path", HOME_STYLE_PATH),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let cur = style_snap.get(i).copied().unwrap_or(0);
                        ui.label(names.0);
                        let mut v = cur as i32;
                        if ui
                            .add(
                                egui::DragValue::new(&mut v)
                                    .clamp_range(0..=(names.1.len() as i32 - 1))
                                    .speed(1),
                            )
                            .changed()
                            && (0..names.1.len() as i32).contains(&v)
                        {
                            style_edits.push((i, v as u8));
                        }
                        let label = names
                            .1
                            .get(cur as usize)
                            .copied()
                            .unwrap_or("unknown (out of range for the enum)");
                        ui.label(format!("{label}  [{}]", style_snap.get(i).copied().unwrap_or(0)));
                        ui.end_row();
                    }
                });

            ui.add_space(6.0);
            ui.strong("Weather state");
            ui.label(
                "Raw u16 seed, not a weather id. The game derives the visible weather as state % 5 (spring/summer) or % 6 (autumn/winter).",
            );
            let mut wsv = weather_snap as i32;
            if ui
                .add(egui::DragValue::new(&mut wsv).clamp_range(0..=65535).speed(1))
                .changed()
                && (0..=65535).contains(&wsv)
            {
                weather_edit = Some(wsv as u16);
            }
            ui.label(format!(
                "{}: spring/summer bucket {}, autumn/winter bucket {}",
                weather_snap,
                weather_snap % 5,
                weather_snap % 6
            ));

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.strong("House follow-mons");
                ui.label(
                    egui::RichText::new("species in the house; * marks a shiny follow-mon")
                        .small()
                        .weak(),
                );
            });
            egui::Grid::new("hubwander")
                .num_columns(3)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..wander_snap.len() {
                        let (sp, shiny) = wander_snap.get(i).copied().unwrap_or((0, false));
                        ui.label(format!("[{i}] {}", if sp == 0 { "-".to_string() } else { format!("#{sp} {}", species_name(sp)) }));
                        let mut v = sp as i32;
                        let sw = ui
                            .add(egui::DragValue::new(&mut v).clamp_range(0..=65535).speed(1))
                            .on_hover_text("follow-mon species id (0 = none)");
                        let mut sh = shiny;
                        let cw = ui.checkbox(&mut sh, "shiny");
                        if (sw.changed() || cw.changed()) && (0..=65535).contains(&v) {
                            wander_edits.push((i, v as u16, sh));
                        }
                        ui.end_row();
                    }
                });

            if !style_edits.is_empty() || !wander_edits.is_empty() || weather_edit.is_some() {
                if let Some(sf) = self.file.as_mut() {
                    for (i, v) in style_edits {
                        sf.set_hub_style(i, v);
                    }
                    for (i, sp, sh) in wander_edits {
                        sf.set_hub_wander_mon(i, sp, sh);
                    }
                    if let Some(v) = weather_edit {
                        sf.set_hub_weather_state(v);
                    }
                }
                self.dirty = true;
            }
            ui.add_space(6.0);
            let decor_on = {
                let sf = self.file.as_ref().unwrap();
                sf.decor_on_count(&rb)
            };
            ui.collapsing(format!("Decorations ({decor_on} on)"), |ui| {
                ui.label(
                    "192 slots: 0..160 outdoor, 160..192 indoor. Each is x, y, variant, active.",
                );
                let snap: Vec<(u8, u8, u8, bool)> = {
                    let sf = self.file.as_ref().unwrap();
                    (0..rb.decor_f.n)
                        .map(|k| sf.hub_decoration(&rb, k).unwrap_or((0, 0, 0, false)))
                        .collect()
                };
                let mut edits: Vec<(usize, u8, u8, u8, bool)> = Vec::new();
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("hubdecor")
                            .num_columns(6)
                            .spacing([10.0, 1.0])
                            .show(ui, |ui| {
                                for i in 0..snap.len() {
                                    let (dx, dy, var, act) = snap[i];
                                    if !act {
                                        continue; // only slots the game actually placed
                                    }
                                    // Slots below HOME_DECOR_OUTSIDE are the outdoor
                                    // garden; the rest are indoor rooms.
                                    let region = if i < HOME_DECOR_OUTSIDE {
                                        "outdoor"
                                    } else {
                                        "indoor"
                                    };
                                    ui.label(format!("[{i}] ({region}) {}", decor_name(var)));
                                    let mut x = dx as i32;
                                    let mut y = dy as i32;
                                    let mut v = var as i32;
                                    let xw = ui.add(egui::DragValue::new(&mut x).clamp_range(0..=255).speed(1));
                                    let yw = ui.add(egui::DragValue::new(&mut y).clamp_range(0..=255).speed(1));
                                    let vw = ui.add(egui::DragValue::new(&mut v).clamp_range(0..=255).speed(1)).on_hover_text(format!(
                                        "decoration id; the game's own enum name is {} (id {} is the in-game 'remove' action, never stored in the save)",
                                        deco_variants::DECOR_VARIANT_NAMES
                                            .get(var as usize)
                                            .copied()
                                            .unwrap_or("(out of range)"),
                                        deco_variants::DECOR_VARIANT_REMOVE
                                    ));
                                    let mut a = act;
                                    let aw = ui.checkbox(&mut a, "on");
                                    if (xw.changed()
                                        || yw.changed()
                                        || vw.changed()
                                        || aw.changed())
                                        && (0..=255).contains(&x)
                                        && (0..=255).contains(&y)
                                        && (0..=255).contains(&v)
                                    {
                                        edits.push((i, x as u8, y as u8, v as u8, a));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                if !edits.is_empty() {
                    if let Some(sf) = self.file.as_mut() {
                        for (i, x, y, v, a) in edits {
                            sf.set_hub_decoration(i, x, y, v, a);
                        }
                    }
                    self.dirty = true;
                }
            });
        });

        ui.collapsing("Difficulty / adventure settings (lab)", |ui| {
            let t_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.diff_toggle_f.n)
                    .map(|k| sf.merged[rb.diff_toggle_f.data + k])
                    .collect()
            };
            let r_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.diff_range_f.n)
                    .map(|k| sf.merged[rb.diff_range_f.data + k])
                    .collect()
            };
            let mut t_edits: Vec<(usize, u8)> = Vec::new();
            let mut r_edits: Vec<(usize, u8)> = Vec::new();
            ui.strong("Toggles");
            egui::Grid::new("difftoggle")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for i in 0..CONFIG_TOGGLES.len().min(rb.diff_toggle_f.n * 8) {
                        let byte = t_snap.get(i / 8).copied().unwrap_or(0);
                        let mut on = byte >> (i % 8) & 1 != 0;
                        if ui.checkbox(&mut on, CONFIG_TOGGLES[i]).changed() {
                            t_edits.push((i, if on { 1 } else { 0 }));
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            // pack bit edits back into byte writes
            if !t_edits.is_empty() {
                let mut bytes = t_snap.clone();
                for (bit, want) in t_edits {
                    let b = &mut bytes[bit / 8];
                    if want == 1 {
                        *b |= 1 << (bit % 8);
                    } else {
                        *b &= !(1 << (bit % 8));
                    }
                }
                if let Some(sf) = self.file.as_mut() {
                    for k in 0..bytes.len().min(rb.diff_toggle_f.n) {
                        sf.set_diff_toggle(k, bytes[k]);
                    }
                }
                self.dirty = true;
            }
            ui.add_space(6.0);
            ui.strong("Ranges");
            for i in 0..rb.diff_range_f.n {
                let v = r_snap[i];
                let label = CONFIG_RANGES.get(i).copied().unwrap_or("?");
                ui.horizontal(|ui| {
                    ui.add_sized([190.0, 0.0], egui::Label::new(label).truncate(true));
                    let opts: &[&str] = match i {
                        3 => BATTLE_FORMATS,
                        5 => GAME_MODES,
                        6 => DIFF_PRESETS,
                        7 => REVISION_MODES,
                        8 => TRAINER_ORDERS,
                        _ => &[],
                    };
                    if opts.is_empty() {
                        let mut n = v as i32;
                        if ui
                            .add(egui::DragValue::new(&mut n).clamp_range(-128..=255))
                            .changed()
                        {
                            r_edits.push((i, n as u8));
                        }
                        if i == 0 {
                            ui.label("(trainer level shift, signed)");
                        }
                    } else {
                        let cur = opts.get(v as usize).copied().unwrap_or("?");
                        egui::ComboBox::from_id_source(format!("rng{i}"))
                            .selected_text(format!("{v} — {cur}"))
                            .show_ui(ui, |ui| {
                                for (o, name) in opts.iter().enumerate() {
                                    if ui.selectable_label(v as usize == o, *name).clicked() {
                                        r_edits.push((i, o as u8));
                                    }
                                }
                            });
                    }
                });
            }
            if !r_edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (i, v) in r_edits {
                        sf.set_diff_range(i, v);
                    }
                }
                self.dirty = true;
            }
        });

        ui.collapsing("Daycare (current)", |ui| {
            let sf = self.file.as_ref().unwrap();
            for i in 0..rb.daycare_f.n {
                let off = rb.daycare_f.data + i * DAYCARE_ELEMSZ;
                let m = Mon::from_bytes(&sf.merged[off..off + BOXSZ]);
                if m.species() == 0 {
                    ui.label(format!("slot {i}: empty"));
                    continue;
                }
                let sel = matches!(self.sel, Some(Sel::BlockDaycare(x)) if x == i);
                if ui
                    .selectable_label(sel, format!("slot {i}: {}", m.display_name()))
                    .clicked()
                {
                    self.sel = Some(Sel::BlockDaycare(i));
                }
            }
            ui.label("Select a slot to edit it like a boxed Pokémon.");
        });

        if rb.ok && rb.safari_f.n > 0 {
            let sf = self.file.as_ref().unwrap();
            let rows: Vec<(usize, SafariMon)> = (0..rb.safari_f.n)
                .filter_map(|i| sf.safari_mon(&rb, i).map(|s| (i, s)))
                .filter(|(_, s)| s.species() != 0 || self.show_empty_safari)
                .collect();
            ui.collapsing(format!("Safari zone roster ({} entries)", rb.safari_f.n), |ui| {
                ui.checkbox(&mut self.show_empty_safari, "show empty slots");
                ui.label("These are the wild safari-zone spawns (species, IVs, nature, ball, shiny, spawn priority).");
                let mut edits: Vec<(usize, SafariMon)> = Vec::new();
                for (i, mut s) in rows {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{i:3}"));
                        let mut sp = s.species();
                        Self::filtered_combo(
                            ui,
                            &format!("saf{i}sp"),
                            &mut self.species_filter,
                            all_species(),
                            sp,
                            &mut |v| sp = v,
                        );
                        if sp != s.species() {
                            s.set_species(sp);
                            edits.push((i, s.clone()));
                        }
                        let mut nick = s.nickname();
                        if ui.add(egui::TextEdit::singleline(&mut nick).desired_width(84.0)).changed() {
                            s.set_nickname(&nick);
                            edits.push((i, s.clone()));
                        }
                        for k in 0..6 {
                            let mut v = s.iv(k) as i32;
                            if ui
                                .add(egui::DragValue::new(&mut v).clamp_range(0..=31).prefix("IV"))
                                .changed()
                            {
                                s.set_iv(k, v as u8);
                                edits.push((i, s.clone()));
                            }
                        }
                        let mut nat = s.nature() as usize;
                        let mut nat_changed = false;
                        egui::ComboBox::from_id_source(format!("saf{i}nat"))
                            .selected_text(NATURES.get(nat).copied().unwrap_or("?"))
                            .show_ui(ui, |ui| {
                                for (k, n) in NATURES.iter().enumerate() {
                                    if ui.selectable_label(nat == k, *n).clicked() {
                                        nat = k;
                                        nat_changed = true;
                                    }
                                }
                            });
                        if nat_changed {
                            s.set_nature(nat as u8);
                            edits.push((i, s.clone()));
                        }
                        let mut sh = s.shiny();
                        if ui.checkbox(&mut sh, "shiny").changed() {
                            s.set_shiny(sh);
                            edits.push((i, s.clone()));
                        }
                        let mut pr = s.priority as i32;
                        if ui
                            .add(egui::DragValue::new(&mut pr).clamp_range(0..=255).prefix("pri "))
                            .changed()
                        {
                            s.priority = pr as u8;
                            edits.push((i, s.clone()));
                        }
                    });
                }
                for (i, s) in edits {
                    self.file.as_mut().unwrap().set_safari_mon(i, &s);
                    self.dirty = true;
                }
            });
        }

        if rb.ok && rb.unique_f.n > 0 {
            let sf = self.file.as_ref().unwrap();
            let rows: Vec<(usize, u32, u16, u16)> = (0..rb.unique_f.n)
                .filter_map(|i| sf.unique_mon(&rb, i).map(|(a, b, c)| (i, a, b, c)))
                .collect();
            ui.collapsing("Dynamic unique-mon spawns", |ui| {
                ui.label("Timed roaming/unique spawns: custom-mon id, species, despawn countdown (steps). id 0 = none pending.");
                for (i, cid, mut sp, mut cd) in rows {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("slot {i}  id {cid}"));
                        let mut s = sp;
                        Self::filtered_combo(
                            ui,
                            &format!("uniq{i}sp"),
                            &mut self.species_filter,
                            all_species(),
                            s,
                            &mut |v| s = v,
                        );
                        if s != sp {
                            sp = s;
                            self.file.as_mut().unwrap().set_unique_mon(i, cid, sp, cd);
                            self.dirty = true;
                        }
                        let mut c = cd as i32;
                        if ui
                            .add(egui::DragValue::new(&mut c).clamp_range(0..=65535).prefix("countdown "))
                            .changed()
                        {
                            cd = c as u16;
                            self.file.as_mut().unwrap().set_unique_mon(i, cid, sp, cd);
                            self.dirty = true;
                        }
                    });
                }
            });
        }

        if rb.stash.ok {
            ui.collapsing("Stashed hub state (sRunRestoreBlock)", |ui| {
                ui.label(format!(
                    "money {}  playTime {}s",
                    rb.stash.money, rb.stash.play_time
                ));
                for (i, m) in rb.stash.party.iter().enumerate() {
                    if m.species() == 0 {
                        continue;
                    }
                    let sel = matches!(self.sel, Some(Sel::StashParty(x)) if x == i);
                    if ui
                        .selectable_label(sel, format!("[{i}] {}", m.display_name()))
                        .clicked()
                    {
                        self.sel = Some(Sel::StashParty(i));
                    }
                }
                for (i, m) in rb.stash.daycare.iter().enumerate() {
                    if m.species() == 0 {
                        continue;
                    }
                    let sel = matches!(self.sel, Some(Sel::StashDaycare(x)) if x == i);
                    if ui
                        .selectable_label(sel, format!("daycare[{i}]: {}", m.display_name()))
                        .clicked()
                    {
                        self.sel = Some(Sel::StashDaycare(i));
                    }
                }
                let mut money = rb.stash.money;
                if ui
                    .add(egui::DragValue::new(&mut money).clamp_range(0..=9_999_999))
                    .changed()
                {
                    self.file.as_mut().unwrap().set_stash_money(money);
                    self.dirty = true;
                }
                // stashed hub bag: quantity here is raw (the bag lives in the Rogue
                // block, not SaveBlock1, so the XOR obfuscation does not apply)
                let bag: Vec<(u16, u16)> = rb.stash.bag.clone();
                let mut bag_edits: Vec<(usize, u16, u16)> = Vec::new();
                ui.add_space(4.0);
                ui.strong("Stashed bag");
                egui::Grid::new("stashbag")
                    .num_columns(3)
                    .spacing([10.0, 2.0])
                    .show(ui, |ui| {
                        for (i, (id, q)) in bag.into_iter().enumerate() {
                            if id == 0 {
                                continue;
                            }
                            ui.monospace(format!("{i:3}"));
                            let mut mid = id;
                            Self::filtered_combo(
                                ui,
                                &format!("sbag{i}"),
                                &mut self.item_filter,
                                all_items(),
                                mid,
                                &mut |v| mid = v,
                            );
                            let mut qq = q;
                            if ui
                                .add(egui::DragValue::new(&mut qq).clamp_range(0..=999))
                                .changed()
                                || mid != id
                            {
                                bag_edits.push((i, mid, qq));
                            }
                            ui.end_row();
                        }
                    });
                if !bag_edits.is_empty() {
                    if let Some(sf) = self.file.as_mut() {
                        for (i, id, q) in bag_edits {
                            sf.set_stash_bag_item(i, id, q);
                        }
                    }
                    self.dirty = true;
                }
            });
        }

        if stash_named {
            ui.separator();
            ui.label(
                "Note: SaveBlock1 currently holds the *in-adventure* party/money; the \
                 stashed block above is the *hub* state (and vice versa when at the hub).",
            );
        }
    }
}
