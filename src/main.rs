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

// Rogue save block: overlays PC boxes 10..13 (boxes[TOTAL=14 alloc, 10 in use][0]).
// Written by SerializeRogueBlock() (src/rogue_save.c) as a sequential stream where
// every SerializeArray carries a u16 element-count prefix -> self-describing walk.
// encryptionKey = 0: the block is plaintext. Verified vs v2.2.0-EX saves.
const ROGUE_BASE: usize = STOR_BASE + 4 + 10 * 30 * 80;
const ROGUE_CAP: usize = 4 * 30 * 80; // 4 leftover boxes
const ROGUE_SECRET: u16 = 27615;
const DAYCARE_ELEMSZ: usize = 84; // 80B BoxPokemon facade + 4 (v2.2.0 build)
const STASH_PARTY: usize = 6;
const STASH_BAG_CAP: usize = 450;
const STASH_DAYCARE: usize = 3;
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

    pub fn level(&self) -> Option<u8> {
        if self.raw.len() == MONSZ {
            Some(self.raw[84])
        } else {
            None
        }
    }
    pub fn set_level(&mut self, v: u8) {
        if self.raw.len() == MONSZ {
            self.raw[84] = v;
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
    pub time_of_day: u16, // minutes since hub midnight
    pub season: u8,
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
        let mut rb = RogueBlock {
            version,
            format,
            ok: false,
            ..Default::default()
        };
        let mut o = base + 8; // ver u16 | fmt u8 | secret u16 | rogueVer u8 | lastSpecies u16
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
        field!(coords_f, 4); // Coords16 in the 2.2.0 build
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
    pub fn set_hub_time(&mut self, minutes: u16) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok {
            return;
        }
        put16(&mut self.merged, rb.time_off, minutes % 1440);
        self.edited = true;
    }
    pub fn set_hub_season(&mut self, s: u8) {
        let Some(rb) = self.rogue_block() else { return };
        if !rb.ok {
            return;
        }
        self.merged[rb.time_off + 2] = s;
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

// ---------------------------------------------------------------- CLI dump
fn cli_dump(path: &str) {
    let sf = match SaveFile::open(path) {
        Ok(sf) => sf,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    say!("file: {path}");
    say!(
        "active slot: {} (ctr {} / {})",
        sf.active_slot / 14 + 1,
        sf.sectors[0].counter,
        sf.sectors[14].counter
    );
    say!(
        "slot1 valid: {}  slot2 valid: {}",
        sf.slot_valid(0),
        sf.slot_valid(14)
    );
    let (h, m, s) = sf.play_time();
    say!(
        "player: {:?} TID {} SID {}  playtime {:02}:{:02}:{:02}  money {}",
        sf.player_name(),
        sf.trainer_id(),
        sf.secret_id(),
        h,
        m,
        s,
        sf.money()
    );
    if let Some(rb) = sf.rogue_block() {
        say!("\nrogue block: v{} fmt{}", rb.version, rb.format);
        if rb.ok {
            say!(
                "  hub time {:02}:{:02}  season {}  weather {}  debug {}",
                rb.time_of_day / 60,
                rb.time_of_day % 60,
                rb.season,
                rb.weather,
                rb.is_debug
            );
            say!(
                "  quests {}  campaigns {}  daycare {}  safari {}",
                rb.quests_f.n,
                rb.campaigns_f.n,
                rb.daycare_f.n,
                rb.safari_f.n
            );
            for i in 0..rb.daycare_f.n.min(3) {
                let off = rb.daycare_f.data + i * DAYCARE_ELEMSZ;
                let mo = Mon::from_bytes(&sf.merged[off..off + BOXSZ]);
                if mo.species() != 0 {
                    say!(
                        "  daycare[{i}] {} ({}) lvl? species={}",
                        mo.nickname(),
                        species_name(mo.species()),
                        mo.species()
                    );
                }
            }
            if rb.stash.ok {
                say!(
                    "  stashed hub state (sRunRestoreBlock): money {} playTime {}",
                    rb.stash.money,
                    rb.stash.play_time
                );
                for (i, mo) in rb.stash.party.iter().enumerate() {
                    if mo.species() != 0 {
                        say!(
                            "    stash party[{i}] {} ({}) lvl {}",
                            mo.nickname(),
                            species_name(mo.species()),
                            mo.level().unwrap_or(0)
                        );
                    }
                }
                for (i, mo) in rb.stash.daycare.iter().enumerate() {
                    if mo.species() != 0 {
                        say!(
                            "    stash daycare[{i}] {} ({})",
                            mo.nickname(),
                            species_name(mo.species())
                        );
                    }
                }
            }
        } else {
            say!("  (stream layout mismatch — header only)");
        }
    }
    for (i, mo) in sf.party().iter().enumerate() {
        say!(
            "\nparty[{i}] {} species={} ({})",
            i,
            mo.species(),
            species_name(mo.species())
        );
        say!(
            "  OT={:?} lvl={} nature={} shiny={} genderFlag={} ball={} exp={} friendship={}",
            mo.ot_name(),
            mo.level().unwrap_or(0),
            NATURES[mo.nature() as usize],
            mo.is_shiny(),
            mo.gender_flag(),
            mo.ball(),
            mo.exp(),
            mo.friendship()
        );
        say!("  IVs={:?} EVs={:?}", mo.ivs(), mo.evs());
        say!(
            "  moves={:?} pp={:?}",
            mo.moves().map(|m| title_lookup(MOVES, m)),
            mo.pps()
        );
        say!(
            "  checksum {} ({}), bad_egg={}",
            mo.checksum_stored(),
            if mo.checksum_ok() { "OK" } else { "BAD" },
            mo.is_bad_egg()
        );
        if let Some((hp, max, st)) = mo.cached_stats() {
            say!(
                "  stats cur/max = {hp}/{max}  atk={} def={} spe={} spa={} spd={}",
                st[0],
                st[1],
                st[2],
                st[3],
                st[4]
            );
        }
    }
    for bx in 0..ACTIVE_BOXES {
        let cnt = (0..BOX_SLOTS)
            .filter(|&s| sf.box_mon(bx, s).species() != 0)
            .count();
        if cnt > 0 {
            let first = (0..BOX_SLOTS)
                .map(|s| sf.box_mon(bx, s))
                .find(|m| m.species() != 0)
                .unwrap();
            say!(
                "box {bx} ({:?}): {} mons, first = {} checksum {}",
                sf.box_name(bx),
                cnt,
                species_name(first.species()),
                if first.checksum_ok() { "OK" } else { "BAD" }
            );
        }
    }
    let items: Vec<String> = sf
        .bag_items()
        .into_iter()
        .chain(sf.pc_items())
        .filter(|(id, _)| *id != 0)
        .map(|(id, q)| format!("{}x{}", q, item_name(id)))
        .collect();
    say!("\nitems: {}", items.join(", "));
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--cli-edit" {
        cli_edit(&args[2], &args[3..]);
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--cli" {
        cli_dump(&args[2]);
        return Ok(());
    }

    // GUI mode. A save may be passed to open it instantly:
    //   er-save-editor FILE.sav            (positional)
    //   er-save-editor --open FILE.sav     (explicit, equivalent)
    // Opening is read-only; nothing is written without Save / Save As.
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
                say!(
                    "usage: er-save-editor [FILE.sav | --open FILE.sav]   GUI (opens file if given)\n\
                    \x20      er-save-editor --cli FILE.sav                 decode save to stdout (read-only)\n\
                    \x20      er-save-editor --cli-edit FILE ...            scripted edits (see --help-edit)"
                );
                return Ok(());
            }
            "--help-edit" => {
                say!(
                    "--cli-edit FILE <subcommand> [--dry-run]\n\
                    \x20 iv PARTY_IDX IV_IDX VALUE [refresh|no-refresh]   set one IV, refresh cached stat\n\
                    \x20 money AMOUNT                                     set money (currently loaded state)\n\
                    \x20 iv-stash PARTY_IDX IV_IDX VALUE                  IV on stashed hub party mon\n\
                    \x20 stash-money AMOUNT                               set stashed hub money (Rogue block)\n\
                    \x20 hub-time MINUTES                                 hub time-of-day (0..1439)\n\
                    \x20 hub-season N                                     0 spring..3 winter\n\
                    \x20 quest INDEX FLAGS [MAXDIFF]                      quest state bitmask\n\
                    \x20 campaign INDEX UNLOCKED COMPLETED [BEST]         0/1 flags\n\
                    \x20 diff-toggle BIT 0|1                              lab difficulty toggle\n\
                    \x20 diff-range IDX VALUE                             lab range slot\n\
                    \x20 noop                                             reseal without data change\n\
                    --dry-run computes edits + checksums in memory and writes zero bytes."
                );
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

// ---------------------------------------------------------------- CLI edit mode
// --cli-edit FILE money AMOUNT
// --cli-edit FILE iv PARTY_IDX IV_IDX VALUE [refresh|no-refresh]
// Write-consent rule: the editor writes to disk ONLY when the user explicitly
// triggers a write. --cli-edit implies consent by invocation; --dry-run is the
// escape hatch that computes every edit + checksum but writes zero bytes.
// There is no autosave, backup, or temp-file path anywhere in the crate.
fn cli_arg_err(what: &str) -> ! {
    eprintln!("error: bad {what}");
    std::process::exit(1);
}

fn cli_edit(path: &str, args_in: &[String]) {
    let dry_run = args_in.iter().any(|a| a == "--dry-run");
    let args: Vec<String> = args_in
        .iter()
        .filter(|a| *a != "--dry-run")
        .cloned()
        .collect();
    let mut sf = match SaveFile::open(path) {
        Ok(sf) => sf,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // argument-count guard: missing CLI args exit cleanly instead of panicking
    let need: usize = match args[0].as_str() {
        "noop" => 1,
        "money" | "stash-money" | "hub-time" | "hub-season" => 2,
        "quest" | "diff-toggle" | "diff-range" => 3,
        "campaign" | "iv" | "iv-stash" => 4,
        _ => 0,
    };
    if args.len() < need {
        eprintln!(
            "error: {} takes {need} argument(s) (see --help-edit)",
            args[0]
        );
        std::process::exit(1);
    }
    match args[0].as_str() {
        "noop" => {
            if dry_run {
                say!("dry run: would resave {path} (checksums recomputed); no bytes written");
                return;
            }
            sf.edited = true;
            sf.save(path).expect("save failed");
            say!("resaved {path} (all checksums recomputed, no data change)");
            return;
        }
        "money" => {
            let v: u32 = args[1].parse().unwrap();
            say!("money {} -> {}", sf.money(), v);
            sf.set_money(v);
        }
        "stash-money" => {
            let v: u32 = args[1].parse().unwrap();
            let rb = sf.rogue_block().ok_or_else(|| "no Rogue block".to_string());
            match rb {
                Ok(rb) if rb.stash.ok => {
                    say!("stashed hub money {} -> {}", sf.stash_money(), v);
                    sf.set_stash_money(v);
                }
                _ => {
                    eprintln!("error: stashed hub state not found/decodable");
                    std::process::exit(1);
                }
            }
        }
        // Rogue-block fields. All of these require a cleanly decoded block; they
        // never guess when the stream layout drifts.
        "hub-time" => {
            let mins: u16 = args[1].parse().unwrap();
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.ok {
                eprintln!("error: Rogue block not decodable (layout mismatch)");
                std::process::exit(1);
            }
            say!(
                "hub time {:02}:{:02} -> {:02}:{:02}",
                rb.time_of_day / 60,
                rb.time_of_day % 60,
                mins / 60,
                mins % 60
            );
            sf.set_hub_time(mins);
        }
        "hub-season" => {
            let s: u8 = args[1].parse().unwrap();
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.ok {
                eprintln!("error: Rogue block not decodable (layout mismatch)");
                std::process::exit(1);
            }
            say!("season {} -> {}", rb.season, s);
            sf.set_hub_season(s);
        }
        "quest" => {
            // quest INDEX FLAGS [MAXDIFF]  (FLAGS = bitmask, see QUEST_STATE_*; 0x ok)
            let idx: usize = args[1]
                .parse()
                .unwrap_or_else(|_| cli_arg_err("quest index"));
            let flags: u16 = match args[2]
                .strip_prefix("0x")
                .or_else(|| args[2].strip_prefix("0X"))
            {
                Some(h) => u16::from_str_radix(h, 16)
                    .unwrap_or_else(|_| cli_arg_err("quest flags (decimal or 0x hex)")),
                None => args[2]
                    .parse()
                    .unwrap_or_else(|_| cli_arg_err("quest flags")),
            };
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.ok || idx >= rb.quests_f.n {
                eprintln!("error: Rogue block not decodable, or quest index out of range");
                std::process::exit(1);
            }
            let w0 = le32(&sf.merged, rb.quests_f.data + idx * 8);
            let name = tables_extra::QUEST_NAMES.get(idx).copied().unwrap_or("?");
            let hc = args
                .get(3)
                .map(|s| s.parse().unwrap())
                .unwrap_or_else(|| ((w0 >> 16) & 7) as u8);
            say!(
                "quest[{idx}] {:?}: flags {:#04x} maxDiff {} -> {:#04x} maxDiff {}",
                name,
                (w0 & 0xFFFF) as u16,
                (w0 >> 16) & 7,
                flags,
                hc
            );
            sf.set_quest_state(idx, flags, hc);
        }
        "campaign" => {
            // campaign INDEX UNLOCKED COMPLETED [BEST]  (0/1 flags)
            let idx: usize = args[1].parse().unwrap();
            let u: bool = args[2].parse::<u8>().unwrap() != 0;
            let c: bool = args[3].parse::<u8>().unwrap() != 0;
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.ok || idx >= rb.campaigns_f.n {
                eprintln!("error: Rogue block not decodable, or campaign index out of range");
                std::process::exit(1);
            }
            let v = le32(&sf.merged, rb.campaigns_f.data + idx * 4);
            let best: u16 = args
                .get(4)
                .map(|s| s.parse().unwrap())
                .unwrap_or((v >> 16) as u16);
            say!(
                "campaign[{idx}] {:?}: unlocked={} completed={} best={} -> {} {} {}",
                CAMPAIGN_NAMES.get(idx).copied().unwrap_or("?"),
                v & 1,
                (v >> 1) & 1,
                v >> 16,
                u,
                c,
                best
            );
            sf.set_campaign(idx, u, c, best);
        }
        "diff-toggle" => {
            // diff-toggle BIT 0|1  (lab difficulty toggle, see CONFIG_TOGGLE_*)
            let bit: usize = args[1].parse().unwrap();
            let on: bool = args[2].parse::<u8>().unwrap() != 0;
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.ok || bit >= rb.diff_toggle_f.n * 8 || bit >= CONFIG_TOGGLES.len() {
                eprintln!("error: Rogue block not decodable, or toggle bit out of range");
                std::process::exit(1);
            }
            let byte = sf.merged[rb.diff_toggle_f.data + bit / 8];
            say!(
                "toggle[{bit}] {:?}: {} -> {}",
                CONFIG_TOGGLES[bit],
                byte >> (bit % 8) & 1,
                on as u8
            );
            let mut b = byte;
            if on {
                b |= 1 << (bit % 8);
            } else {
                b &= !(1 << (bit % 8));
            }
            sf.set_diff_toggle(bit / 8, b);
        }
        "diff-range" => {
            // diff-range IDX VALUE  (lab range slot, see CONFIG_RANGE_*)
            let idx: usize = args[1].parse().unwrap();
            let v: u8 = args[2].parse().unwrap();
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.ok || idx >= rb.diff_range_f.n {
                eprintln!("error: Rogue block not decodable, or range index out of range");
                std::process::exit(1);
            }
            say!(
                "range[{idx}] {:?}: {} -> {}",
                CONFIG_RANGES.get(idx).copied().unwrap_or("?"),
                sf.merged[rb.diff_range_f.data + idx],
                v
            );
            sf.set_diff_range(idx, v);
        }
        "iv" => {
            let pi: usize = args[1].parse().unwrap();
            let ii: usize = args[2].parse().unwrap();
            let v: u8 = args[3].parse().unwrap();
            let refresh = args.get(4).map(|s| s.as_str()) != Some("no-refresh");
            let mut mons = sf.party();
            assert!(pi < mons.len(), "party index out of range");
            let m = &mut mons[pi];
            say!(
                "before: ivs={:?} stats={:?}",
                m.ivs(),
                m.cached_stats().map(|s| s.2)
            );
            m.set_iv(ii, v);
            if refresh {
                let lvl = m.level().unwrap() as u32;
                let nat = m.nature() as usize;
                let bs = base_stats(m.species()).expect("base stats not in table");
                let newv = calc_stat(bs[ii] as u32, v as u32, m.evs()[ii] as u32, lvl, nat, ii);
                let split = nature_split(nat, ii);
                if ii == 0 {
                    m.set_cached_hp(newv as u16);
                } else {
                    m.set_cached_stat(ii - 1, newv as u16);
                }
                say!("refreshed stat idx {ii} -> {} (nature split {split})", newv);
            }
            m.seal();
            sf.set_party_mon(pi, m);
        }
        "iv-stash" => {
            // same as iv, but on the stashed hub party inside the Rogue block
            let pi: usize = args[1].parse().unwrap();
            let ii: usize = args[2].parse().unwrap();
            let v: u8 = args[3].parse().unwrap();
            let rb = sf.rogue_block().unwrap_or_default();
            if !rb.stash.ok || pi >= rb.stash.party.len() {
                eprintln!("error: stashed hub party not decodable, or index out of range");
                std::process::exit(1);
            }
            let mut m = rb.stash.party[pi].clone();
            say!("stash[{}] before: ivs={:?}", pi, m.ivs());
            m.set_iv(ii, v);
            m.seal();
            sf.set_stash_mon(pi, &m);
            say!("stash[{pi}] ivs -> {:?}", m.ivs());
        }
        _ => {
            eprintln!("unknown subcommand");
            std::process::exit(1);
        }
    }
    if dry_run {
        say!("dry run: edits computed + checksums verified in memory; no bytes written to {path}");
        return;
    }
    sf.save(path).expect("save failed");
    say!("saved {path} (mon + sector checksums recomputed)");
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
        let mut o = base + 8;
        // emit a count prefix and skip the element bytes; zeros are fine
        for (n, elemsz) in [
            (232usize, 8usize), // quests
            (7, 4),             // campaigns
            (2, 1),             // areaBuiltFlags
            (11, 4),            // areaCoords (Coords16 era)
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
        sf.set_hub_season(2);
        assert_eq!(sf.merged[time_off + 2], 2);
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
    dirty: bool,
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
        let season = rb.season;
        let mut weather = rb.weather;
        ui.horizontal(|ui| {
            ui.label(format!("save v{}", rb.version));
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
                .selected_text(SEASONS.get(season as usize).copied().unwrap_or("custom"))
                .show_ui(ui, |ui| {
                    for (s, name) in SEASONS.iter().enumerate() {
                        if ui.selectable_label(season as usize == s, *name).clicked() {
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

        ui.collapsing("Hub map (areas + upgrades)", |ui| {
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
            let mut area_edits: Vec<(usize, bool)> = Vec::new();
            let mut up_edits: Vec<(usize, bool)> = Vec::new();
            ui.strong("Areas built");
            for i in 0..HUB_AREAS.len() {
                let byte = area_snap.get(i / 8).copied().unwrap_or(0);
                let mut on = byte >> (i % 8) & 1 != 0;
                if ui.checkbox(&mut on, HUB_AREAS[i]).changed() {
                    area_edits.push((i, on));
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
            if !area_edits.is_empty() || !up_edits.is_empty() {
                if let Some(sf) = self.file.as_mut() {
                    for (i, on) in area_edits {
                        sf.set_hub_area(i, on);
                    }
                    for (i, on) in up_edits {
                        sf.set_hub_upgrade(i, on);
                    }
                }
                self.dirty = true;
            }
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

                if m.level().is_some() {
                    ui.label("Level (cached)");
                    let mut lvl = m.level().unwrap();
                    if ui
                        .add(egui::DragValue::new(&mut lvl).clamp_range(0..=100))
                        .changed()
                    {
                        m.set_level(lvl);
                        changed = true;
                    }
                } else {
                    ui.label("Level: (box mon — computed from exp)");
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
        }

        if changed {
            m.seal();
            self.apply_mon(m);
        }
    }

    fn items_ui(&mut self, ui: &mut egui::Ui) {
        let Some(sf) = &self.file else { return };
        ui.checkbox(&mut self.hide_empty_items, "hide empty slots");
        let mut edits: Vec<(bool, usize, u16, u16)> = Vec::new();
        for (bag, label) in [(true, "Bag"), (false, "PC (x256 box)")] {
            ui.strong(label);
            let items: Vec<(u16, u16)> = if bag { sf.bag_items() } else { sf.pc_items() };
            egui::Grid::new(label)
                .num_columns(4)
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
                        if ui.button(format!("{id:5} {name}")).clicked() {
                            // simple cycle-to-empty behavior: clicking opens a search below
                        }
                        let mut q = qty;
                        if ui
                            .add(egui::DragValue::new(&mut q).clamp_range(0..=999))
                            .changed()
                        {
                            edits.push((bag, i, id, q));
                        }
                        if shown % 4 == 0 {
                            ui.end_row();
                        }
                    }
                    if shown % 4 != 0 {
                        ui.end_row();
                    }
                });
        }
        for (bag, i, id, q) in edits {
            if let Some(sf) = self.file.as_mut() {
                sf.set_item(bag, i, id, q);
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
