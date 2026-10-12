#!/usr/bin/env python3
"""Generate `src/item_pockets.rs`: the game's *effective* pocket for every item id.

Why this file exists
--------------------
The bag in the save is ONE gap-free array (`bagPockets[BAG_ITEM_CAPACITY]`), and at
load time `UpdateBagItemsPointers()` (src/item.c) walks it once per pocket, breaking
out of a run the moment it sees an id whose pocket differs:

    // Expects items to always be sorted
    for (bagPocket = 0; bagPocket < POCKETS_COUNT; ++bagPocket) { ... }

So an item sitting outside its pocket run is silently unreachable in-game. To insert
or move items safely the editor has to know each id's pocket.

The pocket is NOT just `gItems[].pocket`. `ItemGetBaked()` (src/rogue_baked.c) applies
deterministic range/individual patches on top of the raw table, and the evolution-item
set comes from the build's bake data. All of that is replayed here.

Usage:
    python3 tools/gen_item_pockets.py /workspace/hermes/rogue-src \
        --rust src/item_pockets.rs
"""

import argparse
import os
import re
import sys


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def parse_defines(text):
    """Plain `#define NAME <int>` plus the two relative forms Rogue uses."""
    ids = {}
    for m in re.finditer(r"^#define\s+(ITEM_\w+)\s+(\d+)\s*$", text, re.M):
        ids[m.group(1)] = int(m.group(2))
    # Aliases: `#define FIRST_BALL ITEM_MASTER_BALL` and friends. Resolve iteratively
    # because some chains (FIRST_BERRY_INDEX -> ITEM_CHERI_BERRY) are one hop deep.
    alias = {}
    for m in re.finditer(r"^#define\s+((?:FIRST|LAST)_\w+|ITEM_\w+)\s+(ITEM_\w+)\s*$", text, re.M):
        alias[m.group(1)] = m.group(2)
    for _ in range(6):
        for k, v in alias.items():
            if v in ids:
                ids[k] = ids[v]
    return ids


def resolve_relative(ids, text):
    first = int(re.search(r"#define\s+ITEM_ROGUE_ITEM_FIRST\s+(\d+)", text).group(1))
    last = first + 200
    for m in re.finditer(
        r"#define\s+(ITEM_\w+)\s+\(?\s*ITEM_ROGUE_ITEM_FIRST\s*\+\s*(\d+)\s*\)?", text
    ):
        ids.setdefault(m.group(1), first + int(m.group(2)))
    for m in re.finditer(
        r"#define\s+(ITEM_\w+)\s+\(?\s*ITEM_ROGUE_ITEM_LAST\s*\+\s*(\d+)\s*\)?", text
    ):
        ids.setdefault(m.group(1), last + int(m.group(2)))
    return first, last


def block_fields(body):
    out = {}
    for key in ("pocket", "holdEffect", "battleUsage", "price", "fieldUseFunc"):
        m = re.search(r"\.%s\s*=\s*([^,\n]+)" % key, body)
        if m:
            out[key] = m.group(1).strip().rstrip(",").strip()
    m = re.search(r"\.name\s*=\s*_?\(\"([^\"]*)\"\)", body)
    if m:
        out["name"] = m.group(1)
    return out


