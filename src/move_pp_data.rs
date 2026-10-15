//! Base max PP per move id, as the game's own move table stores it.
//!
//! The values come from `data/move_pp.csv`, generated from the game tree by
//! `tools/gen_movepp.py` (`include/constants/moves.h` plus the `gBattleMoves`
//! entries in `src/data/battle_moves.h`). Like the base-stat table it is embedded
//! with `include_str!` and parsed once, so there is no data file to ship or fail to
//! find. The arithmetic on top of these numbers - the PP-Up cap formula, the
//! ladder, the item gates - lives in `src/move_pp.rs`, transcribed from the game.

use std::sync::OnceLock;

const CSV: &str = include_str!("../data/move_pp.csv");

struct Data {
    /// `(move id, base max PP)`, sorted by move id. A move id absent here is a
    /// move the generated table has no entry for; `0` PP is a real value the game
    /// uses (Sketch, Sheer Cold), so absence and zero mean different things.
    table: Vec<(u16, u8)>,
    problems: Vec<String>,
}

static DATA: OnceLock<Data> = OnceLock::new();

fn data() -> &'static Data {
    DATA.get_or_init(|| {
        let mut problems = Vec::new();
        let parsed = crate::csv::Table::parse("data/move_pp.csv", CSV);
        problems.extend(parsed.problems);
        let mut table: Vec<(u16, u8)> = Vec::with_capacity(parsed.rows.len());
        for row in &parsed.rows {
            let (Ok(id), Ok(pp)) = (row[0].parse::<u16>(), row[1].parse::<u8>()) else {
                problems.push(format!(
                    "data/move_pp.csv: row {:?} is not a move id and a PP value",
                    row
                ));
                continue;
            };
            table.push((id, pp));
        }
        table.sort_by_key(|(id, _)| *id);
        for pair in table.windows(2) {
            if pair[0].0 == pair[1].0 {
                problems.push(format!(
                    "data/move_pp.csv: move {} appears twice",
                    pair[0].0
                ));
            }
        }
        Data { table, problems }
    })
}

/// The whole table, sorted by move id: `base_pp` binary-searches it.
pub fn move_pp_table() -> &'static [(u16, u8)] {
    &data().table
}

/// Anything wrong with the embedded table; empty is the expected state and the
/// tests assert that, so a truncated file cannot pass as "this move has no PP".
pub fn parse_problems() -> &'static [String] {
    &data().problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_parses_clean_and_is_not_truncated() {
        assert_eq!(parse_problems(), &[] as &[String], "{:?}", parse_problems());
        // The committed table covers the game's whole move range; a regeneration
        // that loses a chunk of ids used to be reported only on the generator's
        // stdout, where nobody looked.
        assert!(
            move_pp_table().len() >= 800,
            "move PP table shrank to {} rows",
            move_pp_table().len()
        );
        assert!(
            move_pp_table().windows(2).all(|w| w[0].0 < w[1].0),
            "ids must be unique and sorted"
        );
    }
}
