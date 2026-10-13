#!/usr/bin/env python3
"""
get_icons.py -- download the icon-asset set for the er-save-editor UI.

Downloads (PNG only, never animated GIF):
  * assets/icons/pokemon/<species_id>.png   for every species id in the editor
      tables (tables.rs SPECIES, tables_extra.rs SPECIES_EXTRA, species_meta.rs
      SPECIES_META ids)
  * assets/icons/items/<item_id>.png        for every item id in the editor
      tables (tables.rs ITEMS + tables_extra.rs ITEMS_EXTRA + item_pockets.rs)
  * assets/manifest.json                    provenance + counts + missing lists

Art source (ONE style, documented choice):
  The default front-sprite set of the PokeAPI sprites repo under
      sprites/pokemon/
  keyed by PokeAPI *pokemon resource* id (forms use the 10001+ resource ids;
  a handful of cosmetic variants that have no resource id live under
  "<dexnum>-<form>.png", e.g. "493-fighting.png", "201-b.png").
  Items: sprites/items/<slug>.png.

Why this set: Rogue's id space is 1..1025 *plus ~530 form slots* (megas,
Gigantamax, Alolan/Galarian/Hisuian/Paldean, Arceus types, Unown letters,
Alcremie creams x toppings, Vivillon patterns, 2026 "Champions" megas).
The default sprites/pokemon/ set carries every PokeAPI resource id (which
includes every mega/Gmax/regional form) plus the named cosmetic variants;
generation-iii/emerald covers only 422 species and the gen-V black-white
mirror stops at gen-V forms, so neither covers Rogue's id space.

Rogue species-id semantics come from the game header
../rogue-src/include/constants/species.h (same header the editor tables were
generated from; the SPECIES_EXTRA *name column* in the editor tables has
suffix-stripping artifacts, e.g. id 958 is RAICHU_ALOLAN in the game but
reads "Sandshrew" in tables_extra.rs). Save files store game-header ids, so
icons must follow the game header; editor names are only used as a fallback
when the game source is not present.

Style of run: rerunnable and resumable. Existing valid PNGs are kept.
Directory listings and the PokeAPI resource index are cached in
assets/.cache/ (refetched when older than 7 days, or with --refresh-lists).

Usage:
    python3 tools/get_icons.py [--only species|items] [--workers N]
                               [--force] [--offline] [--refresh-lists]
                               [--limit N]            (smoke test)

Stdlib only.
"""

import argparse
import concurrent.futures as cf
import difflib
import json
import os
import random
import re
import struct
import sys
import time
import unicodedata
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

# ---------------------------------------------------------------------------
# paths
# ---------------------------------------------------------------------------
ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
ASSETS = ROOT / "assets"
POKEMON_DIR = ASSETS / "icons" / "pokemon"
ITEMS_DIR = ASSETS / "icons" / "items"
CACHE = ASSETS / ".cache"
MANIFEST = ASSETS / "manifest.json"

GAME_SPECIES_H = ROOT.parent / "rogue-src" / "include" / "constants" / "species.h"

# ---------------------------------------------------------------------------
# sources / style
# ---------------------------------------------------------------------------
SPRITES_REPO = "PokeAPI/sprites"
BW_TREE_URL = ("https://api.github.com/repos/PokeAPI/sprites/git/trees/"
               "master:sprites/pokemon")
ITEM_TREE_URL = ("https://api.github.com/repos/PokeAPI/sprites/git/trees/"
                 "master:sprites/items")
RAW_BASE = "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/"
BW_BASE = RAW_BASE + "pokemon/"
ITEM_BASE = RAW_BASE + "items/"
POKEAPI_LIST = "https://pokeapi.co/api/v2/pokemon?limit=500&offset={off}"
UA = "er-save-editor-icon-fetch/1.0 (https://github.com/; icon pipeline)"
STYLE_ID = "pokeapi-default-sprites-pokemon-96x96"

CACHE_TTL = 7 * 24 * 3600


def log(msg):
    print(msg, flush=True)