def parse_item_table(text, ids):
    """Return {item_id: fields} from `[ITEM_X] = { ... },` blocks.

    Rogue writes most TM blocks with a *move* name (`[ITEM_TM_HYPER_BEAM]`) that the
    constants header never defines; the numeric id lives in the block's display name
    (`_("TM15")`). Those keys are resolved through the name so no TM/HM is dropped:
    a POCKET_NONE id in the bag would truncate `UpdateBagItemsPointers` and hide every
    pocket after it.
    """
    out = {}
    for m in re.finditer(r"^    \[(ITEM_\w+)([^\]]*)\]\s*=\s*\{(.*?)\n    \},", text, re.S | re.M):
        key, off = m.group(1), m.group(2).strip()
        fields = block_fields(m.group(3))
        if key in ids:
            iid = ids[key]
        else:
            nm = fields.get("name", "")
            tm = re.fullmatch(r"(TM|HM)(\d+)", nm)
            if not tm:
                continue
            alias = "ITEM_%s%02d" % (tm.group(1), int(tm.group(2)))
            if alias not in ids:
                continue
            iid = ids[alias]
        add = re.search(r"\+\s*(\d+)\s*$", off)
        if add:
            iid += int(add.group(1))
        out[iid] = fields
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src", help="path to pokeemerald-rogue source tree")
    ap.add_argument("--rust", required=True, help="output rust file")
    args = ap.parse_args()

    src = args.src
    items_h = read(os.path.join(src, "include/constants/items.h"))
    item_const_h = read(os.path.join(src, "include/constants/item.h"))
    ids = parse_defines(items_h)
    first, last = resolve_relative(ids, items_h)
    rogue_h_txt = read(os.path.join(src, "include/constants/rogue.h"))
    for m in re.finditer(r"#define\s+(ITEM_\w+)\s+(\d+)\s*$", rogue_h_txt, re.M):
        ids.setdefault(m.group(1), int(m.group(2)))
    for m in re.finditer(
        r"#define\s+(ITEM_\w+)\s+\(?\s*(?:ITEM_ROGUE_ITEM_FIRST|ITEM_ROGUE_ITEM_LAST)\s*\+\s*(\d+)\s*\)?",
        rogue_h_txt,
    ):
        base = first if "FIRST" in m.group(0) else last
        ids.setdefault(m.group(1), base + int(m.group(2)))

    # Pocket numbering depends on ROGUE_EXPANDED_POCKETS (set when ROGUE_EXPANSION is on).
    expanded = "ROGUE_EXPANDED_POCKETS" in item_const_h and "#define ROGUE_EXPANSION" in read(
        os.path.join(src, "include/constants/item.h")
    )
    pocket_names = {}
    for m in re.finditer(r"#define\s+(POCKET_\w+)\s+(\d+)\s*$", item_const_h, re.M):
        pocket_names.setdefault(m.group(1), int(m.group(2)))
    # The header defines both variants; the expanded block comes first, so `setdefault`
    # above already keeps the expansion values when the build is expanded. Record which
    # numbering we are actually using.
    if "#ifdef ROGUE_EXPANDED_POCKETS" not in item_const_h:
        sys.exit("unexpected item.h: no ROGUE_EXPANDED_POCKETS block")
    # Everything in this repo builds with ROGUE_EXPANSION, so expanded numbering applies.
    POCKET_NONE = pocket_names["POCKET_NONE"]
    POCKET_ITEMS = pocket_names["POCKET_ITEMS"]
    POCKET_HELD = pocket_names["POCKET_HELD_ITEMS"]
    POCKET_MEDICINE = pocket_names["POCKET_MEDICINE"]
    POCKET_STONES = pocket_names["POCKET_STONES"]
    POCKET_BALLS = pocket_names["POCKET_POKE_BALLS"]
    POCKET_TM = pocket_names["POCKET_TM_HM"]
    POCKET_BERRIES = pocket_names["POCKET_BERRIES"]
    POCKET_KEY = pocket_names["POCKET_KEY_ITEMS"]

    van = parse_item_table(read(os.path.join(src, "src/data/items.h")), ids)
    try:
        rog = parse_item_table(read(os.path.join(src, "src/data/rogue_items.h")), ids)
    except FileNotFoundError:
        rog = parse_item_table(read(os.path.join(src, "src/data/rogue/rogue_items.h")), ids)
    table = dict(van)
    for k, v in rog.items():
        table.setdefault(k, v)

    # --- ranges the patches use, resolved from constants -----------------
    def rng(a, b):
        return (ids[a], ids[b])

    hp_up, pp_max = rng("ITEM_HP_UP", "ITEM_PP_MAX")
    berry_lo, berry_hi = rng("FIRST_BERRY_INDEX", "LAST_BERRY_INDEX")
    med_range = rng("ITEM_X_ATTACK", "ITEM_GUARD_SPEC")  # ROGUE_EXPANSION branch
    z_lo, z_hi = rng("ITEM_NORMALIUM_Z", "ITEM_ULTRANECROZIUM_Z")
    drive_lo, drive_hi = rng("ITEM_DOUSE_DRIVE", "ITEM_CHILL_DRIVE")
    plate_lo, plate_hi = rng("ITEM_FLAME_PLATE", "ITEM_PIXIE_PLATE")
    mem_lo, mem_hi = rng("ITEM_FIRE_MEMORY", "ITEM_FAIRY_MEMORY")
    orb_lo, orb_hi = rng("ITEM_ADAMANT_CRYSTAL", "ITEM_LUSTROUS_GLOBE")
    mask_lo, mask_hi = rng("ITEM_CORNERSTONE_MASK", "ITEM_HEARTHFLAME_MASK")
    shard_lo, shard_hi = rng("ITEM_BUG_TERA_SHARD", "ITEM_WATER_TERA_SHARD")
    stellar = ids["ITEM_STELLAR_TERA_SHARD"]
    maxmush = ids["ITEM_MAX_MUSHROOMS"]
    sacred_ash = ids["ITEM_SACRED_ASH"]
    escape_rope = ids["ITEM_ESCAPE_ROPE"]
    soul_dew = ids["ITEM_SOUL_DEW"]
    ability_capsule = ids["ITEM_ABILITY_CAPSULE"]
    ability_patch = ids["ITEM_ABILITY_PATCH"]
    red_orb, blue_orb = ids["ITEM_RED_ORB"], ids["ITEM_BLUE_ORB"]
    a_orb, l_orb, g_orb = ids["ITEM_ADAMANT_ORB"], ids["ITEM_LUSTROUS_ORB"], ids["ITEM_GRISEOUS_ORB"]
    r_sword, r_shield = ids["RUSTED_SWORD" if "RUSTED_SWORD" in ids else "ITEM_RUSTED_SWORD"], ids[
        "ITEM_RUSTED_SHIELD"
    ]

    mega = re.search(
        r"#define\s+IS_MEGA_STONE\(itemId\)\s+(.*)", items_h
    ).group(1)
    mega_ranges = [
        (ids[a], ids[b])
        for a, b in re.findall(r"itemId\s*>=\s*(ITEM_\w+)\s*&&\s*itemId\s*<=\s*(ITEM_\w+)", mega)
    ]

    # --- evolution items: the baked list is what the shipped ROM consults ---
    bake = read(os.path.join(src, "src/data/rogue_bake_data.h"))
    evo_block = re.search(r"const u16 gRogueBake_EvoItems\[\]\s*=\s*\{(.*?)\};", bake, re.S).group(1)
    evo_items = {int(x) for x in re.findall(r"\d+", evo_block)}

    MED_FUNCS = {
        "ItemUseOutOfBattle_Medicine",
        "ItemUseOutOfBattle_ReduceEV",
        "ItemUseOutOfBattle_SacredAsh",
        "ItemUseOutOfBattle_PPRecovery",
        "ItemUseOutOfBattle_PPUp",
        "ItemUseOutOfBattle_RareCandy",
    }

    def in_(v, r):
        return r[0] <= v <= r[1]

    # Safety net: TM/HM ids are a contiguous block and every one of them is
    # POCKET_TM_HM in every generation. Rogue writes most TM blocks under a move-based
    # key that this script cannot resolve to an id, and a POCKET_NONE id sitting in the
    # bag would truncate UpdateBagItemsPointers and hide every pocket after it — so pin
    # the range instead of trusting block-key resolution alone.
    tm_first, tm_last = ids["ITEM_TM01"], ids["ITEM_HM08"]

    def effective(iid, f):
        p_raw = f.get("pocket", "POCKET_NONE")
        p = pocket_names.get(p_raw, -1)
        hold = f.get("holdEffect", "0")
        has_hold = hold not in ("0", "ITEM_EFFECT_NONE", "")
        fu = f.get("fieldUseFunc", "")
        if iid in (0,):
            return POCKET_NONE
        # ItemGetBaked: rogue custom items take the RogueItem pocket verbatim.
        if first <= iid <= last and iid in rog:
            p = pocket_names.get(rog[iid].get("pocket", "POCKET_NONE"), p)
        # range edits
        if hp_up <= iid <= pp_max:
            p = POCKET_MEDICINE
        if in_(iid, med_range):
            p = POCKET_MEDICINE
        if p == POCKET_ITEMS and fu in MED_FUNCS:
            p = POCKET_MEDICINE
        if berry_lo <= iid <= berry_hi:
            p = POCKET_BERRIES
        if iid in evo_items:
            p = POCKET_ITEMS
        elif has_hold and p != POCKET_BERRIES:
            p = POCKET_HELD
        for a, b in mega_ranges:
            if a <= iid <= b:
                p = POCKET_STONES
        if in_(iid, (z_lo, z_hi)) or in_(iid, (drive_lo, drive_hi)) or in_(iid, (plate_lo, plate_hi)):
            p = POCKET_STONES
        if in_(iid, (mem_lo, mem_hi)) or in_(iid, (orb_lo, orb_hi)) or in_(iid, (mask_lo, mask_hi)):
            p = POCKET_STONES
        if (iid >= shard_lo and iid <= shard_hi) or iid == stellar:
            p = POCKET_STONES
        if iid == maxmush or iid == soul_dew:
            p = POCKET_STONES
        if iid in (red_orb, blue_orb, a_orb, l_orb, g_orb, r_sword, r_shield):
            p = POCKET_STONES
        if iid in (ability_capsule, ability_patch):
            p = POCKET_MEDICINE
        if iid == sacred_ash:
            p = POCKET_KEY
        if iid == escape_rope:
            p = POCKET_ITEMS
        if tm_first <= iid <= tm_last:
            p = POCKET_TM
        return p

    pockets = {}
    for iid, f in table.items():
        pockets[iid] = effective(iid, f)

    # charms / key items are marked important by the same patch
    charms = set()
    for iid in range(first, last + 1):
        if iid in table and table[iid].get("pocket") == "POCKET_KEY_ITEMS":
            charms.add(iid)
    charms.add(sacred_ash)

    rows = sorted(pockets.items())
    with open(args.rust, "w", encoding="utf-8") as f:
        f.write(
            "// Generated by tools/gen_item_pockets.py from Pokabbie/pokeemerald-rogue.\n"
            "// Do not edit: the values are the game's EFFECTIVE pocket (gItems/rogue_items\n"
            "// `pocket` plus every ItemGetBaked patch and the baked evolution-item list).\n"
            "//\n"
            "// Pocket numbering is the ROGUE_EXPANDED_POCKETS variant (POCKETS_COUNT=%d).\n"
            "// UpdateBagItemsPointers() splits the flat bag array into one contiguous run\n"
            "// per pocket, in this order, so edits must keep runs contiguous or the item\n"
            "// becomes unreachable in-game.\n"
            % (max(pocket_names.values()) + 1)
        )
        f.write("\n")
        f.write("/// `POCKET_*` constants (include/constants/item.h).\n")
        f.write("pub const POCKET_ORDER: &[(&str, u8)] = &[\n")
        for nm in (
            "ITEMS",
            "HELD_ITEMS",
            "MEDICINE",
            "STONES",
            "POKE_BALLS",
            "TM_HM",
            "BERRIES",
            "POKEBLOCK",
            "KEY_ITEMS",
        ):
            f.write('    ("%s", %d),\n' % (nm.replace("_", " ").title(), pocket_names["POCKET_" + nm]))
        f.write("];\n\n")
        f.write("/// id -> effective pocket. Ids absent from this list have no pocket\n")
        f.write("/// (POCKET_NONE) and must not be placed in the bag.\n")
        f.write("pub const ITEM_POCKETS: &[(u16, u8)] = &[\n")
        for iid, p in rows:
            if p in (-1, POCKET_NONE):
                continue
            f.write("    (%d, %d),\n" % (iid, p))
        f.write("];\n\n")
        f.write("/// `POCKET_*` values for the Rogue pocket layout. Used by the\n")
        f.write("/// editor to reason about runs without magic numbers.\n")
        for nm in (
            "NONE",
            "ITEMS",
            "HELD_ITEMS",
            "MEDICINE",
            "STONES",
            "POKE_BALLS",
            "TM_HM",
            "BERRIES",
            "POKEBLOCK",
            "KEY_ITEMS",
        ):
            # Mirror of include/constants/item.h: the numeric value is pinned
            # and readable even where a constant is only referenced from tests.
            f.write("#[allow(dead_code)]\n")
            f.write("pub const POCKET_%s: u8 = %d;\n" % (nm, pocket_names["POCKET_" + nm]))
        f.write("\n")
        f.write("/// Items that are evolution stones in Rogue (`Rogue_IsEvolutionItem`).\n")
        f.write("pub const EVO_ITEMS: &[u16] = &[\n")
        for iid in sorted(evo_items):
            f.write("    %d,\n" % iid)
        f.write("];\n")
    print("wrote %s: %d ids, %d evo items, pockets %s" % (
        args.rust, len(rows), len(evo_items),
        {k: v for k, v in sorted(pocket_names.items()) if k.startswith("POCKET")}))


if __name__ == "__main__":
    main()
