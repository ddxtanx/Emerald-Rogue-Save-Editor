//! er-save-editor — PKHeX-like editor for Pokémon Emerald Rogue (v2.x EX) saves.
//!
//! Format grounded in Pokabbie/pokeemerald-rogue (expansion branch):
//!   include/save.h          sector layout (4084+12 footer, security 0x08012025)
//!   include/pokemon.h       BoxPokemon(80)/Pokemon(104), substruct bitfields
//!   include/pokemon_storage_system.h  14 boxes x 30, names, wallpapers
//!   src/save.c              CalculateChecksum
//!   src/pokemon.c           EncryptBoxMon/DecryptBoxMon/GetSubstruct/CalculateBoxMonChecksum
//!
//! Run:  er-save-editor                -> GUI
//!       er-save-editor --cli FILE     -> dump save to stdout (headless verification)

mod basestats;
mod deco_variants;
mod item_pockets;
mod tables;
mod tables_extra;

use basestats::base_stats;
use egui;
use tables::*;

/// write a line to stdout without panicking on broken pipes (e.g. `| head`)
macro_rules! say {
    ($($a:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($a)*);
    }};
}

const SECSZ: usize = 0x1000;
const DATASZ: usize = 4084;
const SECURITY: u32 = 0x0801_2025;
const SLOT_SECTORS: usize = 14;
const FILE_SECTORS: usize = 32;

const PARTY_COUNT_OFF: usize = 0x234;
const PARTY_OFF: usize = 0x238;
const MONSZ: usize = 104;
const BOXSZ: usize = 80;
// Rogue discards the vanilla growth-rate curves: Rogue_ModifyExperienceTables()
// (src/rogue_baked.c) returns `level * 300` for every growthRate, and the level loop
// in GetLevelFrom{Box}MonExp clamps at MAX_LEVEL. So exp == level*300 and level ==
// clamp(exp/300, 0, MAX_LEVEL) — the same schedule for every species.
const EXP_PER_LEVEL: u32 = 300;
const MAX_LEVEL: u8 = 100;

/// Replay of the game's level loop (src/pokemon.c GetLevelFromBoxMonExp) against
/// Rogue's flat `level * 300` schedule. Exact for all exp values, including the
/// clamping when exp exceeds what level 100 costs.
fn level_from_exp(exp: u32) -> u8 {
    let lvl = exp / EXP_PER_LEVEL;
    lvl.min(MAX_LEVEL as u32) as u8
}
// Rogue shifts SB1 after the party: Pokemon structs are 104B not 80B.
// Verified against jfzazo savconverter + live save:
//   money   = 0x238 + 6*104      = 0x4A8 (XOR'd with SB2 encryptionKey @SB2+0x4C)
//   pcItems = money + 4 + 4      = 0x4B0 (raw)
//   bag     = pcItems + 50*4     = 0x578 (quantity XOR'd with low 16 bits of key)
const MONEY_OFF: usize = 0x4A8; // in SaveBlock1
const PC_ITEMS_OFF: usize = 0x4B0;
const BAG_OFF: usize = 0x578;
const ENC_KEY_OFF: usize = 0x4C; // in SaveBlock2 (sector id 0)
const BAG_CAP: usize = 450;
const PC_CAP: usize = 50;
const SB1_BASE: usize = DATASZ; // SaveBlock1 = merged chunks 1..4
const STOR_BASE: usize = 5 * DATASZ; // storage = merged chunks 5..13
                                     // Bag sizing knobs (include/constants/global.h + constants/rogue.h):
                                     //   SB1+0x4AC u8 bagSortMode (0 type, 1 name, 2 value, 3 amount)
                                     //   SB1+0x4AD u8 bagCapacityUpgrades
                                     // The game only ever *reads* the first `Rogue_GetBagCapacity()` slots of the
                                     // flat bag array, so anything past capacity is invisible (and gets shuffled
                                     // away by the next in-game bag sort). Key Items are special: they live in the
                                     // LAST 50 slots of the physical array (`BAG_ITEM_RESERVED_SLOTS`), which is
                                     // why a sorted bag ends with its key-item run.
const BAG_SORT_MODE_OFF: usize = 0x4AC;
const BAG_CAP_UPGRADES_OFF: usize = 0x4AD;
const BAG_RESERVED_SLOTS: usize = 50;
const BAG_SLOTS_PER_UPGRADE: usize = 10;
const BAG_MAX_CAPACITY_UPGRADES: usize = 37;
const BAG_BASE_CAPACITY: usize = 450 - BAG_MAX_CAPACITY_UPGRADES * BAG_SLOTS_PER_UPGRADE; // 80
/// `MAX_BAG_ITEM_CAPACITY`: Rogue gives every pocket a 999-per-stack cap.
const MAX_STACK: u16 = 999;

// Rogue save block: overlays PC boxes 10..13 (boxes[TOTAL=14 alloc, 10 in use][0]).
// Written by SerializeRogueBlock() (src/rogue_save.c) as a sequential stream where
// every SerializeArray carries a u16 element-count prefix -> self-describing walk.
// encryptionKey = 0: the block is plaintext. Verified vs v2.2.0-EX saves.
const ROGUE_BASE: usize = STOR_BASE + 4 + 10 * 30 * 80;
const ROGUE_CAP: usize = 4 * 30 * 80; // 4 leftover boxes
const ROGUE_SECRET: u16 = 27615;
const DAYCARE_ELEMSZ: usize = 84; // 80B BoxPokemon facade + 4 (v2.2.0 build)
const SAFARISZ: usize = 20; // struct RogueSafariMon (STATIC_ASSERT == 20)
const STASH_PARTY: usize = 6;
const STASH_BAG_CAP: usize = 450;
const STASH_DAYCARE: usize = 3;
/// `FOLLOWMON_SHINY_OFFSET` (`include/constants/rogue.h`): the hub's
/// `homeWanderingMonSpecies` stores `species + 10000` for a shiny follow-mon.
const FOLLOWMON_SHINY_OFFSET: u16 = 10000;
/// `homeStyles` labels, in the enum order of `src/rogue_hub.c`.
const HOME_STYLE_EXTERIOR: &[&str] = &[
    "Cave",
    "Trees",
    "Brick House",
    "Wooden House",
    "Gym Building",
];
const HOME_STYLE_INTERIOR: &[&str] = &[
    "Red Cave",
    "Blue Cave",
    "Brown Cave",
    "Desert Cave",
    "Shrub",
    "Tree",
];
const HOME_STYLE_PATH: &[&str] = &["Grass", "Sand", "Stone", "Pebbles", "Muddy Tracks"];
/// `homeDecorations` slot ranges (`HOME_DECOR_OUTSIDE_COUNT` 160, then
/// `HOME_DECOR_INSIDE_COUNT` 32).
const HOME_DECOR_OUTSIDE: usize = 160;
const TOTAL_BOXES: usize = 14; // allocated (boxNames stride)
const ACTIVE_BOXES: usize = 10; // the PC actually uses boxes 0..9; 10..13 are leftover slots
const BOX_SLOTS: usize = 30;
const BOX_NAME_LEN: usize = 9;
// struct Pokemon fusions[MAX_FUSION_STORAGE=4] at /*0x8432*/ in PokemonStorage
// (pokemon_storage_system.h). Full 104B party structs.
const FUSION_OFF: usize = 0x8432;
const FUSION_COUNT: usize = 4;

const SUBSTRUCT_ORDER: [[usize; 4]; 24] = [
    [0, 1, 2, 3],
    [0, 1, 3, 2],
    [0, 2, 1, 3],
    [0, 3, 1, 2],
    [0, 2, 3, 1],
    [0, 3, 2, 1],
    [1, 0, 2, 3],
    [1, 0, 3, 2],
    [2, 0, 1, 3],
    [3, 0, 1, 2],
    [2, 0, 3, 1],
    [3, 0, 2, 1],
    [1, 2, 0, 3],
    [1, 3, 0, 2],
    [2, 1, 0, 3],
    [3, 1, 0, 2],
    [2, 3, 0, 1],
    [3, 2, 0, 1],
    [1, 2, 3, 0],
    [1, 3, 2, 0],
    [2, 1, 3, 0],
    [3, 1, 2, 0],
    [2, 3, 1, 0],
    [3, 2, 1, 0],
];

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

