//! Central Pokemon editor pane (fields of the selected mon).
//!
//! One editor, two record shapes. `MonRec` adapts the 104-byte party/box/fusion/
//! daycare `Mon` and the 20-byte Rogue-block safari spawn entry (`SafariMon`) to
//! the SAME field widgets: `mon_shared_fields_ui` draws what both shapes store,
//! and each shape then draws only what its own bytes can actually hold
//! (`mon_full_fields_ui` / `mon_safari_fields_ui`). A section the shape does not
//! store gets no widget at all -- `supported_sections` is what says so -- because
//! a widget with nowhere to write would rewrite a neighbouring field.

use super::*;
use crate::basestats::base_stats;
use crate::searchable_combo::Opts;
use crate::tables::MOVES;

/// One part of the mon editor, so a record shape can name (or refuse) it.
/// Hiding a section is a correctness statement, not cosmetics: an editor widget
/// for a field the record does not store has nowhere to put its value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MonSection {
    // Stored by BOTH record shapes (the shared field UI).
    Species,
    Nickname,
    Ball,
    Nature,
    Shiny,
    Gender,
    Ability,
    Ivs,
    // Only the 104-byte box/party record has bytes for these.
    HeldItem,
    LevelExp,
    OtIdentity,
    PidChecksum,
    Flags,
    Evs,
    Moves,
    MetInfo,
    CachedStats,
    // Only the Rogue-block spawn entry has this one.
}

/// What the 104-byte `Mon` record stores.
const FULL_SECTIONS: &[MonSection] = &[
    MonSection::Species,
    MonSection::Nickname,
    MonSection::Ball,
    MonSection::Nature,
    MonSection::Shiny,
    MonSection::Gender,
    MonSection::Ability,
    MonSection::Ivs,
    MonSection::HeldItem,
    MonSection::LevelExp,
    MonSection::OtIdentity,
    MonSection::PidChecksum,
    MonSection::Flags,
    MonSection::Evs,
    MonSection::Moves,
    MonSection::MetInfo,
    MonSection::CachedStats,
];

/// What a 20-byte Rogue-block safari spawn entry stores: `w0` is
/// species:16 | hpIV:5 | atkIV:5 | defIV:5 | shiny:1, `w1` is
/// speedIV:5 | spAtkIV:5 | spDefIV:5 | pokeball:5 | nature:5 | abilityNum:2 |
/// genderFlag:1, then nickname[10], priorityCounter, customMonLookup. There is
/// no room -- and no need -- for level/EXP, EVs, moves, OT/ID, PID or stats:
/// the game builds the wild mon from scratch and copies these fields in.
const SAFARI_SECTIONS: &[MonSection] = &[
    MonSection::Species,
    MonSection::Nickname,
    MonSection::Ball,
    MonSection::Nature,
    MonSection::Shiny,
    MonSection::Gender,
    MonSection::Ability,
    MonSection::Ivs,
];

/// The selected record, whichever shape it is. This is the ONLY thing the mon
/// editor draws against, so the field widgets exist once.
#[derive(Clone)]
pub(crate) enum MonRec {
    Full(Mon),
    Safari(SafariMon),
}

