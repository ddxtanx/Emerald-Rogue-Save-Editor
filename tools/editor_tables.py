#!/usr/bin/env python3
"""Read the editor's data tables from `data/*.csv` - the single way tools do this.

The id/label tables used to be Rust literals in `src/tables.rs` / `src/tables_extra.rs`
and tools regexed them out of the source. They are CSV now, and a tool that parses
Rust for them fails loudly (`RuntimeError: table SPECIES not found`, which is exactly
how `get_icons.py` broke after the move). Use this module instead of regexing source.

Dialect matches `src/csv.rs`: a header line, `#` comments, blank lines, comma
separation, and double-quoted fields where the value contains a comma (`""` is one
literal quote). One row in the corpus needs quoting: quest slot 27.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"


def _rows(rel):
    """Rows of a table as lists of fields, header/comments/blank lines dropped."""
    out = []
    for raw in (DATA / rel).read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if not out and _is_header(line):
            continue
        out.append(_split(line))
    return out


def _is_header(line):
    first = _split(line)[0]
    return not first.isdigit()


def _split(line):
    """Split one line into fields, honouring double quotes."""
    if '"' not in line:
        return [f.strip() for f in line.split(",")]
    fields, cur, quoted = [], [], False
    it = iter(range(len(line)))
    i = 0
    while i < len(line):
        c = line[i]
        if c == '"':
            if quoted and i + 1 < len(line) and line[i + 1] == '"':
                cur.append('"')
                i += 1
            else:
                quoted = not quoted
        elif c == "," and not quoted:
            fields.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        i += 1
    fields.append("".join(cur).strip())
    return fields


def pairs(rel):
    """`id,label` table -> ([ids], {id: label})."""
    rows = _rows(rel)
    bad = [r for r in rows if len(r) != 2 or not r[0].isdigit()]
    if bad:
        raise RuntimeError(f"{rel}: {len(bad)} malformed rows, first: {bad[0]}")
    return [int(r[0]) for r in rows], {int(r[0]): r[1] for r in rows}


def labels(rel):
    """`index,name` table -> list of labels in index order."""
    return [r[1] for r in _rows(rel)]


def species():
    return pairs("species_names.csv")


def species_extra():
    return pairs("species_names_extra.csv")


def items():
    return pairs("item_names.csv")


def items_extra():
    return pairs("item_names_extra.csv")


def natures():
    return pairs("natures.csv")


def moves():
    return pairs("move_names.csv")


def abilities():
    return pairs("ability_names.csv")


def weather():
    return pairs("weather_names.csv")


def quests():
    return labels("quest_names.csv")


if __name__ == "__main__":
    for name, fn in [
        ("species", species), ("species_extra", species_extra),
        ("items", items), ("items_extra", items_extra),
        ("moves", moves), ("natures", natures), ("abilities", abilities),
        ("weather", weather), ("quests", quests),
    ]:
        r = fn()
        if isinstance(r, tuple):
            ids, table = r
            print(f"{name:15} {len(ids):5} rows  ids {ids[0]}..{ids[-1]}  "
                  f"sample {table[ids[0]]!r}")
        else:
            print(f"{name:15} {len(r):5} rows  sample {r[0]!r}")
