#!/usr/bin/env python3
"""Regenerate src/basestats.rs from the pokeemerald-rogue *expansion* species_info.

Why v2: the original generator matched each `[SPECIES_X] = { ... }` block with a
non-greedy regex that stopped at the first line looking like `},`. species_info
entries contain nested initializers and multi-line macros, so the match was
truncated and the baseHP..baseSpDefense fields were lost -- the species was then
silently dropped. Result: 129 of the 1..905 ids (Butterfree, Raticate, Pikachu,
...) plus every id above 905 (gen 9, megas, gigantamax, Rogue forms) had
base_stats() == None, so cached-stat refresh either panicked or was skipped.

This version:
  * resolves SPECIES_* ids including chained macros in both forms
        #define FORMS_START            SPECIES_ENAMORUS_INCARNATE
        #define SPECIES_VENUSAUR_MEGA  FORMS_START + 1
    so the full id space (megas / gigantamax / Rogue fake forms) is covered;
  * parses blocks with balanced braces instead of a fragile end pattern;
  * follows `*_MISC_INFO` / `*_FAMILY_MISC_INFO` macro indirection, where the
    base stat fields actually live for most species.

Set ROGUE_SRC to an expansion-branch checkout (must contain include/ and
src/data/pokemon/species_info/). Defaults to ../../rogue-src relative to the repo.
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CANDIDATES = [
    os.environ.get("ROGUE_SRC", ""),
    os.path.join(HERE, "..", "..", "rogue-src"),
    os.path.join(HERE, "..", "rogue-src"),
    "rogue-src",
]
SRC = next((c for c in CANDIDATES
            if c and os.path.isdir(os.path.join(c, "src/data/pokemon/species_info"))), None)
if SRC is None:
    sys.exit("could not find the expansion-branch source tree; "
             "set ROGUE_SRC=/path/to/pokeemerald-rogue")

SPECIES_H = os.path.join(SRC, "include/constants/species.h")
INFO_DIR = os.path.join(SRC, "src/data/pokemon/species_info")
OUT = os.path.normpath(os.path.join(HERE, "..", "src/basestats.rs"))

BASE_FIELDS = ["baseHP", "baseAttack", "baseDefense",
               "baseSpeed", "baseSpAttack", "baseSpDefense"]


def resolve_ids():
    """SPECIES_NAME -> numeric id, following alias and +n/-n chains."""
    text = open(SPECIES_H, encoding="utf-8", errors="replace").read()
    raw = {}
    for m in re.finditer(r"^\s*#\s*define\s+((?:SPECIES_|FORMS_)[A-Z0-9_]*|[A-Z0-9_]+_START)"
                         r"\s+(.+?)\s*$", text, re.M):
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
    return {k: v for k, v in ids.items() if v is not None and v >= 0}


def macro_bodies(text):
    """Every `#define NAME <body>` in `text`, with line continuations joined."""
    out = {}
    for m in re.finditer(r"^[ \t]*#[ \t]*define[ \t]+([A-Z_][A-Z0-9_]*)\b", text, re.M):
        rest = text[m.end():]
        first, _, tail = rest.partition("\n")
        acc = [first]
        while acc[-1].rstrip().endswith("\\"):
            nxt, _, tail = tail.partition("\n")
            acc.append(nxt)
        body = " ".join(a.rstrip()[:-1] if a.rstrip().endswith("\\") else a for a in acc)
        out[m.group(1)] = out.get(m.group(1), "") + " " + body
    return out


def blocks(text):
    """Yield (SPECIES_X, body) for `[SPECIES_X] = { ... }`, brace-balanced."""
    for m in re.finditer(r"\[\s*(SPECIES_\w+)\s*\]\s*=\s*\{", text):
        i, depth = m.end(), 1
        while i < len(text) and depth:
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        yield m.group(1), text[m.end():i]


GEN_ORDER = {f"GEN_{n}": n for n in range(1, 10)}


def read_expr(text, i):
    """Read one initializer expression starting at i, stopping at the top-level
    ',' that ends the designated initializer. Paren/brace aware, so ternaries and
    nested initializers are kept intact.
    """
    depth = 0
    start = i
    while i < len(text):
        c = text[i]
        if c in "({":
            depth += 1
        elif c in ")}":
            if depth == 0:
                break
            depth -= 1
        elif c == "," and depth == 0:
            break
        elif c == "\n" and depth == 0 and i > start and text[i - 1] not in "?:+-*/(":
            # a newline at depth 0 with no operator pending ends the initializer
            nxt = text[i:].lstrip()[:1]
            if nxt not in ("?", ":", "+", "-", "*", "/"):
                break
        i += 1
    out = text[start:i].strip()
    return out or None


def eval_value(expr, gen_latest, macros=None, raw=None):
    """Resolve a field initializer expression to an int.

    Handles plain integers, the project's stat-variant ternaries
        P_UPDATED_STATS >= GEN_6 ? 90 : 80
    (include/config/pokemon.h sets P_UPDATED_STATS to GEN_LATEST, and
    include/config.h sets GEN_LATEST to GEN_9), and helper macros used by form
    entries
        .baseAttack = BEEDRILL_ATTACK + 60
    where BEEDRILL_ATTACK itself is such a ternary. Returns None for anything not
    provable, so a value is treated as unknown rather than guessed.
    """
    expr = expr.strip()
    if expr.startswith("(") and expr.endswith(")"):
        inner = expr[1:-1]
        if inner.count("(") == 0:
            return eval_value(inner, gen_latest, macros, raw)
    if re.fullmatch(r"-?\d+", expr):
        return int(expr)

    # NAME <op> GEN_n ? a : b
    mm = re.fullmatch(
        r"([A-Za-z_]\w*)\s*(>=|<=|==|>|<)\s*(GEN_\d)\s*\?\s*(-?\d+)\s*:\s*(-?\d+)", expr)
    if mm:
        flag, op, gen, a, b = mm.groups()
        # resolve the left-hand flag from the repo's own config when available,
        # falling back to GEN_LATEST for the P_UPDATED_* family
        lhs = None
        if raw is not None:
            r = resolve_flag(flag, raw)
            if r and r[0] == "gen":
                lhs = r[1]
        if lhs is None:
            if not flag.startswith("P_UPDATED_"):
                return None
            lhs = gen_latest
        rhs = GEN_ORDER[gen]
        cond = {"<": lhs < rhs, ">": lhs > rhs, ">=": lhs >= rhs,
                "<=": lhs <= rhs, "==": lhs == rhs}[op]
        return int(a) if cond else int(b)

    # MACRO <op> N   (e.g. BEEDRILL_ATTACK + 60)
    mm = re.fullmatch(r"([A-Za-z_]\w*)\s*([+-])\s*(\d+)", expr)
    if mm and macros is not None:
        base = eval_value(macros.get(mm.group(1), ""), gen_latest, macros, raw)
        if base is None:
            return None
        return base + int(mm.group(3)) if mm.group(2) == "+" else base - int(mm.group(3))

    # bare MACRO
    if re.fullmatch(r"[A-Za-z_]\w*", expr) and macros is not None:
        if expr in macros:
            return eval_value(macros[expr], gen_latest, macros, raw)
    return None


def config_gen_latest():
    """Read GEN_LATEST from the repo's own include/config.h."""
    for path in ("include/config.h",):
        p = os.path.join(SRC, path)
        if os.path.exists(p):
            mm = re.search(r"#\s*define\s+GEN_LATEST\s+GEN_(\d)",
                           open(p, encoding="utf-8", errors="replace").read())
            if mm:
                return int(mm.group(1))
    return 9


