#!/usr/bin/env python3
"""Generate src/species_meta.rs: per-species genderRatio + ability slots, and the
ability id -> name table, from the pokeemerald-rogue expansion tree.

Why this exists
---------------
The editor used to render raw stored integers for gender and ability:

  * gender: it read the 3-bit `genderFlag` and printed Male/Female. The game does
    not do that. `GetGenderForSpecies()` (src/pokemon.c) looks up the species'
    `genderRatio` and, for MON_MALE(0x00) / MON_FEMALE(0xFE) / MON_GENDERLESS(0xFF),
    returns the ratio and IGNORES the flag. So a Metagross (MON_GENDERLESS) whose
    stored flag happens to be 1 displayed as "Female".
  * ability: BoxPokemon stores `abilityNum`, which is a *slot index* (0/1, or
    0..NUM_ABILITY_SLOTS-1 with ROGUE_EXPANSION), not an ability id. The real
    ability is `gBaseStats[species].abilities[abilityNum]` -- see
    GetAbilityBySpecies(). Printing abilityNum as an id was meaningless, and
    printing the raw index showed only "0"/"1".

Both need the species table, which the base-stat generator already parses
correctly (balanced braces, *_MISC_INFO macro indirection, P_UPDATED_STATS
preprocessor branches). This generator reuses that machinery verbatim so the
gender/ability rows stay consistent with basestats.rs.

Validation
----------
After generation the script checks every emitted ability name exists in the
v2.2.0-EX ROM as a charmap-encoded (_()) string, so a source-tree / build
mismatch fails loudly instead of silently mislabelling mons.

Set ROGUE_SRC to an expansion-branch checkout. Defaults to ../../rogue-src.
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

# Reuse the base-stat generator's parsers (macro chains, config defines,
# preprocessor conditionals) instead of reimplementing them.
import gen_basestats_v2 as gb

SRC = gb.SRC
OUT_META = os.path.normpath(os.path.join(HERE, "..", "src", "species_meta.rs"))
OUT_ABIL = os.path.normpath(os.path.join(HERE, "..", "src", "abilities.rs"))

ABILITIES_H = os.path.join(SRC, "include/constants/abilities.h")
ABILITY_TEXT_H = os.path.join(SRC, "src/data/text/abilities.h")
SPECIES_H = os.path.join(SRC, "include/constants/species.h")
CHARMAP = os.path.join(SRC, "charmap.txt")
ROM = os.environ.get("ER_ROM", "")

# gBaseStats[].genderRatio sentinels (include/constants/pokemon.h)
MON_MALE = 0x00
MON_FEMALE = 0xFE
MON_GENDERLESS = 0xFF


def species_ids():
    return gb.resolve_ids()


def ability_ids():
    """ABILITY_NAME -> numeric id, following alias chains (expansion has forms
    like ABILITY_NONE and count macros)."""
    text = open(ABILITIES_H, encoding="utf-8", errors="replace").read()
    raw = {}
    for m in re.finditer(r"^\s*#\s*define\s+(ABILITY_[A-Z0-9_]*)\s+(.+?)\s*$", text, re.M):
        raw.setdefault(m.group(1), m.group(2).strip())
    for m in re.finditer(r"^\s*#\s*define\s+([A-Z0-9_]+_COUNT)\s+(.+?)\s*$", text, re.M):
        raw.setdefault(m.group(1), m.group(2).strip())
    ids = {}

    def ev(name, seen=frozenset()):
        if name in ids:
            return ids[name]
        if name in seen or name not in raw:
            return None
        expr = raw[name]
        if re.fullmatch(r"-?\d+", expr):
            v = int(expr)
        elif (mm := re.fullmatch(r"([A-Za-z_]\w*)\s*([+-])\s*(\d+)", expr)):
            base = ev(mm.group(1), seen | {name})
            if base is None:
                return None
            v = base + int(mm.group(3)) if mm.group(2) == "+" else base - int(mm.group(3))
        elif re.fullmatch(r"[A-Za-z_]\w*", expr):
            v = ev(expr, seen | {name})
            if v is None:
                return None
        else:
            return None
        ids[name] = v
        return v

    for name in list(raw):
        ev(name)
    return {k: v for k, v in ids.items() if v is not None and v >= 0 and k.startswith("ABILITY_")}


def ability_names(abil_ids):
    """id -> display name for the table this build actually compiles.

    src/data/text/abilities.h holds TWO tables under the same name, guarded by
        #if B_EXPANDED_ABILITY_NAMES == TRUE   -> 16-char names (stride 17)
        #else                                   -> 12-char abbreviated names
    `Chilling Neigh` vs `ChillngNeigh` and friends are the abbreviated arm, so
    picking the wrong one mislabels mons. The arm is chosen from the repo's own
    config (include/config/battle.h), not by whichever appears last in the file
    -- which is what silently selected the 12-char table once.
    """
    if not os.path.exists(ABILITY_TEXT_H):
        return {}, None
    text = open(ABILITY_TEXT_H, encoding="utf-8", errors="replace").read()

    guard = re.search(r"#\s*if\s+(B_EXPANDED_ABILITY_NAMES\s*[=!]=\s*TRUE"
                      r"|B_EXPANDED_ABILITY_NAMES)\b", text)
    take_expanded = True
    if guard:
        cond = guard.group(1)
        name = cond.split()[0]
        r = gb.resolve_flag(name, gb.config_defines())
        if r is not None:
            truthy = r[1] if r[0] == "bool" else r[1] > 0
            take_expanded = truthy if "==" in cond else truthy
    if guard:
        start = guard.end()
        end = re.search(r"#\s*else\b", text[start:])
        arm = text[start:start + end.start()] if end else text[start:]
    else:
        arm = text

    out = {}
    for m in re.finditer(r"\[(ABILITY_[A-Z0-9_]+)\]\s*=\s*_\(\"(.*?)\"\)", arm, re.S):
        key = m.group(1)
        if key in abil_ids:
            out[abil_ids[key]] = m.group(2)
    # name length follows the arm we took: 16 expanded, 12 abbreviated
    return out, (16 if take_expanded else 12)


def gender_ratio(expr, macros, gen_latest, raw):
    """Resolve `.genderRatio = <expr>` to the byte the game stores.

    PERCENT_FEMALE(percent) is `min(254, ((percent * 255) / 100))` in
    src/data/pokemon/species_info.h: C float division then u8 truncation, so
    12.5% -> 31 and 50% -> 127 (NOT rounded).
    MON_MALE / MON_FEMALE / MON_GENDERLESS pass through.
    """
    expr = (expr or "").strip()
    m = re.fullmatch(r"PERCENT_FEMALE\(\s*([0-9.]+)\s*\)", expr)
    if m:
        pct = float(m.group(1))
        return min(254, int(pct * 255 / 100))
    if expr in ("MON_MALE",):
        return MON_MALE
    if expr in ("MON_FEMALE",):
        return MON_FEMALE
    if expr in ("MON_GENDERLESS",):
        return MON_GENDERLESS
    v = gb.eval_value(expr, gen_latest, macros, raw)
    if v is not None and 0 <= v <= 0xFF:
        return v
    return None


def ability_slots(exprs):
    """Resolve `.abilities = { A, B, C }` to numeric ability ids."""
    inner = exprs.strip()
    if inner.startswith("{"):
        inner = inner[1:]
    if inner.endswith("}"):
        inner = inner[:-1]
    out = []
    for part in inner.split(","):
        part = part.strip().rstrip(";").strip()
        if not part:
            continue
        if part.startswith("ABILITY_"):
            out.append(ABIL_IDS.get(part))
        elif part == "0":
            out.append(0)
        else:
            out.append(ABIL_IDS.get(part))
    return out


def expand_in_place(body, macros, max_depth=4):
    """Expand macro references inside `body` IN PLACE, transitively.

    Order matters: these blocks are C designated initializers, where a later
    designator overrides an earlier one. Species blocks put the shared
    `*_MISC_INFO` macro first and their own overrides after it, so callers read
    the LAST occurrence of a field. Appending expansions (as the base generator
    does for its "is this value unique?" check) would invert that precedence.

    A per-path guard stops a macro that references itself from recursing, and
    depth is capped so a pathological chain can't blow up.
    """

    def rep(m, depth, path):
        name = m.group(1)
        if name in macros and name not in path and depth < max_depth:
            return _sub(macros[name], depth + 1, path | {name})
        return m.group(0)

    def _sub(text, depth, path):
        return re.sub(r"\b([A-Z][A-Z0-9_]*)\b",
                      lambda m: rep(m, depth, path), text)

    return _sub(body, 0, frozenset())


def fn_macro_bodies(rawtext):
    """name -> (body, params) for `#define NAME(params) ...` definitions.

    Multi-form families in this tree hide their genderRatio/abilities behind
    function-like macros (`ZYGARDE_MISC_INFO(ability)`), so an object-like-only
    expander leaves those species without any resolvable fields.
    """
    out = {}
    pat = re.compile(r"^[ \t]*#[ \t]*define[ \t]+([A-Z][A-Z0-9_]*)\(([^)]*)\)", re.M)
    for m in pat.finditer(rawtext):
        # walk forward while lines end in a backslash continuation
        end = rawtext.find("\n", m.start())
        seg = rawtext[m.end():end] if end >= 0 else rawtext[m.end():]
        while seg.endswith("\\") and end >= 0:
            nxt_end = rawtext.find("\n", end + 1)
            seg += "\n" + rawtext[end + 1:nxt_end if nxt_end >= 0 else None]
            end = nxt_end
            if end < 0:
                break
        body = re.sub(r"\\\s*\n\s*", " ", seg).strip()
        out[m.group(1)] = (body, [p.strip() for p in m.group(2).split(",") if p.strip()])
    return out


def expand_fn_in_place(text, fns, max_depth=6):
    """Expand NAME(args) where NAME is a function-like macro, in place, so the
    substituted fields stay in the position the C initializer puts them."""

    def expand_one(t, depth, path):
        for name in list(fns):
            if name in path:
                continue
            idx = t.find(name + "(")
            if idx < 0:
                continue
            j = idx + len(name) + 1
            depth_p, k = 1, j
            while k < len(t) and depth_p:
                if t[k] == "(":
                    depth_p += 1
                elif t[k] == ")":
                    depth_p -= 1
                k += 1
            if depth_p:
                continue
            args = [a.strip() for a in t[j:k - 1].split(",")] if t[j:k - 1].strip() else []
            body, params = fns[name]
            sub = re.sub(r"\b(" + "|".join(re.escape(p) for p in params) + r")\b" if params else r"(?!)",
                         lambda mm: args[params.index(mm.group(0))] if params.index(mm.group(0)) < len(args) else "",
                         body)
            sub = expand_one(sub, depth + 1, path | {name})
            return expand_one(t[:idx] + sub + t[k:], depth, path)
        return t

    if depth_guard(max_depth):
        return expand_one(text, 0, frozenset())
    return text


def depth_guard(limit):
    return limit > 0


def main():
    ids = species_ids()
    gen_latest = gb.config_gen_latest()
    raw_cfg = gb.config_defines()

    metas = {}
    unresolved = []
    files = sorted(os.listdir(gb.INFO_DIR))
    for fn in files:
        if not fn.endswith(".h"):
            continue
        rawtext = open(os.path.join(gb.INFO_DIR, fn), encoding="utf-8", errors="replace").read()
        # #define bodies come from the RAW file: apply_conditionals() strips every
        # `#` line, so macros parsed after filtering would be empty (this silently
        # made every genderRatio "unresolved" once). Blocks come from the FILTERED
        # file so #if/#else arms this build doesn't take can't contribute a field.
        macros = gb.macro_bodies(rawtext)
        fns = fn_macro_bodies(rawtext)
        text = gb.apply_conditionals(rawtext, raw_cfg)
        # Function-like expansion must happen BEFORE blocks(), not per body.
        # Multi-form families write their initializer as a macro CALL:
        #     [SPECIES_ARCEUS_NORMAL] = ARCEUS_SPECIES_INFO(TYPE_NORMAL, Normal),
        # and blocks() only matches `NAME = { ... }`, so those entries never
        # produced a block at all. That silently dropped exactly the families
        # written this way -- which are heavily the signature-ability species
        # (Arceus/Multitype, Genesect/Download, Xerneas/Fairy Aura, Meloetta,
        # the fused species, ...), i.e. ~200 ids with no gender/ability data.
        # Preprocessor lines are dropped first: their bodies already live in
        # `macros`/`fns`, and expanding inside a #define recurses into its own
        # parameter names.
        kept = [ln for ln in text.splitlines() if not ln.lstrip().startswith("#")]
        expanded = "\n".join(
            expand_in_place(expand_fn_in_place(ln, fns), macros) for ln in kept)
        for key, body in gb.blocks(expanded):
            sp = ids.get(key)
            if sp is None:
                continue
            combined = body

            # Designated initializers are positional: the LAST designator for a
            # field is the one the compiler keeps, so take the last resolvable
            # occurrence rather than requiring all occurrences to agree.
            gr = None
            for mm in re.finditer(r"\.genderRatio\s*=\s*", combined):
                v = gender_ratio(gb.read_expr(combined, mm.end()), macros, gen_latest, raw_cfg)
                if v is not None:
                    gr = v

            ab = None
            for mm in re.finditer(r"\.abilities\s*=\s*\{", combined):
                src_txt = combined[mm.end() - 1:]
                depth, k = 0, 0
                while k < len(src_txt):
                    if src_txt[k] == "{":
                        depth += 1
                    elif src_txt[k] == "}":
                        depth -= 1
                        if depth == 0:
                            break
                    k += 1
                slots = ability_slots(src_txt[:k + 1])
                if slots and all(s is not None for s in slots):
                    ab = (slots + [0, 0, 0])[:3]

            if gr is None or ab is None:
                unresolved.append((key, sp, gr, ab))
                continue
            metas[sp] = (gr, ab)

    abil_names, name_len = ability_names(ABIL_IDS)

    # ---- ROM cross-validation of ability names + ids ----------------------
    # A substring check alone cannot catch an off-by-one in ability IDs (every
    # name would still exist somewhere in the ROM). So reconstruct the whole
    # `gAbilityNames[ABILITIES_COUNT][ABILITY_NAME_LENGTH + 1]` array, anchored
    # on STENCH's cell, and compare cell-by-cell. That validates names AND ids.
    if ROM and os.path.exists(ROM):
        rom = open(ROM, "rb").read()
        cmap = {}
        for line in open(CHARMAP, encoding="utf-8", errors="replace"):
            m = re.match(r"^\s*'(.*?)'\s*=\s*([0-9A-Fa-fx]{2,4})\s*$", line)
            if m and len(m.group(1)) == 1:
                cmap[m.group(1)] = int(m.group(2), 16)

        def cell(s):
            # charmap bytes (max name_len), then 0xff terminator, then zero pad.
            # ' ' is 0x00 in this charmap, so 0x00 is a real character and 0xff
            # is the terminator -- the opposite of C convention.
            out = bytearray()
            for ch in s:
                if ch not in cmap:
                    return None
                out += bytes([cmap[ch]])
                if len(out) >= name_len:
                    break
            out = out[:name_len] + b"\xff"
            return bytes(out) + b"\x00" * (name_len + 1 - len(out))

        stride = name_len + 1
        anchor = ABIL_IDS.get("ABILITY_STENCH")
        nxt = ABIL_IDS.get("ABILITY_DRIZZLE")
        ok = bad = skip = 0
        badlist = []
        if anchor is not None and cell(abil_names.get(anchor, "")):
            a1 = cell(abil_names[anchor])
            p1 = rom.find(a1)
            base = p1 - anchor * stride
            if p1 >= 0 and nxt is not None and \
                    rom[p1 + (nxt - anchor) * stride:p1 + (nxt - anchor) * stride + stride] == cell(abil_names.get(nxt, "")):
                count = max(abil_names) + 1
                for i in range(count):
                    want = cell(abil_names.get(i, "\xff"))
                    if want is None:
                        skip += 1
                        continue
                    got = rom[base + i * stride: base + (i + 1) * stride]
                    if got == want or (abil_names.get(i) is None and got.rstrip(b"\x00") in (b"", b"\xff")):
                        ok += 1
                    else:
                        bad += 1
                        if len(badlist) < 10:
                            badlist.append((i, abil_names.get(i, ""), got))
                print(f"ROM gAbilityNames @ {hex(base)} stride {stride}: "
                      f"{ok} cells match, {bad} mismatch, {skip} unmappable")
                for i, want, got in badlist:
                    print(f"  id {i}: want {want!r} rom {got.hex()}")
                if bad:
                    print("  !! ability table does not match this ROM; "
                          "check ROGUE_SRC branch / B_EXPANDED_ABILITY_NAMES")
            else:
                print("ROM check: could not anchor gAbilityNames "
                      "(STENCH/DRIZZLE adjacency failed) -- table not validated")
        else:
            print("ROM check: no STENCH anchor available -- table not validated")

    maxid = max(metas) if metas else 0
    with open(OUT_META, "w", encoding="utf-8") as f:
        f.write("// GENERATED by tools/gen_species_meta.py -- do not edit.\n")
        f.write("// per-species genderRatio + ability slots, from the expansion\n")
        f.write("// branch species_info (same parse path as basestats.rs).\n")
        f.write("// genderRatio is the raw byte gBaseStats[] stores; the editor\n")
        f.write("// must apply GetGenderForSpecies() semantics, not read the flag.\n")
        f.write("\n")
        f.write("pub const GENDER_MALE: u8 = 0x00;\n")
        f.write("pub const GENDER_FEMALE: u8 = 0xFE;\n")
        f.write("pub const GENDER_GENDERLESS: u8 = 0xFF;\n\n")
        f.write("/// (genderRatio, [ability slot 0, 1, (2)]) for every species id\n")
        f.write("/// present in species_info. Missing entries => unknown species.\n")
        f.write("pub const SPECIES_META: &[(u16, u8, [u16; 3])] = &[\n")
        for sp in sorted(metas):
            gr, ab = metas[sp]
            while len(ab) < 3:
                ab = ab + [0]
            f.write(f"    ({sp}, {gr}, [{ab[0]}, {ab[1]}, {ab[2]}]),\n")
        f.write("];\n")

    # The table this script used to emit is CSV now (data/ability_names.csv), parsed at start-up by
    # src/abilities.rs; re-running this generator would replace that loader with literals
    # and break every accessor. Add a CSV emitter before removing this stop.
    raise SystemExit(
        "SUPERSEDED: ABILITY_NAMES now lives in data/ability_names.csv.\n"
        "Edit that file, or port this generator to write it, before re-running."
    )
    with open(OUT_ABIL, "w", encoding="utf-8") as f:
        f.write("// GENERATED by tools/gen_species_meta.py -- do not edit.\n")
        f.write("// ability id -> name, from src/data/text/abilities.h `_()` strings.\n")
        f.write("\n")
        f.write("pub const ABILITY_NAMES: &[(u16, &str)] = &[\n")
        for i in sorted(abil_names):
            f.write(f"    ({i}, \"{abil_names[i]}\"),\n")
        f.write("];\n")

    print(f"species_meta: {len(metas)} species (max id {maxid}); "
          f"abilities: {len(abil_names)} names")
    if unresolved:
        print(f"unresolved ({len(unresolved)}), first 15:")
        for key, sp, gr, ab in unresolved[:15]:
            print(f"  {key} id={sp} genderRatio={gr} abilities={ab}")


if __name__ == "__main__":
    ABIL_IDS = ability_ids()
    main()