impl MonRec {
    pub(crate) fn species(&self) -> u16 {
        match self {
            MonRec::Full(m) => m.species(),
            MonRec::Safari(s) => s.species(),
        }
    }
    pub(crate) fn set_species(&mut self, v: u16) {
        match self {
            MonRec::Full(m) => m.set_species(v),
            MonRec::Safari(s) => s.set_species(v),
        }
    }
    pub(crate) fn nickname(&self) -> String {
        match self {
            MonRec::Full(m) => m.nickname(),
            MonRec::Safari(s) => s.nickname(),
        }
    }
    pub(crate) fn set_nickname(&mut self, s: &str) {
        match self {
            MonRec::Full(m) => m.set_nickname(s),
            MonRec::Safari(rec) => rec.set_nickname(s),
        }
    }
    /// The ball as the game stores it: an item id. `Mon` keeps it in a byte,
    /// the spawn entry in a 5-bit field (masked by its own setter).
    pub(crate) fn ball(&self) -> u16 {
        match self {
            MonRec::Full(m) => m.ball() as u16,
            MonRec::Safari(s) => s.pokeball().raw(),
        }
    }
    pub(crate) fn set_ball(&mut self, v: u16) {
        match self {
            MonRec::Full(m) => m.set_ball(v as u8),
            MonRec::Safari(s) => s.set_pokeball(crate::domain::Ball::from_raw(v)),
        }
    }
    pub(crate) fn nature(&self) -> crate::domain::Nature {
        match self {
            MonRec::Full(m) => m.nature(),
            MonRec::Safari(s) => s.nature(),
        }
    }
    pub(crate) fn set_nature(&mut self, n: crate::domain::Nature) {
        match self {
            MonRec::Full(m) => m.set_nature(n),
            MonRec::Safari(s) => s.set_nature(n),
        }
    }
    pub(crate) fn ability_slot(&self) -> crate::domain::AbilitySlot {
        match self {
            MonRec::Full(m) => m.ability_slot(),
            MonRec::Safari(s) => s.ability_slot(),
        }
    }
    pub(crate) fn set_ability_slot(&mut self, a: crate::domain::AbilitySlot) {
        match self {
            MonRec::Full(m) => m.set_ability_slot(a),
            MonRec::Safari(s) => s.set_ability_slot(a),
        }
    }
    pub(crate) fn gender_flag(&self) -> crate::domain::GenderFlag {
        match self {
            MonRec::Full(m) => m.gender(),
            MonRec::Safari(s) => s.gender(),
        }
    }
    pub(crate) fn set_gender_flag(&mut self, g: crate::domain::GenderFlag) {
        match self {
            MonRec::Full(m) => m.set_gender(g),
            MonRec::Safari(s) => s.set_gender(g),
        }
    }
    pub(crate) fn shiny(&self) -> bool {
        match self {
            MonRec::Full(m) => m.is_shiny(),
            MonRec::Safari(s) => s.shiny(),
        }
    }
    pub(crate) fn set_shiny(&mut self, v: bool) {
        match self {
            MonRec::Full(m) => m.set_shiny(v),
            MonRec::Safari(s) => s.set_shiny(v),
        }
    }
    pub(crate) fn iv(&self, st: crate::domain::StatIdx) -> u8 {
        match self {
            MonRec::Full(m) => m.iv(st),
            MonRec::Safari(s) => s.iv(st),
        }
    }
    pub(crate) fn set_iv(&mut self, st: crate::domain::StatIdx, v: u8) {
        match self {
            MonRec::Full(m) => m.set_iv(st, v),
            MonRec::Safari(s) => s.set_iv(st, v),
        }
    }
    /// All six IVs at once ("31 all" in the editor, "31 IVs" in the popup).
    /// `Mon` has one dedicated setter for its packed word; the spawn entry has
    /// six separate 5-bit fields, so its own accessors get the same value one
    /// field at a time.
    pub(crate) fn set_all_ivs(&mut self, v: u8) {
        match self {
            MonRec::Full(m) => m.set_all_ivs(v),
            MonRec::Safari(s) => {
                for st in crate::domain::StatIdx::ALL {
                    s.set_iv(st, v);
                }
            }
        }
    }

    /// Which editor sections this shape stores; anything else stays hidden.
    pub(crate) fn supported_sections(&self) -> &'static [MonSection] {
        match self {
            MonRec::Full(_) => FULL_SECTIONS,
            MonRec::Safari(_) => SAFARI_SECTIONS,
        }
    }
    pub(crate) fn supports(&self, s: MonSection) -> bool {
        self.supported_sections().contains(&s)
    }

    /// Editor heading for this shape. The spawn entry has no level to show and
    /// its species name must read exactly like the Rogue tab's roster row.
    pub(crate) fn heading(&self) -> String {
        match self {
            MonRec::Full(m) => m.display_name(),
            MonRec::Safari(s) => {
                let sp = species_name(s.species());
                if s.species() == 0 {
                    return "(empty safari slot)".into();
                }
                let nick = s.nickname();
                if !nick.is_empty() && nick != sp {
                    format!("{nick} ({sp})")
                } else {
                    sp.into()
                }
            }
        }
    }

    /// Run once after an edit. `Mon` re-seals (substruct permutation, checksum,
    /// encryption) exactly as the editor always has; `SafariMon`'s accessors
    /// already wrote its two words, so there is nothing extra to seal.
    pub(crate) fn commit(&mut self) {
        match self {
            MonRec::Full(m) => m.seal(),
            MonRec::Safari(_) => {}
        }
    }

    pub(crate) fn full(&self) -> Option<&Mon> {
        match self {
            MonRec::Full(m) => Some(m),
            _ => None,
        }
    }
    pub(crate) fn full_mut(&mut self) -> Option<&mut Mon> {
        match self {
            MonRec::Full(m) => Some(m),
            _ => None,
        }
    }
    pub(crate) fn safari(&self) -> Option<&SafariMon> {
        match self {
            MonRec::Safari(s) => Some(s),
            _ => None,
        }
    }
    // No mutable safari view on purpose: the one spawn-only field (spawn
    // priority) is edited on the Safari Zone roster row and commits through
    // `SaveFile::set_safari_mon`, so nothing here mutates the 20-byte record.
}