def now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# ---------------------------------------------------------------------------
# http
# ---------------------------------------------------------------------------
def http_get(url, timeout=30, max_attempts=6, statuses=None):
    """GET with exponential backoff; honours 429 Retry-After.
    Returns (status, bytes). status 404 returns bytes=b''.
    Records per-URL final status in `statuses` dict when given."""
    last = None
    for attempt in range(1, max_attempts + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA,
                                                       "Accept": "*/*"})
            with urllib.request.urlopen(req, timeout=timeout) as r:
                body = r.read()
                if statuses is not None:
                    statuses[url] = {"status": r.status, "attempts": attempt}
                return r.status, body
        except urllib.error.HTTPError as e:
            last = e.code
            if e.code == 404:
                if statuses is not None:
                    statuses[url] = {"status": 404, "attempts": attempt}
                return 404, b""
            retry_after = e.headers.get("Retry-After") if e.headers else None
            if e.code in (429, 502, 503, 504) or e.code >= 500:
                delay = float(retry_after) if (retry_after or "").isdigit() \
                    else min(60.0, 1.5 ** attempt + random.uniform(0, 1))
                time.sleep(delay)
                continue
            if statuses is not None:
                statuses[url] = {"status": e.code, "attempts": attempt}
            return e.code, b""
        except Exception:
            time.sleep(min(30.0, 1.5 ** attempt + random.uniform(0, 1)))
    if statuses is not None:
        statuses[url] = {"status": last or "ERR", "attempts": max_attempts}
    return last or 0, b""


def fetch_json_cached(name, url_builder, refresh=False, offline=False, paginate=None):
    """Fetch JSON, cache in CACHE/<name>.json; reuse when fresh."""
    CACHE.mkdir(parents=True, exist_ok=True)
    path = CACHE / name
    if not refresh and path.exists() and time.time() - path.stat().st_mtime < CACHE_TTL:
        return json.loads(path.read_text())
    if offline:
        if path.exists():
            log(f"  (offline, using stale cache {name})")
            return json.loads(path.read_text())
        raise SystemExit(f"--offline and no cache for {name}; run online first")
    if paginate:
        data, off = [], 0
        while True:
            status, body = http_get(url_builder(off))
            if status != 200:
                raise RuntimeError(f"listing fetch failed: {url_builder(off)} -> {status}")
            chunk = json.loads(body)
            data.extend(paginate(chunk))
            if len(data) >= chunk.get("count", len(data)):
                break
            off += len(chunk["results"])
    else:
        status, body = http_get(url_builder(0))
        if status != 200:
            raise RuntimeError(f"listing fetch failed: {url_builder(0)} -> {status}")
        data = json.loads(body)
    path.write_text(json.dumps(data))
    return data


def parse_ihdr(data):
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        return None
    if len(data) < 24 or data[12:16] != b"IHDR":
        return None
    w, h = struct.unpack(">II", data[16:24])
    return w, h


# ---------------------------------------------------------------------------
# editor table parsing (id space we must cover)
# ---------------------------------------------------------------------------
def _read(p):
    return (SRC / p).read_text(encoding="utf-8")


def pairs_table(text, name):
    m = re.search(r"pub const %s: &\[[^\]]*\] = &\[(.*?)\n\];" % name, text, re.S)
    if not m:
        raise RuntimeError(f"table {name} not found")
    return [(int(a), b) for a, b in
            re.findall(r'\((\d+),\s*"((?:[^"\\]|\\.)*)"\)', m.group(1))]


def parse_editor_tables():
    tables = _read("tables.rs")
    textra = _read("tables_extra.rs")
    meta_txt = _read("species_meta.rs")
    pockets_txt = _read("item_pockets.rs")

    species = pairs_table(tables, "SPECIES")
    items = pairs_table(tables, "ITEMS")
    sp_extra = pairs_table(textra, "SPECIES_EXTRA")
    it_extra = pairs_table(textra, "ITEMS_EXTRA")
    meta_ids = [int(a) for a in
                re.findall(r"\((\d+),\s*\d+,\s*\[\d+,\s*\d+,\s*\d+\]\)", meta_txt)]
    pocket_ids = [int(a) for a in re.findall(r"\((\d+),\s*\d+\),", pockets_txt)]

    sp_ids = sorted(set([i for i, _ in species] + [i for i, _ in sp_extra] + meta_ids))
    sp_names = {i: n for i, n in species}
    sp_names.update({i: n for i, n in sp_extra})

    it_ids = sorted(set([i for i, _ in items] + [i for i, _ in it_extra] + pocket_ids))
    it_names = {i: n for i, n in items}
    for i, n in it_extra:
        it_names.setdefault(i, n)  # tables.rs wins on the 827 collision
    return sp_ids, sp_names, it_ids, it_names, \
        {"meta_only_ids": sorted(set(meta_ids) - set(sp_names))}


