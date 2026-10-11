#!/usr/bin/env python3
"""Generate supplemental Rust tables for er-save-editor from rogue-src (expansion branch):

- SPECIES_EXTRA: form ids > 905 (megas, gigantamax, Rogue fake forms) as "Base (Tag)"
- ITEMS_EXTRA:   Rogue custom items (827..1072) with pretty names from gRogueItems + stones
- QUEST_NAMES:   the 232 save-slot quest names in exporter order (EX build, #if-gated filtered)

Writes src/tables_extra.rs. Point ROGUE_SRC at a pokeemerald-rogue `expansion` checkout, e.g.
  ROGUE_SRC=../rogue-src python3 tools/gen_tables_v2.py
"""
import re, json, os
from pathlib import Path

SRC = os.environ.get("ROGUE_SRC", str(Path(__file__).resolve().parent.parent / "rogue-src"))
OUT = os.path.join(str(Path(__file__).resolve().parent.parent), "src", "tables_extra.rs")
OUT = os.path.normpath(OUT)

def read(p):
    return open(os.path.join(SRC, p)).read()

# ---------- resolve species id macros ----------
spec_h = read("include/constants/species.h")
defs = {}
for line in spec_h.splitlines():
    m = re.match(r"\s*#define\s+((?:SPECIES_\w+|FORMS_START|GEN\d+_START|GIGANTAMAX_START|SPECIES_ROGUE_START|Tera\w*|NUM_SPECIES|SPECIES_EGG))\s+(.+?)\s*(?://.*)?$", line)
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
    v = None
    if re.fullmatch(r"\d+", expr):
        v = int(expr)
    else:
        m = re.fullmatch(r"(\w+)\s*([+-])\s*(\d+)", expr)
        if m:
            base = resolve(m.group(1), stack + (name,))
            if base is not None:
                v = base + int(m.group(3)) if m.group(2) == "+" else base - int(m.group(3))
        elif re.fullmatch(r"\w+", expr):
            v = resolve(expr, stack + (name,))
    memo[name] = v
    return v

ids = {n: resolve(n) for n in defs}
ids = {k: v for k, v in ids.items() if v is not None}
num_species = resolve("SPECIES_EGG") or max(v for k, v in ids.items() if k.startswith("SPECIES_"))

# ---------- base names from species_info (incl. *_MISC_INFO indirection) ----------
base_name = {}
for gen in range(1, 10):
    try:
        t = read(f"src/data/pokemon/species_info/gen_{gen}.h")
    except FileNotFoundError:
        continue
    for m in re.finditer(r"#define\s+([A-Z0-9_]+)_MISC_INFO\b", t):
        body = t[m.end():]
        nxt = body.find("\n#define")
        seg = body[: nxt if nxt >= 0 else 2000]
        nm = re.search(r"\.speciesName\s*=\s*_\(\"([^\"]*)\"\)", seg)
        if nm:
            base_name.setdefault("SPECIES_" + m.group(1), nm.group(1))
    for m in re.finditer(r"\[?(SPECIES_\w+)\]?\s*=\s*\n", t):
        key = m.group(1)
        if key in base_name:
            continue
        nxt = t.find("\n    [", m.end())
        seg = t[m.end(): nxt if nxt >= 0 else m.end() + 3000]
        nm = re.search(r"\.speciesName\s*=\s*_\(\"([^\"]*)\"\)", seg)
        if nm and nm.group(1).strip("?"):
            base_name[key] = nm.group(1)
        else:
            mi = re.search(r"\b([A-Z0-9_]+)_MISC_INFO\b", seg)
            if mi and "SPECIES_" + mi.group(1) in base_name:
                base_name[key] = base_name["SPECIES_" + mi.group(1)]

def prettify(mac):
    s = mac[len("SPECIES_"):] if mac.startswith("SPECIES_") else mac
    return s.replace("_", " ").title()

# ---------- SPECIES_EXTRA: form ids above the base dex ----------
SUFFIXES = ["_MEGA_X", "_MEGA_Y", "_MEGA_Z", "_MEGA", "_GIGANTAMAX"]
def split_tag(mac):
    for suf in SUFFIXES:
        if mac.endswith(suf):
            tag = suf.lstrip("_").title().replace("_", " ")
            if tag == "Mega X": tag = "Mega X"
            if tag == "Mega Y": tag = "Mega Y"
            if tag == "Mega Z": tag = "Mega-Z"
            if tag == "Gigantamax": tag = "Gigantamax"
            return mac[: -len(suf)], tag
    return mac, None

species_extra = {}
for mac, v in ids.items():
    if not mac.startswith("SPECIES_") or not (905 < v < num_species):
        continue
    stem, tag = split_tag(mac)
    bn = base_name.get(stem) or (prettify(stem) if stem != mac else prettify(mac))
    name = f"{bn} ({tag})" if tag else bn
    species_extra[v] = name