impl Editor {
    pub(crate) fn mon_editor_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut rec) = self.selected_rec() else {
            ui.heading("No Pokémon selected");
            return;
        };
        let mut changed = false;

        // Identity header: only the box record has a PID/OTID/checksum and a
        // flag byte. The spawn entry is a template, spelled out below.
        if rec.supports(MonSection::PidChecksum) {
            let Some(m) = rec.full() else {
                return;
            };
            ui.heading(m.display_name());
            // The form as its own detail: this ROM's species name embeds it
            // ("Darmanitan Standard Mode"), and the heading above shows only
            // the base name, so the form must be spelled out here.
            if let Some(label) = form_label(m.species()) {
                ui.label(format!("Form: {label}"));
            }
            let ck = m.checksum_ok();
            ui.label(format!(
                "PID {:08X}  OTID {:08X}  checksum {} ({})",
                m.pid(),
                m.otid(),
                m.checksum_stored(),
                if ck { "OK" } else { "CORRUPT" },
            ));
            // Flag byte (0x13 in the party struct): bad-egg / has-species /
            // is-egg. Shown rather than edited because the game only ever
            // sets them as a side effect of hatching or receiving a mon.
            ui.label(format!(
                "flags: {}has-species={} is-egg={}",
                if m.is_bad_egg() { "BAD EGG, " } else { "" },
                m.has_species(),
                m.is_egg_boxflag(),
            ));
        } else if let Some(s) = rec.safari() {
            ui.heading(rec.heading());
            ui.colored_label(
                egui::Color32::from_rgb(120, 190, 255),
                "Rogue-block safari spawn entry (20 bytes): a wild-spawn template, not a caught \
                 Pokemon. It stores no level/EXP, EVs, moves, OT/ID, PID or stats, so those \
                 sections are hidden here instead of being invented.",
            );
            // The species picker's selected text is only as good as its id
            // list: Rogue form ids have no vanilla table entry, so name the
            // id that is actually on disk.
            ui.label(format!(
                "stored species id {} reads {}",
                s.species(),
                species_name(s.species())
            ));
        }
        ui.separator();

        // The field widgets both shapes share, drawn once either way.
        changed |= self.mon_shared_fields_ui(ui, &mut rec);
        // ...then only the sections this shape's own bytes can store. Which
        // those are is the record's answer, not a match on the enum.
        if rec.supports(MonSection::LevelExp) {
            if let Some(m) = rec.full_mut() {
                changed |= self.mon_full_fields_ui(ui, m);
            }
        }

        if changed {
            rec.commit();
            // Safari records go back through `SaveFile::set_safari_mon`, the same
            // commit path the Rogue tab's inline rows used; see `apply_rec`.
            self.apply_rec(&rec);
        }
    }

    /// The field widgets BOTH record shapes store, drawn once for either of
    /// them: species, nickname, ball, nature, shiny, gender, ability slot and
    /// the six IVs. `SafariMon` packs all eight into its two bitfield words
    /// through its own accessors, so nothing here has to know which shape it is
    /// driving.
    fn mon_shared_fields_ui(&mut self, ui: &mut egui::Ui, rec: &mut MonRec) -> bool {
        let mut changed = false;

        egui::Grid::new("mongrid")
            .num_columns(4)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Species");
                if let Some(id) = self.combo(
                    ui,
                    "species",
                    species_picker_table(),
                    rec.species(),
                    &Opts::pokemon(),
                ) {
                    rec.set_species(id);
                    changed = true;
                }
                ui.end_row();

                ui.label("Nickname");
                let mut nick = rec.nickname();
                if ui.add(egui::TextEdit::singleline(&mut nick)).changed() {
                    rec.set_nickname(&nick);
                    changed = true;
                }
                ui.label("Ball");
                let mut ball = rec.ball();
                ui.horizontal(|ui| {
                    // Name, not id, and searchable - the request was that the ball
                    // menu say which ball the mon was caught in, using the same
                    // picker class as every other list. Ball ids live in
                    // include/constants/items.h (a ball IS an item to the game), so the
                    // picker uses the ITEM icon namespace: the stored id is the item id,
                    // which is also the icon key, so each row shows that ball's own
                    // sprite instead of a bare name.
                    if let Some(id) = self.combo(
                        ui,
                        "ball",
                        crate::game_data::balls(),
                        ball,
                        &Opts::item()
                            .compact()
                            .with_hint("filter balls")
                            .with_none("(no ball)"),
                    ) {
                        ball = id;
                        rec.set_ball(ball);
                        changed = true;
                    }
                    // Raw value stays editable: Emerald Rogue uses ball ids the
                    // stock list does not cover.
                    if ui
                        .add(egui::DragValue::new(&mut ball).clamp_range(0..=31))
                        .on_hover_text(format!(
                            "raw ball id (the game's own field). Known name for {ball}: {}",
                            crate::game_data::ball_name(ball as u8)
                        ))
                        .changed()
                    {
                        rec.set_ball(ball);
                        changed = true;
                    }
                });
                ui.end_row();

                ui.label("Nature");
                // 25 values, no useful ids: same picker as everything else, with
                // the numbers hidden (nature 7 means nothing; "Relaxed" does).
                let nat = rec.nature();
                if let Some(n) = self.combo(
                    ui,
                    "nature",
                    crate::tables::NATURE_IDS,
                    nat.raw() as u16,
                    &Opts::named().no_id().compact(),
                ) {
                    rec.set_nature(crate::domain::Nature::from_raw(n as u8));
                    changed = true;
                }
                if !nat.is_known() {
                    // pid%25 XOR the modifier can exceed 24. Shown rather than
                    // clamped, because clamping would rewrite the mon.
                    ui.colored_label(
                        egui::Color32::GOLD,
                        format!(
                            "nature value {} is outside 0..=24 - left exactly as stored",
                            nat.raw()
                        ),
                    );
                }
                ui.label("Shiny");
                let mut sh = rec.shiny();
                if ui.checkbox(&mut sh, "").changed() {
                    rec.set_shiny(sh);
                    changed = true;
                }
                // Gender: `genderFlag` is only decisive for variable-ratio
                // species (Rogue forces those to exactly 50% female, so the flag
                // alone decides and no pid comparison happens). A fixed-gender
                // species ignores the flag entirely, so the picker lists what the
                // flag MEANS for this species instead of offering a Male/Female
                // choice the game throws away -- that is why the old row that
                // claimed Metagross (flag 7, MON_GENDERLESS) was "Female (7)"
                // cannot happen through this list.
                let sp = rec.species();
                let gflag = rec.gender_flag();
                let gender_opts = crate::lookup::gender_flag_options(sp, gflag.raw());
                ui.label("Gender");
                if let Some(g) = self.combo(
                    ui,
                    "gender",
                    &gender_opts,
                    gflag.raw() as u16,
                    &Opts::named().compact().with_hint("filter"),
                ) {
                    let g = crate::domain::GenderFlag::from_raw(g as u8);
                    if g != rec.gender_flag() {
                        rec.set_gender_flag(g);
                        changed = true;
                    }
                }
                if let Some(g) = crate::lookup::gender_for_species(sp, gflag.raw()) {
                    // The word the game itself shows for this species and flag,
                    // spelled out next to the picker: the picker names what the
                    // flag MEANS, which is not always the same sentence.
                    ui.label(format!("the game shows this one as {}", g.label()));
                }
                if !gflag.is_expected() {
                    // Shown, never rewritten: a flag outside 0/1 means this save's
                    // struct layout is not what the editor assumed.
                    ui.colored_label(
                        egui::Color32::GOLD,
                        format!(
                            "stored gender flag {} is not 0/1 - this save's struct layout does \
                             not match what the editor assumed",
                            gflag.raw()
                        ),
                    );
                }
                // `abilityNum` is a slot index into the species' ability list, so
                // it is meaningless without the species: the picker lists the
                // abilities THIS species actually has, labelled by the ability
                // name, and NUM_ABILITY_SLOTS (3 here: 2 normal + 1 hidden) so a
                // hidden ability is reachable. An odd stored slot is still listed,
                // so a save we are misreading shows up instead of being rewritten.
                ui.label("Ability");
                let slot = rec.ability_slot();
                let abil_opts = crate::lookup::ability_slot_options(sp, slot.raw());
                if let Some(a) = self.combo(
                    ui,
                    "ability",
                    &abil_opts,
                    slot.raw() as u16,
                    &Opts::named().compact().with_hint("filter abilities"),
                ) {
                    let a = crate::domain::AbilitySlot::from_raw(a as u8);
                    if a != slot {
                        rec.set_ability_slot(a);
                        changed = true;
                    }
                }
                if let crate::domain::AbilitySlot::Unexpected(v) = slot {
                    // `ability_label` resolves the slot against this species, so
                    // the note says what the game would find there rather than
                    // just that the number is odd.
                    ui.colored_label(
                        egui::Color32::GOLD,
                        format!(
                            "stored ability slot {v} is outside this species' list - left \
                             exactly as stored ({})",
                            crate::lookup::ability_label(sp, v)
                        ),
                    );
                }
                ui.end_row();
            });

        // StatIdx labels the six columns; both shapes pack them in struct order
        // (hp, atk, def, spe, spa, spd), so the label list cannot silently
        // disagree with the column it prints.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("IVs");
            for st in crate::domain::StatIdx::ALL {
                let nm = st.label();
                let mut v = rec.iv(st);
                if ui
                    .add(
                        egui::DragValue::new(&mut v)
                            .clamp_range(0..=31)
                            .prefix(format!("{nm}: ")),
                    )
                    .changed()
                {
                    rec.set_iv(st, v);
                    changed = true;
                }
            }
            if ui.button("31 all").clicked() {
                rec.set_all_ivs(31);
                changed = true;
            }
        });
        changed
    }

    /// The sections only the 104-byte box record stores: held item, OT,
    /// level/EXP/friendship, EVs, moves, met info and the cached stat block. A
    /// safari spawn entry has no bytes for any of them, so the editor offers
    /// none of them there (see `SAFARI_SECTIONS`).
    fn mon_full_fields_ui(&mut self, ui: &mut egui::Ui, m: &mut Mon) -> bool {
        let mut changed = false;

        ui.add_space(6.0);
        egui::Grid::new("mongrid_extra")
            .num_columns(4)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Held item");
                if let Some(id) = self.combo(ui, "item", all_items(), m.held_item(), &Opts::item().with_none("(none)")) {
                    m.set_held_item(id);
                    changed = true;
                }
                ui.label("OT");
                ui.label(m.ot_name());
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
                ui.end_row();
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
                    // its text field can actually be clicked - the bug that made
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
                            ui.strong(format!("PP - base {base}"));
                            for (ups, val) in ladder.iter() {
                                let lbl = format!(
                                    "{} PP up{} -> {val}",
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
                    let nat = m.nature();
                    // The nature modifier table has 25 rows; an out-of-range
                    // nature has no row, so recalculation cannot be done
                    // correctly and is refused rather than guessed at.
                    let nat_idx = if nat.is_known() {
                        Some(nat.raw() as usize)
                    } else {
                        None
                    };
                    match (nat_idx, base_stats(m.species())) {
                        (None, _) => {
                            self.mon_note = Some(format!(
                                "nature value {} is outside 0..=24 - stats NOT recalculated",
                                nat.raw()
                            ));
                        }
                        (Some(_), None) => {
                            self.mon_note = Some(format!(
                                "no base stats in table for species {} - stats NOT recalculated",
                                m.species()
                            ));
                        }
                        (Some(nat), Some(bs)) => {
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
                    }
                }
                if let Some(note) = &self.mon_note {
                    ui.colored_label(egui::Color32::GOLD, note);
                }
            });
        }
        changed
    }
}

