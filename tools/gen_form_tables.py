#!/usr/bin/env python3
"""Generate src/form_tables.rs: species-id -> form linkage, from the GAME's own data.

Why this exists
---------------
The game embeds the form in the species name ("Darmanitan Standard Mode" is
species id 555 in this ROM), so the UI showed that full string as the pokemon's
"species name" with no way to see which form a mon actually is. The user asked
for the base name plus a separate Form detail. The base/form split must come
from the game's own linkage, not from string guessing:

  * every species block in a form family sets
        .formSpeciesIdTable = sFamilyFormSpeciesIdTable
    in src/data/pokemon/species_info/gen_N.h (macro chains expanded in place,
    same parse path as gen_basestats_v2 / gen_species_meta);
  * the array itself lives in src/data/pokemon/form_species_tables.h (plus a
    handful of Rogue-only families in form_change_tables.h) as
        static const u16 sFamilyFormSpeciesIdTable[] = { SPECIES_..., ..., FORM_SPECIES_END };
    terminated by FORM_SPECIES_END (0xffff, include/pokemon.h);
  * every block of a family references the SAME array, so families are grouped
    by array, not by block; the FAMILY ANCHOR (base) is the array's first
    entry -- the game's own ordering (e.g. sDarmanitanFormSpeciesIdTable lists
    SPECIES_DARMANITAN_STANDARD_MODE first).

The display names come from the editor's OWN tables (src/tables.rs SPECIES and
src/tables_extra.rs SPECIES_EXTRA), because those are the strings the UI
actually shows and the ids the saves actually carry (they mirror the installed
EX ROM; this source clone's id/name space agrees for the standard families but
not for every Rogue-only id). A member whose editor name shares no word with
the family is therefore a cross-space id collision: its entry is DROPPED and
reported, never guessed back in.

Family display name (the "base" name) rule
------------------------------------------
Most anchors are plain ("Venusaur" = 3). Some anchors are themselves named
after their default form ("Darmanitan Standard Mode" = 555). For those,
species.h declares the family CONSTANT as an alias of the canonical constant
(`#define SPECIES_DARMANITAN SPECIES_DARMANITAN_STANDARD_MODE`) -- the game's
own declaration of the family name. The generator compares the canonical
constant's word tokens with the alias's and splits the display name by the
same word count (the editor's name strings are word-for-word the title-cased
constant names, verified before splitting). When the word counts disagree the
anchor is kept whole and reported -- no suffix guessing.

Output (src/form_tables.rs)
---------------------------
  FORM_BASES:    form id -> family anchor id (never self, never 0)
  FAMILY_NAMES:  anchor id -> family display name (only where it differs
                 from the anchor's own name string)
  FORM_LABELS:   species id -> form label (the form detail shown next to the
                 base name; empty is never emitted)

Set ROGUE_SRC to an expansion-branch checkout. Defaults to ../../rogue-src
(same resolution as gen_basestats_v2).
"""
import os
import re
import sys
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gen_basestats_v2 as gb  # noqa: E402
import gen_species_meta as gsm  # noqa: E402  (in-place macro/fn expansion pipeline)

SRC = gb.SRC
SPECIES_H = os.path.join(SRC, "include/constants/species.h")
ARRAY_FILES = ["src/data/pokemon/form_species_tables.h",
               "src/data/pokemon/form_change_tables.h"]
INFO_DIR = gb.INFO_DIR
REPO = os.path.normpath(os.path.join(HERE, ".."))
OUT = os.path.join(REPO, "src", "form_tables.rs")

FORM_SPECIES_END = 0xFFFF  # include/pokemon.h

DEF_RE = re.compile(r"^\s*#\s*define\s+(SPECIES_[A-Z0-9_]*)\s+(.+?)\s*$", re.M)
ARRAY_RE = re.compile(r"static const u16 (\w*FormSpeciesIdTable)\s*\[\]\s*=\s*\{([^}]*)\}",
                      re.S)
OWNER_RE = re.compile(r"\.formSpeciesIdTable\s*=\s*([A-Za-z_]\w*)")


def editor_species_names():
    """id -> display name from the editor's own generated tables."""
    names = {}
    for fn, const in (("src/tables.rs", "SPECIES"),
                      ("src/tables_extra.rs", "SPECIES_EXTRA")):
        text = open(os.path.join(REPO, fn), encoding="utf-8").read()
        start = text.index(f"pub const {const}: &[")
        end = text.index("];", start)
        for m in re.finditer(r"\((\d+), \"([^\"]*)\"\)", text[start:end]):
            names[int(m.group(1))] = m.group(2)
    return names


