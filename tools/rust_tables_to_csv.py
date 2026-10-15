#!/usr/bin/env python3
"""Extract the id/label tables from the current Rust sources into CSV.

This is a one-time move, run once per table, and it reads the COMMITTED Rust rather
than regenerating from the game tree on purpose: the tables in `src/tables.rs` have
no generator left, so a regeneration would be an unreviewable data change. Extracting
keeps every row identical and makes the diff a pure format change.

Run from the repo root:

    python3 tools/rust_tables_to_csv.py

Then delete the literal arrays from the Rust modules (they become loaders) and
compare the printed row counts against the generator output the next time a table
is regenerated.
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# (rust file, const name, csv file, header)
TABLES = [
    ("src/tables.rs", "SPECIES", "data/species_names.csv", "id,name"),
    ("src/tables.rs", "MOVES", "data/move_names.csv", "id,name"),
    ("src/tables.rs", "ITEMS", "data/item_names.csv", "id,name"),
    ("src/tables.rs", "NATURE_IDS", "data/natures.csv", "id,name"),
    ("src/tables_extra.rs", "SPECIES_EXTRA", "data/species_names_extra.csv", "id,name"),
    ("src/tables_extra.rs", "ITEMS_EXTRA", "data/item_names_extra.csv", "id,name"),
    ("src/abilities.rs", "ABILITY_NAMES", "data/ability_names.csv", "id,name"),
    ("src/weather_names.rs", "WEATHER_NAMES", "data/weather_names.csv", "id,name"),
]

# A `&[&str]` table with no id column: the index IS the id.
INDEX_TABLES = [("src/tables_extra.rs", "QUEST_NAMES", "data/quest_names.csv", "index,name")]

ROW = re.compile(r'\(\s*(\d+)\s*,\s*"((?:[^"\\]|\\.)*)"\s*\),')
PLAIN_ROW = re.compile(r'^\s*"((?:[^"\\]|\\.)*)",\s*$', re.M)

SRC_NOTE = "# Extracted from the Rust table by tools/rust_tables_to_csv.py; regenerate\n" \
           "# with the matching tools/gen_*.py. Do not hand-edit game values.\n"


def block(text, const):
    """Body of `pub const NAME: &[...] = &[ ... ];`, balanced from the literal.

    The anchor must be the `= &[` of the assignment, not the first `&[` after the
    name: that one is the type annotation `&[(u16, &str)]`, and balancing from it
    silently swallows every table in the file.
    """
    decl = text.index(f"pub const {const}")
    anchor = re.compile(r"=\s*&\[").search(text, decl)
    assert anchor, f"{const}: no `= &[` assignment found"
    i = anchor.end()
    depth = 1
    start = i
    while i < len(text):
        if text[i] == "[":
            depth += 1
        elif text[i] == "]":
            depth -= 1
            if depth == 0:
                break
        i += 1
    assert depth == 0, f"{const}: unbalanced array literal"
    return text[start:i]


def needs_quote(label):
    return any(c in label for c in (",", '"'))


def quote(label):
    """RFC-4180 style: wrap in quotes and double any quote inside."""
    if not needs_quote(label):
        return label
    return '"' + label.replace('"', '""') + '"'


def unescape(s):
    return s.replace('\\"', '"').replace("\\n", " ").replace("\\\\", "\\")


def main():
    os.makedirs(os.path.join(ROOT, "data"), exist_ok=True)
    for rel, const, out, header in TABLES:
        text = open(os.path.join(ROOT, rel)).read()
        rows = [(int(a), unescape(b)) for a, b in ROW.findall(block(text, const))]
        assert rows, f"{const}: no rows parsed"
        ids = [r[0] for r in rows]
        assert ids == sorted(ids), f"{const}: rows are not id-sorted"
        assert len(set(ids)) == len(ids), f"{const}: duplicate ids"
        body = [SRC_NOTE, header] + [f"{i},{quote(n)}" for i, n in rows]
        quoted = sum(1 for _, n in rows if needs_quote(n))
        if quoted:
            print(f"  ({const}: {quoted} labels quoted)")
        with open(os.path.join(ROOT, out), "w") as fh:
            fh.write("\n".join(body) + "\n")
        print(f"{out}: {len(rows)} rows (ids {ids[0]}..{ids[-1]})")

    for rel, const, out, header in INDEX_TABLES:
        text = open(os.path.join(ROOT, rel)).read()
        rows = [unescape(m) for m in PLAIN_ROW.findall(block(text, const))]
        assert rows, f"{const}: no rows parsed"
        body = [SRC_NOTE, header] + [f"{i},{quote(n)}" for i, n in enumerate(rows)]
        with open(os.path.join(ROOT, out), "w") as fh:
            fh.write("\n".join(body) + "\n")
        print(f"{out}: {len(rows)} rows (index 0..{len(rows) - 1})")


if __name__ == "__main__":
    main()
