//! Move PP: the game's base values (generated table in `move_pp_data.rs`) plus
//! the game's own PP-Up arithmetic, transcribed rather than approximated.

use crate::move_pp_data::MOVE_PP;

/// PP Ups are 2 bits per move slot (`gPPUpGetMask`, `src/pokemon.c:424`) and the
/// items never push the field past 3: the PP Up branch requires the stored value
/// `<= 2` (`src/pokemon.c:3638`) and PP Max requires `< 3` (`:3914`), after which
/// it sets the field to exactly 3 (`gPPUpAddValues * 3`, `:3918`).
pub const MAX_PP_UPS: u8 = 3;

/// Base max PP as the game's move table stores it (0 for the empty slot).
pub fn base_pp(move_id: u16) -> u8 {
    MOVE_PP
        .binary_search_by(|(id, _)| id.cmp(&move_id))
        .ok()
        .map(|i| MOVE_PP[i].1)
        .unwrap_or(0)
}

/// Max PP with `ups` PP Ups, exactly as `CalculatePPWithBonus` computes it:
/// `basePP + ((basePP * 20 * ups) / 100)` (`src/pokemon.c:3412`).
///
/// The earlier editor version used `base + base/5 * ups`, which divides at a
/// different point and loses a PP for every base that is not a multiple of 5
/// (base 12: +6 instead of +7 over three ups; base 7: +9 instead of +11).
pub fn max_pp(move_id: u16, ups: u8) -> u8 {
    let base = base_pp(move_id) as u32;
    let ups = ups.min(MAX_PP_UPS) as u32;
    (base + (base * 20 * ups) / 100).min(u8::MAX as u32) as u8
}

/// Whether the PP Up / PP Max items would touch this move at all: both branches
/// refuse a move whose current max PP is under 5 (`temp1 > 4` at
/// `src/pokemon.c:3638`, `temp2 >= 5` at `:3914`), which is how Sketch (1 PP) and
/// Sheer Cold (1 PP) are excluded.
pub fn vitamins_apply(move_id: u16) -> bool {
    max_pp(move_id, 0) >= 5
}

/// Current PP after the cap changes from `from_ups` to `to_ups`. The game adds
/// the cap delta to the stored current PP rather than refilling the move
/// (`newCap - oldCap` on top of `MON_DATA_PP1`: `src/pokemon.c:3643-3645` for the
/// PP Up, `:3920-3922` for PP Max). Mirroring that keeps an edited mon consistent
/// (raising ups never leaves it at `0/24`); lowering ups clamps to the new cap.
pub fn bump_current_pp(cur: u8, move_id: u16, from_ups: u8, to_ups: u8) -> u8 {
    let old_cap = max_pp(move_id, from_ups) as i32;
    let new_cap = max_pp(move_id, to_ups) as i32;
    if new_cap >= old_cap {
        (cur as i32 + (new_cap - old_cap)).min(new_cap) as u8
    } else {
        // Nothing in the game lowers PP Ups, so there is no item behaviour to
        // copy: taking them away clamps current PP down to the smaller cap
        // instead of draining the mon by a delta nobody asked for.
        cur.min(new_cap as u8)
    }
}