def species_constants():
    """Split species.h into canonical defines (numeric/arith value) and pure
    aliases (value is one bare identifier). Returns NAME->id plus, per id, the
    canonical constant names and the alias constant names that resolve to it."""
    text = open(SPECIES_H, encoding="utf-8", errors="replace").read()
    ids = gb.resolve_ids()
    canonical, aliases = {}, {}
    for m in DEF_RE.finditer(text):
        name, val = m.group(1), m.group(2).strip()
        if re.fullmatch(r"[A-Za-z_]\w*", val):
            kind = aliases
        elif re.fullmatch(r"-?\d+", val) or re.fullmatch(r"[A-Za-z_]\w*\s*[+-]\s*\d+", val):
            kind = canonical
        else:
            continue
        v = ids.get(name)
        if v is not None:
            kind.setdefault(v, []).append(name)
    return ids, canonical, aliases


def parse_form_arrays(raw_cfg):
    """array ident -> ordered list of member SPECIES_* constant names, from
    every file the arrays live in, filtered for this build's config guards."""
    out = {}
    for fn in ARRAY_FILES:
        path = os.path.join(SRC, fn)
        if not os.path.exists(path):
            continue
        text = gb.apply_conditionals(
            open(path, encoding="utf-8", errors="replace").read(), raw_cfg)
        for m in ARRAY_RE.finditer(text):
            out[m.group(1)] = re.findall(r"\b(SPECIES_[A-Z0-9_]+)\b", m.group(2))
    return out


def expand_file(rawtext, raw_cfg):
    """Same pipeline gen_species_meta uses so designators hidden behind
    *_MISC_INFO / function-like macros become visible in the block bodies."""
    macros = gb.macro_bodies(rawtext)
    fns = gsm.fn_macro_bodies(rawtext)
    text = gb.apply_conditionals(rawtext, raw_cfg)
    kept = [ln for ln in text.splitlines() if not ln.lstrip().startswith("#")]
    return "\n".join(
        gsm.expand_in_place(gsm.expand_fn_in_place(ln, fns), macros) for ln in kept)


def parse_referenced_arrays(raw_cfg):
    """Set of array idents that at least one species_info block names in this
    build config (the family is compiled in)."""
    refs = set()
    for fn in sorted(os.listdir(INFO_DIR)):
        if not fn.endswith(".h"):
            continue
        rawtext = open(os.path.join(INFO_DIR, fn),
                       encoding="utf-8", errors="replace").read()
        expanded = expand_file(rawtext, raw_cfg)
        for _key, body in gb.blocks(expanded):
            for ident in OWNER_RE.findall(body):
                refs.add(ident)
    return refs


def split_anchor_name(canon_const, alias_const, name):
    """Split an anchor's display name into (family, form label) using the WORD
    COUNT difference between the game's own constant names. Returns
    (None, None) when the counts disagree -- shape-guessing a suffix is
    exactly what this generator must not do."""
    t_canon = canon_const.split("_")[1:]
    t_alias = alias_const.split("_")[1:]
    if len(t_canon) <= len(t_alias) or t_canon[:len(t_alias)] != t_alias:
        return None, None
    words = name.split()
    n_form = len(t_canon) - len(t_alias)
    if len(words) != len(t_canon):
        return None, None
    return " ".join(words[:len(words) - n_form]), " ".join(words[len(words) - n_form:])


def norm_key(s):
    """Comparison key that ignores accents, apostrophes, dots and case, so the
    collision guard compares SPECIES rather than byte-exact spellings
    ('Farfetch' d' vs 'Farfetchd', 'Flabebe' vs 'Flabebe' with accents)."""
    stripped = unicodedata.normalize("NFD", s)
    return "".join(c for c in stripped.lower() if c.isascii() and c.isalpha())


def shares_species(a, b):
    """True when both names spell the same species, allowing for the EX ROM's
    truncations and spelling variants. Prefix-only both ways: an arbitrary
    substring match would wrongly tie 'Raichu' into 'Pikachu'; a prefix match
    cannot, because 'Pichu' is not a prefix of 'Pikachu'."""
    ka, kb = norm_key(a), norm_key(b)
    if not ka or not kb:
        return False
    short, long_ = (ka, kb) if len(ka) <= len(kb) else (kb, ka)
    return len(short) >= 3 and long_.startswith(short)