def fields_from(body, macros, gen_latest, raw=None):
    """The six base stats, resolving `*_MISC_INFO` indirection.

    A species block lists one or more shared macros (e.g. RATICATE_MISC_INFO,
    RATTATA_FAMILY_MISC_INFO); the six base-stat fields live in exactly one of
    them, so every macro referenced by the block is expanded transitively and the
    fields are read from the combined text. If two expansions disagree on a field
    the value is reported as ambiguous rather than guessed.
    """
    combined = body
    referenced = {t for t in re.findall(r"\b[A-Z][A-Z0-9_]*\b", body) if t in macros}
    frontier, seen = list(referenced), set(referenced)
    for _ in range(2):
        nxt = []
        for name in frontier:
            combined += " " + macros[name]
            for other in re.findall(r"\b[A-Z][A-Z0-9_]*\b", macros[name]):
                if other in macros and other not in seen:
                    seen.add(other)
                    nxt.append(other)
        frontier = nxt
        if not frontier:
            break

    vals, bad = [], []
    for f in BASE_FIELDS:
        hits = set()
        for mm in re.finditer(rf"\.{f}\s*=\s*", combined):
            expr = read_expr(combined, mm.end())
            if expr is None:
                continue
            v = eval_value(expr, gen_latest, macros, raw)
            if v is not None:
                hits.add(v)
        if len(hits) == 1:
            vals.append(hits.pop())
        else:
            bad.append((f, sorted(hits)))
    if bad:
        return None
    if len(vals) != 6 or not all(1 <= v <= 255 for v in vals):
        return None
    return vals


