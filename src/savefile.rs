//! The open save file: sector container, active-slot selection, merged stream,
//! party/box/item accessors, Rogue block walk, stash, safari/unique mons, and
//! write-back with checksum re-sealing (mGBA trailer preserved).

use crate::format::*;
use crate::game_data::*;
use crate::mon::*;

// ---------------------------------------------------------------- save file
pub(crate) struct Sector {
    pub(crate) data: Vec<u8>, // DATASZ
    pub(crate) id: u16,
    pub(crate) csum: u16,
    pub(crate) security: u32,
    pub(crate) counter: u32,
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
    pub(crate) fn bag_slots_raw(&self, stash: bool) -> Option<Vec<(u16, u16)>> {
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
    pub(crate) fn write_bag_slots(&mut self, stash: bool, slots: &[(u16, u16)]) {
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
    pub(crate) fn bag_run_summary(slots: &[(u16, u16)]) -> Vec<(u8, usize)> {
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
            if item_pocket(*id) == crate::item_pockets::POCKET_NONE {
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
            if item_pocket(*id) == crate::item_pockets::POCKET_KEY_ITEMS {
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
        if pocket == crate::item_pockets::POCKET_NONE {
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
        let budget = if pocket == crate::item_pockets::POCKET_KEY_ITEMS {
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
