//! Rogue-tab UI: adventure state, hub map, quests, difficulty, daycare, safari
//! roster, unique mons.

use super::*;
use crate::domain::DecorVariant;
use crate::searchable_combo::Opts;
use crate::tables_extra;

impl Editor {
    /// One Lab-config slot whose value is a named option list. Generic over the
    /// domain so each slot is validated against its own option table: the raw value
    /// on disk stays addressable even when it is outside that table.
    fn typed_range_picker<D: crate::domain::Domain>(
        &self,
        ui: &mut egui::Ui,
        id: impl std::hash::Hash,
        cur: u8,
    ) -> Option<u8> {
        let dom = D::from_raw(cur);
        let mut picked = None;
        egui::ComboBox::from_id_source(id)
            .selected_text(format!(
                "{cur} - {}",
                if dom.is_known() { dom.name() } else { "?" }
            ))
            .show_ui(ui, |ui| {
                for v in crate::domain::values::<D>() {
                    if ui
                        .selectable_label(v.raw() == cur, format!("{} - {}", v.raw(), v.name()))
                        .clicked()
                    {
                        picked = Some(v.raw());
                    }
                }
            });
        if !dom.is_known() {
            ui.colored_label(
                egui::Color32::GOLD,
                format!("stored value {cur} is not one of this slot's options"),
            );
        }
        picked
    }

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
                     expected v2.2.0 layout - showing header only; editing is disabled for this block.",
                    rb.version
                ),
            );
            return;
        }
        // Which state is live? SaveBlock1 always holds the *currently loaded* state. If
        // the stashed hub party is non-empty while SB1 shows more/different mons, the
        // player is mid-adventure.
        let stash_named = rb.stash.ok && rb.stash.party.iter().any(|m| m.species() != 0);
        ui.heading("Rogue adventure state");
        let mut tod = rb.time_of_day;
        // `seasonCounter` stores DAYS, not a season index. SeasonDay derives the season
        // so this row cannot get the units wrong.
        let season_day = self
            .file
            .as_ref()
            .unwrap()
            .season_day(&rb)
            .unwrap_or(crate::domain::SeasonDay::from_raw(rb.season));
        let season = season_day.season();
        let season_idx = season.index() as usize;
        let mut weather = rb.weather;
        ui.horizontal(|ui| {
            ui.label(format!(
                "save stamp {} ({}) · block format {} · {}",
                rb.version,
                rogue_version_name(rb.version),
                rb.format,
                if rb.has_num_species {
                    "header carries lastKnownNumSpecies"
                } else {
                    "pre-2.1.0 header (no lastKnownNumSpecies)"
                }
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
            if ui
                .button("−1 season")
                .on_hover_text("move the day counter back one season (2 days)")
                .clicked()
            {
                if let Some(sf) = self.file.as_mut() {
                    sf.set_season_day(season_day.advanced_seasons(-1));
                }
                self.dirty = true;
            }
            if ui
                .button("+1 season")
                .on_hover_text("move the day counter forward one season (2 days)")
                .clicked()
            {
                if let Some(sf) = self.file.as_mut() {
                    sf.set_season_day(season_day.advanced_seasons(1));
                }
                self.dirty = true;
            }
            egui::ComboBox::from_id_source("season")
                .selected_text(format!(
                    "{} (day {}/{})",
                    season.name(),
                    season_day.day_in_season() + 1,
                    DAYS_PER_SEASON
                ))
                .show_ui(ui, |ui| {
                    for (s, name) in SEASONS.iter().enumerate() {
                        if ui.selectable_label(season_idx == s, *name).clicked() {
                            if let Some(sf) = self.file.as_mut() {
                                sf.set_hub_season(crate::domain::Season::from_index(s as u8));
                            }
                            self.dirty = true;
                        }
                    }
                });
            if ui
                .add(
                    egui::DragValue::new(&mut weather)
                        .clamp_range(0..=65535)
                        .prefix("weather seed "),
                )
                .on_hover_text(
                    "raw u16 seed, not a weather id: the game folds it through % 5 \
                     (spring/summer) or % 6 (autumn/winter) into the weather an outdoor \
                     map shows - Rogue_ModifyOverworldMapWeather, src/rogue_controller.c:1918-2011",
                )
                .changed()
            {
                if let Some(sf) = self.file.as_mut() {
                    sf.set_weather(weather);
                }
                self.dirty = true;
            }
            // The seed alone says nothing; the season decides which weather it maps to, so
            // show that instead of leaving the player to do the modulo.
            ui.label(format!(
                "in {}: {}",
                season.name(),
                crate::game_data::hub_weather_for(season, weather)
            ));
            ui.label(format!("debug {}", rb.is_debug));
        });

        ui.collapsing("Quests", |ui| {
            let n = rb.quests_f.n;
            let base = rb.quests_f.data;
            let qn = tables_extra::quest_names();
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
            // filter text persists across frames in egui's per-Id data store
            let filt_id = egui::Id::new("quest_filter");
            let mut filt = ui
                .ctx()
                .data_mut(|d| d.get_temp::<String>(filt_id))
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("filter:");
                ui.text_edit_singleline(&mut filt);
                ui.label(format!(
                    "{} quest slots (names from quests.json, exporter order)",
                    n
                ));
            });
            ui.ctx().data_mut(|d| d.insert_temp(filt_id, filt.clone()));
            let f = filt.to_lowercase();
            let mut edits: Vec<(usize, u16, u8)> = Vec::new();
            Self::wrap_scroll_area().max_height(340.0).show(ui, |ui| {
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
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add_sized([240.0, 0.0], egui::Label::new(name).truncate(true));
                        });
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
            // The map first: click/drag buildings on their tiles. The numeric grid below
            // stays for exact coordinates and for the raw pad bytes.
            self.hub_map_visual(ui, &rb);
            ui.add_space(8.0);
            let area_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.built_f.n)
                    .map(|k| sf.merged[rb.built_f.data + k])
                    .collect()
            };
            let coord_snap: Vec<(i8, i8)> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.coords_f.n)
                    .map(|k| sf.hub_area_coord_at(&rb, k).unwrap_or((0, 0)))
                    .collect()
            };
            let built_snap: Vec<bool> = {
                let sf = self.file.as_ref().unwrap();
                (0..HUB_AREAS.len())
                    .map(|k| sf.hub_area_built_at(&rb, k))
                    .collect()
            };
            let coord_raw_snap: Vec<Option<[u8; 4]>> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.coords_f.n)
                    .map(|k| sf.hub_area_coord_raw(&rb, k))
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
                        // The raw bytes are shown so the two padding bytes of the game's padded
                        // `Coords8` are visible, not assumed zero.
                        let raw = match coord_raw_snap.get(i).copied().flatten() {
                            Some([rx, ry, p1, p2]) => {
                                format!(" raw {rx:02X} {ry:02X} {p1:02X} {p2:02X}")
                            }
                            None => " raw ?".to_string(),
                        };
                        let adj = if built_snap.get(i).copied().unwrap_or(false) {
                            format!("({ox},{oy}){raw}")
                        } else {
                            format!("({ox},{oy}) unbuilt{raw}")
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
                    for w in sf.hub_coord_warnings_at(&rb, i, x, y) {
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
                        let mut on = self
                            .file
                            .as_ref()
                            .unwrap()
                            .hub_upgrade_on(&rb, crate::domain::Upgrade::from_raw(i as u8));
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
                        sf.set_hub_area(crate::domain::HubArea::from_raw(i as u8), on);
                    }
                    for (i, on) in up_edits {
                        sf.set_hub_upgrade(crate::domain::Upgrade::from_raw(i as u8), on);
                    }
                    for (i, x, y) in coord_edits {
                        sf.set_hub_area_coord(crate::domain::HubArea::from_raw(i as u8), x, y);
                    }
                }
                self.dirty = true;
            }
        });

        ui.collapsing("House (decorations, styles, follow-mons, weather)", |ui| {
            // Placement by tile grid; the slot table below it stays for exact edits.
            self.house_visual(ui, &rb);
            ui.add_space(8.0);
            let style_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                match sf.home_styles(&rb) {
                    Some((ext, int, path)) => vec![ext.raw(), int.raw(), path.raw()],
                    None => (0..rb.styles_f.n).map(|k| sf.hub_style(&rb, k).unwrap_or(0)).collect(),
                }
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
            // One picker for all three slots, generic over the domain trait: each slot
            // offers its own table's values, so an outdoor value can never be offered for
            // the indoor slot.
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
                "Raw u16 seed, not a weather id. Rogue_ModifyOverworldMapWeather \
                 (src/rogue_controller.c:1918-2011) derives the visible weather as \
                 seed % 5 in spring/summer and seed % 6 in autumn/winter.",
            );
            let mut wsv = weather_snap as i32;
            if ui
                .add(egui::DragValue::new(&mut wsv).clamp_range(0..=65535).speed(1))
                .changed()
                && (0..=65535).contains(&wsv)
            {
                weather_edit = Some(wsv as u16);
            }
            // The buckets are only meaningful as weathers, so name them rather than
            // printing `% 5` / `% 6` results the player has to look up.
            ui.label(format!(
                "seed {}: spring {} / summer {} / autumn {} / winter {}",
                weather_snap,
                crate::game_data::hub_weather_for(crate::domain::Season::Spring, weather_snap),
                crate::game_data::hub_weather_for(crate::domain::Season::Summer, weather_snap),
                crate::game_data::hub_weather_for(crate::domain::Season::Autumn, weather_snap),
                crate::game_data::hub_weather_for(crate::domain::Season::Winter, weather_snap),
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
                        match i {
                            0 => sf.set_exterior_style(crate::domain::ExteriorStyle::from_raw(v)),
                            1 => sf.set_interior_style(crate::domain::InteriorStyle::from_raw(v)),
                            2 => sf.set_path_style(crate::domain::PathStyle::from_raw(v)),
                            // a save with more style slots than the three the game defines: write
                            // it raw rather than dropping the edit
                            _ => sf.set_hub_style(i, v),
                        }
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
                let snap: Vec<(u8, u8, crate::domain::DecorVariant, bool)> = {
                    let sf = self.file.as_ref().unwrap();
                    (0..rb.decor_f.n)
                        .map(|k| sf.hub_decoration(&rb, k).unwrap_or((0, 0, crate::domain::DecorVariant::from_raw(0), false)))
                        .collect()
                };
                let mut edits: Vec<(usize, u8, u8, crate::domain::DecorVariant, bool)> = Vec::new();
                // The one region that keeps a sideways scrollbar: a six-column table of prop
                // rows cannot wrap without redesigning the table, so it pans inside its own
                // frame rather than making the page scroll.
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .max_height(240.0)
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
                                    // Slots below HOME_DECOR_OUTSIDE are the outdoor garden; the
                                    // rest are indoor rooms.
                                    let region = if i < HOME_DECOR_OUTSIDE {
                                        "outdoor"
                                    } else {
                                        "indoor"
                                    };
                                    ui.label(format!("[{i}] ({region}) {}", decor_name(var.raw())));
                                    let mut x = dx as i32;
                                    let mut y = dy as i32;
                                    let mut v = var.raw() as i32;
                                    let xw = ui.add(egui::DragValue::new(&mut x).clamp_range(0..=255).speed(1));
                                    let yw = ui.add(egui::DragValue::new(&mut y).clamp_range(0..=255).speed(1));
                                    let vw = ui.add(egui::DragValue::new(&mut v).clamp_range(0..=255).speed(1)).on_hover_text(format!(
                                        "decoration id; the game's own enum name is {} (id {} is the in-game 'remove' action, never stored in the save)",
                                        deco_variants::DECOR_VARIANT_NAMES
                                            .get(var.raw() as usize)
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
                                        edits.push((i, x as u8, y as u8, DecorVariant::from_raw(v as u8), a));
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
            let t_snap: Vec<bool> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.diff_toggle_f.n * 8)
                    .map(|i| {
                        sf.diff_toggle_bit(&rb, crate::domain::ConfigToggle::from_raw(i as u8))
                    })
                    .collect()
            };
            let r_snap: Vec<u8> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.diff_range_f.n)
                    .map(|k| sf.merged[rb.diff_range_f.data + k])
                    .collect()
            };
            let mut t_edits: Vec<(crate::domain::ConfigToggle, bool)> = Vec::new();
            let mut r_edits: Vec<(crate::domain::ConfigRange, u8)> = Vec::new();
            ui.strong("Toggles");
            egui::Grid::new("difftoggle")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .show(ui, |ui| {
                    for (i, t) in crate::domain::values::<crate::domain::ConfigToggle>()
                        .into_iter()
                        .enumerate()
                    {
                        if i >= rb.diff_toggle_f.n * 8 {
                            break;
                        }
                        let mut on = *t_snap.get(i).unwrap_or(&false);
                        if ui.checkbox(&mut on, t.name()).changed() {
                            t_edits.push((t, on));
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            if !t_edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (t, on) in t_edits {
                        sf.set_diff_toggle_bit(t, on);
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
                    // Which value domain applies is a property of the slot, not a literal at the
                    // call site (`ConfigRange::options`).
                    let range = crate::domain::ConfigRange::from_raw(i as u8);
                    let opts = range.options();
                    if opts.is_empty() {
                        let mut n = v as i32;
                        if ui
                            .add(egui::DragValue::new(&mut n).clamp_range(-128..=255))
                            .changed()
                        {
                            r_edits.push((range, n as u8));
                        }
                        if range.is_signed() {
                            ui.label("(trainer level shift, signed)");
                        }
                    } else {
                        let picked = match i {
                            3 => self.typed_range_picker::<crate::domain::BattleFormat>(
                                ui,
                                format!("rng{i}"),
                                v,
                            ),
                            5 => self.typed_range_picker::<crate::domain::GameMode>(
                                ui,
                                format!("rng{i}"),
                                v,
                            ),
                            6 => self.typed_range_picker::<crate::domain::DiffPreset>(
                                ui,
                                format!("rng{i}"),
                                v,
                            ),
                            7 => self.typed_range_picker::<crate::domain::RevisionMode>(
                                ui,
                                format!("rng{i}"),
                                v,
                            ),
                            8 => self.typed_range_picker::<crate::domain::TrainerOrder>(
                                ui,
                                format!("rng{i}"),
                                v,
                            ),
                            _ => None,
                        };
                        if let Some(nv) = picked {
                            r_edits.push((range, nv));
                        }
                    }
                });
            }
            if !r_edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (range, v) in r_edits {
                        sf.set_diff_range(range, v);
                    }
                }
                self.dirty = true;
            }
        });

        ui.collapsing("Daycare (current)", |ui| {
            // Snapshot first (the save must not stay borrowed while the row helper mutates
            // selection), then draw through the shared mon_row helper: single click =
            // select + Mon tab, double click = popup editor. The old rows only selected on
            // click and offered no double-click path at all -- the reported "double tapping
            // pokemon in the daycare opens nothing" bug.
            let rows: Vec<(usize, Option<String>)> = {
                let sf = self.file.as_ref().unwrap();
                (0..rb.daycare_f.n)
                    .map(|i| {
                        let off = rb.daycare_f.data + i * DAYCARE_ELEMSZ;
                        let m = Mon::from_bytes(&sf.merged[off..off + BOXSZ]);
                        (i, (m.species() != 0).then(|| m.display_name()))
                    })
                    .collect()
            };
            for (i, dn) in rows {
                if let Some(dn) = dn {
                    self.mon_row(ui, Sel::BlockDaycare(i), format!("slot {i}: {dn}"));
                } else {
                    ui.label(format!("slot {i}: empty"));
                }
            }
            ui.label(
                "Single-click a slot to edit it in the Mon tab; double-click opens the popup.",
            );
        });

        if rb.ok && rb.safari_f.n > 0 {
            let sf = self.file.as_ref().unwrap();
            let rows: Vec<(usize, SafariMon)> = (0..rb.safari_f.n)
                .filter_map(|i| sf.safari_mon(&rb, i).map(|s| (i, s)))
                .filter(|(_, s)| s.species() != 0 || self.show_empty_safari)
                .collect();
            ui.collapsing(
                format!("Safari zone roster ({} entries)", rb.safari_f.n),
                |ui| {
                    ui.checkbox(&mut self.show_empty_safari, "show empty slots");
                    ui.label(
                    "These are the wild safari-zone spawns (species, IVs, nature, ball, shiny, \
                     spawn priority). Spawn priority is edited right on the row; single-click a row to
                     edit that entry's species/IVs/nature/ball/ability/gender in the Mon tab -- \
                     the same editor every other Pokemon uses; double-click opens the floating \
                     editor.",
                );
                    for (i, s) in rows {
                        let (row, pr) = self.safari_row(ui, i, &s);
                        let _ = row;
                        // Spawn priority is the one field that stays in the row: it is safari-zone
                        // data (which spawn the game draws first), not a property of the Pokemon,
                        // so the generic mon editor has no section for it. Commit through the same
                        // setter the old inline roster used; the record's other bytes are copied
                        // untouched.
                        if let Some(pr) = pr {
                            let mut c = s.clone();
                            c.priority = pr;
                            if let Some(sf) = self.file.as_mut() {
                                sf.set_safari_mon(i, &c);
                                self.dirty = true;
                            }
                        }
                    }
                },
            );
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
                        if let Some(v) = self.combo(
                            ui,
                            &format!("uniq{i}sp"),
                            species_picker_table(),
                            s,
                            &Opts::pokemon(),
                        ) {
                            s = v;
                        }
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
                    self.mon_row(
                        ui,
                        Sel::StashParty(i),
                        format!("[{i}] {}", m.display_name()),
                    );
                }
                for (i, m) in rb.stash.daycare.iter().enumerate() {
                    if m.species() == 0 {
                        continue;
                    }
                    self.mon_row(
                        ui,
                        Sel::StashDaycare(i),
                        format!("daycare[{i}]: {}", m.display_name()),
                    );
                }
                let mut money = self.file.as_ref().unwrap().stash_money();
                if ui
                    .add(egui::DragValue::new(&mut money).clamp_range(0..=9_999_999))
                    .changed()
                {
                    self.file.as_mut().unwrap().set_stash_money(money);
                    self.dirty = true;
                }
                // stashed hub bag: quantity here is raw (the bag lives in the Rogue block, not
                // SaveBlock1, so the XOR obfuscation does not apply)
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
                            if let Some(v) =
                                self.combo(ui, &format!("sbag{i}"), all_items(), mid, &Opts::item())
                            {
                                mid = v;
                            }
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

    /// One compact safari row: index, species icon, species name (plus a short
    /// shiny marker). Everything else the entry stores -- nickname, the six IVs,
    /// nature, ball, ability slot, gender flag, spawn priority -- lives in the
    /// shared Mon-tab editor, which these rows open through the same click plumbing
    /// as the roster and daycare rows (`apply_mon_row_click`). A row carrying
    /// twelve inline widgets made a 60-row list unreadable, and the editor already
    /// has those widgets. Returns the row's response so a caller (or a test) can
    /// measure where the clickable row actually landed.
    pub(crate) fn safari_row(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        s: &SafariMon,
    ) -> (egui::Response, Option<u8>) {
        let sel = Sel::Safari(i);
        let selected = self.sel.as_ref() == Some(&sel);
        let sp = s.species();
        // `species_name` is exactly what the Mon-tab heading prints for a spawn entry
        // (`MonRec::heading`), so the row and the editor cannot disagree.
        let name = if sp == 0 {
            "(empty)".to_string()
        } else {
            species_name(sp).to_string()
        };
        let shiny = if s.shiny() { " [shiny]" } else { "" };
        // `icons` is borrowed for the whole row, so the row may not touch the rest of
        // self: the closure hands back the row's response and the shared
        // click/double-click semantics are applied after it, exactly as the roster rows
        // do (a row of buttons would swallow `clicked`).
        let (row, pr) = {
            let icons = &self.icons;
            ui.horizontal(|ui| {
                let mut pr_edit = None;
                let ctx = ui.ctx().clone();
                icons::draw_icon(ui, icons, &ctx, IconKind::Pokemon, sp, &name, 22.0);
                ui.monospace(format!("{i:>3}"));
                let lab = ui
                    .selectable_label(selected, format!("{name}{shiny}"))
                    .on_hover_text(format!("stored species id {sp}"));
                // Editable here, in the roster: priorityCounter decides which of these spawns
                // the game draws first. The label still owns the click/select semantics; the
                // DragValue is a separate widget.
                let mut pr = s.priority as i32;
                if ui
                    .add(
                        egui::DragValue::new(&mut pr)
                            .clamp_range(0..=255)
                            .speed(1)
                            .suffix(" pri"),
                    )
                    .on_hover_text(
                        "priorityCounter: the game draws higher-priority spawns first. \
                             Safari-zone data, so it is edited here rather than in the mon editor.",
                    )
                    .changed()
                {
                    pr_edit = Some(pr as u8);
                }
                (lab, pr_edit)
            })
            .inner
        };
        self.apply_mon_row_click(&row, sel);
        (row, pr)
    }
}
