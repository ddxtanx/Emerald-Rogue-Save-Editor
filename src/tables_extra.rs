//! The supplemental name tables: Rogue's added species forms, Rogue's own
//! items, and the quest slot names.
//!
//! Same arrangement as `tables`: rows live in `data/*.csv`, embedded with
//! `include_str!`, parsed on first use, no data files at run time. These are
//! the ids the base game headers never had, which is why they are kept separate
//! from `tables` rather than merged into it.

use std::sync::OnceLock;

const SPECIES_EXTRA_CSV: &str = include_str!("../data/species_names_extra.csv");
const ITEMS_EXTRA_CSV: &str = include_str!("../data/item_names_extra.csv");
const QUEST_NAMES_CSV: &str = include_str!("../data/quest_names.csv");

/// Species form ids above the base dex (906..1573): megas, gigantamax forms,
/// regional and fake forms Rogue defines. `game_data::species_name` looks here
/// after `tables::species`.
pub fn species_extra() -> &'static [(u16, &'static str)] {
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_named("data/species_names_extra.csv", SPECIES_EXTRA_CSV))
}

/// Rogue's own item ids (827..1072): megasteins, fossil fossils, fusion items
/// and the rest of its custom bag content.
pub fn items_extra() -> &'static [(u16, &'static str)] {
    static T: OnceLock<Vec<(u16, &'static str)>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_named("data/item_names_extra.csv", ITEMS_EXTRA_CSV))
}

/// Quest slot names by index; the Rogue block stores 232 quest slots and the
/// game itself has no name string for most of them.
pub fn quest_names() -> &'static [&'static str] {
    static T: OnceLock<Vec<&'static str>> = OnceLock::new();
    T.get_or_init(|| crate::csv::parse_labels("data/quest_names.csv", QUEST_NAMES_CSV))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supplemental_tables_are_present_and_plausible() {
        assert!(
            species_extra().len() >= 600,
            "species_extra: {} rows",
            species_extra().len()
        );
        assert_eq!(species_extra()[0].0, 906, "form ids start above the dex");
        assert!(
            species_extra().windows(2).all(|w| w[0].0 < w[1].0),
            "species_extra ids must increase"
        );
        assert!(
            items_extra().len() >= 80,
            "items_extra: {} rows",
            items_extra().len()
        );
        assert_eq!(
            items_extra()[0].0,
            827,
            "Rogue items start where the dex ends"
        );
        assert!(
            quest_names().len() >= 200,
            "quest_names: {} rows",
            quest_names().len()
        );
    }

    /// The quest list is indexed by slot number, so an off-by-one in the CSV would
    /// silently relabel every quest the Rogue tab shows.
    #[test]
    fn quest_slot_names_stay_aligned_with_their_index() {
        assert_eq!(quest_names()[0], "To Adventure!");
        assert_eq!(quest_names()[1], "Gym Badge 1");
        // The row that forced quoting into the dialect: a dropped row here would
        // silently shift every quest slot after it.
        assert_eq!(quest_names()[27], "Poké Ball, Go!");
        assert!(
            quest_names()
                .iter()
                .enumerate()
                .all(|(i, n)| !n.is_empty() || i >= 190),
            "the early quest slots must all be named"
        );
    }

    #[test]
    fn a_few_added_form_and_item_names_are_anchored() {
        let forms = species_extra();
        assert_eq!(
            forms.iter().find(|(id, _)| *id == 906).map(|(_, n)| *n),
            Some("Venusaur (Mega)")
        );
        assert!(
            forms.iter().any(|(_, n)| n.contains("Silvally")),
            "form families present"
        );
        let items = items_extra();
        assert_eq!(items[0], (827, "Link Cable"));
        // ...and id 827 also exists in `data/item_names.csv`, as the game's own "Rogue
        // Item First" sentinel. `game_data::all_items()` keeps both rows, so a lookup
        // of 827 answers the sentinel. Pinned so that changing either table is a
        // decision someone makes on purpose.
        assert_eq!(
            crate::tables::items().last(),
            Some(&(827, "Rogue Item First")),
            "the base item table ends on the game's sentinel row"
        );
    }
}