/// The PP ladder for one move slot: (ups, max PP) for 0..=MAX_PP_UPS.
pub fn pp_ladder(move_id: u16) -> [(u8, u8); 4] {
    [
        (0, max_pp(move_id, 0)),
        (1, max_pp(move_id, 1)),
        (2, max_pp(move_id, 2)),
        (3, max_pp(move_id, 3)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CalculatePPWithBonus` (`src/pokemon.c:3412`) is `base + (base*20*ups)/100`.
    /// Hand-computed against the tree's own bases: POUND (id 1, base 35) gives
    /// 35/42/49/56, ZEN_HEADBUTT (id 428, base 15) gives 15/18/21/24, and
    /// SHEER_COLD (id 329, base 5) gives 5/6/7/8 - the low base is the interesting
    /// shape, since 5*20/100 = 1 and each up still adds exactly one.
    #[test]
    fn caps_follow_the_game_formula() {
        assert_eq!(pp_ladder(1), [(0, 35), (1, 42), (2, 49), (3, 56)]);
        assert_eq!(pp_ladder(428), [(0, 15), (1, 18), (2, 21), (3, 24)]);
        assert_eq!(pp_ladder(329), [(0, 5), (1, 6), (2, 7), (3, 8)]);
        // Every base PP in this tree is a multiple of 5, so the older
        // `base + base/5*ups` happened to agree here; the game's own expression is
        // used so the cap cannot drift if a base ever stops being one.
        for (id, base) in crate::move_pp_data::MOVE_PP {
            let base = *base as u32;
            if base >= 5 {
                assert_eq!(
                    max_pp(*id, 3),
                    (base + (base * 60) / 100).min(255) as u8,
                    "slot cap for move id {} must match base + base*20*3/100",
                    id
                );
            }
        }
    }

    /// The ups field is 2 bits and the game never stores more than 3 (the PP Up
    /// branch requires `<= 2`, `src/pokemon.c:3638`; PP Max sets exactly 3, `:3918`).
    /// A hand-typed 9 must clamp rather than wrap into the next slot's bits.
    #[test]
    fn ups_past_three_do_not_move_the_cap() {
        assert_eq!(max_pp(1, 9), max_pp(1, 3));
        assert_eq!(MAX_PP_UPS, 3);
    }

    /// Both items refuse a move under 5 PP (`temp1 > 4` at `src/pokemon.c:3638`,
    /// `temp2 >= 5` at `:3914`), which is how SKETCH (id 166, 1 PP) is excluded.
    /// SHEER_COLD sits at exactly 5 and does qualify.
    #[test]
    fn vitamins_are_refused_below_five_pp() {
        assert!(!vitamins_apply(166), "SKETCH has 1 PP");
        assert!(vitamins_apply(329), "SHEER_COLD has exactly 5 PP");
        assert!(!vitamins_apply(0), "the empty move slot takes nothing");
    }

    /// The items add the cap delta to CURRENT PP rather than refilling the move
    /// (`src/pokemon.c:3643-3645` for the PP Up, `:3920-3922` for PP Max), so an
    /// edit that changes ups must not leave the mon at `0/24`. Nothing in the game
    /// lowers PP Ups, so lowering one only clamps current PP to the smaller cap.
    #[test]
    fn changing_ups_bumps_current_pp_the_way_the_item_does() {
        assert_eq!(bump_current_pp(15, 428, 0, 1), 18);
        // Zen Headbutt's caps run 15/18/21/24, so three ups add nine - and the item
        // hands that nine to CURRENT PP rather than refilling a depleted move. The
        // "max PP" button is what fills a slot up to its cap.
        assert_eq!(
            bump_current_pp(0, 428, 0, 3),
            9,
            "a depleted move gains the delta, not the cap"
        );
        assert_eq!(
            bump_current_pp(15, 428, 0, 3),
            24,
            "a full move keeps pace with the cap"
        );
        assert_eq!(
            bump_current_pp(20, 428, 0, 1),
            18,
            "current PP never exceeds the cap"
        );
        // Lowering: clamp to the cap, never drain below what the mon already has.
        assert_eq!(bump_current_pp(24, 428, 3, 0), 15);
        assert_eq!(bump_current_pp(15, 428, 2, 0), 15);
        assert_eq!(bump_current_pp(3, 428, 2, 0), 3);
    }

    /// The reported PP bug, at its root: `MOVE_PP` used to be generated from the
    /// 2.2.07 tree, whose move table stopped at id 376, so Zen Headbutt (428) and
    /// every other newer move rendered as `PP - base 0` with a ladder of zeroes.
    /// Every move the picker offers must have a base PP.
    #[test]
    fn every_move_the_ui_offers_has_a_base_pp() {
        let missing: Vec<(u16, &str)> = crate::tables::MOVES
            .iter()
            .filter(|(id, _)| *id != 0)
            .filter(|(id, _)| base_pp(*id) == 0)
            .map(|(id, name)| (*id, *name))
            .collect();
        assert!(
            missing.is_empty(),
            "{} moves have no PP entry, first few: {:?}",
            missing.len(),
            &missing[..missing.len().min(6)]
        );
        assert!(
            crate::move_pp_data::MOVE_PP.len() >= 800,
            "the PP table looks truncated: {} entries",
            crate::move_pp_data::MOVE_PP.len()
        );
    }
}
