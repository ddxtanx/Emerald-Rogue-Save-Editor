//! Species, move, item and nature names, as the game's own headers name them.
//!
//! The rows live in `data/*.csv` (extracted from the previous generated Rust by
//! `tools/rust_tables_to_csv.py`, so the move to tabular storage changed the
//! format and nothing else). They are embedded with `include_str!` and parsed
//! on first use; the binary still carries its own data and reads no files at
//! run time.
//!
//! These four tables have no generator in `tools/` any more - the headers they
//! came from predate this repo - so the CSVs are the source of truth and the
//! row-count tests below are what stops a bad edit or a truncated file from
//! passing as a game fact. `tools/csv_set.py` exists for editing them without
//! opening a spreadsheet.

use std::sync::OnceLock;

const SPECIES_CSV: &str = include_str!("../data/species_names.csv");
const MOVES_CSV: &str = include_str!("../data/move_names.csv");
const ITEMS_CSV: &str = include_str!("../data/item_names.csv");
const NATURES_CSV: &str = include_str!("../data/natures.csv");

/// Base dex species names, ids 1..905. Rogue's added form ids (906 up) are in
/// `tables_extra::species_extra`; `game_data::species_name` covers both.
pub fn species() -> &'static [(u16, &'static str)] {
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_named("data/species_names.csv", SPECIES_CSV))
}

/// Move names, ids 1..848. Same id space as `move_pp_data`, which stores their
/// PP.
pub fn moves() -> &'static [(u16, &'static str)] {
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_named("data/move_names.csv", MOVES_CSV))
}

/// Item names, ids 1..827. Rogue's own items (827 up) are in
/// `tables_extra::items_extra`; pockets for both live in `item_pockets`.
pub fn items() -> &'static [(u16, &'static str)] {
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_named("data/item_names.csv", ITEMS_CSV))
}

/// The 25 natures, id == the game's `enum Nature` value.
pub fn natures() -> &'static [(u16, &'static str)] {
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_named("data/natures.csv", NATURES_CSV))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Floors, not exact counts: a regeneration from the wrong tree silently loses
    /// whole ranges (the base-stat table lost 201 species that way), and a
    /// truncated CSV looks identical to the UI until someone asks for one id.
    #[test]
    fn the_name_tables_are_present_and_plausible() {
        for (name, table, floor, last) in [
            ("species", species(), 850, 905),
            ("moves", moves(), 800, 848),
            ("items", items(), 800, 827),
            ("natures", natures(), 25, 24),
        ] {
            assert!(
                table.len() >= floor,
                "{name}: {len} rows, below the expected floor",
                len = table.len()
            );
            assert_eq!(
                table.last().map(|(id, _)| *id),
                Some(last),
                "{name}: last id moved"
            );
            assert!(
                table.windows(2).all(|w| w[0].0 < w[1].0),
                "{name}: ids must be strictly increasing"
            );
            assert!(
                table.iter().all(|(_, n)| !n.is_empty()),
                "{name}: empty label"
            );
        }
    }

    /// Anchors the UI reads from, so a column shift or a mis-ordered CSV shows up
    /// here rather than as a plausible-looking wrong name in a picker.
    #[test]
    fn known_names_land_on_known_ids() {
        assert_eq!(species()[0], (1, "Bulbasaur"));
        assert_eq!(
            species().iter().find(|(id, _)| *id == 376).map(|(_, n)| *n),
            Some("Metagross")
        );
        assert_eq!(
            moves().iter().find(|(id, _)| *id == 7).map(|(_, n)| *n),
            Some("Fire Punch")
        );
        assert_eq!(natures()[3], (3, "Adamant"));
        assert_eq!(
            items().iter().find(|(id, _)| *id == 1).map(|(_, n)| *n),
            Some("Poke Ball"),
            "id 1 is the first ball, which is what `game_data::balls()` relies on"
        );
    }
}
