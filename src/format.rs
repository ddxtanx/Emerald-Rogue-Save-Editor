//! Save-container format primitives: sector geometry, merged-stream offsets,
//! endian helpers, Gen-3 string codec, and the substruct permutation table.
//! Offsets verified against the game source and live saves (see
//! docs/FORMAT.md).

pub(crate) const SECSZ: usize = 0x1000;
pub(crate) const DATASZ: usize = 4084;
pub(crate) const SECURITY: u32 = 0x0801_2025;
pub(crate) const SLOT_SECTORS: usize = 14;
pub(crate) const FILE_SECTORS: usize = 32;

pub(crate) const PARTY_COUNT_OFF: usize = 0x234;
pub(crate) const PARTY_OFF: usize = 0x238;
pub(crate) const MONSZ: usize = 104;
pub(crate) const BOXSZ: usize = 80;
// Rogue discards the vanilla growth-rate curves: Rogue_ModifyExperienceTables()
// (src/rogue_baked.c) returns `level * 300` for every growthRate, and the level
// loop in GetLevelFrom{Box}MonExp clamps at MAX_LEVEL. So exp == level*300 and
// level == clamp(exp/300, 0, MAX_LEVEL) - the same schedule for every species.
pub(crate) const EXP_PER_LEVEL: u32 = 300;
pub(crate) const MAX_LEVEL: u8 = 100;

/// Replay of the game's level loop (src/pokemon.c GetLevelFromBoxMonExp)
/// against Rogue's flat `level * 300` schedule. Exact for all exp values,
/// including the clamping when exp exceeds what level 100 costs.
pub(crate) fn level_from_exp(exp: u32) -> u8 {
    let lvl = exp / EXP_PER_LEVEL;
    lvl.min(MAX_LEVEL as u32) as u8
}
// Rogue shifts SB1 after the party: Pokemon structs are 104B not 80B. Verified
// against jfzazo savconverter + live save:
//   money   = 0x238 + 6*104      = 0x4A8 (XOR'd with SB2 encryptionKey @SB2+0x4C)
//   pcItems = money + 4 + 4      = 0x4B0 (raw)
//   bag     = pcItems + 50*4     = 0x578 (quantity XOR'd with low 16 bits of key)
pub(crate) const MONEY_OFF: usize = 0x4A8; // in SaveBlock1
pub(crate) const PC_ITEMS_OFF: usize = 0x4B0;
pub(crate) const BAG_OFF: usize = 0x578;
pub(crate) const ENC_KEY_OFF: usize = 0x4C; // in SaveBlock2 (sector id 0)
pub(crate) const BAG_CAP: usize = 450;
pub(crate) const PC_CAP: usize = 50;
pub(crate) const SB1_BASE: usize = DATASZ; // SaveBlock1 = merged chunks 1..4
pub(crate) const STOR_BASE: usize = 5 * DATASZ; // storage = merged chunks 5..13
// Bag sizing knobs (include/constants/global.h + constants/rogue.h): SB1+0x4AC
// u8 bagSortMode (0 type, 1 name, 2 value, 3 amount) SB1+0x4AD u8
// bagCapacityUpgrades The game only ever *reads* the first
// `Rogue_GetBagCapacity()` slots of the flat bag array, so anything past
// capacity is invisible (and gets shuffled away by the next in-game bag sort).
// Key Items are special: they live in the LAST 50 slots of the physical array
// (`BAG_ITEM_RESERVED_SLOTS`), which is why a sorted bag ends with its key-item
// run.
/// u8 `bagSortMode` (vanilla SB1+0x494; +24 in this build). `ITEM_SORT_MODE_*`
/// in include/constants/rogue.h: 0 type, 1 name, 2 value, 3 amount.
pub(crate) const BAG_SORT_MODE_OFF: usize = 0x4AC;
pub(crate) const BAG_SORT_MODES: [&str; 4] = ["type", "name", "value", "amount"];
pub(crate) const BAG_CAP_UPGRADES_OFF: usize = 0x4AD;
pub(crate) const BAG_RESERVED_SLOTS: usize = 50;
pub(crate) const BAG_SLOTS_PER_UPGRADE: usize = 10;
pub(crate) const BAG_MAX_CAPACITY_UPGRADES: usize = 37;
pub(crate) const BAG_BASE_CAPACITY: usize = 450 - BAG_MAX_CAPACITY_UPGRADES * BAG_SLOTS_PER_UPGRADE; // 80
/// `MAX_BAG_ITEM_CAPACITY`: Rogue gives every pocket a 999-per-stack cap.
pub(crate) const MAX_STACK: u16 = 999;

