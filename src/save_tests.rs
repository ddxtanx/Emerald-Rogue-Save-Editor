#![allow(unused_imports)]
use crate::basestats::base_stats;
use crate::deco_variants;
use crate::domain::{DecorVariant, ExteriorStyle, HubArea, InteriorStyle, PathStyle, Pocket};
use crate::format::*;
use crate::game_data::*;
use crate::item_pockets;
use crate::mon::*;
use crate::savefile::*;
use crate::tables::NATURE_IDS;

mod tests {
    use super::*;

    #[test]
    fn safari_iv_columns_are_the_six_stat_indices_and_the_field_is_five_bits() {
        // The UI labels each column from StatIdx, so a row cannot print "SpA"
        // over the SpD bits. Struct order is hp, atk, def, spe, spa, spd.
        let labels: Vec<&str> = crate::domain::StatIdx::ALL
            .iter()
            .map(|s| s.label())
            .collect();
        assert_eq!(labels, ["HP", "Atk", "Def", "Spe", "SpA", "SpD"]);

        // The save packs IVs in 5-bit fields. The safari setters check their
        // input and refuse to write a value that would not fit, rather than
        // spilling into the neighbouring field -- the GUI clamps to 0..=31 so a
        // user cannot reach this, but a hand-built edit must not corrupt.
        let mut s = SafariMon::default();
        let r = std::panic::catch_unwind(|| {
            let mut t = s.clone();
            t.set_iv(crate::domain::StatIdx::Atk, 32);
            t
        });
        assert!(r.is_err(), "IV 32 must not silently overflow a 5-bit field");
        // A legal 31 round-trips exactly, in the column the label says.
        s.set_iv(crate::domain::StatIdx::Spd, 31);
        assert_eq!(s.iv(crate::domain::StatIdx::Spd), 31);
        assert_eq!(
            s.iv(crate::domain::StatIdx::Hp),
            0,
            "neighbour field untouched"
        );
    }

    #[test]
    fn safari_bitfields_roundtrip_without_bleed() {
        // struct RogueSafariMon is two bitfield words + tail. Setting every
        // field to a distinct value and reading it back proves the shifts/masks;
        // asserting the raw words afterwards proves no neighbour was clobbered.
        let mut s = SafariMon::default();
        s.set_species(1005);
        for (k, st) in crate::domain::StatIdx::ALL.iter().enumerate() {
            s.set_iv(*st, (k as u8) + 1); // 1..6
        }
        s.set_shiny(true);
        s.set_pokeball(crate::domain::Ball::from_raw(23));
        s.set_nature(crate::domain::Nature::Quirky); // 24
        s.set_ability_slot(crate::domain::AbilitySlot::Hidden); // 2
        s.set_gender(crate::domain::GenderFlag::Female); // 1
        s.priority = 99;
        s.custom_lookup = 7;

        assert_eq!(s.species(), 1005);
        for (k, st) in crate::domain::StatIdx::ALL.iter().enumerate() {
            assert_eq!(s.iv(*st), (k as u8) + 1, "iv {k}");
        }
        assert!(s.shiny());
        assert_eq!(s.pokeball().raw(), 23);
        assert_eq!(s.nature(), crate::domain::Nature::Quirky);
        assert_eq!(s.ability_slot(), crate::domain::AbilitySlot::Hidden);
        assert_eq!(s.gender(), crate::domain::GenderFlag::Female);

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
        assert_eq!(s.iv(crate::domain::StatIdx::Def), 3);
        assert_eq!(s.w0 & (1 << 31), 0);

        // out-of-range writes must panic rather than corrupt a neighbour
        let r = std::panic::catch_unwind(|| {
            let mut t = SafariMon::default();
            t.set_iv(crate::domain::StatIdx::Hp, 32);
        });
        assert!(r.is_err(), "IV 32 must not silently overflow a 5-bit field");
    }

