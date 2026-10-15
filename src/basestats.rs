//! Base stats per species, as the game defines them.
//!
//! The numbers live in `data/basestats.csv` (and the declared-without-data ids in
//! `data/species_without_game_data.csv`), both generated from the game tree by
//! `tools/gen_basestats_v2.py`. They are embedded here with `include_str!`, so the
//! binary carries its data and there is no file to find, ship, or version-match at
//! run time; parsing happens once, on first use.
//!
//! Two things this file must keep honest, because both have already gone wrong
//! once and were only noticed by a user:
//!
//! * a table can silently lose families (201 species disappeared when the
//!   generator only understood brace-form `species_info` entries and not
//!   `[SPECIES_X] = MACRO(args),`), so `game_data::tests` asserts that every id
//!   the editor can name either has stats or is on the declared-without-data list;
//! * a failure to parse must stay visible: `parse_problems` is reported in the
//!   wording of a failed stat recalculation, so a truncated table cannot be
//!   mistaken for "the game has no stats for this species".

use std::sync::OnceLock;

const CSV: &str = include_str!("../data/basestats.csv");
const WITHOUT_DATA_CSV: &str = include_str!("../data/species_without_game_data.csv");

struct Data {
    /// `(species id, [HP, Atk, Def, Spe, SpA, SpD])`, sorted by id.
    table: Vec<(u16, [u16; 6])>,
    /// Ids the game declares in `include/constants/species.h` but gives no
    /// `src/data/pokemon/species_info/` entry for: placeholder and unimplemented
    /// form ids (Scatterbug/Spewpa cloaks, Totem forms, Pikachu/Eevee Partner, and
    /// Rogue's own custom megas in a given checkout).
    without_data: Vec<u16>,
    problems: Vec<String>,
}

static DATA: OnceLock<Data> = OnceLock::new();

fn data() -> &'static Data {
    DATA.get_or_init(|| {
        let mut problems = Vec::new();

        let stats = crate::csv::Table::parse("data/basestats.csv", CSV);
        problems.extend(stats.problems);
        let mut table: Vec<(u16, [u16; 6])> = Vec::with_capacity(stats.rows.len());
        for row in &stats.rows {
            let Ok(id) = row[0].parse::<u16>() else {
                problems.push(format!("data/basestats.csv: species id {:?}", row[0]));
                continue;
            };
            let mut vals = [0u16; 6];
            let mut bad = false;
            for (v, field) in vals.iter_mut().zip(&row[1..]) {
                match field.parse::<u16>() {
                    Ok(n) if n <= 255 => *v = n,
                    _ => {
                        problems.push(format!(
                            "data/basestats.csv: species {id} has stat {:?}, which is not a \
                             base stat",
                            field
                        ));
                        bad = true;
                        break;
                    }
                }
            }
            if !bad {
                table.push((id, vals));
            }
        }
        table.sort_by_key(|(id, _)| *id);
        for pair in table.windows(2) {
            if pair[0].0 == pair[1].0 {
                problems.push(format!(
                    "data/basestats.csv: species {} appears twice with different values",
                    pair[0].0
                ));
            }
        }

        let without =
            crate::csv::Table::parse("data/species_without_game_data.csv", WITHOUT_DATA_CSV);
        problems.extend(without.problems);
        let without_data: Vec<u16> = without
            .rows
            .iter()
            .filter_map(|row| row[0].parse::<u16>().ok())
            .collect();

        Data {
            table,
            without_data,
            problems,
        }
    })
}

/// Base stats for a species: `[HP, Attack, Defense, Speed, SpA, SpD]`,
/// exactly the order the editor's stat formula expects. `None` means this build's
/// table has no entry - see [`species_without_game_data`] for the ids where the
/// game itself has no data.
pub fn base_stats(sp: u16) -> Option<[u16; 6]> {
    let d = data();
    d.table
        .binary_search_by_key(&sp, |(id, _)| *id)
        .ok()
        .map(|i| d.table[i].1)
}

/// Ids the game declares but never defines stats for.
pub fn species_without_game_data() -> &'static [u16] {
    &data().without_data
}

/// Anything wrong with the embedded tables: a bad row, a duplicate id, a stat out
/// of range. Empty is the expected state, and the test suite asserts that; the UI
/// quotes it when a lookup misses, so a damaged table cannot be mistaken for a
/// game limitation.
pub fn parse_problems() -> &'static [String] {
    &data().problems
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded tables must parse clean: a short row or a non-numeric stat is
    /// a broken file, and "no base stats for species N" is the wrong diagnosis for
    /// that.
    #[test]
    fn embedded_tables_parse_without_problems() {
        assert_eq!(parse_problems(), &[] as &[String], "{:?}", parse_problems());
    }

    /// A regeneration that drops a family is the failure mode this project keeps
    /// hitting, so the row count has a floor rather than being merely printed by
    /// the generator. (Counts come from the tree as of the committed tables; the
    /// generator prints the live numbers.)
    #[test]
    fn the_tables_are_not_silently_truncated() {
        let d = data();
        assert!(
            d.table.len() >= 1450,
            "base-stat table shrank to {} rows; a generator regression usually drops whole \
             families",
            d.table.len()
        );
        assert!(
            !d.without_data.is_empty(),
            "the declared-without-data list must still be emitted by the generator"
        );
        // The order the stat formula assumes: HP first and, for a species the
        // report named, present at all.
        assert_eq!(base_stats(716).map(|b| b[0]), Some(126), "Xerneas base HP");
    }
}