def parse_game_species(path):
    """game species.h -> {id: CONSTANT_NAME_WITHOUT_PREFIX}. Authoritative."""
    txt = path.read_text(encoding="utf-8")
    lines = {}
    for m in re.finditer(
            r"^\s*#define\s+([A-Z0-9_]+)((?:\s+[A-Za-z0-9_+]+)+)\s*"
            r"(?:/\*.*\*/)?\s*$", txt, re.M):
        lines[m.group(1)] = " ".join(m.group(2).split())

    resolved = {}

    def tok(t):
        return t[len("SPECIES_"):] if t.startswith("SPECIES_") else t

    def resolve(name, depth=0):
        if name in resolved:
            return resolved[name]
        if depth > 12:
            return None
        v = lines.get(name)
        if v is None:
            return None
        if re.fullmatch(r"\d+", v):
            r = int(v)
        else:
            m2 = re.fullmatch(r"([A-Z0-9_]+)\s*\+\s*(\d+)", v)
            if m2:
                b = resolve(m2.group(1), depth + 1)
                r = None if b is None else b + int(m2.group(2))
            else:
                r = resolve(v, depth + 1)
        if r is not None:
            resolved[name] = r
        return r

    out = {}
    for n in lines:
        if not n.startswith("SPECIES_") or n == "SPECIES_NONE":
            continue
        v = resolve(n)
        if v and v > 0:
            out[v] = n[len("SPECIES_"):]
    return out


# ---------------------------------------------------------------------------
# species id -> BW-directory filename
# ---------------------------------------------------------------------------
# Curated overrides: game CONSTANT -> pokeapi resource slug or direct filename
SPECIES_OVERRIDES = {
    "PIKACHU_PARTNER": "pikachu-partner-cap",
    "NECROZMA_DUSK_MANE": "necrozma-dusk",
    "NECROZMA_DAWN_WINGS": "necrozma-dawn",
    "NECROZMA_ULTRA_NECROZMA": "necrozma-ultra",
    "UNOWN_EMARK": "201-exclamation.png",
    "UNOWN_QMARK": "201-question.png",
    # base-form-equivalent cosmetic slots -> base dex file
    "CHERRIM_OVERCAST": "421.png",
    "DEERLING_SPRING": "585.png",
    "SAWSBUCK_SPRING": "586.png",
    "BURMY_PLANT": "412.png",
    "WORMADAM_PLANT": "413.png",
    # calyrex rider forms: PokeAPI resource is "calyrex-ice"/"calyrex-shadow"
    "CALYREX_ICE_RIDER": "calyrex-ice",
    "CALYREX_SHADOW_RIDER": "calyrex-shadow",
}

# token rewrites applied when building the kebab slug (whole token match)
TOKEN_FIX = {
    "ALOLAN": "alola", "GALARIAN": "galar", "HISUIAN": "hisui",
    "GIGANTAMAX": "gmax", "PH": "phd", "EMARK": "exclamation",
    "QMARK": "question",
    # Gen-8/9 Vivillon pattern rename: Marine pattern -> "Meadow" name
    "MEADOW": "marine",
}
# tokens dropped when they are structural rather than form-differentiating
TOKEN_DROP = {"PH_D_MARKER", "FLOWER", "CLOAK", "TRIM", "SEA"}
# combined-token fixes applied to the raw joined slug
SLUG_FIX = {
    "pikachu-ph-d": "pikachu-phd",
}


