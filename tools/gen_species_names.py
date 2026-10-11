#!/usr/bin/env python3
"""Build a complete species id -> name table for Emerald Rogue EX from the game source.

Handles the chained `#define SPECIES_X OTHER + n` style in include/constants/species.h
and the `.speciesName = _("Name")` designated initializers in
src/data/pokemon/species_info/gen_N.h (gen 1-9 + Rogue fake/mega forms).
"""
import re, json, sys, os

SRC = os.environ.get("ROGUE_SRC", "rogue-src")
OUT = os.environ.get("SPECIES_JSON", "species_names.json")

# ---- 1. resolve species id macros -------------------------------------------
defs = {}
with open(os.path.join(SRC, "include/constants/species.h")) as f:
    for line in f:
        # capture SPECIES_* AND the anchor macros (FORMS_START, GEN9_START, ...)
        m = re.match(r"\s*#define\s+((?:SPECIES_|FORMS_START|GEN\d+_START|GIGANTAMAX_START|SPECIES_ROGUE_START|Tera_START|SPECIES_TERA)\w*)\s+(.+?)\s*(?://.*)?$", line)
        if not m:
            continue
        name, val = m.group(1), m.group(2).strip()
        if val.startswith("(") and val.endswith(")"):
            val = val[1:-1].strip()
        defs[name] = val

memo = {}

def resolve(name, stack=()):
    if name in memo:
        return memo[name]
    if name in stack or name not in defs:
        return None
    expr = defs[name]
    if re.fullmatch(r"\d+", expr):
        v = int(expr)
    else:
        m = re.fullmatch(r"(\w+)\s*([+-])\s*(\d+)", expr)
        if m:
            base = resolve(m.group(1), stack + (name,))
            if base is None:
                return None
            v = base + int(m.group(3)) if m.group(2) == "+" else base - int(m.group(3))
        elif re.fullmatch(r"\w+", expr):
            v = resolve(expr, stack + (name,))
            if v is None:
                return None
        else:
            return None
    memo[name] = v
    return v

ids = {}
for n in defs:
    v = resolve(n)
    if v is not None:
        ids[n] = v

unresolved = [n for n in defs if resolve(n) is None]

# ---- 2. species names ------------------------------------------------------
names = {}
nameless = []
for gen in range(1, 10):
    path = os.path.join(SRC, f"src/data/pokemon/species_info/gen_{gen}.h")
    with open(path) as f:
        t = f.read()
    # designated initialisers at line start:    [SPECIES_ARCANINE] =
    parts = re.split(r"\n\s*\[(SPECIES_\w+)\]\s*=\s*\n", t)
    # re.split with one capture group: parts[0]=text, parts[1]=KEY, parts[2]=text, parts[3]=KEY, ...
    for i in range(1, len(parts), 2):
        key, block = parts[i], parts[i + 1] if i + 1 < len(parts) else ""
        m = re.search(r"\.speciesName\s*=\s*_\(\"([^\"]*)\"\)", block)
        nm = m.group(1) if m else None
        if key in ids:
            if nm and nm.strip("?"):
                names[ids[key]] = nm
            else:
                nameless.append((key, ids[key]))
        else:
            nameless.append((key + " (no id)", ids.get(key)))

json.dump({str(k): v for k, v in sorted(names.items())}, open(OUT, "w"))
print(f"resolved macros: {len(ids)}  max id: {max(ids.values()) if ids else 0}")
print(f"unresolved macros: {len(unresolved)} {unresolved[:12]}")
print(f"names: {len(names)}  blocks w/o usable name: {len(nameless)} {nameless[:8]}")
for sp in (59, 376, 461, 635, 636, 707, 801, 906, 1025):
    print("  ", sp, names.get(sp, "<none>"))
print("wrote", OUT)