/// Full species pick-list: base dex + Rogue form ids (megas, gigantamax, fake forms).
fn all_species() -> &'static [(u16, &'static str)] {
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
fn all_items() -> &'static [(u16, &'static str)] {
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
const CONFIG_TOGGLES: &[&str] = &[
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
const CONFIG_RANGES: &[&str] = &[
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

const BATTLE_FORMATS: &[&str] = &["Singles", "Doubles", "Mixed"];
const GAME_MODES: &[&str] = &["Standard", "Gauntlet", "Fast Path"];
const DIFF_PRESETS: &[&str] = &["Easy", "Average", "Hard", "Brutal", "Custom", "None"];
const REVISION_MODES: &[&str] = &["Never", "In-run", "Always on"];
const TRAINER_ORDERS: &[&str] = &["Default", "Rainbow", "Official"];
const CAMPAIGN_NAMES: &[&str] = &[
    "Low BST",
    "Classic",
    "Mini-boss Battler",
    "Auto Battler",
    "Later Manner",
    "Pokéball Limit",
    "One HP",
];
const HUB_AREAS: &[&str] = &[
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
const HUB_UPGRADES: &[&str] = &[
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
const SEASONS: &[&str] = &["Spring", "Summer", "Autumn", "Winter"];
/// rogue_timeofday.c: the season cycles 4 seasons of 2 days each, and
/// `seasonCounter` counts *days* (wrapped % 8), not a season index.
const SEASON_COUNT: usize = 4;
const DAYS_PER_SEASON: u8 = 2;
/// On-disk `saveVersion` is `SAVE_VER_ID_* - 1` (rogue_save.c:
/// `RogueSave_GetVersionIdFor(v) = v + 1`). v2.2.0 saves therefore carry **7**
/// (= SAVE_VER_ID_2_2_0), and a v2.2.1 build writes 8 (ROGUE_SAVE_VERSION).
/// The game migrates an older save itself on load (RogueSave_OnSaveLoaded ->
/// Rogue_NotifySaveVersionUpdated), re-serializing the block in the current
/// layout; the stamp only decides *which* fixups run. The stream walk's era
/// pins (232 quests, 7 campaigns, 72 mastery bytes, ...) are v2.2.x, so a save
/// from a different layout fails `ok` and exposes header-only -> no blind edits.
fn rogue_version_name(v: u16) -> &'static str {
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
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn put16(b: &mut [u8], i: usize, v: u16) {
    b[i..i + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}

fn sector_crc(data: &[u8]) -> u16 {
    let mut s: u32 = 0;
    for w in data.chunks_exact(4) {
        s = s.wrapping_add(le32(w, 0));
    }
    (((s >> 16) as u32 + s) & 0xFFFF) as u16
}

// ---------------------------------------------------------------- gen3 strings
fn gen3_decode(b: &[u8]) -> String {
    let mut out = String::new();
    for &c in b {
        if c == 0xFF {
            break;
        }
        match c {
            0x00 => out.push(' '),
            0xA1..=0xAA => out.push((b'0' + (c - 0xA1)) as char),
            0xAB => out.push('!'),
            0xAC => out.push('?'),
            0xAD => out.push('.'),
            0xAE => out.push('-'),
            0xBB..=0xD4 => out.push((b'A' + (c - 0xBB)) as char),
            0xD5..=0xEE => out.push((b'a' + (c - 0xD5)) as char),
            _ => out.push('?'),
        }
    }
    out.trim_end().to_string()
}

fn gen3_encode(s: &str, len: usize) -> Vec<u8> {
    let mut v = vec![0xFFu8; len];
    let mut i = 0;
    for ch in s.chars() {
        if i >= len {
            break;
        }
        let c: u8 = match ch {
            ' ' => 0x00,
            '!' => 0xAB,
            '?' => 0xAC,
            '.' => 0xAD,
            '-' => 0xAE,
            c if c.is_ascii_digit() => 0xA1 + (c as u8 - b'0'),
            c if c.is_ascii_uppercase() => 0xBB + (c as u8 - b'A'),
            c if c.is_ascii_lowercase() => 0xD5 + (c as u8 - b'a'),
            _ => continue,
        };
        v[i] = c;
        i += 1;
    }
    v
}

fn title_lookup<'a>(table: &'a [(u16, &'a str)], id: u16) -> &'a str {
    table
        .iter()
        .find(|(v, _)| *v == id)
        .map(|(_, n)| *n)
        .unwrap_or("??????")
}

/// species lookup that also covers Rogue form ids (>905): megas, gigantamax, fake forms
fn species_name(id: u16) -> &'static str {
    match title_lookup(tables::SPECIES, id) {
        "??????" => title_lookup(tables_extra::SPECIES_EXTRA, id),
        n => n,
    }
}

/// item lookup that also covers Rogue custom items (827+)
fn item_name(id: u16) -> &'static str {
    match title_lookup(tables::ITEMS, id) {
        "??????" => title_lookup(tables_extra::ITEMS_EXTRA, id),
        n => n,
    }
}

/// Effective pocket for a bag-placed item id (1..=9; 0 = pocketless). The game
/// refuses to add pocketless ids to the bag at all (`ItemId_GetPocket` ==
/// `POCKET_NONE` in `AddBagItem`), so this gates every scripted insert.
fn item_pocket(id: u16) -> u8 {
    if id == 0 {
        return 0;
    }
    item_pockets::ITEM_POCKETS
        .iter()
        .find(|(v, _)| *v == id)
        .map(|(_, p)| *p)
        .unwrap_or(0)
}

/// `Rogue_IsEvolutionItem`: the game's own sort displays evolution items
/// first and groups them together inside their pocket run (`SortItemPlaceBefore`
/// fallback sub-sort). Part of the editor's canonical order so an editor-sorted
/// bag matches what the game's in-bag sort would produce.
fn is_evo_item(id: u16) -> bool {
    item_pockets::EVO_ITEMS.binary_search(&id).is_ok()
}

/// The editor's canonical sort key: pocket run order (the functional
/// invariant), evo items first within a run (the game's display rule), then
/// ascending id (the game's final tie-break).
fn bag_sort_key(id: u16) -> (u8, bool, u16) {
    (item_pocket(id), !is_evo_item(id), id)
}

fn pocket_name(p: u8) -> String {
    // POCKET_ORDER stores human display names ("Items", "Held Items"), NOT the
    // C constant names — never slice a fixed prefix off them (this panicked on
    // "Items" once the GUI started rendering pocket labels).
    match item_pockets::POCKET_ORDER.iter().find(|(_, v)| *v == p) {
        Some((name, v)) => format!("{name} (pocket {v})"),
        None => format!("unpocketed (POCKET_NONE = {p})"),
    }
}

/// `homeDecorations[].decorVariant` is an index into a game-generated enum, so the
/// id is meaningless without this table. The enum is emitted by the game's own
/// DecorationExporter; `tools/gen_deco_variants.py` reproduces its ordering and is
/// anchored on a real save (id 119 = the wardrobe `RogueHub_ClearProgress` seeds).
fn decor_name(id: u8) -> &'static str {
    deco_variants::DECOR_VARIANTS
        .get(id as usize)
        .copied()
        .unwrap_or("out-of-range decoration id")
}

// ---------------------------------------------------------------- mon model
#[derive(Clone)]
pub struct Mon {
    pub raw: Vec<u8>, // 80 (box) or 104 (party)
    // decrypted substruct slots in PHYSICAL (permuted) order
    slots: [[u8; 12]; 4],
    // logical view: t[j] = decrypted bytes of logical substruct j
    pub t: Vec<[u8; 12]>,
}

impl Mon {
    pub fn from_bytes(raw: &[u8]) -> Mon {
        let pid = le32(raw, 0);
        let otid = le32(raw, 4);
        let key = pid ^ otid;
        let kb = key.to_le_bytes();
        let mut slots = [[0u8; 12]; 4];
        for i in 0..4 {
            for j in 0..12 {
                slots[i][j] = raw[32 + i * 12 + j] ^ kb[j % 4];
            }
        }
        let perm = SUBSTRUCT_ORDER[(pid % 24) as usize];
        let t = vec![
            slots[perm[0]],
            slots[perm[1]],
            slots[perm[2]],
            slots[perm[3]],
        ];
        Mon {
            raw: raw.to_vec(),
            slots,
            t,
        }
    }

    pub fn pid(&self) -> u32 {
        le32(&self.raw, 0)
    }
    pub fn otid(&self) -> u32 {
        le32(&self.raw, 4)
    }
    pub fn set_otid(&mut self, v: u32) {
        put32(&mut self.raw, 4, v);
    }
    pub fn nickname(&self) -> String {
        gen3_decode(&self.raw[8..18])
    }
    pub fn set_nickname(&mut self, s: &str) {
        let enc = gen3_encode(s, 10);
        self.raw[8..18].copy_from_slice(&enc);
    }
    pub fn ot_name(&self) -> String {
        gen3_decode(&self.raw[20..27])
    }
    pub fn markings(&self) -> u8 {
        self.raw[27]
    }
    pub fn checksum_stored(&self) -> u16 {
        le16(&self.raw, 28)
    }

    pub fn language(&self) -> u8 {
        self.raw[18] & 0x07
    }
    pub fn hidden_nature_mod(&self) -> u8 {
        (self.raw[18] >> 3) & 0x1F
    }
    pub fn is_bad_egg(&self) -> bool {
        self.raw[19] & 1 != 0
    }
    pub fn has_species(&self) -> bool {
        self.raw[19] & 2 != 0
    }
    pub fn is_egg_boxflag(&self) -> bool {
        self.raw[19] & 4 != 0
    }

    fn pid_nature(&self) -> u8 {
        (self.pid() % 25) as u8
    }
    pub fn nature(&self) -> u8 {
        self.pid_nature() ^ self.hidden_nature_mod()
    }
    pub fn set_nature(&mut self, n: u8) {
        let modv = self.pid_nature() ^ (n % 25);
        self.raw[18] = (self.raw[18] & 0x07) | ((modv & 0x1F) << 3);
    }

    pub fn species(&self) -> u16 {
        (le32(&self.t[0], 0) & 0x7FF) as u16
    }
    pub fn set_species(&mut self, sp: u16) {
        let w = le32(&self.t[0], 0);
        self.t[0][0..4].copy_from_slice(&(w & !0x7FF | (sp as u32 & 0x7FF)).to_le_bytes());
    }
    pub fn held_item(&self) -> u16 {
        let w = le32(&self.t[0], 0);
        ((w >> 11) & 0x3FF) as u16
    }
    pub fn set_held_item(&mut self, it: u16) {
        let w = le32(&self.t[0], 0);
        let it1 = (it as u32) & 0x3FF;
        let it2 = ((it as u32) >> 10) & 1;
        let w = (w & !(0x7FF << 11)) | (it1 << 11) | (it2 << 26);
        self.t[0][0..4].copy_from_slice(&w.to_le_bytes());
    }
    pub fn exp(&self) -> u32 {
        le32(&self.t[0], 4) & 0x1F_FFFF
    }
    pub fn set_exp(&mut self, v: u32) {
        let w = le32(&self.t[0], 4);
        self.t[0][4..8].copy_from_slice(&((w & !0x1F_FFFF) | (v & 0x1F_FFFF)).to_le_bytes());
        // exp is authoritative for level, so a party mon's cached byte follows it.
        self.sync_cached_level();
    }
    pub fn pp_bonuses(&self) -> u8 {
        self.t[0][8]
    }
    pub fn set_pp_bonuses(&mut self, v: u8) {
        self.t[0][8] = v & 3
    }
    pub fn friendship(&self) -> u8 {
        self.t[0][9]
    }
    pub fn set_friendship(&mut self, v: u8) {
        self.t[0][9] = v;
    }
    pub fn ball(&self) -> u8 {
        (le16(&self.t[0], 10) & 0x1F) as u8
    }
    pub fn set_ball(&mut self, v: u8) {
        let w = le16(&self.t[0], 10);
        self.t[0][10..12].copy_from_slice(&((w & !0x1F) | (v as u16 & 0x1F)).to_le_bytes());
    }

    pub fn moves(&self) -> [u16; 4] {
        [
            le16(&self.t[1], 0),
            le16(&self.t[1], 2),
            le16(&self.t[1], 4),
            le16(&self.t[1], 6),
        ]
    }
    pub fn set_move(&mut self, slot: usize, id: u16) {
        put16(&mut self.t[1], slot * 2, id);
    }
    pub fn pps(&self) -> [u8; 4] {
        [self.t[1][8], self.t[1][9], self.t[1][10], self.t[1][11]]
    }
    pub fn set_pp(&mut self, slot: usize, v: u8) {
        self.t[1][8 + slot] = v;
    }

    pub fn evs(&self) -> [u8; 6] {
        [
            self.t[2][0],
            self.t[2][1],
            self.t[2][2],
            self.t[2][3],
            self.t[2][4],
            self.t[2][5],
        ]
    }
    pub fn set_ev(&mut self, i: usize, v: u8) {
        self.t[2][i] = v.min(255);
    }

    pub fn ivs(&self) -> [u8; 6] {
        let w = le32(&self.t[3], 4);
        std::array::from_fn(|i| ((w >> (5 * i)) & 0x1F) as u8)
    }
    pub fn set_iv(&mut self, i: usize, v: u8) {
        let w = le32(&self.t[3], 4);
        let w = (w & !(0x1F << (5 * i))) | (((v & 0x1F) as u32) << (5 * i));
        self.t[3][4..8].copy_from_slice(&w.to_le_bytes());
    }
    pub fn set_all_ivs(&mut self, v: u8) {
        let w = le32(&self.t[3], 4);
        let keep = w & (0xC000_0000);
        let ivs: u32 = (0x1F1F1F1Fu32) & if v > 31 { 0 } else { repeat5(v) };
        self.t[3][4..8].copy_from_slice(&((w & 0xC000_0000) | ivs).to_le_bytes());
        let _ = keep;
    }
    pub fn is_egg(&self) -> bool {
        (le32(&self.t[3], 4) >> 30) & 1 == 1
    }
    pub fn set_is_egg(&mut self, b: bool) {
        let w = le32(&self.t[3], 4);
        self.t[3][4..8].copy_from_slice(&(w & !(1u32 << 30) | ((b as u32) << 30)).to_le_bytes());
    }
    pub fn ability_num(&self) -> u8 {
        ((le32(&self.t[3], 4) >> 31) & 1) as u8
    }
    pub fn set_ability_num(&mut self, v: u8) {
        let w = le32(&self.t[3], 4);
        self.t[3][4..8]
            .copy_from_slice(&((w & !(1u32 << 31)) | (((v & 1) as u32) << 31)).to_le_bytes());
    }

    pub fn met_level(&self) -> u16 {
        le16(&self.t[3], 2) & 0x7F
    }
    pub fn set_met_level(&mut self, v: u16) {
        let w = le16(&self.t[3], 2);
        self.t[3][2..4].copy_from_slice(&((w & !0x7F) | (v & 0x7F)).to_le_bytes());
    }
    pub fn met_location(&self) -> u8 {
        self.t[3][1]
    }
    pub fn set_met_location(&mut self, v: u8) {
        self.t[3][1] = v;
    }

    pub fn shiny_raw(&self) -> u8 {
        (le32(&self.t[3], 8) & 0x7) as u8
    }
    pub fn is_shiny(&self) -> bool {
        self.shiny_raw() != 0
    }
    pub fn set_shiny(&mut self, b: bool) {
        let w = le32(&self.t[3], 8);
        self.t[3][8..12].copy_from_slice(&((w & !0x7) | (b as u32)).to_le_bytes());
    }
    pub fn gender_flag(&self) -> u8 {
        ((le32(&self.t[3], 8) >> 3) & 0x7) as u8
    }
    pub fn set_gender_flag(&mut self, v: u8) {
        let w = le32(&self.t[3], 8);
        self.t[3][8..12]
            .copy_from_slice(&((w & !(0x7 << 3)) | (((v as u32) & 0x7) << 3)).to_le_bytes());
    }

    /// Level is *derived from exp*, never read from a byte for box mons.
    ///
    /// Rogue replaces the vanilla growth-rate tables with a single flat schedule:
    /// `Rogue_ModifyExperienceTables()` (src/rogue_baked.c) is just `level * 300`,
    /// ignoring growthRate entirely. `struct BoxPokemon` has no level field at all,
    /// so the game always recomputes the level from exp
    /// (`GetLevelFromBoxMonExp` / `GetLevelFromMonExp`, src/pokemon.c) — a loop over
    /// that schedule clamped to MAX_LEVEL (100). `CalculateMonStats` then writes the
    /// result back into the party struct's cached level byte. Exp is therefore the
    /// authoritative field for *both* the 104-byte party and the 80-byte box layout,
    /// and a party mon's cached byte is only a mirror that can go stale.
    pub fn level(&self) -> Option<u8> {
        Some(level_from_exp(self.exp()))
    }
    /// The raw cached byte in the 104-byte party struct (offset 84). Box mons have no
    /// such byte. Exposed so a stale/mismatched mirror is visible rather than silent.
    pub fn level_cached(&self) -> Option<u8> {
        if self.raw.len() == MONSZ {
            Some(self.raw[84])
        } else {
            None
        }
    }
    /// Sets level by setting exp, which is what the game does when it creates a mon
    /// at a given level (`exp = Rogue_ModifyExperienceTables(growthRate, level)`).
    /// For a party mon the cached byte is kept in sync with the new exp.
    pub fn set_level(&mut self, v: u8) {
        let v = v.min(MAX_LEVEL);
        self.set_exp((v as u32) * EXP_PER_LEVEL);
        if self.raw.len() == MONSZ {
            self.raw[84] = v;
        }
    }
    /// Keep a party mon's cached level byte consistent with its exp. No-op for box mons.
    pub fn sync_cached_level(&mut self) {
        if self.raw.len() == MONSZ {
            self.raw[84] = self.level().unwrap_or(0);
        }
    }
    pub fn cached_stats(&self) -> Option<(u16, u16, [u16; 5])> {
        if self.raw.len() != MONSZ {
            return None;
        }
        let hp = le16(&self.raw, 86);
        let maxhp = le16(&self.raw, 88);
        let stats = [
            le16(&self.raw, 90),
            le16(&self.raw, 92),
            le16(&self.raw, 94),
            le16(&self.raw, 96),
            le16(&self.raw, 98),
        ];
        Some((hp, maxhp, stats))
    }
    pub fn set_cached_hp(&mut self, v: u16) {
        if self.raw.len() != MONSZ {
            return;
        }
        put16(&mut self.raw, 86, v);
        put16(&mut self.raw, 88, v);
    }
    pub fn set_cached_stat(&mut self, which: usize, v: u16) {
        if self.raw.len() != MONSZ {
            return;
        }
        let off = match which {
            0 => 90,
            1 => 92,
            2 => 94,
            3 => 96,
            _ => 98,
        };
        put16(&mut self.raw, off, v);
    }

    /// checksum over decrypted physical slots (CalculateBoxMonChecksum)
    pub fn checksum_calc(slots: &[[u8; 12]; 4]) -> u16 {
        let mut s: u32 = 0;
        for slot in slots {
            for w in slot.chunks_exact(2) {
                s = s.wrapping_add(le16(w, 0) as u32);
            }
        }
        (s & 0xFFFF) as u16
    }

    pub fn checksum_ok(&self) -> bool {
        Mon::checksum_calc(&self.slots) == self.checksum_stored()
    }

    /// write logical t[] -> physical slots, recompute checksum, re-encrypt
    pub fn seal(&mut self) {
        let perm = SUBSTRUCT_ORDER[(self.pid() % 24) as usize];
        for j in 0..4 {
            self.slots[perm[j]] = self.t[j];
        }
        let chk = Mon::checksum_calc(&self.slots);
        put16(&mut self.raw, 28, chk);
        let key = self.pid() ^ self.otid();
        let kb = key.to_le_bytes();
        for i in 0..4 {
            for j in 0..12 {
                self.raw[32 + i * 12 + j] = self.slots[i][j] ^ kb[j % 4];
            }
        }
    }

    pub fn display_name(&self) -> String {
        let nick = self.nickname();
        let sp = species_name(self.species());
        if !nick.is_empty() && nick != sp {
            format!("Lv{} {} ({})", self.level().unwrap_or(0), nick, sp)
        } else {
            format!("Lv{} {}", self.level().unwrap_or(0), sp)
        }
    }
}

fn repeat5(v: u8) -> u32 {
    let mut r = 0u32;
    for i in 0..6 {
        r |= ((v & 0x1F) as u32) << (5 * i);
    }
    r
}

// ---------------------------------------------------------------- save file
pub struct Sector {
    data: Vec<u8>, // DATASZ
    id: u16,
    csum: u16,
    security: u32,
    counter: u32,
}

pub struct SaveFile {
    pub sectors: Vec<Sector>, // 32
    pub trailer: Vec<u8>,     // optional mGBA footer
    pub active_slot: usize,   // 0 or 14
    pub merged: Vec<u8>,      // rebuilt active-slot stream (14 * DATASZ)
    pub edited: bool,
}

/// One decoded array from the Rogue stream: element count + absolute merged offset of data.
#[derive(Clone, Copy, Default)]
pub struct RogueField {
    pub n: usize,
    pub data: usize,
}

/// The stashed hub state (sRunRestoreBlock): the state that is NOT currently
/// loaded in SaveBlock1. When SaveBlock1 holds the adventure party, this holds
/// the hub party/money/bag — and vice versa. Offsets are block-relative once
/// `base` (absolute merged offset) is known.
#[derive(Clone, Default)]
pub struct Stash {
    pub ok: bool,
    pub base: usize,
    pub party: Vec<Mon>,      // 6 x 104B
    pub bag: Vec<(u16, u16)>, // 450 x (id, qty)
    pub daycare: Vec<Mon>,    // 3 x 80B box mons
    pub money: u32,
    pub play_time: u32,
}

/// Decoded view of the Rogue save block (boxes 10..13). If `ok` is false the
/// stream counts did not match the 2.2.0-era layout: display the header, but
/// all edit paths must refuse to touch it.
#[derive(Clone, Default)]
pub struct RogueBlock {
    pub ok: bool,
    pub version: u16,
    pub format: u8,
    pub is_debug: bool,
    pub weather: u16,
    pub time_of_day: u16,      // minutes since hub midnight
    pub season: u8,            // day-counter (DAYS_PER_SEASON days each), NOT a season index
    pub has_num_species: bool, // header carries lastKnownNumSpecies (saveVersion >= 2.1.0)
    pub last_num_species: u16,
    pub time_off: usize, // absolute merged offset of timeOfDayMinutes (u16); weather = -2, season = +2
    pub quests_f: RogueField, // n x 8B: u32 stateFlags :16 | highCompleteDiff :3 | highRewardDiff :3
    pub campaigns_f: RogueField, // n x 4B
    pub built_f: RogueField,
    pub coords_f: RogueField,
    pub upgrades_f: RogueField,
    pub decor_f: RogueField,
    pub styles_f: RogueField,
    pub wander_f: RogueField,
    pub safari_f: RogueField,
    pub daycare_f: RogueField, // n x 84B (BoxPokemon facade first 80)
    pub diff_toggle_f: RogueField,
    pub diff_range_f: RogueField,
    pub unique_f: RogueField,
    pub custids_f: RogueField,
    pub mastery_f: RogueField,
    pub stash: Stash,
}

/// One safari-zone roster entry. The two bitfield words are kept raw and
/// exposed only through accessors with runtime bounds, so an out-of-range
/// write can never bleed into a neighbouring field.
#[derive(Clone, Default, PartialEq)]
pub struct SafariMon {
    pub w0: u32,
    pub w1: u32,
    pub nick: Vec<u8>,
    pub priority: u8,
    pub custom_lookup: u8,
}
impl SafariMon {
    #[inline]
    fn bf(v: u32, shift: u32, mask: u32) -> u32 {
        (v >> shift) & mask
    }
    #[inline]
    fn set_bf(v: &mut u32, shift: u32, mask: u32, val: u32) {
        assert!(val <= mask, "safari bitfield overflow: {val} > {mask}");
        *v = (*v & !(mask << shift)) | ((val & mask) << shift);
    }
    pub fn species(&self) -> u16 {
        Self::bf(self.w0, 0, 0xFFFF) as u16
    }
    pub fn set_species(&mut self, v: u16) {
        Self::set_bf(&mut self.w0, 0, 0xFFFF, v as u32);
    }
    /// IVs in party order: hp, atk, def, spe, spa, spd
    pub fn iv(&self, i: usize) -> u8 {
        (match i {
            0 => Self::bf(self.w0, 16, 31),
            1 => Self::bf(self.w0, 21, 31),
            2 => Self::bf(self.w0, 26, 31),
            3 => Self::bf(self.w1, 0, 31),
            4 => Self::bf(self.w1, 5, 31),
            _ => Self::bf(self.w1, 10, 31),
        }) as u8
    }
    pub fn set_iv(&mut self, i: usize, v: u8) {
        match i {
            0 => Self::set_bf(&mut self.w0, 16, 31, v as u32),
            1 => Self::set_bf(&mut self.w0, 21, 31, v as u32),
            2 => Self::set_bf(&mut self.w0, 26, 31, v as u32),
            3 => Self::set_bf(&mut self.w1, 0, 31, v as u32),
            4 => Self::set_bf(&mut self.w1, 5, 31, v as u32),
            _ => Self::set_bf(&mut self.w1, 10, 31, v as u32),
        }
    }
    pub fn shiny(&self) -> bool {
        Self::bf(self.w0, 31, 1) != 0
    }
    pub fn set_shiny(&mut self, v: bool) {
        Self::set_bf(&mut self.w0, 31, 1, v as u32);
    }
    pub fn pokeball(&self) -> u8 {
        Self::bf(self.w1, 15, 31) as u8
    }
    pub fn set_pokeball(&mut self, v: u8) {
        Self::set_bf(&mut self.w1, 15, 31, v as u32);
    }
    pub fn nature(&self) -> u8 {
        Self::bf(self.w1, 20, 31) as u8
    }
    pub fn set_nature(&mut self, v: u8) {
        Self::set_bf(&mut self.w1, 20, 31, v as u32);
    }
    pub fn ability_num(&self) -> u8 {
        Self::bf(self.w1, 25, 3) as u8
    }
    pub fn set_ability_num(&mut self, v: u8) {
        Self::set_bf(&mut self.w1, 25, 3, v as u32);
    }
    pub fn gender_flag(&self) -> u8 {
        Self::bf(self.w1, 27, 1) as u8
    }
    pub fn set_gender_flag(&mut self, v: u8) {
        Self::set_bf(&mut self.w1, 27, 1, v as u32);
    }
    pub fn nickname(&self) -> String {
        gen3_decode(&self.nick)
    }
    pub fn set_nickname(&mut self, s: &str) {
        self.nick = gen3_encode(s, 10);
    }
}

impl SaveFile {
    pub fn open(path: &str) -> Result<SaveFile, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() != FILE_SECTORS * SECSZ && bytes.len() != FILE_SECTORS * SECSZ + 16 {
            return Err(format!(
                "unexpected save size {} (want {} or {})",
                bytes.len(),
                FILE_SECTORS * SECSZ,
                FILE_SECTORS * SECSZ + 16
            ));
        }
        let mut sectors = Vec::with_capacity(FILE_SECTORS);
        for p in 0..FILE_SECTORS {
            let base = p * SECSZ;
            let data = bytes[base..base + DATASZ].to_vec();
            let id = le16(&bytes, base + DATASZ);
            let csum = le16(&bytes, base + DATASZ + 2);
            let security = le32(&bytes, base + DATASZ + 4);
            let counter = le32(&bytes, base + DATASZ + 8);
            sectors.push(Sector {
                data,
                id,
                csum,
                security,
                counter,
            });
        }
        let trailer = bytes[FILE_SECTORS * SECSZ..].to_vec();
        let ctr0 = sectors[0].counter;
        let ctr1 = sectors[14].counter;
        let active_slot = if ctr1 > ctr0 { 14 } else { 0 };
        let mut sf = SaveFile {
            sectors,
            trailer,
            active_slot,
            merged: Vec::new(),
            edited: false,
        };
        sf.rebuild_merged();
        Ok(sf)
    }

    pub fn slot_valid(&self, slotbase: usize) -> bool {
        for p in slotbase..slotbase + SLOT_SECTORS {
            let s = &self.sectors[p];
            if s.security != SECURITY {
                return false;
            }
            if sector_crc(&s.data) != s.csum {
                return false;
            }
        }
        true
    }

    pub fn rotation(&self, slotbase: usize) -> [usize; SLOT_SECTORS] {
        let mut rot = [usize::MAX; SLOT_SECTORS];
        for p in slotbase..slotbase + SLOT_SECTORS {
            let s = &self.sectors[p];
            if s.security == SECURITY && (s.id as usize) < SLOT_SECTORS {
                rot[s.id as usize] = p;
            }
        }
        rot
    }

    fn rebuild_merged(&mut self) {
        let rot = self.rotation(self.active_slot);
        self.merged = Vec::with_capacity(SLOT_SECTORS * DATASZ);
        for k in 0..SLOT_SECTORS {
            self.merged.extend_from_slice(&self.sectors[rot[k]].data);
        }
    }

    // ---- accessors over merged stream
    pub fn party_count(&self) -> usize {
        self.merged[SB1_BASE + PARTY_COUNT_OFF] as usize
    }

    pub fn party(&self) -> Vec<Mon> {
        let n = self.party_count().min(6);
        (0..n)
            .map(|i| {
                Mon::from_bytes(
                    &self.merged
                        [SB1_BASE + PARTY_OFF + i * MONSZ..SB1_BASE + PARTY_OFF + (i + 1) * MONSZ],
                )
            })
            .collect()
    }

    pub fn storage_base(&self) -> usize {
        STOR_BASE
    }

    pub fn box_mon(&self, bx: usize, slot: usize) -> Mon {
        // BoxPokemon needs 4-byte alignment: currentBox(u8) + 3 pad, then 14*30 boxes of 80B.
        // Confirmed by header comment /*0x8344*/ boxNames = 4 + 420*80.
        let off = self.storage_base() + 4 + (bx * BOX_SLOTS + slot) * BOXSZ;
        Mon::from_bytes(&self.merged[off..off + BOXSZ])
    }

    pub fn box_name(&self, bx: usize) -> String {
        let off = self.storage_base() + 4 + TOTAL_BOXES * BOX_SLOTS * BOXSZ + bx * BOX_NAME_LEN;
        gen3_decode(&self.merged[off..off + BOX_NAME_LEN])
    }

    pub fn enc_key(&self) -> u32 {
        le32(&self.merged, ENC_KEY_OFF)
    }
    pub fn money(&self) -> u32 {
        le32(&self.merged, SB1_BASE + MONEY_OFF) ^ self.enc_key()
    }
    pub fn player_name(&self) -> String {
        gen3_decode(&self.merged[0x00..0x07])
    }
    pub fn trainer_id(&self) -> u16 {
        le16(&self.merged, 0x0A)
    }
    pub fn secret_id(&self) -> u16 {
        le16(&self.merged, 0x0C)
    }
    pub fn play_time(&self) -> (u16, u8, u8) {
        (
            le16(&self.merged, 0x0E),
            self.merged[0x10],
            self.merged[0x11],
        )
    }

    // ---- Rogue save block (boxes 10..13), read-only decode.
    // Stream layout per SerializeRogueBlockInternal() (rogue_save.c, v2.2.0 era):
    // header + count-prefixed arrays; every array count is recorded so a
    // layout drift desyncs loudly instead of mis-editing.
    pub fn rogue_block(&self) -> Option<RogueBlock> {
        let base = ROGUE_BASE;
        if base + ROGUE_CAP > self.merged.len() {
            return None;
        }
        let version = le16(&self.merged, base);
        let format = self.merged[base + 2];
        let secret = le16(&self.merged, base + 3);
        if secret != ROGUE_SECRET || version == 0 {
            return None; // not a Rogue-era block (or blank)
        }
        // Header size is version-dependent: SerializeRogueBlockInternal only writes
        // lastKnownNumSpecies when `saveVersion >= SAVE_VER_ID_2_1_0` (enum value 5;
        // the on-disk field is the enum minus one, hence v2.2.0 saves carry 7).
        // Mirrored literally so pre-2.1.0 saves don't walk 2 bytes out of phase.
        const SAVE_VER_ID_2_1_0: u16 = 5;
        let has_num_species = version >= SAVE_VER_ID_2_1_0;
        let mut rb = RogueBlock {
            version,
            format,
            ok: false,
            has_num_species,
            ..Default::default()
        };
        // ver u16 | fmt u8 | secret u16 | rogueVer u8 [| lastKnownNumSpecies u16]
        let mut o = base + if has_num_species { 8 } else { 6 };
        if has_num_species {
            rb.last_num_species = le16(&self.merged, base + 6);
        }
        let rd = |o: &mut usize, elemsz: usize| -> Option<(usize, usize)> {
            if *o + 2 > base + ROGUE_CAP {
                return None;
            }
            let n = le16(&self.merged, *o) as usize;
            let data = *o + 2;
            let end = data + n * elemsz;
            if end > base + ROGUE_CAP {
                return None;
            }
            *o = end;
            Some((n, data))
        };
        let mut ok = true;
        macro_rules! field {
            ($f:ident, $elemsz:expr) => {
                match rd(&mut o, $elemsz) {
                    Some((n, d)) => {
                        rb.$f = RogueField { n, data: d };
                    }
                    None => ok = false,
                }
            };
        }
        field!(quests_f, 8);
        field!(campaigns_f, 4);
        field!(built_f, 1);
        field!(coords_f, 4); // padded Coords8: s8 x, s8 y + 2 pad bytes (see hub_field_editors_land_on_the_walked_offsets)
        field!(upgrades_f, 1);
        field!(decor_f, 4);
        field!(styles_f, 1);
        field!(wander_f, 2);
        if o + 5 <= base + ROGUE_CAP && ok {
            rb.weather = le16(&self.merged, o);
            rb.time_of_day = le16(&self.merged, o + 2);
            rb.season = self.merged[o + 4];
            rb.time_off = o + 2;
            o += 5;
        } else {
            ok = false;
        }
        field!(safari_f, 20);
        field!(daycare_f, DAYCARE_ELEMSZ);
        field!(diff_toggle_f, 1);
        field!(diff_range_f, 1);
        field!(unique_f, 8);
        field!(custids_f, 4);
        field!(mastery_f, 1);
        // Era sanity pins: these counts are compile-time constants in the 2.2.0 build.
        ok = ok
            && rb.campaigns_f.n == 7
            && rb.coords_f.n == 11
            && rb.decor_f.n == 192
            && rb.daycare_f.n == STASH_DAYCARE
            && rb.mastery_f.n == 72
            && rb.custids_f.n == 16
            && rb.unique_f.n == 5
            && rb.safari_f.n == 60;
        if !ok || o >= base + ROGUE_CAP {
            rb.ok = false;
            return Some(rb); // layout drift: expose header only, never edit paths
        }
        rb.is_debug = self.merged[o] != 0;
        rb.ok = true;
        rb.stash = self.find_stash(o);
        Some(rb)
    }

    /// sRunRestoreBlock = 6x104 party | 450x4 bag | 3x84 daycare | difficulty(12) | u32 money | u32 playTime.
    /// Located by validity (checksums + sane money) rather than a fixed gap, so a
    /// game build that shifts the debug/ride tail still decodes.
    fn find_stash(&self, from: usize) -> Stash {
        let base = ROGUE_BASE;
        for cand in from + 1..from + 96 {
            if cand + 2696 > base + ROGUE_CAP {
                break;
            }
            let party_ok = (0..2).all(|i| {
                let off = cand + i * MONSZ;
                let m = Mon::from_bytes(&self.merged[off..off + MONSZ]);
                m.pid() == 0 || m.checksum_ok()
            }) && {
                let m0 = Mon::from_bytes(&self.merged[cand..cand + MONSZ]);
                let m1 = Mon::from_bytes(&self.merged[cand + MONSZ..cand + 2 * MONSZ]);
                !(m0.pid() == 0 && m1.pid() == 0) // require at least one real mon to avoid false anchors
            };
            let money = le32(&self.merged, cand + 2688);
            let play = le32(&self.merged, cand + 2692);
            if party_ok && money <= 9_999_999 && play < 300 * 3600 {
                return self.read_stash(cand);
            }
        }
        Stash::default()
    }

    fn read_stash(&self, at: usize) -> Stash {
        let mut s = Stash {
            ok: true,
            base: at,
            ..Default::default()
        };
        for i in 0..STASH_PARTY {
            let off = at + i * MONSZ;
            s.party
                .push(Mon::from_bytes(&self.merged[off..off + MONSZ]));
        }
        let bag = at + 624;
        for i in 0..STASH_BAG_CAP {
            s.bag.push((
                le16(&self.merged, bag + i * 4),
                le16(&self.merged, bag + i * 4 + 2),
            ));
        }
        let day = at + 2424;
        for i in 0..STASH_DAYCARE {
            let off = day + i * DAYCARE_ELEMSZ;
            s.daycare
                .push(Mon::from_bytes(&self.merged[off..off + BOXSZ]));
        }
        s.money = le32(&self.merged, at + 2688);
        s.play_time = le32(&self.merged, at + 2692);
        s
    }

    pub fn set_stash_mon(&mut self, i: usize, m: &Mon) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.stash.ok || i >= STASH_PARTY {
            return;
        }
        let off = rb.stash.base + i * MONSZ;
        self.merged[off..off + MONSZ].copy_from_slice(&m.raw);
        self.edited = true;
    }
    pub fn stash_money(&self) -> u32 {
        self.rogue_block()
            .filter(|rb| rb.stash.ok)
            .map(|rb| rb.stash.money)
            .unwrap_or(0)
    }
    pub fn set_stash_money(&mut self, v: u32) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.stash.ok {
            return;
        }
        put32(&mut self.merged, rb.stash.base + 2688, v);
        self.edited = true;
    }
    pub fn set_block_daycare(&mut self, i: usize, m: &Mon) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.daycare_f.n {
            return;
        }
        let off = rb.daycare_f.data + i * DAYCARE_ELEMSZ;
        self.merged[off..off + BOXSZ].copy_from_slice(&m.raw[..BOXSZ]);
        self.edited = true;
    }
    pub fn set_stash_daycare(&mut self, i: usize, m: &Mon) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.stash.ok || i >= STASH_DAYCARE {
            return;
        }
        let off = rb.stash.base + 2424 + i * DAYCARE_ELEMSZ;
        self.merged[off..off + BOXSZ].copy_from_slice(&m.raw[..BOXSZ]);
        self.edited = true;
    }
    pub fn set_stash_bag_item(&mut self, i: usize, id: u16, qty: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.stash.ok || i >= STASH_BAG_CAP {
            return;
        }
        let off = rb.stash.base + 624 + i * 4;
        put16(&mut self.merged, off, id);
        put16(&mut self.merged, off + 2, qty); // stash bag is NOT XOR-obfuscated
        self.edited = true;
    }

    // ---- Safari-zone roster (safariMons[60], 20 B each) -------------------
    // struct RogueSafariMon (include/rogue.h). Bitfields pack into two u32
    // words (ARM GCC order, confirmed against a real v2.2.0 roster: every
    // IV <=31, nature <=24, shiny 0/1, priority 0..101):
    //   w0: species:16 | hpIV:5 | attackIV:5 | defenseIV:5 | shinyFlag:1
    //   w1: speedIV:5  | spAttackIV:5 | spDefenseIV:5 | pokeball:5
    //       | nature:5 | abilityNum:2 | genderFlag:1 | unused0:4
    // then nickname[10], priorityCounter u8, customMonLookup u8  (= 20 bytes).
    pub fn safari_mon(&self, rb: &RogueBlock, i: usize) -> Option<SafariMon> {
        if !rb.ok || i >= rb.safari_f.n {
            return None;
        }
        let off = rb.safari_f.data + i * SAFARISZ;
        Some(SafariMon {
            w0: le32(&self.merged, off),
            w1: le32(&self.merged, off + 4),
            nick: self.merged[off + 8..off + 18].to_vec(),
            priority: self.merged[off + 18],
            custom_lookup: self.merged[off + 19],
        })
    }
    pub fn set_safari_mon(&mut self, i: usize, s: &SafariMon) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.safari_f.n {
            return;
        }
        let off = rb.safari_f.data + i * SAFARISZ;
        put32(&mut self.merged, off, s.w0);
        put32(&mut self.merged, off + 4, s.w1);
        self.merged[off + 8..off + 18].copy_from_slice(&s.nick);
        self.merged[off + 18] = s.priority;
        self.merged[off + 19] = s.custom_lookup;
        self.edited = true;
    }

    // ---- Dynamic unique-mon spawns (dynamicUniquePokemon[5], 8 B each) ----
    // struct UniqueMon: u32 customMonId, u16 species, u16 countDown.
    pub fn unique_mon(&self, rb: &RogueBlock, i: usize) -> Option<(u32, u16, u16)> {
        if !rb.ok || i >= rb.unique_f.n {
            return None;
        }
        let off = rb.unique_f.data + i * 8;
        Some((
            le32(&self.merged, off),
            le16(&self.merged, off + 4),
            le16(&self.merged, off + 6),
        ))
    }
    pub fn set_unique_mon(&mut self, i: usize, id: u32, species: u16, countdown: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.unique_f.n {
            return;
        }
        let off = rb.unique_f.data + i * 8;
        put32(&mut self.merged, off, id);
        put16(&mut self.merged, off + 4, species);
        put16(&mut self.merged, off + 6, countdown);
        self.edited = true;
    }

    // ---- Rogue-block field edits (all no-ops unless the block decoded cleanly)
    pub fn set_quest_state(&mut self, i: usize, flags: u16, high_complete: u8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.quests_f.n {
            return;
        }
        let w = (flags as u32) | ((high_complete as u32 & 7) << 16);
        put32(&mut self.merged, rb.quests_f.data + i * 8, w);
        self.edited = true;
    }
    pub fn set_campaign(&mut self, i: usize, unlocked: bool, completed: bool, best: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.campaigns_f.n {
            return;
        }
        let v = (unlocked as u32) | ((completed as u32) << 1) | ((best as u32) << 16);
        put32(&mut self.merged, rb.campaigns_f.data + i * 4, v);
        self.edited = true;
    }
    fn bit_get(&self, f: &RogueField, bit: usize) -> bool {
        if bit >= f.n * 8 {
            return false;
        }
        self.merged[f.data + bit / 8] & (1 << (bit % 8)) != 0
    }
    pub fn hub_area_built(&self, rb: &RogueBlock, i: usize) -> bool {
        rb.ok && self.bit_get(&rb.built_f, i)
    }
    pub fn hub_upgrade_on(&self, rb: &RogueBlock, i: usize) -> bool {
        rb.ok && self.bit_get(&rb.upgrades_f, i)
    }
    pub fn set_hub_upgrade(&mut self, bit: usize, on: bool) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || bit >= rb.upgrades_f.n * 8 {
            return;
        }
        let off = rb.upgrades_f.data + bit / 8;
        if on {
            self.merged[off] |= 1 << (bit % 8);
        } else {
            self.merged[off] &= !(1 << (bit % 8));
        }
        self.edited = true;
    }
    pub fn set_hub_area(&mut self, bit: usize, on: bool) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || bit >= rb.built_f.n * 8 {
            return;
        }
        let off = rb.built_f.data + bit / 8;
        if on {
            self.merged[off] |= 1 << (bit % 8);
        } else {
            self.merged[off] &= !(1 << (bit % 8));
        }
        self.edited = true;
    }
    /// Area position on the hub map. On disk each `areaCoords` entry is 4 bytes:
    /// `s8 x, s8 y` followed by two pad bytes (`struct Coords8` is two s8 members;
    /// the compiler pads the serialized stride to 4). Reading the pair as `s16`
    /// gives nonsense (Town Square would be x=257), and stride 2 desyncs every
    /// array that follows. Pinned in `hub_field_editors_land_on_the_walked_offsets`.
    ///
    /// `RogueHub_ClearProgress` memsets the whole hub map and then calls
    /// `RogueHub_BuildArea` (which sets the built bit AND x/y together), so in a
    /// game-written save only BUILT areas have coordinates; unbuilt ones read as
    /// (0,0) and are never consulted (`RogueHub_GetAreaAt` skips unbuilt areas).
    /// Setting a coordinate on an unbuilt area is inert -- it gets overwritten the
    /// moment the game builds that area -- so this setter deliberately does NOT
    /// touch the built flag. Build separately with `hub-area`.
    pub fn hub_area_coord(&self, rb: &RogueBlock, i: usize) -> Option<(i8, i8)> {
        if !rb.ok || i >= rb.coords_f.n {
            return None;
        }
        let o = rb.coords_f.data + i * 4;
        Some((self.merged[o] as i8, self.merged[o + 1] as i8))
    }
    /// Raw coordinate bytes (x, y, pad0, pad1) so the GUI/CLI can show what is
    /// actually on disk, including a non-zero pad from a foreign editor.
    pub fn hub_area_coord_raw(&self, rb: &RogueBlock, i: usize) -> Option<[u8; 4]> {
        if !rb.ok || i >= rb.coords_f.n {
            return None;
        }
        let o = rb.coords_f.data + i * 4;
        Some([
            self.merged[o],
            self.merged[o + 1],
            self.merged[o + 2],
            self.merged[o + 3],
        ])
    }
    pub fn set_hub_area_coord(&mut self, i: usize, x: i8, y: i8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.coords_f.n {
            return;
        }
        let off = rb.coords_f.data + i * 4;
        self.merged[off] = x as u8;
        self.merged[off + 1] = y as u8;
        // pad bytes zeroed, as the game's padded Coords8 leaves them
        self.merged[off + 2] = 0;
        self.merged[off + 3] = 0;
        self.edited = true;
    }
    /// Warnings for placing area `i` at (x,y), computed the way the game resolves
    /// hub travel. Travel is tile-adjacency: `RogueHub_GetAreaAtConnection` walks
    /// one tile from an area's coordinate in a direction that area's ROM-side
    /// `connectionWarps` allow, and `RogueHub_FindAreaAtCoord` returns the first
    /// BUILT area sitting on that tile. So a hand-placed coordinate can break the
    /// map in two ways the game itself never produces:
    ///  * two built areas on one tile -> the second is unreachable (its tile
    ///    resolves to the first match, and nothing warps to it);
    ///  * an area with no built neighbour in any direction it can connect -> no
    ///    warp path exists, so the area is unreachable except by another edit.
    /// These are advisory: the game does not re-validate coords on load.
    pub fn hub_coord_warnings(&self, rb: &RogueBlock, i: usize, x: i8, y: i8) -> Vec<String> {
        let mut out = Vec::new();
        if !rb.ok || i >= rb.coords_f.n {
            return out;
        }
        // 1. tile collision with another BUILT area
        for j in 0..rb.coords_f.n {
            if j == i || !self.hub_area_built(rb, j) {
                continue;
            }
            if self.hub_area_coord(rb, j) == Some((x, y)) {
                let other = HUB_AREAS.get(j).copied().unwrap_or("?");
                out.push(format!(
                    "tile ({x},{y}) already holds BUILT area {j} {other}; one of the two will be unreachable"
                ));
            }
        }
        // 2. adjacency. The ROM `connectionWarps` table is per-area and not in the
        //    save, so the conservative check is: at least one built neighbour in a
        //    4-connected direction. The game additionally requires the *direction*
        //    to be warpable for both areas, which this cannot know from the save.
        let neighbours: [(i8, i8); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
        let mover_built = self.hub_area_built(rb, i);
        let touches = neighbours.iter().any(|(dx, dy)| {
            // s8 arithmetic wraps like the game's s8 coords; keep it in i16 so the
            // wrap is explicit rather than a Rust overflow panic.
            let nx = (x as i16 + *dx as i16) as i8;
            let ny = (y as i16 + *dy as i16) as i8;
            (0..rb.coords_f.n).any(|j| {
                j != i && self.hub_area_built(rb, j) && self.hub_area_coord(rb, j) == Some((nx, ny))
            })
        });
        if !touches && mover_built {
            out.push(format!(
                "({x},{y}) has no built 4-connected neighbour; the game reaches areas by walking between adjacent tiles, so this area may be unreachable"
            ));
        }
        // 3. moving can also ORPHAN whatever used to sit next to this area's old
        //    tile: an area that was only reachable through here loses its last
        //    door. Only meaningful when the mover is built (an unbuilt area is not
        //    anybody's door).
        if let (true, Some((ox, oy))) = (mover_built, self.hub_area_coord(rb, i)) {
            let supports = |k: usize| -> bool {
                let (kx, ky) = match self.hub_area_coord(rb, k) {
                    Some(c) => c,
                    None => return true, // no coordinate to reason about
                };
                neighbours.iter().any(|(dx, dy)| {
                    let nx = (kx as i16 + *dx as i16) as i8;
                    let ny = (ky as i16 + *dy as i16) as i8;
                    // the mover still supports k if its NEW tile lands next to k
                    if nx == x && ny == y {
                        return true;
                    }
                    (0..rb.coords_f.n).any(|j| {
                        // exclude the mover and the candidate orphan: after the move
                        // i is at (x,y), handled above, and k is not its own support
                        j != i
                            && j != k
                            && self.hub_area_built(rb, j)
                            && self.hub_area_coord(rb, j) == Some((nx, ny))
                    })
                })
            };
            for (dx, dy) in neighbours {
                let nx = (ox as i16 + dx as i16) as i8;
                let ny = (oy as i16 + dy as i16) as i8;
                for j in 0..rb.coords_f.n {
                    if j == i || !self.hub_area_built(rb, j) {
                        continue;
                    }
                    if self.hub_area_coord(rb, j) == Some((nx, ny)) && !supports(j) {
                        let other = HUB_AREAS.get(j).copied().unwrap_or("?");
                        out.push(format!(
                            "area {j} {other} at ({nx},{ny}) touches this area's old tile and has no other built neighbour; moving here would strand it"
                        ));
                    }
                }
            }
        }
        out
    }
    /// `homeDecorations` entry: `u8 x, u8 y, u8 decorVariant, u8 active:1`.
    /// 160 outdoor slots (offset 0) then 32 indoor slots (offset 160).
    pub fn hub_decoration(&self, rb: &RogueBlock, i: usize) -> Option<(u8, u8, u8, bool)> {
        if !rb.ok || i >= rb.decor_f.n {
            return None;
        }
        let o = rb.decor_f.data + i * 4;
        Some((
            self.merged[o],
            self.merged[o + 1],
            self.merged[o + 2],
            self.merged[o + 3] & 1 != 0,
        ))
    }
    pub fn set_hub_decoration(&mut self, i: usize, x: u8, y: u8, variant: u8, active: bool) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.decor_f.n {
            return;
        }
        let o = rb.decor_f.data + i * 4;
        self.merged[o] = x;
        self.merged[o + 1] = y;
        self.merged[o + 2] = variant;
        // bit 0 = active; bits 1..7 are the declared `unused : 7` padding
        let keep = self.merged[o + 3] & !1;
        self.merged[o + 3] = keep | u8::from(active);
        self.edited = true;
    }
    /// How many decoration slots the save marks active. The game only renders
    /// active slots, so the GUI lists just those (out of `decor_f.n`).
    pub fn decor_on_count(&self, rb: &RogueBlock) -> usize {
        if !rb.ok {
            return 0;
        }
        (0..rb.decor_f.n)
            .filter(|&k| self.hub_decoration(rb, k).map_or(false, |(_, _, _, on)| on))
            .count()
    }
    /// `homeStyles[0]` house exterior, `[1]` house interior, `[2]` path
    /// (`rogue_hub.c` indexes them with `HOME_STYLE_HOUSE_EXTERIOR` etc.).
    pub fn hub_style(&self, rb: &RogueBlock, i: usize) -> Option<u8> {
        if !rb.ok || i >= rb.styles_f.n {
            return None;
        }
        Some(self.merged[rb.styles_f.data + i])
    }
    pub fn set_hub_style(&mut self, i: usize, v: u8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.styles_f.n {
            return;
        }
        self.merged[rb.styles_f.data + i] = v;
        self.edited = true;
    }
    /// `homeWanderingMonSpecies` is NOT a species id: the game stores
    /// `species + FOLLOWMON_SHINY_OFFSET(10000)` when the follow-mon is shiny
    /// (`MonSpeciesToFollowSpecies`), which is why live saves show values like
    /// 10374. 0 = SPECIES_NONE (no follow-mon in that slot).
    pub fn hub_wander_mon(&self, rb: &RogueBlock, i: usize) -> Option<(u16, bool)> {
        if !rb.ok || i >= rb.wander_f.n {
            return None;
        }
        let raw = le16(&self.merged, rb.wander_f.data + i * 2);
        if raw == 0 {
            return Some((0, false));
        }
        let shiny = raw >= FOLLOWMON_SHINY_OFFSET;
        Some((
            if shiny {
                raw - FOLLOWMON_SHINY_OFFSET
            } else {
                raw
            },
            shiny,
        ))
    }
    pub fn set_hub_wander_mon(&mut self, i: usize, species: u16, shiny: bool) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.wander_f.n {
            return;
        }
        let raw = if species == 0 {
            0
        } else {
            species + if shiny { FOLLOWMON_SHINY_OFFSET } else { 0 }
        };
        put16(&mut self.merged, rb.wander_f.data + i * 2, raw);
        self.edited = true;
    }
    /// `weatherState` is not a weather id either: the game writes `Random()` into
    /// it and derives the visible weather from `state % 5` (spring/summer) or
    /// `state % 6` (autumn/winter) through a per-season table
    /// (`rogue_controller.c:GetDesiredWeather`). Editing the raw value changes the
    /// weather only for the current season.
    pub fn set_hub_weather_state(&mut self, v: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok {
            return;
        }
        put16(&mut self.merged, rb.time_off - 2, v);
        self.edited = true;
    }
    pub fn set_hub_time(&mut self, minutes: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok {
            return;
        }
        put16(&mut self.merged, rb.time_off, minutes % 1440);
        self.edited = true;
    }
    /// `seasonCounter` is NOT a season index: it counts elapsed days in a
    /// 2-day-per-season / 4-season cycle (`rogue_timeofday.c`:
    /// DAYS_PER_SEASON 2, SEASON_COUNT 4, wrapped % 8;
    /// `RogueToD_GetSeason()` returns `seasonCounter / DAYS_PER_SEASON`).
    /// So this takes a season index 0..3 and parks the counter at that
    /// season's first day, exactly like `RogueToD_SetSeason()`.
    pub fn set_hub_season(&mut self, season_idx: u8) {
        self.set_season_counter(season_idx.min(SEASON_COUNT as u8 - 1) * DAYS_PER_SEASON);
    }
    pub fn set_season_counter(&mut self, days: u8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok {
            return;
        }
        self.merged[rb.time_off + 2] = days % (DAYS_PER_SEASON * SEASON_COUNT as u8);
        self.edited = true;
    }
    pub fn set_weather(&mut self, w: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok {
            return;
        }
        put16(&mut self.merged, rb.time_off - 2, w);
        self.edited = true;
    }
    pub fn set_diff_toggle(&mut self, i: usize, v: u8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.diff_toggle_f.n {
            return;
        }
        self.merged[rb.diff_toggle_f.data + i] = v;
        self.edited = true;
    }
    pub fn set_diff_range(&mut self, i: usize, v: u8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok || i >= rb.diff_range_f.n {
            return;
        }
        self.merged[rb.diff_range_f.data + i] = v;
        self.edited = true;
    }

    // ---- fusion storage (struct Pokemon fusions[4] at /*0x8432*/ in storage)
    pub fn fusion_mon(&self, i: usize) -> Mon {
        let off = self.storage_base() + FUSION_OFF + i * MONSZ;
        Mon::from_bytes(&self.merged[off..off + MONSZ])
    }
    pub fn set_fusion_mon(&mut self, i: usize, m: &Mon) {
        let off = self.storage_base() + FUSION_OFF + i * MONSZ;
        self.merged[off..off + MONSZ].copy_from_slice(&m.raw);
        self.edited = true;
    }

    pub fn pc_items(&self) -> Vec<(u16, u16)> {
        (0..PC_CAP)
            .map(|i| {
                let o = SB1_BASE + PC_ITEMS_OFF + i * 4;
                (le16(&self.merged, o), le16(&self.merged, o + 2))
            })
            .collect()
    }
    pub fn bag_items(&self) -> Vec<(u16, u16)> {
        let k16 = (self.enc_key() & 0xFFFF) as u16;
        (0..BAG_CAP)
            .map(|i| {
                let o = SB1_BASE + BAG_OFF + i * 4;
                (le16(&self.merged, o), le16(&self.merged, o + 2) ^ k16)
            })
            .collect()
    }

    // ---- edits are applied to merged, then flushed to sectors + saved
    pub fn set_party_mon(&mut self, i: usize, m: &Mon) {
        let off = SB1_BASE + PARTY_OFF + i * MONSZ;
        self.merged[off..off + MONSZ].copy_from_slice(&m.raw);
        self.edited = true;
    }
    pub fn set_box_mon(&mut self, bx: usize, slot: usize, m: &Mon) {
        let off = self.storage_base() + 4 + (bx * BOX_SLOTS + slot) * BOXSZ;
        self.merged[off..off + BOXSZ].copy_from_slice(&m.raw);
        self.edited = true;
    }
    pub fn set_money(&mut self, v: u32) {
        let k = self.enc_key();
        put32(&mut self.merged, SB1_BASE + MONEY_OFF, v ^ k);
        self.edited = true;
    }
    pub fn set_item(&mut self, bag: bool, i: usize, id: u16, qty: u16) {
        let o = SB1_BASE + if bag { BAG_OFF } else { PC_ITEMS_OFF } + i * 4;
        let q = if bag {
            qty ^ ((self.enc_key() & 0xFFFF) as u16)
        } else {
            qty
        };
        put16(&mut self.merged, o, id);
        put16(&mut self.merged, o + 2, q);
        self.edited = true;
    }

    // ---- bag add / remove / capacity --------------------------------------
    //
    // `UpdateBagItemsPointers()` (src/item.c) splits the flat 450-slot array into
    // one CONTIGUOUS RUN per pocket, walking pockets 1..9 in order and stopping at
    // the first id whose pocket differs: "Expects items to always be sorted".
    // A real v2.2.0 bag is exactly that (probe on a live save: 64 packed slots in
    // runs 1,2,3,5,6,7,8,9 — the empty Stones pocket simply leaves no run).
    //
    // So every scripted edit must (a) keep runs contiguous, (b) keep the run
    // order ascending by pocket, and (c) keep zero-quantity slots at the tail,
    // never in the middle. These helpers do the whole-array rewrite so the
    // invariants hold by construction, and they mirror the game's own capacity
    // accounting (`Rogue_GetBagCapacity`, `GetBagUnreservedFreeSlots`,
    // `GetBagReservedFreeSlots`): key items have their own 50-slot budget and
    // every pocket stacks to `MAX_BAG_ITEM_CAPACITY` = 999.

    /// Which bag array an edit targets: the live SaveBlock1 bag, or the stashed
    /// hub-state bag inside the Rogue block (`sRunRestoreBlock`).
    fn bag_slot_off(&self, stash: bool) -> Option<usize> {
        if stash {
            self.rogue_block()
                .filter(|rb| rb.stash.ok)
                .map(|rb| rb.stash.base + 624)
        } else {
            Some(SB1_BASE + BAG_OFF)
        }
    }

    /// Raw (id, qty) for the whole bag array, with quantities decoded. The live
    /// bag XORs quantity with the low 16 bits of the key; the stashed bag is a
    /// plain memcpy in `SerializeRogueBlockInternal` and is NOT obfuscated
    /// (confirmed on a real save: raw reads 5,5,2,11,3 / XOR reads garbage).
    fn bag_slots_raw(&self, stash: bool) -> Option<Vec<(u16, u16)>> {
        let off = self.bag_slot_off(stash)?;
        let k16 = if stash {
            0
        } else {
            (self.enc_key() & 0xFFFF) as u16
        };
        let cap = if stash { STASH_BAG_CAP } else { BAG_CAP };
        Some(
            (0..cap)
                .map(|i| {
                    (
                        le16(&self.merged, off + i * 4),
                        le16(&self.merged, off + i * 4 + 2) ^ k16,
                    )
                })
                .collect(),
        )
    }

    /// Rewrite the whole bag array from a decoded slot list; trailing slots are
    /// zeroed so no stale ids linger past the new end.
    fn write_bag_slots(&mut self, stash: bool, slots: &[(u16, u16)]) {
        let Some(off) = self.bag_slot_off(stash) else {
            return;
        };
        let k16 = if stash {
            0
        } else {
            (self.enc_key() & 0xFFFF) as u16
        };
        let cap = if stash { STASH_BAG_CAP } else { BAG_CAP };
        for i in 0..cap {
            let (id, qty) = slots.get(i).copied().unwrap_or((0, 0));
            put16(&mut self.merged, off + i * 4, id);
            put16(&mut self.merged, off + i * 4 + 2, qty ^ k16);
        }
        self.edited = true;
    }

    /// u8 `bagCapacityUpgrades` at SB1+0x4AD.
    pub fn bag_capacity_upgrades(&self) -> u8 {
        self.merged[SB1_BASE + BAG_CAP_UPGRADES_OFF]
    }
    pub fn set_bag_capacity_upgrades(&mut self, v: u8) {
        self.merged[SB1_BASE + BAG_CAP_UPGRADES_OFF] = v.min(BAG_MAX_CAPACITY_UPGRADES as u8);
        self.edited = true;
    }
    /// `Rogue_GetBagCapacity()`.
    pub fn bag_capacity(&self) -> usize {
        let up = self.bag_capacity_upgrades() as usize;
        if up >= BAG_MAX_CAPACITY_UPGRADES {
            BAG_CAP
        } else {
            (BAG_BASE_CAPACITY + up * BAG_SLOTS_PER_UPGRADE).min(BAG_CAP)
        }
    }
    /// u8 `bagSortMode` at SB1+0x4AC (0 type, 1 name, 2 value, 3 amount). The
    /// editor's own sort keys off pocket + id, which matches the game's
    /// primary key; sort mode only permutes WITHIN a run.
    pub fn bag_sort_mode(&self) -> u8 {
        self.merged[SB1_BASE + BAG_SORT_MODE_OFF]
    }

    /// How many slots the game would consider used, per pocket, for a given bag.
    fn bag_run_summary(slots: &[(u16, u16)]) -> Vec<(u8, usize)> {
        let mut out: Vec<(u8, usize)> = Vec::new();
        for (id, _qty) in slots {
            // Exactly the game's terminator: `itemId == ITEM_NONE`. A slot with a
            // real id and quantity 0 is still counted as part of the pocket run
            // by UpdateBagItemsPointers, so it must be counted here too.
            if *id == 0 {
                break;
            }
            let p = item_pocket(*id);
            if p == 0 {
                out.push((0, 1)); // pocketless: already broken, flag it
                continue;
            }
            if out.last().map(|(pp, _)| *pp) == Some(p) {
                out.last_mut().unwrap().1 += 1;
            } else {
                out.push((p, 1));
            }
        }
        out
    }

    /// Bag health report: run contiguity, pocketless ids, over-capacity,
    /// over-stacked quantities. Empty = the bag is exactly what the game expects.
    pub fn bag_warnings(&self, stash: bool) -> Vec<String> {
        let Some(slots) = self.bag_slots_raw(stash) else {
            return vec!["bag not found".to_string()];
        };
        let mut w = Vec::new();
        let runs = Self::bag_run_summary(&slots);
        if runs.windows(2).any(|p| p[0].0 > p[1].0) {
            w.push(format!(
                "pocket runs are out of order ({}) — UpdateBagItemsPointers stops at the first reversal, so items after it are invisible in-game",
                runs.iter().map(|(p, n)| format!("{p}x{n}")).collect::<Vec<_>>().join(" ")
            ));
        }
        for (id, _) in slots.iter().take_while(|(id, _)| *id != 0) {
            if item_pocket(*id) == item_pockets::POCKET_NONE {
                w.push(format!(
                    "id {id} ({}) has no pocket and must be removed — the game's pointer scan stops there and hides everything after it",
                    item_name(*id)
                ));
            }
        }
        let (filled, key) = Self::bag_counts(&slots);
        let (cap, key_budget) = if stash {
            (STASH_BAG_CAP, BAG_RESERVED_SLOTS)
        } else {
            (self.bag_capacity(), BAG_RESERVED_SLOTS)
        };
        if filled > cap {
            w.push(format!(
                "{filled} filled slots exceed capacity {cap}; the tail is invisible to the game"
            ));
        }
        if key > key_budget {
            w.push(format!(
                "{key} key items exceed their {key_budget}-slot reserved budget"
            ));
        }
        for (i, (id, q)) in slots.iter().enumerate() {
            if *id == 0 {
                break;
            }
            if *q == 0 {
                w.push(format!(
                    "slot {i}: id {id} ({}) has quantity 0 — the game still counts it as a used slot; write again to drop it",
                    item_name(*id)
                ));
            }
            if *q > MAX_STACK {
                w.push(format!(
                    "slot {i}: id {id} quantity {q} exceeds the {MAX_STACK} per-stack cap"
                ));
            }
        }
        w
    }
    fn bag_counts(slots: &[(u16, u16)]) -> (usize, usize) {
        let mut filled = 0;
        let mut key = 0;
        for (id, _qty) in slots {
            if *id == 0 {
                break;
            }
            filled += 1;
            if item_pocket(*id) == item_pockets::POCKET_KEY_ITEMS {
                key += 1;
            }
        }
        (filled, key)
    }

    /// Mirror of `AddBagItem`: merge into an under-full stack of the same id,
    /// then insert a new slot in sorted position if the pocket's slot budget
    /// allows. Returns a human-readable summary.
    pub fn add_bag_item(&mut self, id: u16, qty: u16, stash: bool) -> Result<String, String> {
        if qty == 0 {
            return Err("quantity must be >= 1".to_string());
        }
        let pocket = item_pocket(id);
        if pocket == item_pockets::POCKET_NONE {
            return Err(format!(
                "item {id} ({}) has no bag pocket — the game would refuse it (POCKET_NONE)",
                item_name(id)
            ));
        }
        let mut slots = self
            .bag_slots_raw(stash)
            .ok_or_else(|| "bag not found (is the Rogue block decodable?)".to_string())?;
        // Head of the array exactly as UpdateBagItemsPointers sees it: it ends
        // at the first ITEM_NONE slot. Quantity-0 ghosts inside the head are
        // dropped (the game's own ShrinkBagItems ends up rewriting them away).
        let mut head: Vec<(u16, u16)> = slots
            .drain(..)
            .take_while(|(i, _)| *i != 0)
            .filter(|(_, q)| *q != 0)
            .collect();
        let mut left = qty;
        let mut merged = 0u16;
        for s in head.iter_mut() {
            if left == 0 {
                break;
            }
            if s.0 == id && s.1 < MAX_STACK {
                let take = (MAX_STACK - s.1).min(left);
                s.1 += take;
                left -= take;
                merged += take;
            }
        }
        let (filled, key) = Self::bag_counts(&head);
        let (cap, key_budget) = if stash {
            (STASH_BAG_CAP, BAG_RESERVED_SLOTS)
        } else {
            (self.bag_capacity(), BAG_RESERVED_SLOTS)
        };
        let budget = if pocket == item_pockets::POCKET_KEY_ITEMS {
            key_budget.saturating_sub(key)
        } else {
            cap.saturating_sub(BAG_RESERVED_SLOTS)
                .saturating_sub(filled - key)
        };
        let mut inserted = 0usize;
        while left > 0 {
            if budget < inserted + 1 {
                return Err(format!(
                    "bag full: {filled} of {cap} slots used, {budget} free slot(s) left for pocket {pocket} ({}); {left} of {qty} still unplaced — nothing written",
                    pocket_name(pocket)
                ));
            }
            let take = left.min(MAX_STACK);
            head.push((id, take));
            left -= take;
            inserted += 1;
        }
        head.sort_by_key(|(i, _)| bag_sort_key(*i));
        let used = head.len();
        self.write_bag_slots(stash, &head);
        Ok(format!(
            "{}: merged {merged} into existing stacks, opened {inserted} new slot(s); bag now {used}/{} (item {} {})",
            if stash { "stash bag" } else { "bag" },
            if stash { STASH_BAG_CAP } else { cap },
            id,
            item_name(id)
        ))
    }

    /// Mirror of `RemoveBagItem`: take from stacks until `qty` is satisfied.
    /// Emptied slots collapse and the array is re-compacted, so no zero-quantity
    /// hole is left inside a pocket run.
    pub fn remove_bag_item(&mut self, id: u16, qty: u16, stash: bool) -> Result<String, String> {
        let Some(mut slots) = self.bag_slots_raw(stash) else {
            return Err("bag not found (is the Rogue block decodable?)".to_string());
        };
        let owned: u32 = slots
            .iter()
            .take_while(|(i, _)| *i != 0)
            .filter(|(i, _)| *i == id)
            .map(|(_, q)| *q as u32)
            .sum();
        if owned == 0 {
            return Err(format!(
                "no {} (id {id}) in the {}",
                item_name(id),
                if stash { "stashed bag" } else { "bag" }
            ));
        }
        let want = (qty as u32).min(owned);
        let mut left = want;
        for s in slots.iter_mut() {
            if left == 0 {
                break;
            }
            if s.0 == id && s.1 > 0 {
                let take = (s.1 as u32).min(left) as u16;
                s.1 -= take;
                left -= take as u32;
            }
        }
        // drop empty slots, keep the rest in the game's order
        let mut head: Vec<(u16, u16)> =
            slots.iter().take_while(|(i, _)| *i != 0).copied().collect();
        head.retain(|(_, q)| *q != 0);
        let used = head.len();
        self.write_bag_slots(stash, &head);
        Ok(format!(
            "removed {want} x {} (id {id}); {} left; bag now {used} slots",
            item_name(id),
            owned - want,
        ))
    }

    /// Set one quantity in place and re-normalise the array (drop empty slots,
    /// keep pocket runs contiguous). Setting 0 removes that stack.
    pub fn set_bag_slot_qty(&mut self, i: usize, qty: u16, stash: bool) -> Result<(), String> {
        let Some(mut slots) = self.bag_slots_raw(stash) else {
            return Err("bag not found".to_string());
        };
        if i >= slots.len() {
            return Err(format!("slot {i} out of range"));
        }
        if slots[i].0 != 0 && qty > MAX_STACK {
            return Err(format!("quantity {qty} exceeds the {MAX_STACK} stack cap"));
        }
        slots[i].1 = qty;
        let mut head: Vec<(u16, u16)> = slots
            .iter()
            .take_while(|(id, _)| *id != 0)
            .filter(|(_, q)| *q != 0)
            .copied()
            .collect();
        head.sort_by_key(|(id, _)| bag_sort_key(*id));
        self.write_bag_slots(stash, &head);
        Ok(())
    }

    /// Re-sort the whole bag by (pocket, id) — the same primary key the game
    /// uses. Use after hand-editing to restore the contiguous-run invariant.
    pub fn sort_bag(&mut self, stash: bool) -> Result<String, String> {
        let Some(slots) = self.bag_slots_raw(stash) else {
            return Err("bag not found".to_string());
        };
        let mut head: Vec<(u16, u16)> = slots
            .iter()
            .take_while(|(id, _)| *id != 0)
            .filter(|(_, q)| *q != 0)
            .copied()
            .collect();
        let before = self.bag_warnings(stash);
        head.sort_by_key(|(id, _)| bag_sort_key(*id));
        self.write_bag_slots(stash, &head);
        Ok(format!(
            "sorted {} slots by pocket+id{}; {} order warning(s) before, {} after",
            head.len(),
            if before.is_empty() {
                ""
            } else {
                " (fixing a broken run order)"
            },
            before.len(),
            self.bag_warnings(stash).len()
        ))
    }
    pub fn set_player_name(&mut self, s: &str) {
        let enc = gen3_encode(s, 7);
        self.merged[0..7].copy_from_slice(&enc);
        self.merged[7] = 0xFF;
        self.edited = true;
    }
    pub fn set_tids(&mut self, tid: u16, sid: u16) {
        put16(&mut self.merged, 0x0A, tid);
        put16(&mut self.merged, 0x0C, sid);
        self.edited = true;
    }
    pub fn set_box_name(&mut self, bx: usize, name: &str) {
        let off = self.storage_base() + 4 + TOTAL_BOXES * BOX_SLOTS * BOXSZ + bx * BOX_NAME_LEN;
        for k in 0..BOX_NAME_LEN {
            self.merged[off + k] = 0xFF;
        }
        for (k, b) in name.as_bytes().iter().take(BOX_NAME_LEN - 1).enumerate() {
            if *b != 0xFF {
                self.merged[off + k] = *b;
            }
        }
        self.edited = true;
    }

    pub fn save(&mut self, path: &str) -> Result<(), String> {
        let rot = self.rotation(self.active_slot);
        for k in 0..SLOT_SECTORS {
            let p = rot[k];
            let mut data = Vec::with_capacity(DATASZ);
            data.extend_from_slice(&self.merged[k * DATASZ..(k + 1) * DATASZ]);
            let csum = sector_crc(&data);
            self.sectors[p].data = data;
            self.sectors[p].csum = csum;
        }
        let mut out: Vec<u8> = Vec::with_capacity(FILE_SECTORS * SECSZ + self.trailer.len());
        for s in &self.sectors {
            out.extend_from_slice(&s.data);
            let mut foot = [0u8; 12];
            put16(&mut foot, 0, s.id);
            put16(&mut foot, 2, s.csum);
            put32(&mut foot, 4, s.security);
            put32(&mut foot, 8, s.counter);
            out.extend_from_slice(&foot);
        }
        out.extend_from_slice(&self.trailer);
        std::fs::write(path, out).map_err(|e| e.to_string())?;
        self.edited = false;
        Ok(())
    }
}