// Rogue save block: overlays PC boxes 10..13 (boxes[TOTAL=14 alloc, 10 in
// use][0]). Written by SerializeRogueBlock() (src/rogue_save.c) as a sequential
// stream where every SerializeArray carries a u16 element-count prefix ->
// self-describing walk. encryptionKey = 0: the block is plaintext. Verified vs
// v2.2.0-EX saves.
pub(crate) const ROGUE_BASE: usize = STOR_BASE + 4 + 10 * 30 * 80;
pub(crate) const ROGUE_CAP: usize = 4 * 30 * 80; // 4 leftover boxes
pub(crate) const ROGUE_SECRET: u16 = 27615;
pub(crate) const DAYCARE_ELEMSZ: usize = 84; // 80B BoxPokemon facade + 4 (v2.2.0 build)
pub(crate) const SAFARISZ: usize = 20; // struct RogueSafariMon (STATIC_ASSERT == 20)
/// Spawn priority a hand-added Safari Zone entry gets (the row edits this u8).
/// 100 sits above the values the game's own spawn tree uses, so a mon the user
/// adds by hand is not buried under the defaults.
pub(crate) const SAFARI_ADD_PRIORITY: u8 = 100;
pub(crate) const STASH_PARTY: usize = 6;
pub(crate) const STASH_BAG_CAP: usize = 450;
pub(crate) const STASH_DAYCARE: usize = 3;
/// `FOLLOWMON_SHINY_OFFSET` (`include/constants/rogue.h`): the hub's
/// `homeWanderingMonSpecies` stores `species + 10000` for a shiny follow-mon.
pub(crate) const FOLLOWMON_SHINY_OFFSET: u16 = 10000;
/// `homeStyles` labels, in the enum order of `src/rogue_hub.c`.
pub(crate) const HOME_STYLE_EXTERIOR: &[&str] = &[
    "Cave",
    "Trees",
    "Brick House",
    "Wooden House",
    "Gym Building",
];
pub(crate) const HOME_STYLE_INTERIOR: &[&str] = &[
    "Red Cave",
    "Blue Cave",
    "Brown Cave",
    "Desert Cave",
    "Shrub",
    "Tree",
];
pub(crate) const HOME_STYLE_PATH: &[&str] = &["Grass", "Sand", "Stone", "Pebbles", "Muddy Tracks"];
/// `homeDecorations` slot ranges (`HOME_DECOR_OUTSIDE_COUNT` 160, then
/// `HOME_DECOR_INSIDE_COUNT` 32).
pub(crate) const HOME_DECOR_OUTSIDE: usize = 160;
pub(crate) const TOTAL_BOXES: usize = 14; // allocated (boxNames stride)
pub(crate) const ACTIVE_BOXES: usize = 10; // the PC actually uses boxes 0..9; 10..13 are leftover slots
pub(crate) const BOX_SLOTS: usize = 30;
pub(crate) const BOX_NAME_LEN: usize = 9;
// struct Pokemon fusions[MAX_FUSION_STORAGE=4] at /*0x8432*/ in PokemonStorage
// (pokemon_storage_system.h). Full 104B party structs.
pub(crate) const FUSION_OFF: usize = 0x8432;
pub(crate) const FUSION_COUNT: usize = 4;

pub(crate) const SUBSTRUCT_ORDER: [[usize; 4]; 24] = [
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

pub(crate) fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

pub(crate) fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
pub(crate) fn put16(b: &mut [u8], i: usize, v: u16) {
    b[i..i + 2].copy_from_slice(&v.to_le_bytes());
}
pub(crate) fn put32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn sector_crc(data: &[u8]) -> u16 {
    let mut s: u32 = 0;
    for w in data.chunks_exact(4) {
        s = s.wrapping_add(le32(w, 0));
    }
    (((s >> 16) as u32 + s) & 0xFFFF) as u16
}

// ---------------------------------------------------------------- gen3 strings

pub(crate) fn gen3_decode(b: &[u8]) -> String {
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

pub(crate) fn gen3_encode(s: &str, len: usize) -> Vec<u8> {
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