def species_candidates(const, res_names, res_ids, dir_set):
    """Return (filename, how) or (None, reason). dir_set = BW dir filenames.
    res_names: set of pokeapi pokemon resource names; res_ids: name->id."""
    if const in SPECIES_OVERRIDES:
        ov = SPECIES_OVERRIDES[const]
        if ov.endswith(".png"):
            return (ov, "override-file") if ov in dir_set else (None, "override missing in set")
        fn = f"{res_ids[ov]}.png" if ov in res_ids else None
        return (fn, "override-slug") if fn and fn in dir_set else \
            (None, f"override slug {ov} unresolved")

    # Zygarde's Aura Break form *is* Power Construct; Minior names the
    # meteor/core token after the colour, not before it.
    if "AURA_BREAK" in const:
        const = const.replace("AURA_BREAK", "POWER_CONSTRUCT")
    if const.startswith("MINIOR_METEOR_"):
        const = "MINIOR_" + const[len("MINIOR_METEOR_"):] + "_METEOR"

    # Alcremie cream x topping variants (dex 869): "869-<cream>-<topping>-sweet.png"
    if const.startswith("ALCREMIE_") and "GIGANTAMAX" not in const:
        toks = const.split("_")
        base = toks[0]
        rest = toks[1:]
        topping = rest[0] if rest else None
        cream = "_".join(rest[1:]).lower() if len(rest) > 1 else None
        if topping and cream:
            if cream == "vanilla_cream" and topping == "STRAWBERRY":
                return "869.png", "alcremie-default"
            fn = f"869-{cream.replace('_','-')}-{topping.lower()}-sweet.png"
            if fn in dir_set:
                return fn, "alcremie-variant"
        # else fall through to the generic path (base fallback covers it)

    toks = const.split("_")
    # PH_D -> PHD
    if "PH" in toks and "D" in toks:
        i = toks.index("PH")
        if i + 1 < len(toks) and toks[i + 1] == "D":
            toks = toks[:i] + ["PH_D_MARKER"] + toks[i + 2:]
    fixed = [(t if t not in TOKEN_FIX else TOKEN_FIX[t]) for t in toks
             if t not in TOKEN_DROP]
    slug = SLUG_FIX.get("-".join(x.lower() for x in fixed),
                        "-".join(x.lower() for x in fixed))

    # 1. exact pokeapi resource
    if slug in res_names:
        return f"{res_ids[slug]}.png", "resource-exact"

    # 2. variant filename "<base-dex>-<variant-slug>.png" (unown letters,
    #    arceus types, vivillon, furfrou trims, flowers, cloaks, seas,
    #    genesect drives, shellos east, deerling seasons, ...)
    best_base = None
    for n in range(len(toks) - 1, 0, -1):
        # longest prefix that resolves to a *species* resource id <=1025
        base_join = "-".join(t.lower() for t in toks[:n])
        if base_join in res_ids and res_ids[base_join] <= 1025:
            best_base = res_ids[base_join]
            variant = toks[n:]
            vslug = "-".join((TOKEN_FIX.get(t, t)).lower() for t in variant
                             if t not in TOKEN_DROP and t != "CORE"
                             and t != "DRIVE")
            if vslug:
                fn = f"{best_base}-{vslug}.png"
                if fn in dir_set:
                    return fn, "dex-variant"
            break

    # 3. drop CORE / meteor shuffles for minior: MINIOR_CORE_ORANGE -> minior-orange
    slug2 = "-".join(t.lower() for t in toks
                     if t not in TOKEN_FIX and t not in ("CORE",))
    slug2 = "-".join(TOKEN_FIX.get(t, t).lower() for t in toks if t != "CORE")
    if slug2 in res_names:
        return f"{res_ids[slug2]}.png", "resource-drop-core"

    # 4. fuzzy: nearest resource name, nearest named file stem
    m = difflib.get_close_matches(slug, res_names, n=1, cutoff=0.8)
    if m:
        return f"{res_ids[m[0]]}.png", f"fuzzy-resource:{m[0]}"
    stems = {s: s for s in res_names}
    named = [fn[:-4] for fn in dir_set if not fn.split(".")[0].isdigit()]
    m = difflib.get_close_matches(slug, named, n=1, cutoff=0.8)
    if m:
        return f"{m[0]}.png", f"fuzzy-named:{m[0]}"

    # 5. default-form slot whose variant file does not exist (ARCEUS_NORMAL,
    #    CASTFORM_NORMAL, FURFROU_NATURAL, SHELLOS_WEST_SEA, PHONY, ...):
    #    draw it as the base species sprite.
    if best_base:
        bfn = f"{best_base}.png"
        if bfn in dir_set:
            return bfn, "base-fallback"
    return None, f"unmapped slug '{slug}'"


# ---------------------------------------------------------------------------
# item name -> sprites/items slug
# ---------------------------------------------------------------------------
def kebab(name):
    s = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", s.lower())
    return re.sub(r"-+", "-", s).strip("-")


# curated fixes: editor display name -> sprites/items slug
ITEM_FIXES = {
    "X Defend": "x-defense",
    "X Special Defend": "x-special-defense",
    "X Special Defense": "x-special-defense",
    "Poke Ball": "poke-ball",
    "Hp Up": "hp-up",
    "Pp Up": "pp-up",
    "Pp Max": "pp-max",
    "Root Fossil": "root-fossil",
    "Vs Seeker": "vs-seeker",
    "Vs Recorder": "vs-recorder",
    "Vs Robot": "vs-robot",
    "Vs Rider": "vs-rider",
    "Vs Rider (water)": "vs-rider-water",
    "Douse Drive": "douse-drive",
    "Shock Drive": "shock-drive",
    "Burn Drive": "burn-drive",
    "Chill Drive": "chill-drive",
    "Rage Candy Bar": "rage-candy-bar",
    "Berry Juice": "berry-juice",
    "Old Amber": "old-amber",
    "Dna Splicers": "dna-splicers",
    "X Accuracy": "x-accuracy",
    "Dire Hit": "dire-hit",
    "Light Clay": "light-clay",
    "Micle Berry": "micle-berry",
    "Kelpsi Tree Wont": "kelpsy-tree",
    # names that exist in the sprite set under their older/alternate names
    "Max Honey": "honey",                    # 'stick' below is the Gen-1 leek
    "Leek": "stick",
    "Rotom Catalog": "roto-catch",           # PokeAPI's historical slug
    "Small Coin Case": "coin-case",
    "Large Coin Case": "coin-case",
    "Adamant Crystal": "adamant-orb",        # SV rename of the BDSP orb
    "Lustrous Globe": "lustrous-orb",
    "Max Mushrooms": "big-mushroom",
    "Enigma Berry E Reader": "enigma-berry",
    # Exp Candies have no icon in the showdown set; the candy stand-in beats a
    # letter badge for what is a very common bag item (recorded as a fix).
    "Exp Candy Xs": "rare-candy", "Exp Candy S": "rare-candy",
    "Exp Candy M": "rare-candy", "Exp Candy L": "rare-candy",
    "Exp Candy Xl": "rare-candy",
}
# generic stand-ins: per-number TM/HM ids and POKEBLOCK variants have no art
# in the public set, but generic case/kit icons do.
TM_RE = re.compile(r"^(Tm|Hm|Mt|Tr)\d{1,3}$")
PLACEHOLDER_RE = re.compile(
    r"^(Reserved \d+|Unused \d+|Rogue Item First|Berry Pop|Quest (Book|Log)|"
    r"Rogue Ball|Healing Flask|DEBUG )")


