//! Game-side data tables and their lookups (names, pockets, difficulty config,
//! seasons, hub areas/upgrades). These mirror constants from the game tree; the
//! big static tables live in tables.rs / tables_extra.rs / move_pp.rs.

use crate::tables;
use crate::tables_extra;

/// A mon's ball field is a **5-bit item id**, not a small table index:
/// `/*0x0A*/ u16 pokeball:5; //31 balls` (include/pokemon.h:120). A ball IS an
/// item to the game -- `value = ITEM_POKE_BALL; SetBoxMonData(boxMon,
/// MON_DATA_POKEBALL, &value)` (src/pokemon.c:950-951), `GetMonData(..,
/// MON_DATA_POKEBALL) == ITEM_LUXURY_BALL` (src/pokemon.c:3506), and the
/// summary screen turns the stored id back into a sprite with `ItemIdToBallId`
/// (src/pokemon_summary_screen.c:4645).
///
/// So the ball list is the generated item table restricted to
/// `FIRST_BALL..=LAST_BALL` (include/constants/items.h:39-40 -- on this
/// expansion branch ITEM_POKE_BALL = 1 .. ITEM_ROGUE_BALL = 28), deliberately
/// NOT a second hand-written list. An earlier copy of this table carried the
/// 2.2.07-era master-branch numbering (see the `rogue-src-2207` checkout:
/// ITEM_MASTER_BALL = 1, ITEM_POKE_BALL = 4, ITEM_ROGUE_BALL = 13), which made
/// the mon panel label a Poké Ball "Master Ball" while the bag tab called that
/// same id a Poké Ball. Deriving the list from the item table makes that
/// contradiction unrepresentable, and the ids are also the icon keys
/// (`assets/icons/items/<id>.png`), which is what lets the ball picker draw the
/// real ball sprites.
pub(crate) const FIRST_BALL: u16 = 1; // ITEM_POKE_BALL
pub(crate) const LAST_BALL: u16 = 28; // ITEM_ROGUE_BALL

pub(crate) fn balls() -> &'static [(u16, &'static str)] {
    use std::sync::OnceLock;
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| {
        all_items()
            .iter()
            .copied()
            .filter(|(id, _)| (FIRST_BALL..=LAST_BALL).contains(id))
            .collect()
    })
}

pub(crate) fn ball_name(ball: u8) -> &'static str {
    if ball == 0 {
        // ITEM_NONE: a mon with no ball recorded (eggs, event gifts that skip it).
        return "(no ball)";
    }
    balls()
        .iter()
        .find(|(id, _)| *id == ball as u16)
        .map(|(_, n)| *n)
        .unwrap_or("(no ball / invalid)")
}

/// Full species pick-list: base dex + Rogue form ids (megas, gigantamax, fake
/// forms).
pub(crate) fn all_species() -> &'static [(u16, &'static str)] {
    use std::sync::OnceLock;
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| {
        let mut v: Vec<(u16, &'static str)> = tables::SPECIES.iter().copied().collect::<Vec<_>>();
        v.extend(tables_extra::SPECIES_EXTRA.iter().copied());
        v.sort_by_key(|(id, _)| *id);
        v
    })
}

/// Full item pick-list: vanilla items + Rogue custom items + Z-A mega stones.
pub(crate) fn all_items() -> &'static [(u16, &'static str)] {
    use std::sync::OnceLock;
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| {
        let mut v: Vec<(u16, &'static str)> = tables::ITEMS.iter().copied().collect::<Vec<_>>();
        v.extend(tables_extra::ITEMS_EXTRA.iter().copied());
        v.sort_by_key(|(id, _)| *id);
        v
    })
}

/// Lab difficulty-config toggles, index order per include/constants/rogue.h.
pub(crate) const CONFIG_TOGGLES: &[&str] = &[
    "Exp. share (all party)",
    "Allow over-level mons",
    "EV gain",
    "Overworld mon encounters",
    "Wipe bag at adventure start",
    "Run mode switching",
    "Diverse trainers",
    "Affection/ribbons",
    "Release mons in adventure",
    "Trainers: Rogue gym roster",
    "Trainers: Kanto",
    "Trainers: Johto",
    "Trainers: Hoenn",
    "Trainers: Sinnoh",
    "Trainers: Unova",
    "Trainers: Kalos",
    "Trainers: Alola",
    "Trainers: Galar",
    "Trainers: Paldea",
    "Trainers: Gen 10 (placeholder)",
];

