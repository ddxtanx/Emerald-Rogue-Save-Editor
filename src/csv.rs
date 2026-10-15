//! A deliberately small CSV reader for the game-data tables this editor ships.
//!
//! The tables are authored as CSV in `data/` because that is diffable,
//! inspectable with ordinary tools, and editable without touching Rust. They
//! are then embedded into the binary with `include_str!` and parsed once at
//! startup, so the shipped `.exe` still has no data files to find,
//! version-match, or fail to load. Parsing never happens against the
//! filesystem.
//!
//! The dialect is the one these generated tables need, nothing more:
//!
//! * a header line, which is skipped;
//! * blank lines and lines starting with `#` are comments;
//! * comma-separated fields, trimmed;
//! * a field may be wrapped in double quotes when it contains a comma, and `""`
//! inside quotes is one literal quote. Exactly one row in the corpus needs it
//! (quest slot 27, `Poké Ball, Go!`): the game's own string says so, and
//! dropping that row would mis-number every quest after it;
//! * a row whose field count differs from the header is reported as a problem
//! rather than guessed at.
//!
//! Callers get the rows plus the problems found; a table that failed to parse
//! keeps working with whatever parsed, and the problems stay visible so a
//! truncated file cannot masquerade as "this species has no stats" - the exact
//! confusion the base-stat table caused when a generator silently dropped 201
//! species.

use std::borrow::Cow;
use std::sync::Mutex;

/// Every problem any table loader has reported, kept for the load path and the
/// tests. A `Mutex` rather than a `OnceLock` because the tables load lazily and
/// independently.
static PROBLEMS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Split one CSV line into trimmed fields.
///
/// Lines without a quote - nearly all of them - borrow their fields from
/// `line`, which for these tables is the embedded `'static` source. Only a line
/// with quoting allocates.
fn split_fields<'a>(line: &'a str) -> Vec<Cow<'a, str>> {
    if !line.contains('"') {
        return line.split(',').map(|f| Cow::Borrowed(f.trim())).collect();
    }

    let mut out: Vec<Cow<'a, str>> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if quoted && chars.peek() == Some(&'"') {
                    // `""` inside a quoted field is one literal quote.
                    field.push('"');
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            }
            ',' if !quoted => out.push(Cow::Owned(std::mem::take(&mut field).trim().to_string())),
            _ => field.push(c),
        }
    }
    out.push(Cow::Owned(field.trim().to_string()));
    out
}

/// One parsed table: rows as fields, plus anything malformed.
pub(crate) struct Table<'a> {
    /// Data rows in file order.
    pub rows: Vec<Vec<Cow<'a, str>>>,
    /// Human-readable problems, one per bad line ("file:line: reason" style).
    pub problems: Vec<String>,
}