def item_candidates(name, dir_set):
    slug = kebab(name)
    if slug in dir_set:
        return slug, None
    if name in ITEM_FIXES and ITEM_FIXES[name] in dir_set:
        return ITEM_FIXES[name], f"fix:'{name}' -> {ITEM_FIXES[name]}"
    # showdown shows bag-state variants under a double-dash suffix:
    # "normalium-z--bag", "n-solarizer--split", "dna-splicers--split", ...
    for suf in ("--bag", "--split", "--merge"):
        if slug + suf in dir_set:
            return slug + suf, f"variant-suffix -> {slug}{suf}"
    # feather/mochi stat items are drawn with the HGSS "wing" art
    if slug.endswith("-feather") or slug.endswith("-mochi"):
        alt = slug.rsplit("-", 1)[0] + "-wing"
        if alt in dir_set:
            return alt, f"wing -> {alt}"
    # per-number TM/HM: generic TM case
    if TM_RE.match(name) and "tm-case" in dir_set:
        return "tm-case", "tm-case stand-in"
    # POKEBLOCK variants: generic kit
    if "{POKEBLOCK}" in name and "pokeblock-kit" in dir_set:
        return "pokeblock-kit", "pokeblock-kit stand-in"
    # strip trailing punctuation words / try singular-plural
    alts = [slug.rstrip('-')]
    if slug.endswith('s') and slug[:-1] in dir_set:
        return slug[:-1], f"depluralize -> {slug[:-1]}"
    if not slug.endswith('s') and slug + 's' in dir_set:
        return slug + 's', f"pluralize -> {slug}s"
    if slug.endswith('_berry') and slug.replace('_berry', '-berry') in dir_set:
        return slug.replace('_berry', '-berry'), None
    m = difflib.get_close_matches(slug, dir_set, n=1, cutoff=0.86)
    if m:
        return m[0], f"fuzzy -> {m[0]}"
    return None, None