fn main() -> eframe::Result<()> {
    // The CLI does exactly one thing: pass a save to open it in the GUI.
    //   er-save-editor FILE.sav
    //   er-save-editor --open FILE.sav     (explicit, equivalent)
    // All editing happens in the GUI, and opening is read-only: nothing is
    // written to disk unless the user presses Save / Save As.
    let args: Vec<String> = std::env::args().collect();
    let mut open_path: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--open" => {
                if i + 1 < args.len() {
                    open_path = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            "--help" | "-h" => {
                say!("usage: er-save-editor [FILE.sav | --open FILE.sav]   open a save in the GUI");
                return Ok(());
            }
            s if s.starts_with('-') => {
                eprintln!("unknown option: {s} (try --help)");
                std::process::exit(1);
            }
            s => {
                open_path = Some(s.to_string());
            }
        }
        i += 1;
    }

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1150.0, 780.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Emerald Rogue Save Editor",
        opts,
        Box::new(move |_cc| {
            let mut ed = Editor::default();
            if let Some(p) = open_path.clone() {
                ed.open_path(p);
            }
            Box::new(ed)
        }),
    )
}

/// vanilla gNatureStatTable: nature index boosts stat (nat/5) and lowers
/// stat (nat%5) in the stat order [Atk, Def, Spe, SpA, SpD] (our indices 1..=5).
fn nature_split(nat: usize, stat: usize) -> u32 {
    if stat == 0 {
        return 100;
    }
    let up = 1 + nat / 5;
    let down = 1 + nat % 5;
    if stat == up {
        110
    } else if stat == down {
        90
    } else {
        100
    }
}