def config_defines():
    """Flatten `#define NAME VALUE` from include/config.h and include/config/*.h.

    These headers define the feature flags that guard base-stat variants, e.g.
        P_UPDATED_STATS  -> GEN_LATEST -> GEN_9
        P_FAMILY_WEEDLE  -> P_GEN_1_POKEMON -> TRUE
    so branch selection follows the repo's own build configuration instead of a
    guess.
    """
    raw = {}
    cfg_dir = os.path.join(SRC, "include/config")
    files = [os.path.join(SRC, "include/config.h")]
    if os.path.isdir(cfg_dir):
        files += [os.path.join(cfg_dir, f) for f in sorted(os.listdir(cfg_dir))
                  if f.endswith(".h")]
    for path in files:
        if not os.path.exists(path):
            continue
        text = open(path, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r"^[ \t]*#[ \t]*define[ \t]+([A-Z_][A-Z0-9_]*)\b[ \t]*(.*)$",
                             text, re.M):
            v = m.group(2)
            cut = v.find("//")  # trailing comment is not part of the value
            if cut != -1:
                v = v[:cut]
            raw.setdefault(m.group(1), v.strip())
    return raw


TRUEISH = {"TRUE", "1", "yes"}
FALSEISH = {"FALSE", "0", "no", ""}


def resolve_flag(name, raw, seen=frozenset()):
    """Resolve a config flag to ('gen', n) | ('bool', bool) | None."""
    if name in seen or name not in raw:
        return None
    v = raw[name].strip()
    if v in TRUEISH:
        return ("bool", True)
    if v in FALSEISH:
        return ("bool", False)
    if re.fullmatch(r"GEN_\d", v):
        return ("gen", GEN_ORDER[v])
    return resolve_flag(v, raw, seen | {name})


def eval_if(expr, raw):
    """Evaluate a preprocessor condition used inside species_info. Returns True /
    False, or None when the condition cannot be resolved from config."""
    expr = expr.strip()
    m = re.fullmatch(r"([A-Za-z_]\w*)\s*(>=|<=|==|>|<)\s*(GEN_\d)", expr)
    if m:
        lhs = resolve_flag(m.group(1), raw)
        if lhs is None or lhs[0] != "gen":
            return None
        rhs = GEN_ORDER[m.group(3)]
        op = m.group(2)
        return {"<": lhs[1] < rhs, ">": lhs[1] > rhs, ">=": lhs[1] >= rhs,
                "<=": lhs[1] <= rhs, "==": lhs[1] == rhs}[op]
    if re.fullmatch(r"[A-Za-z_]\w*", expr):
        r = resolve_flag(expr, raw)
        if r is None:
            return None
        if r[0] == "bool":
            return r[1]
        return r[1] > 0
    return None


def apply_conditionals(text, raw):
    """Keep only the lines the C preprocessor would keep for this build config.

    Branches whose condition cannot be resolved from the repo's own config are
    dropped entirely (both arms), so an unprovable value is never silently picked.
    `//` comment lines are stripped so commented-out alternates can't be matched.
    """
    out, stack = [], []  # stack entry: [taking_this_arm, some_arm_taken]
    for line in text.split("\n"):
        s = line.strip()
        m = re.match(r"#\s*if[ \t]+(.+)$", s)
        if m:
            cond = eval_if(m.group(1), raw)
            taking = bool(cond) if cond is not None else False
            stack.append([taking, taking])
            continue
        if re.match(r"#\s*(ifdef|ifndef)\b", s):
            stack.append([False, False])
            continue
        if re.match(r"#\s*else\b", s):
            if stack:
                stack[-1][0] = not stack[-1][1]
                stack[-1][1] = stack[-1][1] or stack[-1][0]
            continue
        if re.match(r"#\s*endif\b", s):
            if stack:
                stack.pop()
            continue
        if re.match(r"#", s):
            continue  # #define/#include handled by config_defines / macro_bodies
        if stack and not all(f[0] for f in stack):
            continue
        cut = line.find("//")
        if cut != -1:
            line = line[:cut]
        out.append(line)
    return "\n".join(out)