/// Every glyph the last frame painted, concatenated. Test-only: it is how the
/// UI tests assert what the user would actually see (a widget that exists but
/// is not painted, or is painted with the wrong name, still fails).
#[cfg(test)]
pub(crate) fn painted_text(out: &egui::FullOutput) -> String {
    fn walk(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Vec(v) => {
                for s in v {
                    walk(s, out);
                }
            }
            egui::Shape::Text(t) => {
                out.push_str(t.galley.text());
                out.push('\n');
            }
            _ => {}
        }
    }
    let mut s = String::new();
    for cs in &out.shapes {
        walk(&cs.shape, &mut s);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safari_entry(species: u16) -> SafariMon {
        let mut s = SafariMon::default();
        s.nick = vec![0u8; 10];
        s.set_species(species);
        s
    }

    /// The shared field UI must be reachable for a Rogue-block safari entry, and
    /// that record must name exactly the sections it has nowhere to store. A
    /// spawn entry is 20 bytes of template: no level/EXP, EVs, moves, OT/ID, PID
    /// or stat bytes exist, so the editor must hide those sections instead of
    /// inventing widgets whose writes would land in a neighbouring field.
    #[test]
    fn a_safari_record_names_the_sections_it_lacks_and_reaches_the_shared_field_ui() {
        let mut rec = MonRec::Safari(safari_entry(252)); // Treecko

        // Hidden for a spawn entry -- the whole point of the section list.
        for hidden in [
            MonSection::LevelExp,
            MonSection::Evs,
            MonSection::Moves,
            MonSection::OtIdentity,
            MonSection::PidChecksum,
            MonSection::Flags,
            MonSection::HeldItem,
            MonSection::MetInfo,
            MonSection::CachedStats,
        ] {
            assert!(
                !rec.supports(hidden),
                "a safari entry must not offer {hidden:?}"
            );
        }
        // Shared with the box record -- one implementation serves both.
        for shared in [
            MonSection::Species,
            MonSection::Nickname,
            MonSection::Ball,
            MonSection::Nature,
            MonSection::Shiny,
            MonSection::Gender,
            MonSection::Ability,
            MonSection::Ivs,
        ] {
            assert!(rec.supports(shared), "the shared UI must offer {shared:?}");
        }
        for sec in [
            MonSection::Species,
            MonSection::Nickname,
            MonSection::Ball,
            MonSection::Nature,
            MonSection::Shiny,
            MonSection::Gender,
            MonSection::Ability,
            MonSection::Ivs,
        ] {
            assert!(
                rec.supports(sec),
                "spawn entry should support {sec:?} in the shared editor"
            );
        }
        // The box record keeps everything the entry lacks.
        let full = MonRec::Full(Mon::blank(0, "OT", 252, 5));
        assert!(full.supports(MonSection::Moves) && full.supports(MonSection::LevelExp));
        assert_eq!(
            full.supported_sections().len() + rec.supported_sections().len() - 8,
            FULL_SECTIONS.len() + SAFARI_SECTIONS.len() - 8,
            "the two section lists share the eight shared fields exactly"
        );

        // Writes go through the SafariMon accessors (the bitfields are the only
        // place these values can live).
        rec.set_iv(crate::domain::StatIdx::Spd, 31);
        rec.set_iv(crate::domain::StatIdx::Hp, 7);
        rec.set_shiny(true);
        rec.set_ball(2);
        rec.set_nature(crate::domain::Nature::from_raw(7));
        rec.set_ability_slot(crate::domain::AbilitySlot::Hidden);
        rec.set_gender_flag(crate::domain::GenderFlag::Female);
        rec.set_nickname("SILVA");
        let s = match rec.safari() {
            Some(s) => s,
            None => panic!("the adapter still wraps a SafariMon"),
        };
        assert_eq!(s.iv_at(5), 31, "Spd IV landed in w1");
        assert_eq!(s.iv_at(0), 7, "HP IV landed in w0");
        assert!(s.shiny());
        assert_eq!(s.pokeball().raw(), 2);
        assert_eq!(s.nature().raw(), 7);
        assert_eq!(s.ability_slot().raw(), 2);
        assert_eq!(s.gender().raw(), 1);
        assert_eq!(s.nickname(), "SILVA");
        assert_eq!(rec.species(), 252);
        rec.set_all_ivs(31);
        assert!(
            (0..6).all(|i| rec.safari().map(|s| s.iv_at(i) == 31).unwrap_or(false)),
            "\"31 all\" must reach all six 5-bit IV fields of the spawn entry"
        );
        // Sealing is a box-record operation; a spawn entry has nothing to seal.
        rec.commit();

        // And the ONE shared field UI renders for it, with no save file open.
        // egui measures a `Grid` on the first frame and only paints its cells on
        // the next, so the assertion runs against the LAST frame.
        let mut app = Editor::default();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 1200.0),
            )),
            focused: true,
            ..Default::default()
        };
        let mut painted = String::new();
        for _ in 0..3 {
            let out = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.mon_shared_fields_ui(ui, &mut rec);
                });
            });
            painted = painted_text(&out);
        }
        for want in [
            "Species", "Nickname", "Ball", "Nature", "Gender", "Ability", "IVs",
        ] {
            assert!(painted.contains(want), "the shared UI must paint {want}");
        }
        assert!(
            painted.contains("SpD: 31"),
            "the SafariMon's own IV must show in the shared IV row:\n{painted}"
        );
        assert!(
            !painted.contains("Moves") && !painted.contains("EVs"),
            "the shared UI must not draw box-only sections for a spawn entry"
        );
    }
}