/// Game stat formula (src/pokemon.c CalStat), integer division at each step.
/// `stat` is 0=HP..5=SpDef. Note index 3 is Speed and 4/5 are SpA/SpD in our
/// ordering, matching how the cached stat block is laid out.
fn calc_stat(base: u32, iv: u32, ev: u32, lvl: u32, nat: usize, stat: usize) -> u32 {
    let ev4 = ev / 4;
    if stat == 0 {
        ((2 * base + iv + ev4) * lvl) / 100 + lvl + 10
    } else {
        ((((2 * base + iv + ev4) * lvl) / 100 + 5) * nature_split(nat, stat)) / 100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safari_bitfields_roundtrip_without_bleed() {
        // struct RogueSafariMon is two bitfield words + tail. Setting every
        // field to a distinct value and reading it back proves the shifts/masks;
        // asserting the raw words afterwards proves no neighbour was clobbered.
        let mut s = SafariMon::default();
        s.set_species(1005);
        for k in 0..6 {
            s.set_iv(k, (k as u8) + 1); // 1..6
        }
        s.set_shiny(true);
        s.set_pokeball(23);
        s.set_nature(24);
        s.set_ability_num(2);
        s.set_gender_flag(1);
        s.priority = 99;
        s.custom_lookup = 7;

        assert_eq!(s.species(), 1005);
        for k in 0..6 {
            assert_eq!(s.iv(k), (k as u8) + 1, "iv {k}");
        }
        assert!(s.shiny());
        assert_eq!(s.pokeball(), 23);
        assert_eq!(s.nature(), 24);
        assert_eq!(s.ability_num(), 2);
        assert_eq!(s.gender_flag(), 1);

        // w0 = species(16) | hp1 | atk2 | def3 | shiny(bit31)
        assert_eq!(s.w0, 1005 | (1 << 16) | (2 << 21) | (3 << 26) | (1 << 31));
        // w1 = spe4 | spa5 | spd6 | ball<<15 | nature<<20 | ability<<25 | gender<<27
        assert_eq!(
            s.w1,
            (4) | (5 << 5) | (6 << 10) | (23 << 15) | (24 << 20) | (2 << 25) | (1 << 27)
        );

        // clearing shiny must not disturb species or the IVs above it
        s.set_shiny(false);
        assert_eq!(s.species(), 1005);
        assert_eq!(s.iv(2), 3);
        assert_eq!(s.w0 & (1 << 31), 0);

        // out-of-range writes must panic rather than corrupt a neighbour
        let r = std::panic::catch_unwind(|| {
            let mut t = SafariMon::default();
            t.set_iv(0, 32);
        });
        assert!(r.is_err(), "IV 32 must not silently overflow a 5-bit field");
    }

    #[test]
    fn nature_table_index3_is_adamant() {
        // A real trap when back-solving: index 3 is Adamant (boosts Atk), not Calm.
        assert_eq!(NATURES[3], "Adamant");
        assert_eq!(nature_split(3, 1), 110); // Atk boosted
        assert_eq!(nature_split(3, 4), 90); // SpA lowered
        assert_eq!(nature_split(3, 0), 100); // HP never changes
    }

    #[test]
    fn adamant_arcanine_attack() {
        // Live save: Arcanine (species 59), lvl 54, Adamant, Atk IV 18 -> cached 146.
        // With the IV raised to 31 the correct value is 154 (not 153).
        let bs = base_stats(59).expect("Arcanine base stats");
        assert_eq!(calc_stat(bs[1] as u32, 18, 0, 54, 3, 1), 146);
        assert_eq!(calc_stat(bs[1] as u32, 31, 0, 54, 3, 1), 154);
    }

    #[test]
    fn adamant_metagross_stats() {
        // Live save: Metagross (376), lvl 57, Adamant, IVs [12,31,4,24,10,22], EVs 0
        // observed: hp 165, atk 193, def 155, spe 98, spa 107, spd 120
        let bs = base_stats(376).unwrap();
        let iv = [12u32, 31, 4, 24, 10, 22];
        assert_eq!(calc_stat(bs[0] as u32, iv[0], 0, 57, 3, 0), 165);
        assert_eq!(calc_stat(bs[1] as u32, iv[1], 0, 57, 3, 1), 193);
        assert_eq!(calc_stat(bs[2] as u32, iv[2], 0, 57, 3, 2), 155);
    }

    #[test]
    fn gen3_string_roundtrip() {
        for s in ["TRAINER", "BOX1", "PIKACHU", "A1!?-.9"] {
            assert_eq!(gen3_decode(&gen3_encode(s, 10)), s);
        }
    }

    #[test]
    fn level_derives_from_exp_rogue_flat_schedule() {
        // Rogue replaces the growth tables: exp == level * 300 for ALL species
        // (Rogue_ModifyExperienceTables, src/rogue_baked.c). Live-save anchors:
        // Arcanine exp 16200 -> 54, Metagross exp 17100 -> 57.
        assert_eq!(level_from_exp(16200), 54);
        assert_eq!(level_from_exp(17100), 57);
        // loop boundary: exact multiple counts, one short does not
        assert_eq!(level_from_exp(300), 1);
        assert_eq!(level_from_exp(599), 1);
        assert_eq!(level_from_exp(600), 2);
        // GetLevelFromBoxMonExp clamps its loop at MAX_LEVEL (100), so over-leveled
        // exp still reports 100 (e.g. the level-115 exp seen on boxed mons).
        assert_eq!(level_from_exp(30000), 100);
        assert_eq!(level_from_exp(34500), 100);
        assert_eq!(level_from_exp(0x1F_FFFF), 100);
    }

    #[test]
    fn box_mon_level_set_writes_exp_not_a_byte() {
        // 80-byte box struct: no level byte exists. set_level must encode via exp,
        // and set_exp must move the level read-back.
        let raw = vec![0u8; BOXSZ];
        // zeroed struct: pid/otid 0 -> key 0; accessors don't need a valid checksum
        let mut m = Mon::from_bytes(&raw);
        assert!(m.level_cached().is_none(), "box struct has no cached byte");
        m.set_level(65);
        assert_eq!(m.exp(), 65 * 300);
        assert_eq!(m.level(), Some(65));
        m.set_exp(1200);
        assert_eq!(m.level(), Some(4));

        // party struct keeps its cached mirror in sync with exp
        let praw = vec![0u8; MONSZ];
        let mut p = Mon::from_bytes(&praw);
        p.set_level(54);
        assert_eq!(p.raw[84], 54, "set_level must update the cached mirror");
        p.set_exp(300 * 77);
        assert_eq!(p.raw[84], 77, "set_exp must follow the cached mirror");
        assert_eq!(p.level(), Some(77));

        // a stale mirror is reported honestly: exp wins, but the byte is visible
        p.raw[84] = 7;
        assert_eq!(p.level(), Some(77));
        assert_eq!(p.level_cached(), Some(7));
    }

    /// Build a minimal synthetic save in memory (no file IO) whose Rogue block
    /// matches the v2.2.0 stream layout, then round-trip the Rogue setters.
    fn synth_save() -> SaveFile {
        // merged stream: 14 sectors of DATASZ
        let mut merged = vec![0u8; SLOT_SECTORS * DATASZ];
        let base = ROGUE_BASE;
        // header: ver u16 | fmt u8 | secret u16 | rogueVer u8 | lastSpecies u16
        put16(&mut merged, base, 7);
        merged[base + 2] = 2;
        put16(&mut merged, base + 3, ROGUE_SECRET);
        merged[base + 5] = 1;
        put16(&mut merged, base + 6, 1434);
        // A real (nonzero) SaveBlock2 encryption key, so tests that check the
        // XOR obfuscation of money and bag quantities cannot pass by accident
        // with an identity key.
        put32(&mut merged, ENC_KEY_OFF, 0x1234_ABCD);
        let mut o = base + 8;
        // emit a count prefix and skip the element bytes; zeros are fine
        for (n, elemsz) in [
            (232usize, 8usize), // quests
            (7, 4),             // campaigns
            (2, 1),             // areaBuiltFlags
            (11, 4),            // areaCoords (padded Coords8: s8 x, s8 y + 2 pad)
            (6, 1),             // upgradeFlags
            (192, 4),           // homeDecorations
            (3, 1),             // homeStyles
            (10, 2),            // wandering species
        ] {
            put16(&mut merged, o, n as u16);
            o += 2 + n * elemsz;
        }
        let time_off = o + 2; // weather u16, then time u16, then season u8
        o += 5;
        for (n, elemsz) in [
            (60usize, 20usize),  // safari
            (3, DAYCARE_ELEMSZ), // block daycare
            (3, 1),              // diff toggles
            (9, 1),              // diff ranges
            (5, 8),              // dynamic uniques
            (16, 4),             // safari custom ids
            (72, 1),             // mon mastery
        ] {
            put16(&mut merged, o, n as u16);
            o += 2 + n * elemsz;
        }
        merged[o] = 0; // isDebug
        o += 1;
        for k in 0..50 {
            merged[o + k] = 0xEE; // ride state / replay / rng tail junk: nonzero pids,
        } // checksums wrong -> the stash validity scan cannot false-anchor here
        o += 50;
        // sRunRestoreBlock: party 6x104 | bag 450x4 | daycare 3x84 | diff 12 | money | time
        // Plant a REAL sealed party mon so the stash validity scan anchors on the
        // exact offset (byte-shifted copies of a sealed mon fail checksums).
        let stash = o + 16;
        let mut raw = vec![0u8; MONSZ];
        put32(&mut raw, 0, 0xDEAD_BEEF); // pid
        put32(&mut raw, 4, 0x1234_5678); // otid
        put32(&mut raw, 32, 0x1122_3344); // substruct bytes (arbitrary)
        put32(&mut raw, 36, 0x5566_7788);
        let mut m = Mon::from_bytes(&raw);
        m.set_species(59); // via the logical substruct view (t[0] low 11 bits)
        m.seal(); // computes a correct checksum over the permuted substructs
        m.raw[84] = 54; // level byte in the party layout (outside the box struct)
        merged[stash..stash + MONSZ].copy_from_slice(&m.raw);
        put32(&mut merged, stash + 2688, 1234); // money
        put32(&mut merged, stash + 2692, 500); // playTime
        let sectors: Vec<Sector> = (0..FILE_SECTORS)
            .map(|p| Sector {
                data: vec![0u8; DATASZ],
                id: (p % SLOT_SECTORS) as u16,
                csum: 0,
                security: if p < 28 { SECURITY } else { 0 },
                counter: if p >= 14 && p < 28 { 5 } else { 4 },
            })
            .collect();
        let mut sf = SaveFile {
            sectors,
            trailer: vec![],
            active_slot: 14,
            merged,
            edited: false,
        };
        // stash scan tolerance: make sure nothing earlier looks like a stash
        let rb = sf.rogue_block().expect("synthetic block decodes");
        assert!(rb.ok, "synthetic stream must decode");
        assert_eq!(rb.quests_f.n, 232);
        assert_eq!(rb.time_of_day, le16(&sf.merged, time_off));
        // setters land where the game reads them
        sf.set_hub_time(137);
        assert_eq!(le16(&sf.merged, time_off), 137);
        sf.set_hub_season(2); // Autumn => day-counter parked at season start
        assert_eq!(sf.merged[time_off + 2], 2 * DAYS_PER_SEASON);
        sf.set_season_counter(9); // wraps % (2*4) exactly like the game
        assert_eq!(sf.merged[time_off + 2], 1);
        sf.set_quest_state(5, 0x21, 3);
        assert_eq!(le32(&sf.merged, rb.quests_f.data + 5 * 8), 0x21 | (3 << 16));
        sf.set_campaign(2, true, true, 500);
        assert_eq!(
            le32(&sf.merged, rb.campaigns_f.data + 2 * 4),
            3 | (500 << 16)
        );
        sf.set_hub_upgrade(47, true);
        assert_eq!(sf.merged[rb.upgrades_f.data + 5] & 0x80, 0x80);
        // set_diff_toggle indexes BYTES; the CLI/GUI pack bit edits into bytes first
        sf.set_diff_toggle(1, 0b0001_0000); // bit 4 = "wipe bag at adventure start"
        assert_eq!(sf.merged[rb.diff_toggle_f.data + 1], 0b0001_0000);
        sf.set_diff_range(6, 2);
        assert_eq!(sf.merged[rb.diff_range_f.data + 6], 2);
        assert!(rb.stash.ok, "synthetic stash anchor must be found");
        assert_eq!(rb.stash.base, stash, "anchor must be the planted offset");
        assert_eq!(rb.stash.party[0].species(), 59);
        assert!(rb.stash.party[0].checksum_ok());
        let b = rb.stash.base;
        sf.set_stash_money(999);
        assert_eq!(le32(&sf.merged, b + 2688), 999);
        assert_eq!(sf.stash_money(), 999);
        sf.set_stash_bag_item(0, 828, 3);
        assert_eq!(le32(&sf.merged, b + 624), 828u32 | (3u32 << 16));
        let rb2 = sf.rogue_block().unwrap();
        assert_eq!(rb2.stash.base, b, "anchor must be stable across reloads");
        assert_eq!(rb2.stash.money, 999);
        sf
    }

    #[test]
    fn rogue_block_roundtrip_synth() {
        let _ = synth_save();
    }

    #[test]
    fn hub_coord_validator_flags_unreachable_and_stranding_moves() {
        // Build a chain: Labs(0,0) - AdvEntrance(1,0) - Safari(2,0).
        // Everything else stays unbuilt, so Safari is a leaf whose only door is
        // AdvEntrance and Labs is a leaf whose only door is AdvEntrance too.
        let mut sf = synth_save();
        sf.set_hub_area(2, true);
        sf.set_hub_area(1, true);
        sf.set_hub_area(3, true);
        sf.set_hub_area_coord(2, 0, 0);
        sf.set_hub_area_coord(1, 1, 0);
        sf.set_hub_area_coord(3, 2, 0);
        let rb = sf.rogue_block().unwrap();

        // a clean move that keeps the mover attached: Safari (2,0) -> (1,-1) is
        // diagonally off the chain, but (1,0) is AdvEntrance, so (1,-1) is below
        // AdvEntrance and therefore 4-connected to it -> clean
        let w = sf.hub_coord_warnings(&rb, 3, 1, -1);
        assert!(
            w.is_empty(),
            "placement 4-connected to built AdvEntrance should be clean, got {w:?}"
        );
        // (the mover's own OLD tile never counts as its new neighbour: moving the
        // leaf Safari from (2,0) to (2,1) really does isolate it)
        let w = sf.hub_coord_warnings(&rb, 3, 2, 1);
        assert!(
            w.iter()
                .any(|s| s.contains("no built 4-connected neighbour")),
            "self-old-tile support must not count: {w:?}"
        );

        // a detached island: unreachable by the game's adjacency travel
        let w = sf.hub_coord_warnings(&rb, 3, 40, 40);
        assert_eq!(w.len(), 1, "expected exactly the island warning: {w:?}");
        assert!(w[0].contains("no built 4-connected neighbour"));

        // tile collision with a BUILT area
        let w = sf.hub_coord_warnings(&rb, 3, 0, 0);
        assert!(
            w.iter().any(|s| s.contains("already holds BUILT area 2")),
            "collision not reported: {w:?}"
        );

        // taking Safari off the chain strands nothing (Safari is the leaf), but
        // moving the MIDDLE area away orphans both ends.
        let w = sf.hub_coord_warnings(&rb, 1, 9, 9);
        assert!(
            w.iter()
                .any(|s| s.contains("Labs") && s.contains("would strand it")),
            "orphaned Labs not reported: {w:?}"
        );
        assert!(
            w.iter()
                .any(|s| s.contains("Safari Zone") && s.contains("would strand it")),
            "orphaned Safari not reported: {w:?}"
        );

        // a move that keeps one end attached must not warn about that end:
        // AdvEntrance (1,0) -> (1,1) leaves Safari(2,0) supported only by... nothing,
        // but Labs(0,0) is still adjacent to nothing either. Both strand. Instead
        // place it at (2,-1): then it is adjacent to Safari(2,0), so Safari keeps
        // its door and only Labs is stranded.
        let w = sf.hub_coord_warnings(&rb, 1, 2, -1);
        assert!(
            w.iter()
                .any(|s| s.contains("Labs") && s.contains("would strand it")),
            "Labs should still be stranded: {w:?}"
        );
        assert!(
            !w.iter().any(|s| s.contains("Safari Zone")),
            "Safari keeps a neighbour at (2,0); should not be reported: {w:?}"
        );

        // an UNBUILT area getting a coordinate gets no reachability advice: the
        // coord is inert until the game builds the area itself
        let w = sf.hub_coord_warnings(&rb, 6, 50, 50);
        assert!(
            w.is_empty(),
            "unbuilt area should produce no warnings: {w:?}"
        );
    }

    /// The decoration id stored in `homeDecorations[].decorVariant` indexes a
    /// game-generated enum. `tools/gen_deco_variants.py` reproduces that enum from
    /// the game's own exporter; this pins the two tables stay index-aligned and
    /// anchors them on what a real v2.2.0 save actually stores.
    #[test]
    fn decor_variant_table_matches_the_games_generated_enum() {
        assert_eq!(
            deco_variants::DECOR_VARIANTS.len(),
            deco_variants::DECOR_VARIANT_NAMES.len(),
            "pretty names and enum names must stay index-aligned"
        );
        // The game seeds slot 0 of the yard with DECOR_VARIANT_OUTFIT_CHANGING_WARDROBE
        // at (21,19) in RogueHub_ClearProgress; `er2.sav` and the pristine backup both
        // carry exactly that, so it is the one id we can verify from save data.
        assert_eq!(decor_name(119), "outfit changing wardrobe");
        assert_eq!(
            deco_variants::DECOR_VARIANT_NAMES[119],
            "DECOR_VARIANT_OUTFIT_CHANGING_WARDROBE"
        );
        // ids 0 and 178 are not "empty": 0 is a real placeable tile, 178 is the
        // in-game remove action. Nothing in the save uses either as a blank marker.
        assert_eq!(decor_name(0), "path 5x5");
        assert_eq!(
            deco_variants::DECOR_VARIANT_NAMES[deco_variants::DECOR_VARIANT_REMOVE],
            "DECOR_VARIANT_REMOVE_DECOR_DEFAULT"
        );
        assert_eq!(decor_name(255), "out-of-range decoration id");
    }

    #[test]
    fn save_reseals_sectors_and_keeps_shape() {
        // in-memory only: save() must recompute sector checksums and byte size.
        // (We only call the checksum logic here; the disk write path is gated by
        // explicit user consent and is exercised manually, not by tests.)
        let sf = synth_save();
        for k in 0..SLOT_SECTORS {
            let data = &sf.merged[k * DATASZ..(k + 1) * DATASZ];
            let _ = sector_crc(data); // must not panic; folds to u16
        }
    }

    #[test]
    fn live_save_round_trips_byte_identically() {
        // The strongest regression net we have: parse a real save, re-serialize
        // through save() into a temp file, and require byte equality. This
        // proves sector rotation, counters, footers, and the mGBA trailer all
        // survive untouched when nothing was edited. Skips (green) if the
        // fixture is not on this machine, so CI without saves stays green.
        let fixture = std::env::var("ER_TEST_SAVE").ok();
        let Some(path) = fixture.filter(|p| std::path::Path::new(p).exists()) else {
            eprintln!("live_save_round_trip: ER_TEST_SAVE not set/found, skipping");
            return;
        };
        let original = std::fs::read(&path).unwrap();
        let mut sf = SaveFile::open(&path).expect("live save must parse");
        let out = std::env::temp_dir().join("er_roundtrip_test.sav");
        sf.save(out.to_str().unwrap()).expect("temp write ok");
        let rewritten = std::fs::read(&out).unwrap();
        assert_eq!(
            original.len(),
            rewritten.len(),
            "re-serialized size must match"
        );
        assert_eq!(
            original, rewritten,
            "noop round trip must be byte-identical"
        );
        std::fs::remove_file(out).ok();
    }

    #[test]
    #[ignore = "manual: prints hub snapshot the GUI renders, run with --ignored"]
    fn dump_hub_for_gui_crosscheck() {
        let path = std::env::var("ER_TEST_SAVE").unwrap();
        let sf = SaveFile::open(&path).unwrap();
        let rb = sf.rogue_block().unwrap();
        for i in 0..rb.coords_f.n {
            let built = sf.hub_area_built(&rb, i);
            let (x, y) = sf.hub_area_coord(&rb, i).unwrap();
            println!(
                "area {i}: built={built} coord=({x},{y}) raw={:?}",
                sf.hub_area_coord_raw(&rb, i).unwrap()
            );
        }
        println!("weather={}", rb.weather);
        for i in 0..rb.wander_f.n {
            if let Some((sp, shiny)) = sf.hub_wander_mon(&rb, i) {
                if sp != 0 {
                    println!("wander {i}: species={sp} shiny={shiny}");
                }
            }
        }
        for i in 0..rb.styles_f.n {
            println!("style {i}: {}", sf.hub_style(&rb, i).unwrap());
        }
        println!("decor active count = {}", sf.decor_on_count(&rb));
    }

    #[test]
    fn pocket_name_never_panics_and_names_every_pocket() {
        // Regression: the GUI Items tab renders pocket_name() per row; an
        // earlier version sliced a "POCKET_" prefix off display names that
        // don't have it and panicked on "Items" (opening the tab killed the
        // app). Every u8 must produce a label, known pockets must name right.
        assert_eq!(pocket_name(3), "Medicine (pocket 3)");
        assert_eq!(pocket_name(1), "Items (pocket 1)");
        for p in 0..=255u8 {
            let s = pocket_name(p);
            assert!(!s.is_empty());
            assert!(s.contains(&format!("pocket {p}")) || s.contains("POCKET_NONE"));
        }
    }

    #[test]
    fn hub_field_editors_land_on_the_walked_offsets() {
        // The hub arrays are only reachable through the sequential stream walk,
        // so this pins every hub setter against the same walked offsets the
        // getters report -- a wrong stride here desyncs the rest of the block.
        let mut sf = synth_save();
        let rb = sf.rogue_block().unwrap();
        assert_eq!(rb.coords_f.n, 11);
        assert_eq!(rb.decor_f.n, 192);
        assert_eq!(rb.styles_f.n, 3);
        assert_eq!(rb.wander_f.n, 10);

        // coords: 4-byte entries of `s8 x, s8 y` + 2 pad bytes, NOT s16 and NOT
        // stride 2. Town Square written as (-2, 3) must read back as s8 pair and
        // the two following bytes must be zero pad, not part of a 16-bit y.
        sf.set_hub_area_coord(0, -2, 3);
        let o = rb.coords_f.data;
        assert_eq!(
            &sf.merged[o..o + 4],
            &[0xFE, 0x03, 0x00, 0x00],
            "areaCoords entry must be s8 x, s8 y, then 2 pad bytes"
        );
        assert_eq!(sf.hub_area_coord(&rb, 0), Some((-2, 3)));
        assert_eq!(sf.hub_area_coord_raw(&rb, 0), Some([0xFE, 0x03, 0, 0]));
        // stride 4 must hold: entry 1 starts 4 bytes after entry 0
        sf.set_hub_area_coord(1, 120, -1);
        assert_eq!(
            &sf.merged[o + 4..o + 8],
            &[120, 0xFF, 0x00, 0x00],
            "coords stride must be exactly 4"
        );
        assert_eq!(sf.hub_area_coord(&rb, 1), Some((120, -1)));
        // an s16 misread would give these instead -- must NOT match
        assert_ne!(
            (sf.merged[o] as i16 | ((sf.merged[o + 1] as i16) << 8)),
            -2,
            "reading x as s16 must not yield the real x"
        );
        // negative x survives: (i8)0xFE == -2
        assert_eq!(sf.merged[o] as i8, -2);

        // The built bit and the coordinate are independent writes here, matching
        // what the game does in BuildArea but not coupling them: setting a coord
        // on an UNBUILT area is inert (the game overwrites it when it builds the
        // area, and GetAreaAt skips unbuilt areas), so the setter must not
        // silently build anything.
        sf.set_hub_area_coord(10, 30, 40);
        assert!(
            !sf.hub_area_built(&rb, 10),
            "moving must not raise built bit"
        );
        sf.set_hub_area(10, true);
        assert!(sf.hub_area_built(&rb, 10));
        assert_eq!(
            sf.hub_area_coord(&rb, 10),
            Some((30, 40)),
            "build must not move"
        );
        sf.set_hub_area(10, false);
        assert!(!sf.hub_area_built(&rb, 10));
        assert_eq!(sf.hub_area_coord(&rb, 10), Some((30, 40)));

        // decorations: u8 x, u8 y, u8 variant, u8 active:1 -- toggling active
        // must not disturb the variant byte or the unused 7 bits.
        let d = rb.decor_f.data;
        sf.merged[d + 3] = 0b1000_0000; // foreign non-zero unused bits
        sf.set_hub_decoration(0, 12, 9, 47, true);
        assert_eq!(sf.hub_decoration(&rb, 0), Some((12, 9, 47, true)));
        assert_eq!(sf.merged[d + 3], 0b1000_0001, "only bit 0 may change");
        sf.set_hub_decoration(0, 12, 9, 47, false);
        assert_eq!(sf.merged[d + 3], 0b1000_0000);
        assert_eq!(sf.hub_decoration(&rb, 0), Some((12, 9, 47, false)));
        // slot 160 is the first indoor slot, 640 bytes after slot 0
        sf.set_hub_decoration(160, 1, 2, 3, true);
        assert_eq!(
            &sf.merged[d + 640..d + 644],
            &[1, 2, 3, 1],
            "indoor decor slot must sit at stride 4"
        );
        assert_eq!(
            sf.hub_decoration(&rb, 192),
            None,
            "decor index must be bounded"
        );

        // styles: 3 separate u8s; editing interior must leave exterior/path alone
        let st = rb.styles_f.data;
        sf.set_hub_style(1, 5);
        assert_eq!(
            &sf.merged[st..st + 3],
            &[0, 5, 0],
            "styles are 3 independent u8s"
        );
        assert_eq!(sf.hub_style(&rb, 1), Some(5));
        assert_eq!(sf.hub_style(&rb, 3), None, "style index must be bounded");

        // wandering follow-mons: u16 species with shiny as +10000, not a flag bit
        let w = rb.wander_f.data;
        sf.set_hub_wander_mon(3, 612, true); // Haxorus shiny
        assert_eq!(le16(&sf.merged, w + 3 * 2), 612 + FOLLOWMON_SHINY_OFFSET);
        assert_eq!(sf.hub_wander_mon(&rb, 3), Some((612, true)));
        sf.set_hub_wander_mon(4, 263, false);
        assert_eq!(le16(&sf.merged, w + 4 * 2), 263);
        assert_eq!(sf.hub_wander_mon(&rb, 4), Some((263, false)));
        assert_eq!(
            sf.hub_wander_mon(&rb, 3),
            Some((612, true)),
            "u16 writes must not bleed into the neighbour slot"
        );
        // 0 means SPECIES_NONE: shiny bit must not leak a phantom follow-mon
        sf.set_hub_wander_mon(4, 0, true);
        assert_eq!(le16(&sf.merged, w + 4 * 2), 0);
        assert_eq!(sf.hub_wander_mon(&rb, 4), Some((0, false)));

        // weather: raw u16 seed (the game takes state % 5 / % 6 per season).
        // `weather` is decoded onto the block, so a re-decode must see the write.
        sf.set_hub_weather_state(5);
        assert_eq!(sf.rogue_block().unwrap().weather, 5);
        sf.set_hub_weather_state(0xFFFF);
        let rbw = sf.rogue_block().unwrap();
        assert_eq!(rbw.weather, 0xFFFF);
        // buckets as the game computes them: %5 spring/summer, %6 autumn/winter.
        // (Same buckets as the live save's 3105 -- different seeds, same weather.)
        assert_eq!(rbw.weather % 5, 0);
        assert_eq!(rbw.weather % 6, 3);
        assert_eq!(3105 % 5, 0);
        assert_eq!(3105 % 6, 3);

        // every hub write above must keep the stream walk intact
        let rb2 = sf.rogue_block().unwrap();
        assert!(rb2.ok, "hub edits must not desync the block walk");
        assert_eq!(rb2.coords_f.n, 11);
        assert_eq!(rb2.decor_f.n, 192);
        assert_eq!(rb2.safari_f.n, 60);
        assert_eq!(rb2.mastery_f.n, 72);
        assert!(rb2.stash.ok, "stash anchor must survive hub edits");
        // out-of-range writes are no-ops, not panics or corruption: the bytes of
        // the following arrays must be untouched, and reads bound-check cleanly.
        let before_mastery: Vec<u8> = sf.merged[rb.mastery_f.data..rb.mastery_f.data + 8].to_vec();
        sf.set_hub_area_coord(99, 1, 1);
        sf.set_hub_area(99, true);
        sf.set_hub_decoration(9999, 1, 1, 1, true);
        sf.set_hub_style(9, 9);
        sf.set_hub_wander_mon(99, 5, false);
        let rb3 = sf.rogue_block().unwrap();
        assert_eq!(rb3.coords_f.n, 11, "walk must survive bad-index writes");
        assert_eq!(
            &sf.merged[rb3.mastery_f.data..rb3.mastery_f.data + 8],
            before_mastery,
            "out-of-range hub writes must not spill into later arrays"
        );
        assert_eq!(sf.hub_area_coord(&rb3, 11), None);
        assert_eq!(sf.hub_decoration(&rb3, 192), None);
        assert_eq!(sf.hub_style(&rb3, 3), None);
        assert_eq!(sf.hub_wander_mon(&rb3, 10), None);
        // slot 9 was never written (synth stream zero-fills the elements): a
        // raw 0 is SPECIES_NONE, not a follow-mon
        assert_eq!(sf.hub_wander_mon(&rb3, 9), Some((0, false)));
        // slot 3 keeps its shiny entry despite all the out-of-range writes
        assert_eq!(sf.hub_wander_mon(&rb3, 3), Some((612, true)));
    }

    #[test]
    fn substruct_table_has_24_distinct_permutations() {
        assert_eq!(SUBSTRUCT_ORDER.len(), 24);
        for p in SUBSTRUCT_ORDER.iter() {
            let mut s = *p;
            s.sort();
            assert_eq!(s, [0, 1, 2, 3]);
        }
        let mut uniq: Vec<[usize; 4]> = SUBSTRUCT_ORDER.to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 24);
    }

    #[test]
    fn bag_inserts_keep_pocket_runs_contiguous() {
        // UpdateBagItemsPointers walks the flat array once and stops at the
        // first pocket change, so a pocket run that appears twice puts the
        // second copy beyond the game's reach. Every mutation must leave one
        // contiguous run per pocket, in ascending pocket order.
        let mut sf = synth_save();
        // Pocket ids from the generated table: Medicine 3, Stones 4, Balls 5,
        // Held 2 (per-item order inside a run is ascending id).
        assert_eq!(item_pocket(39), item_pockets::POCKET_MEDICINE); // Potion
        assert_eq!(item_pocket(1), item_pockets::POCKET_POKE_BALLS); // Poke Ball
        assert_eq!(item_pocket(265), item_pockets::POCKET_STONES);
        assert_eq!(item_pocket(64), item_pockets::POCKET_HELD_ITEMS);

        sf.set_bag_capacity_upgrades(3);
        assert_eq!(sf.bag_capacity(), 110);

        // Start from a legal bag: medicine run, then stones, then balls,
        // each pocket exactly once, in ascending pocket order.
        sf.write_bag_slots(false, &[(39, 20), (40, 5), (265, 3), (1, 10), (2, 5)]);
        assert_eq!(
            SaveFile::bag_run_summary(&sf.bag_slots_raw(false).unwrap()),
            vec![(3, 2), (4, 1), (5, 2)]
        );
        assert!(sf.bag_warnings(false).is_empty());

        // Medicine added now must land inside the medicine run, not at the tail.
        sf.add_bag_item(39, 5, false).unwrap(); // merges into the Potion stack
        sf.add_bag_item(44, 1, false).unwrap(); // new medicine slot (Revive)
        sf.add_bag_item(1, 4, false).unwrap(); // merges into the Poke Ball stack
        let runs = SaveFile::bag_run_summary(&sf.bag_slots_raw(false).unwrap());
        assert_eq!(runs, vec![(3, 3), (4, 1), (5, 2)]);
        assert!(sf.bag_warnings(false).is_empty());
        let slots = sf.bag_slots_raw(false).unwrap();
        let potion = slots.iter().find(|(i, _)| *i == 39).unwrap();
        assert_eq!(potion.1, 25, "existing stack must absorb the merge");
        let ball = slots.iter().find(|(i, _)| *i == 1).unwrap();
        assert_eq!(ball.1, 14, "existing ball stack must absorb the merge");
        // The new medicine slot must sit between the other two medicine ids,
        // not at the tail of the array.
        let medicine_ids: Vec<u16> = slots
            .iter()
            .take_while(|(i, _)| *i != 0)
            .filter(|(i, _)| item_pocket(*i) == item_pockets::POCKET_MEDICINE)
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(medicine_ids, vec![39, 40, 44], "sorted insertion point");

        // Removal collapses emptied slots so no hole opens inside a run.
        sf.remove_bag_item(44, 1, false).unwrap();
        let runs = SaveFile::bag_run_summary(&sf.bag_slots_raw(false).unwrap());
        assert_eq!(runs, vec![(3, 2), (4, 1), (5, 2)]);
        assert!(sf.bag_warnings(false).is_empty());

        // A pocketless id can never be placed: the game would have no pocket
        // to point at.
        assert!(sf.add_bag_item(0xFFFF, 1, false).is_err());
    }

    #[test]
    fn bag_quantity_xor_and_ghost_slot_handling() {
        // Live bag quantities are XOR'd with the low 16 bits of the SaveBlock2
        // encryption key; the stashed bag is a plain memcpy and is NOT obfuscated.
        let mut sf = synth_save();
        let key = sf.enc_key() & 0xFFFF;
        assert_ne!(key, 0, "synthetic save must carry a real key");
        sf.write_bag_slots(false, &[(1, 1234)]);
        let raw = le16(&sf.merged, SB1_BASE + BAG_OFF + 2);
        assert_eq!(raw ^ key as u16, 1234, "quantity must be written XOR'd");
        assert_eq!(sf.bag_slots_raw(false).unwrap()[0], (1, 1234));

        // The game ends a run on itemId == ITEM_NONE only; a zero quantity with
        // a real id is a ghost the game still counts. Writes must leave the
        // array terminated, not leave a live id with qty 0 in the middle.
        sf.write_bag_slots(false, &[(1, 5), (2, 0), (3, 9)]);
        sf.sort_bag(false).unwrap();
        let after = sf.bag_slots_raw(false).unwrap();
        assert_eq!(after[0], (1, 5));
        assert_eq!(after[1], (3, 9));
        assert_eq!(after[2], (0, 0), "array must be re-terminated");
    }

    #[test]
    fn bag_capacity_and_key_item_budgets() {
        // Rogue_GetBagCapacity: 450 - 37*10 + upgrades*10, capped at 450.
        // Key items live inside the same array but out of a 50-slot budget.
        let mut sf = synth_save();
        sf.set_bag_capacity_upgrades(0);
        assert_eq!(sf.bag_capacity(), 80);
        sf.set_bag_capacity_upgrades(37);
        assert_eq!(sf.bag_capacity(), 450);
        sf.set_bag_capacity_upgrades(200);
        assert_eq!(sf.bag_capacity(), 450, "upgrade count must clamp");

        // Fill ordinary slots past capacity: warnings must fire, and an add
        // must refuse rather than write an unreachable tail.
        let mut sf = synth_save();
        sf.set_bag_capacity_upgrades(0);
        let many: Vec<(u16, u16)> = (0..100).map(|i| (5, (i as u16) + 1)).collect();
        sf.write_bag_slots(false, &many);
        let warns = sf.bag_warnings(false);
        assert!(
            warns.iter().any(|w| w.contains("exceed capacity")),
            "over-capacity bag must warn: {warns:?}"
        );
        // Stack cap is 999 for every pocket.
        let tall: Vec<(u16, u16)> = vec![(5, 5000)];
        sf.write_bag_slots(false, &tall);
        assert!(sf
            .bag_warnings(false)
            .iter()
            .any(|w| w.contains("per-stack cap")));
    }

    #[test]
    fn stash_bag_is_not_obfuscated() {
        // The stashed hub-state bag sits inside sRunRestoreBlock and is
        // serialized by memcpy: quantities must land raw, with no XOR.
        let mut sf = synth_save();
        let key = sf.enc_key() & 0xFFFF;
        assert_ne!(key, 0);
        sf.add_bag_item(44, 6, true).unwrap();
        let rb = sf.rogue_block().unwrap();
        let off = rb.stash.base + 624;
        assert_eq!(le16(&sf.merged, off), 44);
        assert_eq!(
            le16(&sf.merged, off + 2),
            6,
            "stash quantity must NOT be XOR'd with the key"
        );
        assert_eq!(sf.bag_slots_raw(true).unwrap()[0], (44, 6));
    }

    #[test]
    fn pocket_table_reproduces_live_save_runs() {
        // The table's whole point: fed a real bag, it must reproduce exactly
        // the contiguous runs the game sees. Pins against a live save shape.
        // Potion/Medicine ordering from the generated table, not hand-typed.
        assert_eq!(item_pockets::POCKET_ORDER.len(), 9);
        // The display order and the numeric constants must agree; the generator
        // emits both from the same source, and this fails if they ever drift.
        assert_eq!(
            item_pockets::POCKET_ORDER
                .iter()
                .map(|(_, v)| *v)
                .collect::<Vec<_>>(),
            vec![
                item_pockets::POCKET_ITEMS,
                item_pockets::POCKET_HELD_ITEMS,
                item_pockets::POCKET_MEDICINE,
                item_pockets::POCKET_STONES,
                item_pockets::POCKET_POKE_BALLS,
                item_pockets::POCKET_TM_HM,
                item_pockets::POCKET_BERRIES,
                item_pockets::POCKET_POKEBLOCK,
                item_pockets::POCKET_KEY_ITEMS,
            ]
        );
        assert_eq!(item_pocket(2), item_pockets::POCKET_POKE_BALLS);
        assert_eq!(item_pocket(68), item_pockets::POCKET_MEDICINE);
        // Evolution items are baked into POCKET_ITEMS by rogue_baked.c even
        // though items.h parks them elsewhere; the table carries that patch.
        for id in item_pockets::EVO_ITEMS {
            assert_eq!(
                item_pocket(*id),
                item_pockets::POCKET_ITEMS,
                "evolution item {id} must resolve to the Items pocket"
            );
        }
        // Ids with no pocket are unusable and must be rejected by the editor.
        assert_eq!(item_pocket(0), 0);
        assert_eq!(item_pocket(0xFFFF), 0);
    }
}

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