/// Lab difficulty-config range slots, index order per
/// include/constants/rogue.h.
pub(crate) const CONFIG_RANGES: &[&str] = &[
    "Trainer level shift",
    "(unused: item)",
    "(unused: legendary)",
    "Battle format",
    "Pokédex variant",
    "Game mode",
    "Difficulty preset",
    "Adventure revision mode",
    "Trainer order",
];

pub(crate) const BATTLE_FORMATS: &[&str] = &["Singles", "Doubles", "Mixed"];
pub(crate) const GAME_MODES: &[&str] = &["Standard", "Gauntlet", "Fast Path"];
pub(crate) const DIFF_PRESETS: &[&str] = &["Easy", "Average", "Hard", "Brutal", "Custom", "None"];
pub(crate) const REVISION_MODES: &[&str] = &["Never", "In-run", "Always on"];
pub(crate) const TRAINER_ORDERS: &[&str] = &["Default", "Rainbow", "Official"];
pub(crate) const CAMPAIGN_NAMES: &[&str] = &[
    "Low BST",
    "Classic",
    "Mini-boss Battler",
    "Auto Battler",
    "Later Manner",
    "Pokéball Limit",
    "One HP",
];
pub(crate) const HUB_AREAS: &[&str] = &[
    "Town Square",
    "Adventure Entrance",
    "Labs",
    "Safari Zone",
    "Challenge Frontier",
    "Berry Field",
    "Home",
    "Marts",
    "Ride Training",
    "Day Care",
    "Market",
];
pub(crate) const HUB_UPGRADES: &[&str] = &[
    "Town Square: School",
    "Town Square: Tutorites",
    "Adventure Entrance: Random starter",
    "Adventure Entrance: Adventure replay",
    "Lab: Unique mon lab",
    "Lab: Unique mon slots 1",
    "Lab: Unique mon slots 2",
    "Lab: Rarity Rare",
    "Lab: Rarity Epic",
    "Lab: Rarity Exotic",
    "Lab: Rarity Legendary (reserved)",
    "Home: Lower floor",
    "Home: Upper floor (reserved)",
    "Home: Edit style",
    "Berry Field: Brewing",
    "Berry Field: Brewing upgrade (reserved)",
    "Berry Field: Extra field 1",
    "Berry Field: Extra field 2",
    "Berry Field: Extra field 3",
    "Berry Field: Higher yield 1",
    "Berry Field: Higher yield 2",
    "Berry Field: Higher yield 3",
    "Safari Zone: Legends cave",
    "Marts: General stock",
    "Marts: Poké Balls",
    "Marts: Poké Ball stock",
    "Marts: TMs",
    "Marts: TM stock",
    "Marts: Traveler battle enhancers",
    "Marts: Traveler held items",
    "Day Care: Capacity 1",
    "Day Care: Capacity 2",
    "Day Care: Breeder",
    "Day Care: Breeder seen dex",
    "Day Care: Breeder legends (reserved)",
    "Day Care: Phone 1",
    "Day Care: Tea shop",
    "Day Care: Exp share 1",
    "Day Care: Exp share 2",
    "Day Care: Exp share 3",
    "Market: Pokéblock blenders",
    "Market: Bakery",
    "Market: Treat shop",
    "Market: Treat shop stock",
    "Market: Bank",
    "Day Care: Phone 2",
    "Day Care: Phone 3",
    "Lab: Unique typings",
];
pub(crate) const SEASONS: &[&str] = &["Spring", "Summer", "Autumn", "Winter"];
/// rogue_timeofday.c: the season cycles 4 seasons of 2 days each, and
/// `seasonCounter` counts *days* (wrapped % 8), not a season index.
pub(crate) const SEASON_COUNT: usize = 4;
pub(crate) const DAYS_PER_SEASON: u8 = 2;
/// On-disk `saveVersion` is `SAVE_VER_ID_* - 1` (rogue_save.c:
/// `RogueSave_GetVersionIdFor(v) = v + 1`). v2.2.0 saves therefore carry **7**
/// (= SAVE_VER_ID_2_2_0), and a v2.2.1 build writes 8 (ROGUE_SAVE_VERSION). The
/// game migrates an older save itself on load (RogueSave_OnSaveLoaded ->
/// Rogue_NotifySaveVersionUpdated), re-serializing the block in the current
/// layout; the stamp only decides *which* fixups run. The stream walk's era
/// pins (232 quests, 7 campaigns, 72 mastery bytes, ...) are v2.2.x, so a save
/// from a different layout fails `ok` and exposes header-only -> no blind
/// edits.
pub(crate) fn rogue_version_name(v: u16) -> &'static str {
    // on-disk stamp -> SAVE_VER_ID_{v+1}
    match v + 1 {
        1 => "1.x",
        2 => "2.0-prerelease",
        3 => "2.0.0",
        4 => "2.0.1",
        5 => "2.1.0",
        6 => "2.1.1",
        7 => "2.1.2",
        8 => "2.2.0",
        9 => "2.2.1",
        _ => "unknown/newer-than-editor",
    }
}

