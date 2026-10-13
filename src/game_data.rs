//! Game-side data tables and their lookups (names, pockets, difficulty config,
//! seasons, hub areas/upgrades). These mirror constants from the game tree; the
//! big static tables live in tables.rs / tables_extra.rs / move_pp.rs.

use crate::tables;
use crate::tables_extra;

/// Poké Balls, in the game's own id order: `FIRST_BALL` (ITEM_MASTER_BALL = 1)
/// through `LAST_BALL` (ITEM_ROGUE_BALL = 13) in include/constants/items.h.
/// A mon's `ball` field stores this item id, so the same ids double as the icon
/// keys (assets/icons/items/<id>.png).
pub(crate) const BALLS: &[(u16, &str)] = &[
    (1, "Master Ball"),
    (2, "Ultra Ball"),
    (3, "Great Ball"),
    (4, "Poké Ball"),
    (5, "Safari Ball"),
    (6, "Net Ball"),
    (7, "Dive Ball"),
    (8, "Nest Ball"),
    (9, "Repeat Ball"),
    (10, "Timer Ball"),
    (11, "Luxury Ball"),
    (12, "Premier Ball"),
    (13, "Rogue Ball"),
];

pub(crate) fn ball_name(ball: u8) -> &'static str {
    BALLS
        .iter()
        .find(|(id, _)| *id == ball as u16)
        .map(|(_, n)| *n)
        .unwrap_or("(no ball / invalid)")
}

/// Full species pick-list: base dex + Rogue form ids (megas, gigantamax, fake forms).
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

/// Lab difficulty-config range slots, index order per include/constants/rogue.h.
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
/// (= SAVE_VER_ID_2_2_0), and a v2.2.1 build writes 8 (ROGUE_SAVE_VERSION).
/// The game migrates an older save itself on load (RogueSave_OnSaveLoaded ->
/// Rogue_NotifySaveVersionUpdated), re-serializing the block in the current
/// layout; the stamp only decides *which* fixups run. The stream walk's era
/// pins (232 quests, 7 campaigns, 72 mastery bytes, ...) are v2.2.x, so a save
/// from a different layout fails `ok` and exposes header-only -> no blind edits.
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

/// species lookup that also covers Rogue form ids (>905): megas, gigantamax, fake forms
pub(crate) fn species_name(id: u16) -> &'static str {
    match title_lookup(tables::SPECIES, id) {
        "??????" => title_lookup(tables_extra::SPECIES_EXTRA, id),
        n => n,
    }
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

/// `Rogue_IsEvolutionItem`: the game's own sort displays evolution items
/// first and groups them together inside their pocket run (`SortItemPlaceBefore`
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
    // Display names in POCKET_ORDER are human labels ("Items"), not the C
    // constant names — never slice a prefix off them (that panicked once).
    if p.is_known() {
        format!("{} (pocket {})", p.name(), p.raw())
    } else {
        format!("unpocketed (POCKET_NONE = {})", p.raw())
    }
}

/// `homeDecorations[].decorVariant` is an index into a game-generated enum, so the
/// id is meaningless without this table. The enum is emitted by the game's own
/// DecorationExporter; `tools/gen_deco_variants.py` reproduces its ordering and is
/// anchored on a real save (id 119 = the wardrobe `RogueHub_ClearProgress` seeds).
pub(crate) fn decor_name(id: u8) -> &'static str {
    crate::deco_variants::DECOR_VARIANTS
        .get(id as usize)
        .copied()
        .unwrap_or("out-of-range decoration id")
}

// ---------------------------------------------------------------- mon model