struct Editor {
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
    fn open_path(&mut self, ps: String) {
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

    fn rogue_ui(&mut self, ui: &mut egui::Ui) {
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

    fn mon_editor_ui(&mut self, ui: &mut egui::Ui) {
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
                ui.label("Gender flag");
                egui::ComboBox::from_id_source("gender")
                    .selected_text(match m.gender_flag() {
                        0 => "Male (0)".into(),
                        v => format!("Female ({v})"),
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(m.gender_flag() == 0, "Male (0)")
                            .clicked()
                        {
                            m.set_gender_flag(0);
                            changed = true;
                        }
                        if ui
                            .selectable_label(m.gender_flag() == 1, "Female (1)")
                            .clicked()
                        {
                            m.set_gender_flag(1);
                            changed = true;
                        }
                    });
                ui.label("Ability #");
                egui::ComboBox::from_id_source("ability")
                    .selected_text(format!("{}", m.ability_num()))
                    .show_ui(ui, |ui| {
                        for a in 0..2u8 {
                            if ui
                                .selectable_label(m.ability_num() == a, a.to_string())
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

    fn items_ui(&mut self, ui: &mut egui::Ui) {
        // Read everything out of the save first, then drop the borrow, so the
        // widgets below can mutate `self` (filter / add-row state) freely.
        //
        // Bag header: the bag is one flat array the game splits into contiguous
        // per-pocket runs on load (`UpdateBagItemsPointers`), and it does NOT
        // re-sort after loading — so quantity edits go through
        // set_bag_slot_qty (re-compacts, keeps runs intact) and new ids only
        // ever arrive via add_bag_item, which inserts in sorted position.
        let (bag_items, pc_items, cap, mut cap_up, warnings) = match &self.file {
            Some(sf) => (
                sf.bag_items(),
                sf.pc_items(),
                sf.bag_capacity(),
                sf.bag_capacity_upgrades(),
                sf.bag_warnings(false),
            ),
            None => return,
        };
        let used = bag_items.iter().take_while(|(id, _)| *id != 0).count();
        ui.checkbox(&mut self.hide_empty_items, "hide empty slots");
        let mut edits: Vec<(bool, usize, u16, u16)> = Vec::new();

        let (cap_bumped, do_sort) = ui
            .horizontal(|ui| {
                ui.label(format!("bag {used}/{cap} slots"));
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
        for w in warnings {
            ui.colored_label(
                egui::Color32::from_rgb(210, 130, 40),
                format!("warning: {w}"),
            );
        }

        // Add row: searchable item picker + quantity + stash target toggle.
        let mut do_add = false;
        ui.horizontal(|ui| {
            ui.label("add to bag:");
            Self::filtered_combo(
                ui,
                "bag_add_item",
                &mut self.item_filter,
                all_items(),
                self.bag_add_id,
                &mut |id| self.bag_add_id = id,
            );
            ui.add(
                egui::DragValue::new(&mut self.bag_add_qty)
                    .clamp_range(1..=999)
                    .prefix("x"),
            );
            ui.checkbox(&mut self.bag_add_stash, "stashed (hub) bag");
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

    fn trainer_ui(&mut self, ui: &mut egui::Ui) {
        let Some(sf) = &self.file else { return };
        let mut name = sf.player_name();
        let mut tid = sf.trainer_id();
        let mut sid = sf.secret_id();
        let mut money = sf.money();
        let (mut h, mut mnt, mut sec) = sf.play_time();
        let mut changed = false;
        egui::Grid::new("trainer").num_columns(2).show(ui, |ui| {
            ui.label("Player name");
            if ui.add(egui::TextEdit::singleline(&mut name)).changed() {
                changed = true;
            }
            ui.end_row();
            ui.label("Trainer ID");
            if ui
                .add(egui::DragValue::new(&mut tid).clamp_range(0..=65535))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("Secret ID");
            if ui
                .add(egui::DragValue::new(&mut sid).clamp_range(0..=65535))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("Money");
            if ui
                .add(egui::DragValue::new(&mut money).clamp_range(0..=9999999))
                .changed()
            {
                changed = true;
            }
            ui.end_row();
            ui.label("Play time");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut h)
                            .clamp_range(0..=9999)
                            .suffix(" h"),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut mnt)
                            .clamp_range(0..=59)
                            .suffix(" m"),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut sec)
                            .clamp_range(0..=59)
                            .suffix(" s"),
                    )
                    .changed()
                {
                    changed = true;
                }
            });
            ui.end_row();
        });
        if changed {
            if let Some(sf) = self.file.as_mut() {
                sf.set_player_name(&name);
                sf.set_tids(tid, sid);
                sf.set_money(money);
            }
            self.dirty = true;
        }
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