pub(crate) fn title_lookup<'a>(table: &'a [(u16, &'a str)], id: u16) -> &'a str {
    table
        .iter()
        .find(|(v, _)| *v == id)
        .map(|(_, n)| *n)
        .unwrap_or("??????")
}

/// species lookup that also covers Rogue form ids (>905): megas, gigantamax,
/// fake forms
pub(crate) fn species_name(id: u16) -> &'static str {
    match title_lookup(tables::SPECIES, id) {
        "??????" => title_lookup(tables_extra::SPECIES_EXTRA, id),
        n => n,
    }
}

// ---- form linkage (generated from the game's own formSpeciesIdTable data) ---
// The UI shows BASE names ("Darmanitan") with the form as a separate detail,
// because this ROM bakes the form into the species name string ("Darmanitan
// Standard Mode"). The mapping is the game's: see src/form_tables.rs and
// tools/gen_form_tables.py.

/// The family anchor (base) id for a species id. A form id maps to the id that
/// owns its family's formSpeciesIdTable; any other id is its own base. (In this
/// ROM the family anchor is itself the default-form id: species.h aliases
/// SPECIES_DARMANITAN onto SPECIES_DARMANITAN_STANDARD_MODE = 555.)
pub(crate) fn form_base_id(id: u16) -> u16 {
    crate::form_tables::FORM_BASES
        .iter()
        .find(|(f, _)| *f == id)
        .map(|(_, b)| *b)
        .unwrap_or(id)
}

/// The form label for a species id, when the id names a specific form.
pub(crate) fn form_label(id: u16) -> Option<&'static str> {
    crate::form_tables::FORM_LABELS
        .iter()
        .find(|(s, _)| *s == id)
        .map(|(_, l)| *l)
}

/// Base (family) display name for a species id: what the roster, popup title
/// and Mon tab show. For a form id this is the family name; for a plain id it
/// is just its own name.
pub(crate) fn species_base_name(id: u16) -> &'static str {
    if id == 0 {
        return species_name(id);
    }
    let base = form_base_id(id);
    crate::form_tables::FAMILY_NAMES
        .iter()
        .find(|(a, _)| *a == base)
        .map(|(_, n)| *n)
        .unwrap_or_else(|| species_name(base))
}

/// The species pick-list with unambiguous rows: a form id renders as "Base
/// (Form)" -- e.g. "Darmanitan (Standard Mode)" -- because form ids are
/// distinct ids the user picks, and two family members must not look alike.
pub(crate) fn species_picker_table() -> &'static [(u16, &'static str)] {
    use std::sync::OnceLock;
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| {
        all_species()
            .iter()
            .map(|(id, name)| {
                let decorated = match form_label(*id) {
                    Some(label) => {
                        let base = species_base_name(*id);
                        if name.starts_with('(') && name.ends_with(')') {
                            format!("{base} {name}")
                        } else {
                            format!("{base} ({label})")
                        }
                    }
                    None => name.to_string(),
                };
                (*id, Box::leak(decorated.into_boxed_str()) as &'static str)
            })
            .collect()
    })
}