# ---------------------------------------------------------------------------
# download worker
# ---------------------------------------------------------------------------
def download(out_path, url, force, statuses, stats):
    if not force and out_path.exists():
        data = out_path.read_bytes()
        if parse_ihdr(data):
            stats["kept_existing"] += 1
            return "kept"
        out_path.unlink()  # corrupt/non-PNG: re-download
    status, body = http_get(url, statuses=statuses)
    if status == 404:
        stats["http_404"] += 1
        return "404"
    if status != 200 or not parse_ihdr(body):
        stats["failed"] += 1
        return f"fail:{status}"
    tmp = out_path.with_suffix(out_path.suffix + ".part")
    tmp.write_bytes(body)
    tmp.replace(out_path)
    stats["downloaded"] += 1
    return "ok"


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", choices=["species", "items"])
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--force", action="store_true", help="re-download even if valid PNG exists")
    ap.add_argument("--offline", action="store_true")
    ap.add_argument("--refresh-lists", action="store_true")
    ap.add_argument("--limit", type=int, default=0, help="cap downloads (smoke test)")
    args = ap.parse_args()

    log("== er-save-editor icon pipeline ==")
    sp_ids, sp_names, it_ids, it_names, extras = parse_editor_tables()
    log(f"editor tables: {len(sp_ids)} species ids (max {max(sp_ids)}), "
        f"{len(it_ids)} item ids (max {max(it_ids)}), "
        f"{len(extras['meta_only_ids'])} meta-only unnamed ids")

    game_names = None
    if GAME_SPECIES_H.exists():
        game_names = parse_game_species(GAME_SPECIES_H)
        log(f"game species.h: {len(game_names)} constants resolved "
            f"(authoritative id -> name)")
    else:
        log(f"WARNING: {GAME_SPECIES_H} not found; falling back to editor table "
            f"names (form-heavy ids will be under-mapped)")

    # listings (cached)
    refresh = args.refresh_lists
    bw_raw = fetch_json_cached("bw_dir.json", lambda _: BW_TREE_URL,
                               refresh, args.offline)
    bw_files = {t["path"] for t in bw_raw["tree"]
                if t["type"] == "blob" and t["path"].endswith(".png")}
    items_raw = fetch_json_cached("items_dir.json", lambda _: ITEM_TREE_URL,
                                  refresh, args.offline)
    item_files = {t["path"] for t in items_raw["tree"]
                  if t["type"] == "blob" and t["path"].endswith(".png")}
    item_slugs = {f[:-4] for f in item_files}
    log(f"listing: BW set {len(bw_files)} PNGs, items set {len(item_files)} PNGs")

    res_list = fetch_json_cached(
        "pokeapi_pokemon.json", lambda off: POKEAPI_LIST.format(off=off),
        refresh, args.offline,
        paginate=lambda ch: [{"name": r["name"],
                              "id": int(r["url"].rstrip("/").split("/")[-1])}
                             for r in ch["results"]])
    res_ids = {r["name"]: r["id"] for r in res_list}
    # PokeAPI names gendered species with a -male/-female suffix (the id-668
    # resource is literally "pyroar-male"); register the bare name as an alias.
    for nm, rid in list(res_ids.items()):
        for suf in ("-male", "-female"):
            if nm.endswith(suf) and nm[:-len(suf)] not in res_ids:
                res_ids[nm[:-len(suf)]] = rid
    res_names = list(res_ids)
    log(f"pokeapi pokemon resources: {len(res_ids)}")

    statuses = {}
    stats = {"downloaded": 0, "kept_existing": 0, "http_404": 0, "failed": 0}
    jobs = []  # (out_path, url, id, meta)
    species_plan, items_plan = [], []

    # ---- species plan
    _species_fixes = []
    if args.only != "items":
        fixes_log = []
        for sid in sp_ids:
            const = game_names.get(sid) if game_names else None
            ed_name = sp_names.get(sid)
            label = const or ed_name
            if label is None:
                species_plan.append({"id": sid, "status": "missing",
                                     "reason": "no name in game header or editor tables",
                                     "editor_name": None})
                continue
            fn, how = species_candidates(label, set(res_names), res_ids, bw_files)
            if fn is None:
                species_plan.append({"id": sid, "status": "missing",
                                     "reason": how, "game_name": const,
                                     "editor_name": ed_name})
            else:
                if how and not how.startswith("resource-exact"):
                    fixes_log.append({"id": sid, "game_name": label,
                                      "file": fn, "rule": how})
                species_plan.append({"id": sid, "status": "planned", "file": fn,
                                     "game_name": const, "editor_name": ed_name,
                                     "rule": how})
                jobs.append((POKEMON_DIR / f"{sid}.png", BW_BASE + fn,
                             sid, {"kind": "species", "rule": how}))
        log(f"species plan: {sum(1 for p in species_plan if p['status']=='planned')} "
            f"planned, {sum(1 for p in species_plan if p['status']=='missing')} missing")
        _species_fixes = fixes_log

    # ---- items plan
    _item_fixes = []
    if args.only != "species":
        item_fixes_log = []
        for iid in it_ids:
            name = it_names.get(iid)
            if name is None:
                items_plan.append({"id": iid, "status": "missing",
                                   "reason": "no name in ITEMS/ITEMS_EXTRA"})
                continue
            slug, note = item_candidates(name, item_slugs)
            if slug is None:
                reason = ("placeholder/sentinel id - no public art exists"
                          if PLACEHOLDER_RE.match(name) else
                          "no matching sprite in PokeAPI items set")
                items_plan.append({"id": iid, "status": "missing",
                                   "reason": reason, "name": name})
            else:
                if note:
                    item_fixes_log.append({"id": iid, "name": name,
                                           "slug": slug, "fix": note})
                items_plan.append({"id": iid, "status": "planned", "slug": slug,
                                   "name": name, "fix": note})
                jobs.append((ITEMS_DIR / f"{iid}.png", ITEM_BASE + slug + ".png",
                             iid, {"kind": "item", "slug": slug}))
        log(f"items plan: {sum(1 for p in items_plan if p['status']=='planned')} "
            f"planned, {sum(1 for p in items_plan if p['status']=='missing')} missing")
        _item_fixes = item_fixes_log

    if args.limit:
        jobs = jobs[:args.limit]
        log(f"--limit {args.limit}: capping downloads to {len(jobs)}")

    POKEMON_DIR.mkdir(parents=True, exist_ok=True)
    ITEMS_DIR.mkdir(parents=True, exist_ok=True)

    # ---- download
    t0 = time.time()
    results = {}
    with cf.ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = {ex.submit(download, p, u, args.force, statuses, stats): (p, meta)
                for (p, u, i, meta) in jobs}
        done = 0
        for f in cf.as_completed(futs):
            p, meta = futs[f]
            results[str(p)] = f.result()
            done += 1
            if done % 200 == 0:
                log(f"  {done}/{len(jobs)} files ...")
    log(f"downloads done in {time.time()-t0:.1f}s")

    # ---- verification walk over what we planned for
    def verify(plan, out_dir, key):
        ok_ids, bad, total = [], [], 0
        dims = []
        for ent in plan:
            if ent["status"] != "planned":
                continue
            f = out_dir / f"{ent['id']}.png"
            data = f.read_bytes() if f.exists() else b""
            ih = parse_ihdr(data)
            if ih:
                ok_ids.append(ent["id"])
                total += len(data)
                if len(dims) < 12:
                    dims.append({"file": f.name, "w": ih[0], "h": ih[1],
                                 "bytes": len(data)})
            else:
                bad.append(ent["id"])
        return ok_ids, bad, total, dims

    sp_ok = sp_bad = it_ok = it_bad = []
    sp_bytes = it_bytes = 0
    sp_dims = it_dims = []
    if args.only != "items":
        sp_ok, sp_bad, sp_bytes, sp_dims = verify(species_plan, POKEMON_DIR, "id")
    if args.only != "species":
        it_ok, it_bad, it_bytes, it_dims = verify(items_plan, ITEMS_DIR, "id")

    # ---- manifest
    sp_missing = [p for p in species_plan if p["status"] == "missing"]
    it_missing = [p for p in items_plan if p["status"] == "missing"]
    # full id -> file map with on-disk availability, so the manifest is itself
    # a lookup table (the app's icons.rs never requires it, but tools can).
    sp_ok_set, it_ok_set = set(sp_ok), set(it_ok)
    species_files = {
        str(p["id"]): {
            "file": f"pokemon/{p['id']}.png",
            "source": p.get("file"),
            "name": p.get("game_name") or p.get("editor_name"),
            "available": p["id"] in sp_ok_set,
            **({} if p["status"] == "planned" else {"reason": p.get("reason")}),
        } for p in species_plan}
    items_files = {
        str(p["id"]): {
            "file": f"items/{p['id']}.png",
            "source": (p["slug"] + ".png") if p.get("slug") else None,
            "name": p.get("name"),
            "available": p["id"] in it_ok_set,
            **({} if p["status"] == "planned" else {"reason": p.get("reason")}),
        } for p in items_plan}
    manifest = {
        "generated_at": now_iso(),
        "style": {
            "id": STYLE_ID,
            "name": "Smogon/Showdown in-game front sprites (PNG, ~96x96), "
                    "mirrored by the PokeAPI sprites repo",
            "species_base_url": BW_BASE,
            "species_keying": "PokeAPI pokemon resource id "
                              "(forms at 10001+; cosmetic variants as "
                              "<dexnum>-<form>.png)",
            "example_verified_url": (BW_BASE +
                                     (next((p["file"] for p in species_plan
                                            if p.get("id") == 906), "905.png"))),
            "items_base_url": ITEM_BASE,
            "why": "single set covering base dex 1..1025 plus mega/Gigantamax/"
                   "regional/Arceus/Unown/Alcremie/Vivillon forms as PNG; "
                   "gen-iii emerald covers only 422 species, 'other/home' is "
                   ">100 MB and partly 3D renders, official-artwork is not an "
                   "in-game style. Accepted trade-off: within this curated set "
                   "each species is drawn in its debut-generation style.",
            "licence": "PokeAPI/sprites repo is CC0-licensed hosting of art that "
                       "remains Nintendo/Creatures Inc./GAME FREAK inc. property; "
                       "personal/non-commercial editor use. See assets/README.md.",
        },
        "species": {
            "requested_ids": len(sp_ids),
            "planned": sum(1 for p in species_plan if p["status"] == "planned"),
            "verified_png_on_disk": len(sp_ok),
            "missing_count": len(sp_missing),
            "missing_ids": [p["id"] for p in sp_missing],
            "missing_detail": sp_missing,
            "planned_but_not_on_disk": sp_bad,
            "name_source": ("game header rogue-src/include/constants/species.h"
                            " (authoritative for save-file ids; editor "
                            "SPECIES_EXTRA names have suffix-stripping "
                            "artifacts) with editor-table fallback"),
            "mapping_fixes": _species_fixes,
        },
        "items": {
            "requested_ids": len(it_ids),
            "planned": sum(1 for p in items_plan if p["status"] == "planned"),
            "verified_png_on_disk": len(it_ok),
            "missing_count": len(it_missing),
            "missing_ids": [p["id"] for p in it_missing],
            "missing_detail": it_missing,
            "planned_but_not_on_disk": it_bad,
            "slug_fixes_applied": _item_fixes,
            "note": "Rogue-only items (Rogue Ball, Quest Book, Healing Flask, "
                    "Gimmick stones beyond SwSh, Scarlet/Violet-era items) have "
                    "no public sprite; the app draws its own fallback badge.",
        },
        "http": {
            "workers": args.workers,
            "downloaded_this_run": stats["downloaded"],
            "kept_existing": stats["kept_existing"],
            "unexpected_404": stats["http_404"],
            "failed": stats["failed"],
            "non_200_urls": {u: v for u, v in statuses.items()
                             if v["status"] != 200},
        },
        "verification": {
            "png_magic_checked": True,
            "pokemon_files": len(sp_ok), "pokemon_total_bytes": sp_bytes,
            "items_files": len(it_ok), "items_total_bytes": it_bytes,
            "total_bytes": sp_bytes + it_bytes,
            "sample_dimensions_pokemon": sp_dims,
            "sample_dimensions_items": it_dims,
        },
        "files": {"pokemon": species_files, "items": items_files},
        "manifest_entries": len(species_files) + len(items_files),
        "caches": ["assets/.cache/bw_dir.json", "assets/.cache/items_dir.json",
                   "assets/.cache/pokeapi_pokemon.json"],
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2))

    # icons.rs gates on the presence of assets/icons/ but resolves
    # root.join("<kind>/<id>.png) where root is the *assets* dir; keep
    # assets/icons/{pokemon,items} canonical and hardlink mirrors directly
    # under assets/ so both the documented layout and the literal code path
    # resolve. Also expose the manifest at assets/icons/manifest.json.
    mirrored = 0
    for kind in ("pokemon", "items"):
        src_dir, dst_dir = ASSETS / "icons" / kind, ASSETS / kind
        dst_dir.mkdir(parents=True, exist_ok=True)
        for f in src_dir.glob("*.png"):
            t = dst_dir / f.name
            if t.exists() or t.is_symlink():
                t.unlink()
            try:
                os.link(f, t)
            except OSError:
                t.write_bytes(f.read_bytes())
            mirrored += 1
    (ASSETS / "icons" / "manifest.json").write_text(MANIFEST.read_text())
    log(f"mirrored {mirrored} hardlinks under {ASSETS} for icons.rs's "
        f"root.join layout; manifest also at {ASSETS / 'icons' / 'manifest.json'}")

    # ---- summary table
    log("")
    log("== summary ==")
    log(f"{'category':10} {'requested':>9} {'mapped':>7} {'on-disk PNG':>11} "
        f"{'missing':>7}")
    log(f"{'pokemon':10} {len(sp_ids):>9} "
        f"{sum(1 for p in species_plan if p['status']=='planned'):>7} "
        f"{len(sp_ok):>11} {len(sp_missing):>7}")
    log(f"{'items':10} {len(it_ids):>9} "
        f"{sum(1 for p in items_plan if p['status']=='planned'):>7} "
        f"{len(it_ok):>11} {len(it_missing):>7}")
    log(f"\ntop 15 missing species ids (id: game/editor name -- reason):")
    for p in sp_missing[:15]:
        log(f"  {p['id']}: {p.get('game_name') or p.get('editor_name')} -- {p['reason']}")
    log(f"\ntop 15 missing item ids (id: name -- reason):")
    for p in it_missing[:15]:
        log(f"  {p['id']}: {p.get('name')} -- {p['reason']}")
    log(f"\ntotal bytes: pokemon {sp_bytes/1e6:.2f} MB + items {it_bytes/1e6:.2f} MB "
        f"= {(sp_bytes+it_bytes)/1e6:.2f} MB")
    log(f"manifest -> {MANIFEST}")
    log("sample pokemon dims: " + str([(d['file'], f"{d['w']}x{d['h']}") for d in sp_dims[:4]]))
    log("sample items dims:   " + str([(d['file'], f"{d['w']}x{d['h']}") for d in it_dims[:4]]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
