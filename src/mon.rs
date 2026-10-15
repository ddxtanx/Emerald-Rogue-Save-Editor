//! Pokemon structs (80B BoxPokemon / 104B party Pokemon): decode, encode,
//! bitfield accessors, and stat math. Level derives from exp (Rogue uses a flat
//! exp schedule), see EXP_PER_LEVEL.

use crate::format::*;
use crate::game_data::*;

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
    pub fn set_pid(&mut self, v: u32) {
        put32(&mut self.raw, 0, v);
    }
    pub fn set_ot_name(&mut self, s: &str) {
        let enc = gen3_encode(s, 7);
        self.raw[20..27].copy_from_slice(&enc);
    }
    /// Build a fresh, self-consistent mon for an empty slot: species, level
    /// (through the Rogue flat exp schedule, since level is derived from exp), a
    /// random PID, the save's OTID/OT name, a Poké Ball, ability slot 0, and a
    /// nickname matching the species. Cached stats stay zero, which is what the
    /// game itself writes: party stats are recomputed by `CalculateMonStats` on
    /// load and box mons derive everything from exp.
    ///
    /// The struct is party-sized (104 B). `set_box_mon` truncates to the leading 80
    /// B, which is exactly `struct BoxPokemon`.
    pub fn blank(otid: u32, ot_name: &str, species: u16, level: u8) -> Mon {
        let mut m = Mon::from_bytes(&vec![0u8; 104]);
        m.set_pid(random_pid());
        m.set_otid(otid);
        m.set_species(species);
        m.set_level(level.max(1));
        m.set_met_level(level.max(1) as u16);
        m.set_ball(4); // ITEM_POKE_BALL
        m.set_friendship(70);
        m.set_ability_num(0);
        m.set_gender_flag(0);
        m.set_nickname(crate::game_data::species_name(species));
        if !ot_name.is_empty() {
            m.set_ot_name(ot_name);
        }
        m.seal();
        m
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
    pub fn checksum_stored(&self) -> u16 {
        le16(&self.raw, 28)
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
    /// The effective nature: `pid % 25` XOR the stored modifier. Because the two
    /// are XOR'd the result can land past 24, so the return type carries
    /// `Unexpected` rather than clamping (`set_nature` used to do `n % 25`, which
    /// silently rewrote such a mon).
    pub fn nature(&self) -> crate::domain::Nature {
        crate::domain::Nature::from_raw(self.nature_raw())
    }
    pub fn nature_raw(&self) -> u8 {
        self.pid_nature() ^ self.hidden_nature_mod()
    }
    pub fn set_nature(&mut self, n: crate::domain::Nature) {
        // Write the modifier that reproduces exactly this nature. No `% 25`: the 5-bit
        // field can express values above 24 and an edited save may hold one; clamping
        // would turn the game's byte into ours.
        let modv = self.pid_nature() ^ (n.raw() & 0x1F);
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
    /// The `PP Bonuses` byte at t[0]+8 holds one 2-bit field per move slot (`u8
    /// ppUps:2` x4). Only the per-slot accessors below are exposed: a whole-byte
    /// setter is how the other three slots used to get wiped. PP Ups applied to one
    /// move slot: 0..=3. The game caps it at 3 (pokemon.c `gPPUpGetMask`,
    /// item_use.c refuses a 4th).
    pub fn pp_bonus(&self, slot: usize) -> u8 {
        (self.t[0][8] >> (slot * 2)) & 3
    }
    pub fn set_pp_bonus(&mut self, slot: usize, ups: u8) {
        let ups = ups.min(3) as u8;
        let b = self.t[0][8];
        self.t[0][8] = (b & !(3 << (slot * 2))) | (ups << (slot * 2));
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
    pub fn iv(&self, i: crate::domain::StatIdx) -> u8 {
        self.iv_at(i.index())
    }
    pub fn iv_at(&self, i: usize) -> u8 {
        ((le32(&self.t[3], 4) >> (5 * i)) & 0x1F) as u8
    }
    pub fn set_iv(&mut self, i: crate::domain::StatIdx, v: u8) {
        self.set_iv_at(i.index(), v)
    }
    pub fn set_iv_at(&mut self, i: usize, v: u8) {
        let w = le32(&self.t[3], 4);
        let w = (w & !(0x1F << (5 * i))) | (((v & 0x1F) as u32) << (5 * i));
        self.t[3][4..8].copy_from_slice(&w.to_le_bytes());
    }
    /// All six IV fields at once. This deliberately loops the same masked setter
    /// the per-field widgets use, instead of packing the word separately: an
    /// earlier version ANDed the spread value with `0x1F1F1F1F`, which is a
    /// BYTE-wise mask (six 0x1F bytes = 48 bits, truncated to 32) rather than the
    /// 30-bit IV field mask, so "31 all" wrote 0x1F1F1F1F and the six 5-bit fields
    /// came back shifted - 31, 24, 7, 30, 17, 15 - instead of six 31s.
    pub fn set_all_ivs(&mut self, v: u8) {
        let v = v.min(31);
        for i in 0..6 {
            self.set_iv_at(i, v);
        }
    }
    pub fn is_egg(&self) -> bool {
        (le32(&self.t[3], 4) >> 30) & 1 == 1
    }
    pub fn set_is_egg(&mut self, b: bool) {
        let w = le32(&self.t[3], 4);
        self.t[3][4..8].copy_from_slice(&(w & !(1u32 << 30) | ((b as u32) << 30)).to_le_bytes());
    }
    /// abilityNum is a **2-bit** field: substruct3 +0x08, bits 29..30
    /// (PokemonSubstruct3 in include/pokemon.h). Bit 31 of the u32 at +0x04 is
    /// `gigantamaxFactor` -- reading that bit was the vanilla Gen-3 layout, where
    /// abilityNum really was 1 bit there. In this ROM the 1-bit reading can never
    /// express slot 2 (the hidden ability), which is how abilities displayed wrong.
    pub fn ability_slot(&self) -> crate::domain::AbilitySlot {
        crate::domain::AbilitySlot::from_raw(self.ability_num())
    }
    pub fn ability_num(&self) -> u8 {
        ((le32(&self.t[3], 8) >> 29) & 3) as u8
    }
    pub fn set_ability_slot(&mut self, slot: crate::domain::AbilitySlot) {
        self.set_ability_num(slot.raw())
    }
    pub fn set_ability_num(&mut self, v: u8) {
        let w = le32(&self.t[3], 8);
        self.t[3][8..12]
            .copy_from_slice(&((w & !(3u32 << 29)) | (((v & 3) as u32) << 29)).to_le_bytes());
    }
    pub fn gigantamax_factor(&self) -> bool {
        (le32(&self.t[3], 4) >> 31) & 1 == 1
    }
    pub fn set_gigantamax_factor(&mut self, b: bool) {
        let w = le32(&self.t[3], 4);
        self.t[3][4..8].copy_from_slice(&(w & !(1u32 << 31) | ((b as u32) << 31)).to_le_bytes());
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
    pub fn gender(&self) -> crate::domain::GenderFlag {
        crate::domain::GenderFlag::from_raw(self.gender_flag())
    }
    pub fn set_gender(&mut self, g: crate::domain::GenderFlag) {
        self.set_gender_flag(g.raw())
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
    /// so the game always recomputes the level from exp (`GetLevelFromBoxMonExp` /
    /// `GetLevelFromMonExp`, src/pokemon.c) - a loop over that schedule clamped to
    /// MAX_LEVEL (100). `CalculateMonStats` then writes the result back into the
    /// party struct's cached level byte. Exp is therefore the authoritative field
    /// for *both* the 104-byte party and the 80-byte box layout, and a party mon's
    /// cached byte is only a mirror that can go stale.
    pub fn level(&self) -> Option<u8> {
        Some(level_from_exp(self.exp()))
    }
    /// The raw cached byte in the 104-byte party struct (offset 84). Box mons have
    /// no such byte. Exposed so a stale/mismatched mirror is visible rather than
    /// silent.
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
    /// Keep a party mon's cached level byte consistent with its exp. No-op for box
    /// mons.
    pub fn sync_cached_level(&mut self) {
        if self.raw.len() == MONSZ {
            self.raw[84] = self.level().unwrap_or(0);
        }
    }
    /// The same record sized for the party: a box record is the first 80 bytes of a
    /// 104-byte party record, and the 24-byte live-stat tail is zeroed for the
    /// caller to fill - normally straight after, via `recalculate_cached_stats`.
    /// Returns `self` untouched when it is already party-sized.
    pub fn party_sized(mut self) -> Mon {
        if self.raw.len() < MONSZ {
            self.raw.resize(MONSZ, 0);
        }
        self
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
        // Base (family) name: a form mon reads "Darmanitan", not "Darmanitan Standard
        // Mode". See crate::game_data::species_base_name.
        let sp = species_base_name(self.species());
        if !nick.is_empty() && nick != sp {
            format!("Lv{} {} ({})", self.level().unwrap_or(0), nick, sp)
        } else {
            format!("Lv{} {}", self.level().unwrap_or(0), sp)
        }
    }
}

/// vanilla gNatureStatTable: nature index boosts stat (nat/5) and lowers stat
/// (nat%5) in the stat order [Atk, Def, Spe, SpA, SpD] (our indices 1..=5).
pub(crate) fn nature_split(nat: usize, stat: usize) -> u32 {
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
/// ordering, matching how the cached stat block is laid out. Recompute the
/// cached stat block the way the game's `CalculateMonStats` does. Gen 3 caches
/// stats in the save, so an IV / EV / level change stays invisible in-game
/// until this runs - which is what the editor's "recalculate stats" button
/// needs, and any copy that lands in the party. `Err` names why it cannot be
/// done: an out-of-range nature has no row in the 25-entry modifier table, and
/// an unknown species has no base-stat row.
pub(crate) fn recalculate_cached_stats(m: &mut Mon) -> Result<u32, String> {
    // The stat block lives at bytes 86..98, which only exist in a 104-byte party
    // record. Writing it into an 80-byte box record would silently do nothing, so
    // refuse out loud instead. Box stats are not stored at all: the game computes
    // them when the mon leaves the box.
    if m.raw.len() != MONSZ {
        return Err(format!(
            "this record is {} bytes and stores no stat block",
            m.raw.len()
        ));
    }
    let lvl = m.level().unwrap_or(0) as u32;
    let nat = m.nature();
    if !nat.is_known() {
        return Err(format!("nature value {} is outside 0..=24", nat.raw()));
    }
    let Some(bs) = crate::basestats::base_stats(m.species()) else {
        // Two very different reasons for a miss, and the user cannot tell them apart
        // from the outside: the game itself has no species_info entry for this id (a
        // placeholder / unimplemented form id - the constant exists in species.h, the
        // data does not), versus a gap in the editor's generated table, which is a bug
        // and should read like one.
        let sp = m.species();
        if crate::basestats::species_without_game_data().contains(&sp) {
            return Err(format!(
                "species {sp} ({}) has no entry in the game's own species_info table, so its \
                 base stats are not defined by the game either",
                crate::game_data::species_name(sp)
            ));
        }
        return Err(format!(
            "no base stats in table for species {sp} ({}); the game DOES define this species, \
             so the editor's generated table is missing it",
            crate::game_data::species_name(sp)
        ));
    };
    let (evs, ivs) = (m.evs(), m.ivs());
    let ni = nat.raw() as usize;
    m.set_cached_hp(calc_stat(bs[0] as u32, ivs[0] as u32, evs[0] as u32, lvl, ni, 0) as u16);
    for i in 1..6 {
        let v = calc_stat(bs[i] as u32, ivs[i] as u32, evs[i] as u32, lvl, ni, i);
        m.set_cached_stat(i - 1, v as u16);
    }
    Ok(lvl)
}

pub(crate) fn calc_stat(base: u32, iv: u32, ev: u32, lvl: u32, nat: usize, stat: usize) -> u32 {
    let ev4 = ev / 4;
    if stat == 0 {
        ((2 * base + iv + ev4) * lvl) / 100 + lvl + 10
    } else {
        ((((2 * base + iv + ev4) * lvl) / 100 + 5) * nature_split(nat, stat)) / 100
    }
}

/// A PID for a mon we are creating. It sets the substruct permutation (`pid %
/// 24`), the encryption key (`pid ^ otid`), and the nature, so it must be
/// non-zero and varied; two mons with the same PID/OTID pair would encrypt
/// identically. Seeded from the clock plus a stack address, which is plenty for
/// "give the new mon a different identity".
fn random_pid() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 + d.as_secs() << 12)
        .unwrap_or(0x1234_abcd);
    let salt = &nanos as *const u64 as usize as u64;
    let mixed = nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(salt) >> 11;
    let pid = (mixed as u32) ^ ((mixed >> 32) as u32);
    if pid == 0 {
        0x2c2b_1b18 // Gen 3 has a reserved PID of 0; any non-zero value works
    } else {
        pid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon_of_species(sp: u16) -> Mon {
        let mut m = Mon::from_bytes(&vec![0u8; 80]);
        m.set_species(sp);
        m
    }

    /// The user's request #3: a mon with a form reads as its BASE name, not the
    /// ROM's form-qualified speciesName string. 555's stored name string is
    /// "Darmanitan Standard Mode"; the display shows "Darmanitan".
    #[test]
    fn a_form_mon_displays_its_base_name() {
        assert_eq!(
            crate::game_data::species_name(555),
            "Darmanitan Standard Mode"
        );
        assert_eq!(mon_of_species(555).display_name(), "Lv0 Darmanitan");
        assert_eq!(mon_of_species(1092).display_name(), "Lv0 Darmanitan");
        // A plain id is untouched.
        assert_eq!(mon_of_species(143).display_name(), "Lv0 Snorlax");
    }

    /// User report: "31 all" does not set every IV to 31. This isolates WHERE it
    /// goes wrong: the one-word write (`set_all_ivs`) against six masked single
    /// writes (`set_iv_at`), and then the seal + re-read the editor performs.
    ///
    /// Deliberately synthetic rather than read from a real save: the regression pin
    /// must run on CI too, where no save is present. A stat block only exists in a
    /// 104-byte party record. On an 80-byte box record the writes would land
    /// nowhere, so the helper must say so rather than report success - and
    /// `party_sized` is what makes a copy writable.
    #[test]
    fn stat_recalculation_refuses_a_box_record_and_works_once_sized_for_the_party() {
        let made = Mon::blank(0x1234_5678, "Garrett", 59, 54); // Arcanine, L54
        assert_eq!(made.raw.len(), 104, "a fresh record is party-sized");
        // Box records reach the editor as the first 80 bytes (`from_bytes` on a BOXSZ
        // slice), which is the shape whose stat writes would go nowhere.
        let mut boxed = Mon::from_bytes(&made.raw[..80]);
        let err = recalculate_cached_stats(&mut boxed)
            .expect_err("an 80-byte record has no stat block to write");
        assert!(err.contains("stores no stat block"), "unhelpful: {err}");
        assert_eq!(boxed.cached_stats(), None, "nothing may be written");

        let mut party = boxed.clone().party_sized();
        assert_eq!(party.raw.len(), 104, "grown to the party layout");
        let lvl = recalculate_cached_stats(&mut party).expect("now it can compute");
        assert_eq!(lvl, 54, "level comes from exp, which the box slice carries");
        let (cur, max, st) = party.cached_stats().expect("stat block present");
        assert_eq!(cur, max, "a freshly computed mon is at full HP");
        assert!(
            max > 0 && st.iter().all(|v| *v > 0),
            "all stats non-zero: {st:?}"
        );
        // Cross-check against the game formula for a level-54 Arcanine (base 90 HP).
        let bs = crate::basestats::base_stats(59).expect("Arcanine has base stats");
        let ivs = party.ivs();
        let evs = party.evs();
        let expect_hp = calc_stat(
            bs[0] as u32,
            ivs[0] as u32,
            evs[0] as u32,
            54,
            party.nature().raw() as usize,
            0,
        );
        assert_eq!(max as u32, expect_hp, "HP must match the game formula");
    }

    /// The user's report: "I could not recompute xerneas' stat changes upon editing
    /// the save." Xerneas (716) was one of 201 species missing from the generated
    /// table, so the button refused with "no base stats in table".
    #[test]
    fn a_xerneas_can_be_stat_recomputed() {
        let mut m = Mon::blank(0x1234_5678, "Garrett", 716, 50);
        assert_eq!(m.species(), 716);
        let lvl = recalculate_cached_stats(&mut m).expect("Xerneas must be computable");
        assert_eq!(lvl, 50);
        let (cur, max, st) = m.cached_stats().expect("stat block present");
        assert_eq!(cur, max);
        // Xerneas base HP 126 (XERNEAS_MISC_INFO, gen_6.h:4586), level 50.
        let bs = crate::basestats::base_stats(716).expect("table entry");
        assert_eq!(bs[0], 126, "base HP comes from the tree");
        let ivs = m.ivs();
        let evs = m.evs();
        let ni = m.nature().raw() as usize;
        assert_eq!(
            max as u32,
            calc_stat(bs[0] as u32, ivs[0] as u32, evs[0] as u32, 50, ni, 0),
            "HP must match the game formula"
        );
        assert!(st.iter().all(|v| *v > 0), "all stats non-zero: {st:?}");
    }

    /// The other half of the same report: an id the game declares but never defines
    /// must say so, rather than implying the editor's table is broken.
    #[test]
    fn an_id_the_game_declares_without_data_says_so_rather_than_blaming_the_table() {
        let sp = 1481; // "Mimikyu Totem" in the editor's name table
        assert!(
            crate::basestats::species_without_game_data().contains(&sp),
            "expected {sp} to be declared-without-data"
        );
        let mut m = Mon::blank(0x1234_5678, "Garrett", sp, 30);
        let err = recalculate_cached_stats(&mut m).expect_err("no game data to compute from");
        assert!(
            err.contains("species_info table"),
            "message must blame the missing game entry, not the editor: {err}"
        );
        assert!(
            !err.contains("editor's generated table is missing"),
            "{err}"
        );
    }

    #[test]
    fn the_all_iv_write_matches_six_single_iv_writes() {
        let seed: [u8; 6] = [23, 10, 21, 31, 21, 8];
        let make = || {
            let mut m = Mon::from_bytes(&vec![0u8; 80]);
            m.set_species(59);
            for (i, v) in seed.iter().enumerate() {
                m.set_iv_at(i, *v);
            }
            // The word also holds the is-egg bit above the six fields; an all-IV write must
            // not clobber it.
            m.set_is_egg(true);
            m
        };
        let base = make();
        assert_eq!(base.ivs(), seed, "the seed record must decode as written");
        eprintln!(
            "before: ivs={:?} word=0x{:08X} egg={}",
            base.ivs(),
            le32(&base.t[3], 4),
            base.is_egg()
        );

        let mut a = base.clone();
        a.set_all_ivs(31);
        let mut b = base.clone();
        for i in 0..6 {
            b.set_iv_at(i, 31);
        }
        eprintln!(
            "set_all_ivs -> ivs={:?} word=0x{:08X}\nsix set_iv_at -> ivs={:?} word=0x{:08X}",
            a.ivs(),
            le32(&a.t[3], 4),
            b.ivs(),
            le32(&b.t[3], 4)
        );

        let mut c = base.clone();
        c.set_all_ivs(31);
        c.seal();
        eprintln!("after seal(): ivs={:?}", c.ivs());

        assert_eq!(
            a.ivs(),
            [31; 6],
            "set_all_ivs must leave all six fields at 31"
        );
        assert_eq!(
            le32(&a.t[3], 4),
            le32(&b.t[3], 4),
            "one-word and six-field writes must produce the same word"
        );
        assert!(a.is_egg(), "an all-IV write must preserve the egg bit");
        assert_eq!(c.ivs(), [31; 6], "seal() must not disturb the IV word");
        assert!(c.is_egg(), "seal() must not disturb the egg bit");
    }
}