/// item lookup that also covers Rogue custom items (827+)
pub(crate) fn item_name(id: u16) -> &'static str {
    match title_lookup(tables::ITEMS, id) {
        "??????" => title_lookup(tables_extra::ITEMS_EXTRA, id),
        n => n,
    }
}

/// Effective pocket for a bag-placed item id (1..=9; 0 = pocketless). The game
/// refuses to add pocketless ids to the bag at all (`ItemId_GetPocket` ==
/// `POCKET_NONE` in `AddBagItem`), so this gates every scripted insert.
pub(crate) fn item_pocket(id: u16) -> crate::domain::Pocket {
    crate::domain::Pocket::from_raw(item_pocket_raw(id))
}
pub(crate) fn item_pocket_raw(id: u16) -> u8 {
    if id == 0 {
        return 0;
    }
    crate::item_pockets::ITEM_POCKETS
        .iter()
        .find(|(v, _)| *v == id)
        .map(|(_, p)| *p)
        .unwrap_or(0)
}

/// `Rogue_IsEvolutionItem`: the game's own sort displays evolution items first
/// and groups them together inside their pocket run (`SortItemPlaceBefore`
/// fallback sub-sort). Part of the editor's canonical order so an editor-sorted
/// bag matches what the game's in-bag sort would produce.
pub(crate) fn is_evo_item(id: u16) -> bool {
    crate::item_pockets::EVO_ITEMS.binary_search(&id).is_ok()
}

/// The editor's canonical sort key: pocket run order (the functional
/// invariant), evo items first within a run (the game's display rule), then
/// ascending id (the game's final tie-break).
pub(crate) fn bag_sort_key(id: u16) -> (crate::domain::Pocket, bool, u16) {
    (item_pocket(id), !is_evo_item(id), id)
}