def json_lit(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    names = editor_species_names()
    ids, canonical, aliases = species_constants()
    raw_cfg = gb.config_defines()
    arrays = parse_form_arrays(raw_cfg)
    referenced = parse_referenced_arrays(raw_cfg)

    form_bases, family_names, form_labels = {}, {}, {}
    warns, dropped = [], []

    fams = sorted(arr for arr in arrays if arr in referenced)
    missing_ref = sorted(arr for arr in referenced if arr not in arrays)
    for arr in missing_ref:
        warns.append(f"{arr}: referenced by species_info blocks but not defined "
                     f"under this build config (family skipped)")

    for arr in fams:
        consts = arrays[arr]
        members = []
        for c in consts:
            mid = ids.get(c)
            if mid in (None, 0, FORM_SPECIES_END):
                if mid is None:
                    warns.append(f"{arr}: constant {c} unresolved (dropped)")
                continue
            members.append((c, mid))
        if not members:
            warns.append(f"{arr}: no resolvable members (skipped)")
            continue
        anchor_const, anchor = members[0]
        name_a = names.get(anchor)
        if name_a is None:
            warns.append(f"{arr}: anchor {anchor_const} (id {anchor}) has no name "
                         f"in the editor's species tables (family skipped)")
            continue
        # family display name: split ONLY via the game's own alias constants
        fam, anchor_label = name_a, None
        for alias_const in sorted(aliases.get(anchor, []), key=len):
            for canon in canonical.get(anchor, []):
                fam_try, lab_try = split_anchor_name(canon, alias_const, name_a)
                if fam_try:
                    fam, anchor_label = fam_try, lab_try
                    break
            if anchor_label:
                break
        if fam != name_a:
            family_names[anchor] = fam
        if anchor_label:
            form_labels[anchor] = anchor_label
        for c, mid in members[1:]:
            if mid == anchor:
                continue
            if mid in form_bases:
                warns.append(f"{arr}: id {mid} already anchored to {form_bases[mid]} "
                             f"(kept first)")
                continue
            n_m = names.get(mid)
            if n_m is not None and not shares_species(n_m, fam):
                dropped.append(f"{arr}: id {mid} ({c}) is {n_m!r} in the editor's "
                               f"id space -- not a {fam!r} family member; entry dropped")
                continue
            form_bases[mid] = anchor
            if n_m is None or norm_key(n_m) == norm_key(fam):
                continue
            if n_m.startswith(fam + " "):
                lab = n_m[len(fam) + 1:]
                if lab.startswith("(") and lab.endswith(")"):
                    lab = lab[1:-1]
                form_labels[mid] = lab
            else:
                # non-prefixed form name (Mega-style): keep it whole as the label
                form_labels[mid] = n_m

    with open(OUT, "w", encoding="utf-8") as f:
        f.write("// GENERATED by tools/gen_form_tables.py -- do not edit.\n")
        f.write("// species-id -> form linkage straight from the game: every family's\n")
        f.write("// .formSpeciesIdTable array (src/data/pokemon/form_species_tables.h\n")
        f.write("// and form_change_tables.h), anchored at its first entry and resolved\n")
        f.write("// through include/constants/species.h. Display names come from the\n")
        f.write("// editor's own tables (tables.rs / tables_extra.rs) because those are\n")
        f.write("// the strings the UI renders and the ids the saves carry.\n\n")
        f.write("/// form species id -> family anchor (base) species id. Never self,\n")
        f.write("/// never zero. Species absent here are their own base.\n")
        f.write("pub const FORM_BASES: &[(u16, u16)] = &[\n")
        for k in sorted(form_bases):
            f.write(f"    ({k}, {form_bases[k]}),\n")
        f.write("];\n\n")
        f.write("/// family anchor id -> family display name, only where the family\n")
        f.write("/// name differs from the anchor's own (form-qualified) name string.\n")
        f.write("pub const FAMILY_NAMES: &[(u16, &str)] = &[\n")
        for k in sorted(family_names):
            f.write(f"    ({k}, {json_lit(family_names[k])}),\n")
        f.write("];\n\n")
        f.write("/// species id -> form label, the detail shown next to the base name.\n")
        f.write("/// Only ids that actually name a specific form appear here.\n")
        f.write("pub const FORM_LABELS: &[(u16, &str)] = &[\n")
        for k in sorted(form_labels):
            f.write(f"    ({k}, {json_lit(form_labels[k])}),\n")
        f.write("];\n")

    print(f"families: {len(fams)}; form->base entries: {len(form_bases)}; "
          f"family-name overrides: {len(family_names)}; form labels: {len(form_labels)}")
    for probe in (555, 1092, 990, 869, 1216, 906, 550, 143):
        b = form_bases.get(probe, probe)
        fam = family_names.get(b, names.get(b, "(no name)"))
        print(f"  id {probe}: base={b} family={fam!r} "
              f"label={form_labels.get(probe)!r} name={names.get(probe)!r}")
    if dropped:
        print(f"cross-space id collisions dropped ({len(dropped)}):")
        for w in dropped:
            print(f"  {w}")
    if warns:
        print(f"warnings ({len(warns)}), first 25:")
        for w in warns[:25]:
            print(f"  {w}")


if __name__ == "__main__":
    main()