# ---------- ITEMS_EXTRA ----------
items_h = read("include/constants/items.h")
rogue_h = read("include/constants/rogue.h")
first = int(re.search(r"#define\s+ITEM_ROGUE_ITEM_FIRST\s+(\d+)", items_h).group(1))
last = first + 200
rog_items = {}
for src_txt in (items_h, rogue_h):
    for m in re.finditer(r"#define\s+(ITEM_\w+)\s+\(?\s*ITEM_ROGUE_ITEM_FIRST\s*\+\s*(\d+)\s*\)?", src_txt):
        rog_items[m.group(1)] = first + int(m.group(2))
stones = {}
for m in re.finditer(r"#define\s+(ITEM_\w+)\s+\(?\s*ITEM_ROGUE_ITEM_LAST\s*\+\s*(\d+)\s*\)?", items_h):
    stones[m.group(1)] = last + int(m.group(2))

rog_src = read("src/data/rogue_items.h")
item_names = {}
for m in re.finditer(r"\[(ITEM_\w+)\s*-\s*ITEM_ROGUE_ITEM_FIRST\]\s*=\s*\{(.*?)\.itemId", rog_src, re.S):
    key, body = m.group(1), m.group(2)
    names = re.findall(r"\.name\s*=\s*_\(\"([^\"]*)\"\)", body)
    if not names:
        continue
    titled = [n for n in names if n != n.upper()]
    item_names[key] = titled[0] if titled else names[0]

def stone_name(mac):
    s = mac[len("ITEM_"):]
    if s.endswith("ITE") or s.endswith("RITE"):
        base = s[:-3]
        suffix = ""
        if base.endswith("_Z"):
            base, suffix = base[:-2], " (Z)"
        elif base.endswith("_X"):
            base, suffix = base[:-2], " X"
        elif base.endswith("_Y"):
            base, suffix = base[:-2], " Y"
        return base.replace("_", " ").title() + "ite" + suffix if not suffix else base.replace("_", " ").title() + "ite" + suffix
    return s.replace("_", " ").title()

extra_items = {}
for key, v in rog_items.items():
    if key in item_names:
        extra_items[v] = item_names[key]
for key, v in stones.items():
    extra_items[v] = stone_name(key)
extra_items = {k: v for k, v in extra_items.items() if first <= k <= last + 45}

# ---------- QUEST_NAMES ----------
base = "src/data/rogue"
qorder = []
def gather(relpath):
    d = json.loads(read(os.path.join(base, relpath).replace(os.sep, "/")))
    dirpart = os.path.dirname(relpath)
    defaults = d.get("defaults", {})
    for inc in d.get("includes", []):
        gather(os.path.join(dirpart, inc) if dirpart else inc)
    for quests in d.get("quest_groups", {}).values():
        for q in quests:
            cond = q.get("#if") or ""
            if "!defined(ROGUE_EXPANSION)" in cond:
                continue
            name = q.get("name") or defaults.get("name")
            tp = q.get("template_params", {})
            if isinstance(tp, dict) and name and "MON_SPECIES" in tp:
                name = name.replace("%MON_SPECIES%", tp["MON_SPECIES"].split("SPECIES_", 1)[-1].title().replace("_", " "))
            qorder.append(name or "?")
qroot = json.loads(read(os.path.join(base, "quests.json")))
for inc in qroot["includes"]:
    gather(inc)

# ---------- emit ----------
def ruststr(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')

with open(OUT, "w") as f:
    f.write("// Generated from Pokabbie/pokeemerald-rogue (expansion branch) headers. Do not edit.\n")
    f.write("// Supplemental tables: species form ids > 905, Rogue custom items, quest slot names.\n\n")
    f.write("pub const SPECIES_EXTRA: &[(u16, &str)] = &[\n")
    for k in sorted(species_extra):
        f.write(f'    ({k}, "{ruststr(species_extra[k])}"),\n')
    f.write("];\n\n")
    f.write("pub const ITEMS_EXTRA: &[(u16, &str)] = &[\n")
    for k in sorted(extra_items):
        f.write(f'    ({k}, "{ruststr(extra_items[k])}"),\n')
    f.write("];\n\n")
    f.write("pub const QUEST_NAMES: &[&str] = &[\n")
    for n in qorder:
        f.write(f'    "{ruststr(n)}",\n')
    f.write("];\n")

print(f"species_extra: {len(species_extra)} (max {max(species_extra) if species_extra else 0})")
print(f"items_extra:   {len(extra_items)} (range {min(extra_items) if extra_items else 0}..{max(extra_items) if extra_items else 0})")
print(f"quest_names:   {len(qorder)}")
print("species samples:", [(k, species_extra[k]) for k in sorted(species_extra)[:8]])
print("item samples:", [(k, extra_items[k]) for k in sorted(extra_items)[:8]])
print("wrote", OUT)