pub(crate) fn pocket_name(p: crate::domain::Pocket) -> String {
    // Display names in POCKET_ORDER are human labels ("Items"), not the C constant
    // names - never slice a prefix off them (that panicked once).
    if p.is_known() {
        format!("{} (pocket {})", p.name(), p.raw())
    } else {
        format!("unpocketed (POCKET_NONE = {})", p.raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generated table that silently drops families is the failure mode this
    /// project keeps hitting. 201 species -- Xerneas (716), Alcremie (869), the 17
    /// Arceus and 17 Silvally forms, Genesect's drives, Minior's cores, Ogerpon's
    /// masks -- were absent because their entries read `[SPECIES_X] = NAME(args),`
    /// and the generator only followed bare `NAME` indirection. The table looked
    /// complete, and the editor answered "no base stats in table" for mons that
    /// have base stats. So: every species id the editor can NAME must either have
    /// stats or be on the generated declared-without-data list (constants the tree
    /// declares with no species_info entry -- placeholder/unimplemented forms).
    #[test]
    fn every_named_species_either_has_stats_or_is_declared_without_game_data() {
        let mut gaps = Vec::new();
        for (id, name) in tables::SPECIES
            .iter()
            .chain(tables_extra::SPECIES_EXTRA.iter())
        {
            if crate::basestats::base_stats(*id).is_none()
                && !crate::basestats::species_without_game_data().contains(id)
            {
                gaps.push((*id, *name));
            }
        }
        assert!(
            gaps.is_empty(),
            "{} named species have no base stats and are not declared without game data: \
             {:?}",
            gaps.len(),
            &gaps[..gaps.len().min(12)]
        );
    }

    /// The declared-without-data list must not become a place to hide generator
    /// bugs, so the species that real mons actually use are pinned by name here.
    #[test]
    fn real_species_are_never_on_the_without_game_data_list() {
        for (id, name) in [
            (716u16, "Xerneas"),
            (869, "Alcremie"),
            (493, "Arceus"),
            (649, "Genesect"),
            (1176, "Silvally (Fighting)"),
            (1236, "Alcremie (Ruby Cream)"),
            (1200, "Minior (Orange core)"),
            (1417, "Ogerpon (Wellspring Mask)"),
        ] {
            assert!(
                !crate::basestats::species_without_game_data().contains(&id),
                "{name} ({id}) must have base stats"
            );
            assert!(
                crate::basestats::base_stats(id).is_some(),
                "{name} ({id}) missing from the table"
            );
        }
    }

    /// Values read straight out of the tree, not remembered: Emerald Rogue
    /// REBALANCES base stats (Alcremie is 65/60/75/64/110/121 in
    /// ALCREMIE_MISC_INFO, not the national-dex 80/85/95/110/110/80), so an
    /// expectation written from memory is not evidence. Table order: [HP, Atk, Def,
    /// Spe, SpA, SpD].
    #[test]
    fn function_like_macro_families_carry_the_trees_values() {
        for (id, want, src) in [
            (
                716u16,
                [126, 131, 95, 99, 131, 98],
                "XERNEAS_MISC_INFO, gen_6.h:4586",
            ),
            (
                869,
                [65, 60, 75, 64, 110, 121],
                "ALCREMIE_MISC_INFO, gen_8.h:3266",
            ),
            (
                493,
                [120, 120, 120, 120, 120, 120],
                "ARCEUS_SPECIES_INFO, gen_4.h",
            ),
            (
                649,
                [71, 120, 95, 99, 120, 95],
                "GENESECT_SPECIES_INFO, gen_5.h",
            ),
            (
                1176,
                [95, 95, 95, 95, 95, 95],
                "SILVALLY_SPECIES_INFO, gen_7.h",
            ),
            (
                1200,
                [60, 100, 60, 120, 100, 60],
                "MINIOR_CORE_SPECIES_INFO, gen_7.h",
            ),
        ] {
            assert_eq!(
                crate::basestats::base_stats(id),
                Some(want),
                "species {id} should match {src}"
            );
        }
    }

    /// The user's example, straight from the game's own linkage: species 555 (ROM
    /// name string "Darmanitan Standard Mode") shows as base "Darmanitan" with
    /// "Standard Mode" as the separate form detail. Note the base id: in this ROM
    /// the FAMILY ANCHOR is 555 itself -- species.h has `#define SPECIES_DARMANITAN
    /// SPECIES_DARMANITAN_STANDARD_MODE`, and id 554 is "Darumaka", the
    /// pre-evolution, not a Darmanitan form entry.
    #[test]
    fn darmanitan_case_from_the_games_linkage() {
        assert_eq!(species_name(555), "Darmanitan Standard Mode");
        assert_eq!(species_name(554), "Darumaka");
        assert_eq!(form_base_id(555), 555, "555 is its family's anchor");
        assert_eq!(form_label(555), Some("Standard Mode"));
        assert_eq!(species_base_name(555), "Darmanitan");
        // Zen Mode lives beyond FORMS_START and anchors to the same family.
        assert_eq!(form_base_id(1092), 555);
        assert_eq!(form_label(1092), Some("Zen Mode"));
        assert_eq!(species_base_name(1092), "Darmanitan");
    }

    #[test]
    fn alcremie_case_keeps_the_full_form_label() {
        assert_eq!(species_base_name(869), "Alcremie");
        assert_eq!(form_label(869), Some("Strawberry Vanilla Cream"));
        assert_eq!(form_base_id(1216), 869);
        assert_eq!(form_label(1216), Some("Strawberry Ruby Cream"));
        assert_eq!(species_base_name(1216), "Alcremie");
    }

    #[test]
    fn a_non_form_species_has_no_form_row_and_keeps_its_name() {
        assert_eq!(form_label(143), None);
        assert_eq!(form_base_id(143), 143);
        assert_eq!(species_base_name(143), "Snorlax");
        assert_eq!(form_label(0), None);
        assert_eq!(species_base_name(0), species_name(0));
    }

    /// The generated table's shape is part of the contract: the pinned entry count
    /// changes only when the game source or the generator changes, and every
    /// emitted pair must be a real, non-chaining, non-zero mapping.
    #[test]
    fn the_generated_linkage_table_is_sane_and_pinned() {
        assert_eq!(
            crate::form_tables::FORM_BASES.len(),
            442,
            "form->base entry count moved: regenerate with tools/gen_form_tables.py \
             and pin the new number only after checking the printed warnings"
        );
        for (form, base) in crate::form_tables::FORM_BASES {
            assert_ne!(*form, 0);
            assert_ne!(*base, 0, "form {form} would anchor to species 0");
            assert_ne!(
                form, base,
                "species {form} is its own base - not a form row"
            );
            assert!(
                !crate::form_tables::FORM_BASES
                    .iter()
                    .any(|(m, _)| *m == *base),
                "base {base} of form {form} is itself a form: families must not chain"
            );
        }
    }

    /// The ball list must be the item table's own ball range. This is the
    /// regression pin for the wrong-branch numbering: id 1 is the Poké Ball on this
    /// branch, and the mon panel may never name an id differently from the bag tab.
    #[test]
    fn ball_ids_are_the_item_range_and_named_exactly_like_the_item_table() {
        let b = balls();
        assert_eq!(
            b.first().map(|(id, _)| *id),
            Some(FIRST_BALL),
            "the range starts at ITEM_POKE_BALL"
        );
        assert_eq!(
            b.last().map(|(id, _)| *id),
            Some(LAST_BALL),
            "the range ends at ITEM_ROGUE_BALL"
        );
        assert_eq!(
            b.len(),
            (LAST_BALL - FIRST_BALL + 1) as usize,
            "ball ids are contiguous on this branch - a gap means the branch reordered
             items, so re-read items.h before trusting any ball label"
        );
        let items = all_items();
        for (id, name) in b {
            let item = items
                .iter()
                .find(|(i, _)| *i == *id)
                .expect("every ball id must exist in the item table");
            assert_eq!(
                item.1, *name,
                "ball {id} named differently in bag vs mon panel"
            );
            assert!(
                item.1.ends_with("Ball"),
                "id {id} ({}) sits in the ball range but is not a ball",
                item.1
            );
        }
        // The two ids the old table had exactly backwards.
        assert_eq!(ball_name(1), "Poke Ball");
        assert_eq!(ball_name(4), "Master Ball");
        assert_eq!(ball_name(28), "Rogue Ball");
        assert_eq!(ball_name(0), "(no ball)", "ITEM_NONE is not a ball");
    }

    /// Species pickers must stay unambiguous: form ids are distinct ids the user
    /// picks, so each renders "Base (Form)".
    #[test]
    fn picker_rows_render_base_form_and_stay_ambiguous() {
        let t = species_picker_table();
        let name = |id: u16| {
            t.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| *n)
                .unwrap_or("<missing>")
        };
        assert_eq!(name(555), "Darmanitan (Standard Mode)");
        assert_eq!(name(1092), "Darmanitan (Zen Mode)");
        assert_eq!(name(869), "Alcremie (Strawberry Vanilla Cream)");
        assert_eq!(name(906), "Venusaur (Mega)");
        assert_eq!(name(143), "Snorlax");
        assert_eq!(name(1), "Bulbasaur", "plain species keep their plain names");
        assert_eq!(
            t.len(),
            all_species().len(),
            "the picker covers exactly the same ids as before"
        );
    }

    /// A met-location byte is a region-map SECTION id, so the picker's labels come
    /// from the game's own `MAPSEC_*` names. Anchors below are literal values from
    /// `include/constants/region_map_sections.h`: id 0 is the Pokemon Hub (a real
    /// place, not "unset"), `0xD5` is MAPSEC_NONE, and the 0xF? trio are the
    /// `METLOC_*` specials.
    #[test]
    fn met_location_sections_are_named_from_the_games_header() {
        for (id, want) in [
            (0x00, "Pokemon Hub"),
            (0x03, "Lavaridge Town"),
            (0x10, "Route 101"),
            (0x1F, "Route 116"),
            (0xD5, "None"),
            (0xFD, "Special Egg"),
            (0xFE, "In Game Trade"),
            (0xFF, "Fateful Encounter"),
        ] {
            let got = crate::map_secs::MAPSECS
                .iter()
                .find(|(v, _)| *v == id)
                .map(|(_, n)| *n);
            assert_eq!(
                got,
                Some(want),
                "met location {id:#04x} must render as {want:?}"
            );
        }
        let n = crate::map_secs::MAPSECS.len();
        assert!(n >= 200, "only {n} sections parsed from the header");
        let mut ids: Vec<u16> = crate::map_secs::MAPSECS.iter().map(|(v, _)| *v).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "a section id appears twice in the table");
        assert!(
            ids.iter().all(|v| *v <= 0xFF),
            "a section id does not fit the u8 met-location byte"
        );
    }

    /// `hubMap.weatherState` is a u16 seed, not a weather id: the season folds it
    /// through `% 5` / `% 6` into the weather an outdoor map shows
    /// (`Rogue_ModifyOverworldMapWeather`, src/rogue_controller.c:1918-2011). The
    /// editor has to show the weather, since the number alone is meaningless.
    #[test]
    fn the_hub_weather_seed_resolves_to_a_weather_per_season() {
        use crate::domain::Season;
        let w = |s: Season, v: u16| hub_weather_for(s, v);
        assert_eq!(w(Season::Spring, 0), "Rain");
        assert_eq!(w(Season::Spring, 1), "Rain Thunderstorm");
        assert_eq!(w(Season::Spring, 2), "Fog Diagonal (Misty Terrain)");
        assert_eq!(w(Season::Spring, 5), "Rain", "spring folds on % 5");
        assert_eq!(w(Season::Summer, 0), "Rain Thunderstorm");
        assert_eq!(w(Season::Summer, 1), "None");
        assert_eq!(w(Season::Autumn, 2), "Leaves (Grassy Terrain)");
        assert_eq!(w(Season::Autumn, 5), "None");
        // Winter really does use WEATHER_SNOW even though the header marks the constant
        // unused for vanilla, so the label must not carry that note.
        assert_eq!(w(Season::Winter, 0), "Snow");
        assert_eq!(w(Season::Winter, 3), "Shade");
        assert_eq!(w(Season::Winter, 4), "None");
        assert_eq!(
            w(Season::Unexpected(9), 0),
            "unknown season: no weather mapping",
            "a season outside the enum must not guess"
        );
    }
}