impl<'a> Table<'a> {
    /// Parse `source` (an `include_str!`-ed table) into rows.
    ///
    /// `name` labels the problems, so a failure says which table it came from; it
    /// is not borrowed by the result, only `source` is.
    pub(crate) fn parse(name: &str, source: &'a str) -> Table<'a> {
        let mut rows = Vec::new();
        let mut problems = Vec::new();
        let mut width = 0usize;
        for (no, raw) in source.lines().enumerate() {
            // Trimmed at both ends: a comment indented inside the file is still a comment,
            // and every field is trimmed anyway.
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = split_fields(line);
            if width == 0 {
                // The first content line is the header.
                width = fields.len();
                continue;
            }
            if fields.len() != width {
                problems.push(format!(
                    "{name} line {}: expected {width} fields, got {} ({line})",
                    no + 1,
                    fields.len()
                ));
                continue;
            }
            rows.push(fields);
        }
        Table { rows, problems }
    }
}

/// Every problem reported so far, for the load path to surface.
pub(crate) fn problems() -> Vec<String> {
    PROBLEMS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Record a problem seen outside the `id,label` readers (a numeric-table
/// loader, for instance, validating its own columns).
pub(crate) fn report(problem: String) {
    PROBLEMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(problem);
}

/// Turn a parsed field into a label that lives as long as the process, which is
/// what the accessors hand out. Unquoted fields already borrow from the
/// embedded source; a quoted one had its escapes removed, so its text exists
/// nowhere else and is kept for the process - the same lifetime a static table
/// would have had, for the single row in the corpus that needs it.
fn label(field: Cow<'static, str>) -> &'static str {
    match field {
        Cow::Borrowed(s) => s,
        Cow::Owned(s) => Box::leak(s.into_boxed_str()),
    }
}

/// Parse an `id,label` table. Problems are also recorded centrally, see
/// [`problems`].
pub(crate) fn parse_named(name: &'static str, source: &'static str) -> Vec<(u16, &'static str)> {
    let t = Table::parse(name, source);
    if !t.problems.is_empty() {
        let mut reg = PROBLEMS.lock().unwrap_or_else(|e| e.into_inner());
        reg.extend(t.problems.iter().cloned());
    }
    let mut out = Vec::with_capacity(t.rows.len());
    for row in &t.rows {
        let Ok(id) = row[0].parse::<u16>() else {
            report(format!("{name}: id field {:?} is not a number", row[0]));
            continue;
        };
        out.push((id, label(row[1].clone())));
    }
    // A duplicate id is reported once. The first match is what a name lookup
    // returns anyway, and dropping rows would make the row-count floors lie.
    for pair in out.windows(2) {
        if pair[0].0 == pair[1].0 {
            report(format!("{name}: id {} appears twice", pair[0].0));
        }
    }
    out
}

/// Parse a two-column table where the first column is the row's index
/// (`QUEST_NAMES`), returning just the labels in order.
pub(crate) fn parse_labels(name: &'static str, source: &'static str) -> Vec<&'static str> {
    let t = Table::parse(name, source);
    if !t.problems.is_empty() {
        let mut reg = PROBLEMS.lock().unwrap_or_else(|e| e.into_inner());
        reg.extend(t.problems.iter().cloned());
    }
    // Rows whose field count disagreed with the header are already in `problems`,
    // which is what a short or long table looks like.
    t.rows.iter().map(|row| label(row[1].clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every embedded table must parse clean. This is what turns a truncated or
    /// mis-shaped CSV into a build failure instead of a missing name in a picker,
    /// which is how the 201-missing-species gap first presented to a user.
    #[test]
    fn every_embedded_table_parses_clean() {
        assert!(!crate::tables::species().is_empty());
        assert!(!crate::tables::moves().is_empty());
        assert!(!crate::tables::items().is_empty());
        assert_eq!(crate::tables::natures().len(), 25);
        assert!(!crate::tables_extra::species_extra().is_empty());
        assert!(!crate::tables_extra::items_extra().is_empty());
        assert_eq!(
            crate::tables_extra::quest_names()[27],
            "Poké Ball, Go!",
            "the quoted row must survive parsing with its slot number"
        );
        assert!(!crate::abilities::ability_names().is_empty());
        assert!(!crate::weather_names::weather_names().is_empty());
        assert_eq!(problems(), Vec::<String>::new(), "{:?}", problems());
    }

    /// Rows rendered as `a|b|c` so assertions read next to `Cow` fields.
    fn rows(t: &Table<'_>) -> Vec<String> {
        t.rows
            .iter()
            .map(|r| r.iter().map(|f| f.as_ref()).collect::<Vec<_>>().join("|"))
            .collect()
    }

    #[test]
    fn header_blank_lines_and_comments_are_skipped() {
        let t = Table::parse("t", "# generated\nid,name\n1,a\n\n  # a comment\n2,b\n");
        assert_eq!(rows(&t), vec!["1|a", "2|b"]);
        assert!(t.problems.is_empty(), "{:?}", t.problems);
    }

    /// A short or long row is a problem to report, not a row to guess at: a
    /// silently-partial table is what makes "no stats for this species" look like a
    /// game limitation instead of a broken file.
    #[test]
    fn a_misshapen_row_is_reported_and_skipped() {
        let t = Table::parse("t", "id,hp,atk\n1,10,20\n2,30\n3,40,50,60\n");
        assert_eq!(rows(&t), vec!["1|10|20"]);
        assert_eq!(t.problems.len(), 2, "{:?}", t.problems);
        assert!(t.problems[0].contains("t line 3"), "{:?}", t.problems[0]);
    }

    /// The game's own quest string is `Poké Ball, Go!`. Quoting keeps the row and
    /// its index, so the quest list stays aligned with its slot number.
    #[test]
    fn a_quoted_field_may_contain_a_comma() {
        let t = Table::parse(
            "t",
            "index,name\n26,Anything\n27,\"Poké Ball, Go!\"\n28,End",
        );
        assert_eq!(rows(&t), vec!["26|Anything", "27|Poké Ball, Go!", "28|End"]);
        assert!(t.problems.is_empty(), "{:?}", t.problems);
    }

    #[test]
    fn doubled_quotes_inside_a_quoted_field_become_one_quote() {
        let t = Table::parse("t", "id,name\n7,\"say \"\"hi\"\" now\"");
        assert_eq!(rows(&t), vec!["7|say \"hi\" now"]);
        assert!(t.problems.is_empty(), "{:?}", t.problems);
    }

    /// Unquoted fields must borrow rather than copy: that is what lets the name
    /// accessors hand out `&'static str` without any allocation at all.
    #[test]
    fn unquoted_fields_stay_borrowed() {
        let t = Table::parse("t", "id,name\n1,Bulbasaur");
        assert!(matches!(t.rows[0][1], Cow::Borrowed("Bulbasaur")));
    }

    /// Unparseable numbers are reported too: a caller cannot otherwise tell a
    /// truncated file from a genuinely-zero stat.
    #[test]
    fn a_non_numeric_field_is_a_problem_for_the_caller_to_see() {
        let t = Table::parse("t", "id,hp\n1,10\n2,");
        assert_eq!(t.rows.len(), 2);
        let nums: Vec<u8> = t
            .rows
            .iter()
            .filter_map(|r| r[1].parse::<u8>().ok())
            .collect();
        assert_eq!(nums, vec![10]);
    }
}