    #[test]
    fn nature_table_index3_is_adamant() {
        // A real trap when back-solving: index 3 is Adamant (boosts Atk), not Calm.
        assert_eq!(NATURE_IDS[3], (3, "Adamant"));
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
        sf.set_hub_season(crate::domain::Season::Autumn); // day-counter parked at season start
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
        sf.set_hub_upgrade(crate::domain::Upgrade::from_raw(47), true);
        assert_eq!(sf.merged[rb.upgrades_f.data + 5] & 0x80, 0x80);
        // set_diff_toggle indexes BYTES; the CLI/GUI pack bit edits into bytes first
        sf.set_diff_toggle(1, 0b0001_0000); // bit 4 = "wipe bag at adventure start"
        assert_eq!(sf.merged[rb.diff_toggle_f.data + 1], 0b0001_0000);
        sf.set_diff_range(crate::domain::ConfigRange::from_raw(6), 2);
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
        sf.set_hub_area(crate::domain::HubArea::from_raw(2), true);
        sf.set_hub_area(crate::domain::HubArea::from_raw(1), true);
        sf.set_hub_area(crate::domain::HubArea::from_raw(3), true);
        sf.set_hub_area_coord(crate::domain::HubArea::from_raw(2), 0, 0);
        sf.set_hub_area_coord(crate::domain::HubArea::from_raw(1), 1, 0);
        sf.set_hub_area_coord(crate::domain::HubArea::from_raw(3), 2, 0);
        let rb = sf.rogue_block().unwrap();

        // a clean move that keeps the mover attached: Safari (2,0) -> (1,-1) is
        // diagonally off the chain, but (1,0) is AdvEntrance, so (1,-1) is below
        // AdvEntrance and therefore 4-connected to it -> clean
        let w = sf.hub_coord_warnings_at(&rb, 3, 1, -1);
        assert!(
            w.is_empty(),
            "placement 4-connected to built AdvEntrance should be clean, got {w:?}"
        );
        // (the mover's own OLD tile never counts as its new neighbour: moving the
        // leaf Safari from (2,0) to (2,1) really does isolate it)
        let w = sf.hub_coord_warnings_at(&rb, 3, 2, 1);
        assert!(
            w.iter()
                .any(|s| s.contains("no built 4-connected neighbour")),
            "self-old-tile support must not count: {w:?}"
        );

        // a detached island: unreachable by the game's adjacency travel
        let w = sf.hub_coord_warnings_at(&rb, 3, 40, 40);
        assert_eq!(w.len(), 1, "expected exactly the island warning: {w:?}");
        assert!(w[0].contains("no built 4-connected neighbour"));

        // tile collision with a BUILT area
        let w = sf.hub_coord_warnings_at(&rb, 3, 0, 0);
        assert!(
            w.iter().any(|s| s.contains("already holds BUILT area 2")),
            "collision not reported: {w:?}"
        );

        // taking Safari off the chain strands nothing (Safari is the leaf), but
        // moving the MIDDLE area away orphans both ends.
        let w = sf.hub_coord_warnings_at(&rb, 1, 9, 9);
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
        let w = sf.hub_coord_warnings_at(&rb, 1, 2, -1);
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
        let w = sf.hub_coord_warnings_at(&rb, 6, 50, 50);
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

    /// A real save is the only proof that the ball id range is the right one: every
    /// mon the game caught must name as a ball. Skips (green) when the fixture is not
    /// on this machine, so CI without saves stays green.
    #[test]
    fn every_ball_in_a_real_save_names_a_known_ball() {
        let Some(path) = std::env::var("ER_TEST_SAVE")
            .ok()
            .filter(|p| std::path::Path::new(p).exists())
        else {
            eprintln!("every_ball_in_a_real_save: ER_TEST_SAVE not set/found, skipping");
            return;
        };
        let sf = SaveFile::open(&path).unwrap();
        let mut mons: Vec<(&'static str, u8)> = sf
            .party()
            .into_iter()
            .map(|m| (species_name(m.species()), m.ball()))
            .collect();
        for bx in 0..crate::format::ACTIVE_BOXES {
            for slot in 0..crate::format::BOX_SLOTS {
                let m = sf.box_mon(bx, slot);
                if m.species() != 0 {
                    mons.push((species_name(m.species()), m.ball()));
                }
            }
        }
        for i in 0..crate::format::FUSION_COUNT {
            let m = sf.fusion_mon(i);
            if m.species() != 0 {
                mons.push((species_name(m.species()), m.ball()));
            }
        }
        assert!(
            mons.len() > 10,
            "expected a populated save, got {} mons",
            mons.len()
        );
        let mut tally: std::collections::BTreeMap<u8, usize> = Default::default();
        for (name, ball) in &mons {
            tally.entry(*ball).and_modify(|c| *c += 1).or_insert(1);
            assert!(
                *ball == 0 || crate::domain::Ball::from_raw(*ball as u16).is_known(),
                "{name} holds ball id {ball}, outside the branch's ball range \
                 ({}..={}) - the ball table and the save disagree",
                crate::game_data::FIRST_BALL,
                crate::game_data::LAST_BALL
            );
        }
        let named: Vec<String> = tally
            .iter()
            .map(|(b, n)| format!("{b}={}x{}", crate::game_data::ball_name(*b), n))
            .collect();
        eprintln!("ball ids in this save: {}", named.join(", "));
    }

    /// `include/constants/species.h` defines ids up to NUM_SPECIES-1 = 1573, and the
    /// Rogue form block (906+) is where the editor's name table used to run out: 140
    /// ids rendered as the `??????` lookup fallback and 70 more named a different
    /// Pokemon than the id (990 printed "Maractus" for Galarian Darmanitan).
    #[test]
    fn every_species_id_the_game_defines_has_a_name() {
        let unnamed: Vec<u16> = (1..1574u16)
            .filter(|id| crate::game_data::species_name(*id) == "??????")
            .collect();
        assert!(unnamed.is_empty(), "species ids with no name: {unnamed:?}");
    }

    /// The corrected names, pinned on the ids that were wrong in a user's save.
    #[test]
    fn rogue_form_ids_name_the_right_species() {
        // (id, base name it must start with, form word it must carry)
        for (id, base, form) in [
            (990u16, "Darmanitan", "Galarian"),
            (954, "Kyogre", "Primal"),
            (1005, "Sliggoo", "Hisuian"),
            (1023, "Pichu", "Spiky"),
            (959, "Sandshrew", "Alolan"),
            (972, "Exeggutor", "Alolan"),
            (1163, "Xerneas", ""),
            (1434, "Pecharunt", ""),
            (1573, "Glimmora", "Mega"),
        ] {
            let n = crate::game_data::species_name(id);
            assert!(
                n.starts_with(base),
                "species {id} named {n:?}, expected base {base:?}"
            );
            if !form.is_empty() {
                assert!(
                    n.contains(form),
                    "species {id} named {n:?}, missing form {form:?}"
                );
            }
        }
    }

    /// The supplemental table must stay disjoint from the base dex and hold each id
    /// once, or a later lookup silently shadows an earlier entry.
    #[test]
    fn species_extra_stays_disjoint_sorted_and_duplicate_free() {
        let ex = crate::tables_extra::SPECIES_EXTRA;
        let base: std::collections::HashSet<u16> =
            crate::tables::SPECIES.iter().map(|(id, _)| *id).collect();
        let mut prev = 0u16;
        for (id, name) in ex {
            assert!(*id > 905, "form table holds base-dex id {id}");
            assert!(*id > prev, "form table not sorted at {id}");
            assert!(!base.contains(id), "id {id} in both tables");
            assert!(
                !name.is_empty() && *name != "??????",
                "id {id} has a junk name"
            );
            prev = *id;
        }
        assert_eq!(ex.last().map(|(id, _)| *id), Some(1573));
    }

    /// The Safari Zone row now names the ability its slot resolves to and the gender
    /// its flag means. In a real save every stored value must therefore name
    /// something real -- a row that falls back to "no ability in this slot" means the
    /// species table or the slot decode is wrong.
    #[test]
    fn every_safari_row_in_a_real_save_names_its_ability_and_gender() {
        let Some(path) = std::env::var("ER_TEST_SAVE")
            .ok()
            .filter(|p| std::path::Path::new(p).exists())
        else {
            eprintln!("every_safari_row_in_a_real_save: ER_TEST_SAVE not set/found, skipping");
            return;
        };
        let sf = SaveFile::open(&path).unwrap();
        let rb = sf.rogue_block().unwrap();
        let mut rows = 0;
        for i in 0..rb.safari_f.n {
            let Some(s) = sf.safari_mon(&rb, i) else {
                continue;
            };
            let sp = s.species();
            if sp == 0 {
                continue;
            }
            rows += 1;
            assert_ne!(
                crate::game_data::species_name(sp),
                "??????",
                "safari row {i} holds species {sp}, which the name table cannot name"
            );
            let slot = s.ability_slot().raw();
            let label = crate::lookup::ability_slot_options(sp, slot)
                .into_iter()
                .find(|(id, _)| *id == slot as u16)
                .map(|(_, l)| l)
                .unwrap();
            assert_ne!(
                label, "no ability in this slot",
                "safari row {i} (species {sp}) slot {slot} resolves to no ability"
            );
            assert_ne!(
                label, "slot outside this species' ability list",
                "safari row {i} stores slot {slot}, past this species' list"
            );
            let flag = s.gender().raw();
            assert!(
                crate::lookup::gender_flag_options(sp, flag)
                    .iter()
                    .any(|(id, _)| *id == flag as u16),
                "safari row {i} gender flag {flag} is not offered by the picker"
            );
        }
        eprintln!("safari rows checked: {rows}");
        assert!(rows > 0, "expected populated safari rows in this save");
    }

    #[test]
    #[ignore = "manual: prints derived gender + resolved ability for a real save, run with --ignored"]
    fn dump_mon_derived_fields() {
        let path = std::env::var("ER_TEST_SAVE").unwrap();
        let sf = SaveFile::open(&path).unwrap();
        for (i, m) in sf.party().into_iter().enumerate() {
            let sp = m.species();
            println!(
                "party[{i}] {} ball {} ({}) | gflag {} -> {} | ability slot {} -> {}",
                species_name(sp),
                m.ball(),
                crate::game_data::ball_name(m.ball()),
                m.gender_flag(),
                crate::lookup::gender_for_species(sp, m.gender_flag())
                    .map(|g| g.label())
                    .unwrap_or("?"),
                m.ability_num(),
                crate::lookup::ability_label(sp, m.ability_num()),
            );
        }
        for bx in 0..10 {
            let mut names = Vec::new();
            for slot in 0..30 {
                let m = sf.box_mon(bx, slot);
                let sp = m.species();
                if sp == 0 {
                    continue;
                }
                names.push(format!(
                    "{} {}[slot{}->{}]",
                    species_name(sp),
                    crate::lookup::gender_for_species(sp, m.gender_flag())
                        .map(|g| g.label())
                        .unwrap_or("?"),
                    m.ability_num(),
                    crate::lookup::ability_label(sp, m.ability_num()),
                ));
            }
            if !names.is_empty() {
                println!("box {bx}: {}", names.join(", "));
            }
        }
    }

    /// Each safari roster row is bit-packed fields whose names only exist via the
    /// species table, so this prints every populated row with the names the game
    /// would use (ability / gender / ball). Ground truth for the roster UI.
    #[test]
    #[ignore = "manual: prints safari roster rows with resolved names, run with --ignored"]
    fn dump_safari_roster() {
        let path = std::env::var("ER_TEST_SAVE").unwrap();
        let sf = SaveFile::open(&path).unwrap();
        let rb = sf.rogue_block().unwrap();
        let mut populated = 0;
        for i in 0..rb.safari_f.n {
            let Some(s) = sf.safari_mon(&rb, i) else {
                continue;
            };
            let sp = s.species();
            if sp == 0 {
                continue;
            }
            populated += 1;
            println!(
                "safari[{i:3}] sp {:>4} {:<13} abil slot {:>2} -> {:<18} gender {:>2} -> {:<9} ball {:>2} ({})",
                sp,
                species_name(sp),
                s.ability_slot().raw(),
                crate::lookup::ability_label(sp, s.ability_slot().raw()),
                s.gender().raw(),
                crate::lookup::gender_for_species(sp, s.gender().raw())
                    .map(|g| g.label())
                    .unwrap_or("?"),
                s.pokeball().raw(),
                ball_name(s.pokeball().raw() as u8),
            );
        }
        println!("populated safari rows: {populated} of n={}", rb.safari_f.n);
    }

    #[test]
    #[ignore = "manual: prints hub snapshot the GUI renders, run with --ignored"]
    fn dump_hub_for_gui_crosscheck() {
        let path = std::env::var("ER_TEST_SAVE").unwrap();
        let sf = SaveFile::open(&path).unwrap();
        let rb = sf.rogue_block().unwrap();
        for i in 0..rb.coords_f.n {
            let built = sf.hub_area_built_at(&rb, i);
            let (x, y) = sf.hub_area_coord_at(&rb, i).unwrap();
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
        // Which props, where: the house grid paints one marker per active slot, and
        // "what is this icon?" can only be answered from these bytes.
        for k in 0..rb.decor_f.n {
            if let Some((x, y, v, active)) = sf.hub_decoration(&rb, k) {
                if active {
                    println!(
                        "decor slot {k}: ({x},{y}) variant {} {:?} range={} name={:?}",
                        v.raw(),
                        v,
                        if k < HOME_DECOR_OUTSIDE {
                            "outdoor"
                        } else {
                            "indoor"
                        },
                        deco_variants::DECOR_VARIANTS.get(v.raw() as usize)
                    );
                }
            }
        }
    }

    #[test]
    fn pocket_name_never_panics_and_names_every_pocket() {
        // Regression: the GUI Items tab renders pocket_name() per row; an
        // earlier version sliced a "POCKET_" prefix off display names that
        // don't have it and panicked on "Items" (opening the tab killed the
        // app). Every u8 must produce a label, known pockets must name right.
        assert_eq!(pocket_name(Pocket::from_raw(3)), "Medicine (pocket 3)");
        assert_eq!(pocket_name(Pocket::from_raw(1)), "Items (pocket 1)");
        for p in 0..=255u8 {
            let s = pocket_name(Pocket::from_raw(p));
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
        sf.set_hub_area_coord(HubArea::from_raw(0), -2, 3);
        let o = rb.coords_f.data;
        assert_eq!(
            &sf.merged[o..o + 4],
            &[0xFE, 0x03, 0x00, 0x00],
            "areaCoords entry must be s8 x, s8 y, then 2 pad bytes"
        );
        assert_eq!(sf.hub_area_coord(&rb, HubArea::from_raw(0)), Some((-2, 3)));
        assert_eq!(sf.hub_area_coord_raw(&rb, 0), Some([0xFE, 0x03, 0, 0]));
        // stride 4 must hold: entry 1 starts 4 bytes after entry 0
        sf.set_hub_area_coord(HubArea::from_raw(1), 120, -1);
        assert_eq!(
            &sf.merged[o + 4..o + 8],
            &[120, 0xFF, 0x00, 0x00],
            "coords stride must be exactly 4"
        );
        assert_eq!(
            sf.hub_area_coord(&rb, HubArea::from_raw(1)),
            Some((120, -1))
        );
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
        sf.set_hub_area_coord(HubArea::from_raw(10), 30, 40);
        assert!(
            !sf.hub_area_built(&rb, HubArea::from_raw(10)),
            "moving must not raise built bit"
        );
        sf.set_hub_area(HubArea::from_raw(10), true);
        assert!(sf.hub_area_built(&rb, HubArea::from_raw(10)));
        assert_eq!(
            sf.hub_area_coord(&rb, HubArea::from_raw(10)),
            Some((30, 40)),
            "build must not move"
        );
        sf.set_hub_area(HubArea::from_raw(10), false);
        assert!(!sf.hub_area_built(&rb, HubArea::from_raw(10)));
        assert_eq!(
            sf.hub_area_coord(&rb, HubArea::from_raw(10)),
            Some((30, 40))
        );

        // decorations: u8 x, u8 y, u8 variant, u8 active:1 -- toggling active
        // must not disturb the variant byte or the unused 7 bits.
        let d = rb.decor_f.data;
        sf.merged[d + 3] = 0b1000_0000; // foreign non-zero unused bits
        sf.set_hub_decoration(0, 12, 9, DecorVariant::from_raw(47), true);
        assert_eq!(
            sf.hub_decoration(&rb, 0),
            Some((12, 9, DecorVariant::from_raw(47), true))
        );
        assert_eq!(sf.merged[d + 3], 0b1000_0001, "only bit 0 may change");
        sf.set_hub_decoration(0, 12, 9, DecorVariant::from_raw(47), false);
        assert_eq!(sf.merged[d + 3], 0b1000_0000);
        assert_eq!(
            sf.hub_decoration(&rb, 0),
            Some((12, 9, DecorVariant::from_raw(47), false))
        );
        // slot 160 is the first indoor slot, 640 bytes after slot 0
        sf.set_hub_decoration(160, 1, 2, DecorVariant::from_raw(3), true);
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
        sf.set_hub_area_coord(HubArea::from_raw(99), 1, 1);
        sf.set_hub_area(HubArea::from_raw(99), true);
        sf.set_hub_decoration(9999, 1, 1, DecorVariant::from_raw(1), true);
        sf.set_hub_style(9, 9);
        sf.set_hub_wander_mon(99, 5, false);
        let rb3 = sf.rogue_block().unwrap();
        assert_eq!(rb3.coords_f.n, 11, "walk must survive bad-index writes");
        assert_eq!(
            &sf.merged[rb3.mastery_f.data..rb3.mastery_f.data + 8],
            before_mastery,
            "out-of-range hub writes must not spill into later arrays"
        );
        assert_eq!(sf.hub_area_coord(&rb3, HubArea::from_raw(11)), None);
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
        assert_eq!(
            item_pocket(39),
            Pocket::from_raw(item_pockets::POCKET_MEDICINE)
        ); // Potion
        assert_eq!(
            item_pocket(1),
            Pocket::from_raw(item_pockets::POCKET_POKE_BALLS)
        ); // Poke Ball
        assert_eq!(
            item_pocket(265),
            Pocket::from_raw(item_pockets::POCKET_STONES)
        );
        assert_eq!(
            item_pocket(64),
            Pocket::from_raw(item_pockets::POCKET_HELD_ITEMS)
        );

        sf.set_bag_capacity_upgrades(3);
        assert_eq!(sf.bag_capacity(), 110);

        // Start from a legal bag: medicine run, then stones, then balls,
        // each pocket exactly once, in ascending pocket order.
        sf.write_bag_slots(false, &[(39, 20), (40, 5), (265, 3), (1, 10), (2, 5)]);
        assert_eq!(
            SaveFile::bag_run_summary(&sf.bag_slots_raw(false).unwrap()),
            vec![
                (Pocket::from_raw(3), 2),
                (Pocket::from_raw(4), 1),
                (Pocket::from_raw(5), 2)
            ]
        );
        assert!(sf.bag_warnings(false).is_empty());

        // Medicine added now must land inside the medicine run, not at the tail.
        sf.add_bag_item(39, 5, false).unwrap(); // merges into the Potion stack
        sf.add_bag_item(44, 1, false).unwrap(); // new medicine slot (Revive)
        sf.add_bag_item(1, 4, false).unwrap(); // merges into the Poke Ball stack
        let runs = SaveFile::bag_run_summary(&sf.bag_slots_raw(false).unwrap());
        assert_eq!(
            runs,
            vec![
                (Pocket::from_raw(3), 3),
                (Pocket::from_raw(4), 1),
                (Pocket::from_raw(5), 2)
            ]
        );
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
            .filter(|(i, _)| item_pocket(*i) == Pocket::from_raw(item_pockets::POCKET_MEDICINE))
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(medicine_ids, vec![39, 40, 44], "sorted insertion point");

        // Removal collapses emptied slots so no hole opens inside a run.
        sf.remove_bag_item(44, 1, false).unwrap();
        let runs = SaveFile::bag_run_summary(&sf.bag_slots_raw(false).unwrap());
        assert_eq!(
            runs,
            vec![
                (Pocket::from_raw(3), 2),
                (Pocket::from_raw(4), 1),
                (Pocket::from_raw(5), 2)
            ]
        );
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
                .map(|(_, v)| Pocket::from_raw(*v))
                .collect::<Vec<_>>(),
            vec![
                Pocket::from_raw(item_pockets::POCKET_ITEMS),
                Pocket::from_raw(item_pockets::POCKET_HELD_ITEMS),
                Pocket::from_raw(item_pockets::POCKET_MEDICINE),
                Pocket::from_raw(item_pockets::POCKET_STONES),
                Pocket::from_raw(item_pockets::POCKET_POKE_BALLS),
                Pocket::from_raw(item_pockets::POCKET_TM_HM),
                Pocket::from_raw(item_pockets::POCKET_BERRIES),
                Pocket::from_raw(item_pockets::POCKET_POKEBLOCK),
                Pocket::from_raw(item_pockets::POCKET_KEY_ITEMS),
            ]
        );
        assert_eq!(
            item_pocket(2),
            Pocket::from_raw(item_pockets::POCKET_POKE_BALLS)
        );
        assert_eq!(
            item_pocket(68),
            Pocket::from_raw(item_pockets::POCKET_MEDICINE)
        );
        // Evolution items are baked into POCKET_ITEMS by rogue_baked.c even
        // though items.h parks them elsewhere; the table carries that patch.
        for id in item_pockets::EVO_ITEMS {
            assert_eq!(
                item_pocket(*id),
                Pocket::from_raw(item_pockets::POCKET_ITEMS),
                "evolution item {id} must resolve to the Items pocket"
            );
        }
        // Ids with no pocket are unusable and must be rejected by the editor.
        assert_eq!(item_pocket(0), Pocket::NONE);
        assert_eq!(item_pocket(0xFFFF), Pocket::NONE);
    }
}

#[test]
fn gender_flag_only_ever_holds_the_values_the_game_writes() {
    // Save files are personal data and never committed, so this checks the bit
    // layout against a real save only when one is reachable: `ER_TEST_SAVE`, or
    // the developer's own save at the default path. Skips (green) otherwise, so
    // CI without saves stays green while local runs still pin the layout.
    let path = std::env::var("ER_TEST_SAVE").unwrap_or_else(|_| "/workspace/hermes/er2.sav".into());
    if !std::path::Path::new(&path).exists() {
        eprintln!(
            "gender_flag_only_ever_holds_the_values_the_game_writes: no save at {path}, skipping"
        );
        return;
    }
    let sf = SaveFile::open(&path).expect("load save");
    // GetGenderForSpecies treats genderFlag as a boolean (0 -> male, anything
    // else -> female), so a correct read of the 3-bit field can never return a
    // value above 1. Anything larger means the bit offsets are wrong and we are
    // reading neighbouring ribbon/shiny bits.
    let mut seen: Vec<u8> = Vec::new();
    let mut check = |m: &Mon| {
        if !seen.contains(&m.gender_flag()) {
            seen.push(m.gender_flag());
        }
    };
    for m in sf.party() {
        if m.species() != 0 {
            check(&m);
        }
    }
    for bx in 0..crate::format::ACTIVE_BOXES {
        for i in 0..crate::format::BOX_SLOTS {
            let m = sf.box_mon(bx, i);
            if m.species() != 0 {
                check(&m);
            }
        }
    }
    seen.sort();
    assert!(
        seen.iter().all(|v| *v <= 1),
        "gender_flag values read from the save: {seen:?} (expected only 0/1)"
    );
    assert!(
        seen.contains(&0) && seen.contains(&1),
        "expected both male and female flags in this save, saw {seen:?}"
    );
}