/// `homeDecorations[].decorVariant` is an index into a game-generated enum, so
/// the id is meaningless without this table. The enum is emitted by the game's
/// own DecorationExporter; `tools/gen_deco_variants.py` reproduces its ordering
/// and is anchored on a real save (id 119 = the wardrobe
/// `RogueHub_ClearProgress` seeds).
pub(crate) fn decor_name(id: u8) -> &'static str {
    crate::deco_variants::DECOR_VARIANTS
        .get(id as usize)
        .copied()
        .unwrap_or("out-of-range decoration id")
}

/// The hub's `weatherState` is a u16 seed, **not** a weather id.
/// `Rogue_ModifyOverworldMapWeather()` (src/rogue_controller.c:1918-2011) folds
/// it per season into the weather an outdoor map shows: `state % 5` in spring
/// (:1941-1956) and summer (:1957-1970), `state % 6` in autumn (:1971-1987) and
/// winter (:1988-2005). Indoor/underground maps keep their own weather, a run
/// in progress uses `VAR_ROGUE_DESIRED_WEATHER` instead (:1929-1933), and
/// before the tutorial finishes the game forces no weather (:1934-1936).
pub(crate) fn hub_weather_for(season: crate::domain::Season, state: u16) -> &'static str {
    use crate::domain::Season;
    use crate::weather_names as w;
    let id = match season {
        Season::Spring => match state % 5 {
            0 => w::WEATHER_RAIN,
            1 => w::WEATHER_RAIN_THUNDERSTORM,
            2 => w::WEATHER_MISTY_FOG,
            _ => w::WEATHER_NONE,
        },
        Season::Summer => match state % 5 {
            0 => w::WEATHER_RAIN_THUNDERSTORM,
            _ => w::WEATHER_NONE,
        },
        Season::Autumn => match state % 6 {
            0 => w::WEATHER_RAIN,
            1 => w::WEATHER_RAIN_THUNDERSTORM,
            2 | 3 => w::WEATHER_LEAVES,
            _ => w::WEATHER_NONE,
        },
        Season::Winter => match state % 6 {
            0 | 1 | 2 => w::WEATHER_SNOW,
            3 => w::WEATHER_SHADE,
            _ => w::WEATHER_NONE,
        },
        Season::Unexpected(_) => return "unknown season: no weather mapping",
    };
    w::weather_name(id)
}

// ---------------------------------------------------------------- mon model