def main():
    ids = resolve_ids()
    if not ids:
        sys.exit("no SPECIES_* ids resolved -- is ROGUE_SRC the expansion branch?")
    max_id = max(ids.values())

    raw = config_defines()
    gl = resolve_flag("GEN_LATEST", raw)
    gen_latest = gl[1] if gl and gl[0] == "gen" else 9
    print(f"build config: GEN_LATEST -> GEN_{gen_latest}; "
          f"P_UPDATED_STATS -> {raw.get('P_UPDATED_STATS', '?')}")
    stats, conflicts, unresolved = {}, [], []
    for fn in sorted(os.listdir(INFO_DIR)):
        if not fn.endswith(".h"):
            continue
        rawtext = open(os.path.join(INFO_DIR, fn), encoding="utf-8", errors="replace").read()
        # #define bodies come from the raw file (they are the source of values);
        # blocks are scanned from the pre-filtered file so #if/#else arms that the
        # build does not take can never contribute a field.
        macros = macro_bodies(rawtext)
        text = apply_conditionals(rawtext, raw)
        for name, body in blocks(text):
            sp = ids.get(name)
            if sp is None or sp > max_id:
                continue
            vals = fields_from(body, macros, gen_latest, raw)
            if vals is None:
                unresolved.append(name)
                continue
            if sp in stats and stats[sp] != vals:
                conflicts.append((sp, name, stats[sp], vals))
            stats[sp] = vals

    print(f"source      : {SRC}")
    print(f"species ids : {len(ids)} resolved, max id {max_id}")
    print(f"base stats  : {len(stats)} parsed (max species id {max(stats)})")
    if conflicts:
        print(f"CONFLICTS   : {len(conflicts)} -> {conflicts[:4]}")
    if unresolved:
        print(f"unresolved  : {len(unresolved)} e.g. {unresolved[:8]}")

    # Anchors taken from the source itself and from the live v2.2.0-EX save.
    # Species 12 is Butterfree, NOT Raticate (19) -- a mis-mapping like that once
    # silently passed. Values are the Gen-6-updated variants the build compiles
    # (P_UPDATED_STATS = GEN_LATEST = GEN_9). Arcanine/Metagross match a live
    # v2.2.0-EX save's cached stats exactly.
    # Values taken from National Dex ordering of the species constants:
    # 12 Butterfree, 19 Rattata, 20 Raticate, 59 Arcanine, 376 Metagross.
    # (An earlier version of this anchor had Raticate at 19, which is Rattata --
    # the assertion caught the slip, which is the point of pinning them.)
    # Anchors copied from the source itself (not from memory) -- and one of them
    # already caught a wrong expectation here, which is exactly the point.
    # Table order is [HP, Atk, Def, Spe, SpA, SpD].
    anchors = {
        12: [60, 45, 50, 70, 90, 80],     # Butterfree (SpA 90 = P_UPDATED_STATS variant)
        19: [30, 56, 35, 72, 25, 35],     # Rattata
        20: [55, 81, 60, 97, 50, 70],     # Raticate
        59: [90, 110, 80, 95, 100, 80],   # Arcanine -- live save's cached SpDef 97
                                        # is only reproducible with base 80, not 70
        376: [80, 135, 130, 70, 95, 90],  # Metagross (live save: 193/155/98/107/120)
    }
    for sp, want in anchors.items():
        if stats.get(sp) != want:
            sys.exit(f"anchor mismatch species {sp}: got {stats.get(sp)} want {want}")
    print("anchors     : OK (Raticate, Arcanine)")

    lines = [
        "// Auto-generated by tools/gen_basestats_v2.py from the pokeemerald-rogue",
        "// expansion branch species_info (incl. *_MISC_INFO indirection). Do not edit.",
        "pub fn base_stats(sp: u16) -> Option<[u16; 6]> {",
        "    match sp {",
    ]
    for sp in sorted(stats):
        v = stats[sp]
        lines.append(f"        {sp} => Some([{v[0]}, {v[1]}, {v[2]}, {v[3]}, {v[4]}, {v[5]}]),")
    lines += ["        _ => None,", "    }", "}", ""]
    with open(OUT, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))
    print(f"wrote       : {OUT} ({len(stats)} species)")


if __name__ == "__main__":
    main()
