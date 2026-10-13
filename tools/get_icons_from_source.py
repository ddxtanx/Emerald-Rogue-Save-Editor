#!/usr/bin/env python3
"""Fetch the item icons straight from the Emerald Rogue source and build the
icon set the editor loads.

    python3 tools/get_icons_from_source.py                 # clone + generate
    python3 tools/get_icons_from_source.py --ref EX-v2.1   # pin a release
    python3 tools/get_icons_from_source.py --source ~/pokeemerald-rogue

What it does:

  1. clones (shallow, sparse -- only graphics/items, src/data and include) the
     game source into a cache directory, or uses an existing checkout;
  2. resolves the item id for every item constant, then follows the game's own
     icon wiring: the id -> (icon graphic, palette) table for regular items plus
     the runtime table for the custom item block, and the INCBIN lines that name
     the PNG and .pal each graphic came from;
  3. writes `<item id>.png` for every id that has art, into --out (default
     assets/icons/items), which is what the editor's icon loader reads.

Existing icons are replaced by the game's own art (pass --keep-existing to
retain files that are already there). Ids with no art in this version are
skipped -- the editor draws a letter-and-ball badge for those instead.

These are the exact pixels the ROM displays: the game's build compresses these
same PNGs into the ROM, and a ROM-side extractor reproduces the same colours and
tile indices (tools/get_icons_from_rom.py, cross-checked against this output).

stdlib only: zlib for PNG, git for the fetch.
"""

from __future__ import annotations

import argparse
import os
import re
import struct
import subprocess
import sys
import zlib

DEFAULT_REPO = "https://github.com/Pokabbie/pokeemerald-rogue"
DEFAULT_REF = "expansion"  # the EX line; EX-v2.1 and older are tags
SPARSE_PATHS = ("graphics/items", "src/data", "include")

TRANSPOSE_TABLES = [
    [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2), (0, 3), (1, 3), (0, 4), (1, 4), (0, 5), (1, 5), (0, 6), (1, 6), (0, 7), (1, 7), (0, 8), (1, 8), (0, 9), (1, 9), (0, 10), (1, 10), (0, 11), (1, 11), (0, 12), (1, 12), (0, 13), (1, 13), (0, 14), (1, 14), (0, 15), (1, 15)],
    [(0, 0), (0, 1), (1, 0), (1, 1), (0, 2), (0, 3), (1, 2), (1, 3)],
    [(0, 0), (1, 0), (0, 1), (1, 1)],
]


def png_decode(blob: bytes):
    """(width, height, [(r, g, b, a), ...]) for a non-interlaced PNG.

    Handles the depths these icon files actually use: palette images at 1/2/4/8
    bits per pixel, and truecolour at 8 (RGB or RGBA).
    """
    if not blob.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("not a PNG")
    pos, palette, trns, pieces, bit_depth, color_type, interlace = 8, None, None, [], 0, 0, 0
    width = height = 0
    while pos < len(blob):
        (length,) = struct.unpack_from(">I", blob, pos)
        kind = blob[pos + 4 : pos + 8]
        body = blob[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, comp, filt, interlace = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            palette = body
        elif kind == b"tRNS":
            trns = body
        elif kind == b"IDAT":
            pieces.append(body)
        elif kind == b"IEND":
            break
    if interlace:
        raise ValueError("interlaced PNGs are not supported")
    if color_type == 3:
        if bit_depth not in (1, 2, 4, 8):
            raise ValueError(f"unsupported palette bit depth {bit_depth}")
        if palette is None:
            raise ValueError("palette PNG without a PLTE chunk")
        channels = 1
    elif color_type in (2, 6):
        if bit_depth != 8:
            raise ValueError(f"unsupported truecolour bit depth {bit_depth}")
        channels = 3 if color_type == 2 else 4
    elif color_type == 0:
        channels = 1
    else:
        raise ValueError(f"unsupported PNG colour type {color_type}")

    bits = channels * bit_depth
    stride = (width * bits + 7) // 8
    bpp = max(1, bits // 8)
    raw = zlib.decompress(b"".join(pieces))
    rows, previous = [], bytearray(stride)
    p = 0
    for _ in range(height):
        filt = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = previous[i]
            c = previous[i - bpp] if i >= bpp else 0
            if filt == 1:
                line[i] = (line[i] + a) & 0xFF
            elif filt == 2:
                line[i] = (line[i] + b) & 0xFF
            elif filt == 3:
                line[i] = (line[i] + ((a + b) >> 1)) & 0xFF
            elif filt == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
            elif filt != 0:
                raise ValueError(f"unknown PNG filter {filt}")
        rows.append(line)
        previous = line

    ordered_palette = None
    index_plane = None
    if color_type == 3 and palette is not None:
        ordered_palette = [tuple(palette[i : i + 3]) for i in range(0, len(palette), 3)]
        index_plane = []

    mask = (1 << bit_depth) - 1
    pixels = []
    for line in rows:
        for x in range(width):
            if color_type == 3:
                bit = x * bit_depth
                byte = line[bit // 8]
                idx = (byte >> (8 - bit_depth - (bit % 8))) & mask
                if index_plane is not None:
                    index_plane.append(idx)
                r, g, b = palette[idx * 3 : idx * 3 + 3]
                a = trns[idx] if trns and idx < len(trns) else 255
            elif color_type == 0:
                bit = x * bit_depth
                val = (line[bit // 8] >> (8 - bit_depth - (bit % 8))) & mask
                gray = val if bit_depth == 8 else (val * 255) // mask
                r = g = b = gray
                a = 255
            else:
                i = x * channels
                r, g, b = line[i], line[i + 1], line[i + 2]
                a = line[i + 3] if channels == 4 else 255
            pixels.append((r, g, b, a))
    return width, height, pixels, ordered_palette, index_plane


def png_encode(path: str, w: int, h: int, pixels) -> None:
    rows = bytearray()
    for y in range(h):
        rows.append(0)
        for x in range(w):
            r, g, b, a = pixels[y * w + x]
            rows += bytes((r, g, b, a))

    def chunk(tag: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    with open(path, "wb") as fh:
        fh.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))


def jasc_palette(path: str):
    """A JASC .pal file -> list of (r, g, b), in palette-index order."""
    lines = open(path).read().splitlines()
    if not lines or "JASC-PAL" not in lines[0]:
        return None
    count = int(lines[2].strip())
    out = []
    for line in lines[3:]:
        parts = line.split()
        if len(parts) < 3:
            continue
        out.append(tuple(int(v) for v in parts[:3]))
        if len(out) >= count:
            break
    return out


def git(cwd: str, *args, capture: bool = True):
    return subprocess.run(("git",) + args, cwd=cwd, capture_output=capture, text=True, check=True)


def ensure_source(cache: str, repo: str, ref: str, refresh: bool) -> str:
    """Sparse-clone the parts of the game source the icon wiring lives in."""
    src = os.path.join(cache, "checkout")
    if not os.path.isdir(os.path.join(src, ".git")):
        os.makedirs(cache, exist_ok=True)
        print(f"cloning {repo} ({ref}) into {src}")
        subprocess.run(
            ("git", "clone", "--depth", "1", "--sparse", "--branch", ref, repo, src),
            check=True,
        )
        git(src, "sparse-checkout", "set", *SPARSE_PATHS)
    else:
        current = git(src, "rev-parse", "--abbrev-ref", "HEAD").stdout.strip()
        if refresh or current != ref:
            print(f"updating {src} to {ref}")
            git(src, "fetch", "--depth", "1", "origin", ref)
            git(src, "checkout", "-q", "FETCH_HEAD")
        git(src, "sparse-checkout", "set", *SPARSE_PATHS)
    head = git(src, "rev-parse", "HEAD").stdout.strip()
    print(f"source: {src} at {head[:12]}")
    return src


def read(path: str) -> str:
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def _eval_int_expr(text: str) -> int:
    """Evaluate a small integer expression: decimal ints, + - ( ) and nothing else.

    C item constants are written as sums of other constants, so this is all the
    arithmetic needed -- and it keeps the parse closed instead of evaluating text.
    Raises ValueError on anything outside that grammar.
    """
    if not re.fullmatch(r"[\d\s+\-()]*", text):
        raise ValueError(f"unexpected characters in {text!r}")
    tokens = re.findall(r"\d+|[()+\-]", text)
    pos = 0

    def primary() -> int:
        nonlocal pos
        if pos >= len(tokens):
            raise ValueError("expression ended early")
        tok = tokens[pos]
        if tok == "(":
            pos += 1
            val = additive()
            if pos >= len(tokens) or tokens[pos] != ")":
                raise ValueError("unbalanced parentheses")
            pos += 1
            return val
        if tok in "+-":
            pos += 1
            val = primary()
            return -val if tok == "-" else val
        if tok.isdigit():
            pos += 1
            return int(tok)
        raise ValueError(f"unexpected token {tok!r}")

    def additive() -> int:
        nonlocal pos
        val = primary()
        while pos < len(tokens) and tokens[pos] in "+-":
            op = tokens[pos]
            pos += 1
            rhs = primary()
            val = val + rhs if op == "+" else val - rhs
        return val

    val = additive()
    if pos != len(tokens):
        raise ValueError("trailing tokens")
    return val


def item_constants(src: str) -> dict[str, int]:
    """ITEM_* constants, including ones defined in terms of other constants."""
    defines: dict[str, str] = {}
    seen_dirs = []
    for rel in ("include/constants", "include"):
        d = os.path.join(src, rel)
        if not os.path.isdir(d) or d in seen_dirs:
            continue
        seen_dirs.append(d)
        for name in sorted(os.listdir(d)):
            if name.endswith(".h"):
                for m in re.finditer(
                    r"^\s*#\s*define\s+(ITEM_\w+)\s+([^\s/][^\n]*?)(?:\s*/\*.*\*/)?\s*$",
                    read(os.path.join(d, name)),
                    re.M,
                ):
                    defines.setdefault(m.group(1), m.group(2).strip())

    values: dict[str, int] = {}
    pending = dict(defines)
    for _ in range(10):  # constants may reference each other; iterate to a fixpoint
        progressed = False
        for key in list(pending):
            term = re.sub(r"ITEM_\w+", lambda m: str(values[m.group(0)]) if m.group(0) in values else "\x00", pending[key])
            if "\x00" in term:
                continue
            try:
                values[key] = _eval_int_expr(term)
            except ValueError:
                continue
            del pending[key]
            progressed = True
        if not progressed:
            break
    return values


ICON_LINE = re.compile(r"const\s+u32\s+(gItemIcon\w*)\s*\[\]\s*=\s*INCBIN_U32\(\"([^\"]+)\"\)")
TABLE_ROW = re.compile(r"\[\s*(ITEM_\w+)\s*\]\s*=\s*\{\s*(gItemIcon\w+)\s*,\s*(gItemIconPalette_\w+)\s*\}")
ROGUE_ROW = re.compile(r"\[\s*(ITEM_\w+)\s*-\s*ITEM_ROGUE_ITEM_FIRST\s*\]\s*=\s*\{(.*?)\}", re.S)
ROGUE_ICON = re.compile(r"\.iconImage\s*=\s*(gItemIcon\w+)[\s\S]*?\.iconPalette\s*=\s*(gItemIconPalette_\w+)")


def icon_files(src: str) -> dict[str, tuple[str, str | None]]:
    """gItemIcon_* symbol -> (source png, palette file or None)."""
    path = os.path.join(src, "src", "data", "graphics", "items.h")
    if not os.path.isfile(path):
        return {}
    out: dict[str, tuple[str, str | None]] = {}
    for name, inc in ICON_LINE.findall(read(path)):
        base, _, _ = inc.partition(".4bpp.lz")
        base, _, _ = base.partition(".gbapal.lz")
        stem = os.path.join(src, base)
        if name.startswith("gItemIconPalette_"):
            out[name] = (stem + ".pal", None)
        else:
            out[name] = (stem + ".png", stem + ".pal")
    # palettes usually live beside the graphics under icon_palettes/
    fixed = {}
    for name, (png, pal) in out.items():
        if pal and not os.path.isfile(pal):
            alt = os.path.join(os.path.dirname(png), "..", "icon_palettes", os.path.basename(png) + ".pal")
            if os.path.isfile(alt):
                pal = alt
            elif name.startswith("gItemIconPalette_"):
                png = alt
        fixed[name] = (png, pal)
    return fixed


def wiring(src: str, ids: dict[str, int], files: dict[str, tuple[str, str | None]]):
    """item id -> (icon symbol, palette symbol), from the game's own tables."""
    resolved: dict[int, tuple[str, str]] = {}
    unresolved = []
    for name in ("item_icon_table.h",):
        path = os.path.join(src, "src", "data", name)
        if not os.path.isfile(path):
            continue
        for const, icon, pal in TABLE_ROW.findall(read(path)):
            if const in ids and icon in files:
                resolved[ids[const]] = (icon, pal)
            else:
                unresolved.append(const)
    rogue = os.path.join(src, "src", "data", "rogue_items.h")
    if os.path.isfile(rogue):
        for const, body in ROGUE_ROW.findall(read(rogue)):
            m = ROGUE_ICON.search(body)
            if not m or const not in ids:
                continue
            icon, pal = m.group(1), m.group(2)
            if icon in files:
                resolved[ids[const]] = (icon, pal)
            else:
                unresolved.append(const)
    return resolved, unresolved


def compose(png_path: str, pal_path: str | None, out_path: str) -> bool:
    """Rebuild what the GBA draws from the game's PNG and palette file.

    A palette PNG's own colour ordering *is* the tile index order the GBA uses,
    and the hardware treats tile index 0 as transparent -- so transparency comes
    from index 0, not from the PNG's tRNS chunk (that is a PNG-side convenience
    the game's converter ignores). Trusting tRNS instead punches holes: interior
    pixels that happen to carry the keyed colour go transparent.

    The palette file supplies the colours for those indices. It sometimes carries
    one extra leading entry (a filler colour), which shifts the colour lookup by
    one while leaving index 0 as the transparent one.
    """
    try:
        w, h, pixels, ordered, index_plane = png_decode(open(png_path, "rb").read())
    except (OSError, ValueError) as exc:
        print(f"  {png_path}: {exc}", file=sys.stderr)
        return False
    cols = jasc_palette(pal_path) if pal_path else None
    if cols:
        cols = cols[:16]
    elif ordered:
        cols = ordered[:16]
    else:
        seen = []
        for px in pixels:
            rgb = px[:3]
            if rgb not in seen:
                seen.append(rgb)
        if len(seen) > 16:
            print(f"  {png_path}: {len(seen)} colours and no .pal to order them", file=sys.stderr)
            return False
        cols = seen

    if index_plane is None:
        # truecolour source: fall back to matching colours into the palette
        index_plane = []
        for r, g, b, _a in pixels:
            best, best_d = 0, None
            for i, c in enumerate(cols):
                d = sum((x - y) * (x - y) for x, y in zip((r, g, b), c))
                if best_d is None or d < best_d:
                    best, best_d = i, d
            index_plane.append(best)

    # a palette file with one more entry than the PNG leads with a filler colour
    shift = 1 if len(cols) == len(ordered or []) + 1 else 0

    out = []
    for j in index_plane:
        slot = j + shift if j + shift < len(cols) else j
        cr, cg, cb = cols[slot] if slot < len(cols) else (0, 0, 0)
        out.append((cr, cg, cb, 0 if j == 0 else 255))
    png_encode(out_path, w, h, out)
    return True


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--source", help="use this existing game source checkout instead of cloning")
    ap.add_argument("--repo", default=DEFAULT_REPO, help=f"source repository (default {DEFAULT_REPO})")
    ap.add_argument("--ref", default=DEFAULT_REF, help=f"branch, tag or commit (default {DEFAULT_REF})")
    ap.add_argument("--cache", default=None, help="where to clone (default: alongside this repo in .cache/er-source)")
    ap.add_argument("--refresh", action="store_true", help="re-fetch even if the checkout is already at --ref")
    ap.add_argument("--out", default="assets/icons/items", help="where to write <item id>.png")
    ap.add_argument("--keep-existing", action="store_true", help="do not overwrite icons that already exist")
    ap.add_argument("--prune-unreferenced", action="store_true",
                    help="delete <id>.png files in --out that this version of the game has no art for")
    ap.add_argument("--dry-run", action="store_true", help="report what would be written; write nothing")
    ap.add_argument("--verbose", action="store_true", help="one line per icon")
    args = ap.parse_args(argv)

    cache = args.cache or os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), ".cache", "er-source")
    src = args.source or ensure_source(cache, args.repo, args.ref, args.refresh)
    for needed in ("src/data/item_icon_table.h", "src/data/graphics/items.h"):
        if not os.path.isfile(os.path.join(src, needed)):
            print(
                f"error: {needed} is missing from {src}.\n"
                "That usually means the sparse checkout is incomplete -- re-run without --source, "
                "or point --source at a full Emerald Rogue checkout.",
                file=sys.stderr,
            )
            return 1

    ids = item_constants(src)
    files = icon_files(src)
    wired, unresolved = wiring(src, ids, files)
    if not wired:
        print("error: no item icons could be resolved from that checkout", file=sys.stderr)
        return 1
    if not args.dry_run:
        os.makedirs(args.out, exist_ok=True)

    written = kept = failed = 0
    missing_art = []
    top = max(ids.values()) if ids else 0
    for item_id in sorted(wired):
        icon_sym, pal_sym = wired[item_id]
        png, pal = files[icon_sym]
        if pal_sym and pal_sym in files:
            pal = files[pal_sym][0] if os.path.isfile(files[pal_sym][0]) else pal
        out_path = os.path.join(args.out, f"{item_id}.png")
        if os.path.exists(out_path) and args.keep_existing:
            kept += 1
            continue
        if args.dry_run:
            written += 1
            continue
        if not compose(png, pal if pal and os.path.isfile(pal) else None, out_path):
            failed += 1
            continue
        written += 1
        if args.verbose:
            print(f"  id {item_id}: {os.path.relpath(png, src)} -> {out_path}")
    for item_id in range(0, min(top, 1200) + 1):
        if item_id not in wired:
            missing_art.append(item_id)

    pruned = 0
    if args.prune_unreferenced and not args.dry_run:
        for entry in sorted(os.listdir(args.out)):
            if not entry.endswith(".png"):
                continue
            stem = entry[:-4]
            if stem.isdigit() and int(stem) not in wired:
                os.remove(os.path.join(args.out, entry))
                pruned += 1

    print(
        f"icons: {written} written, {kept} kept, {failed} failed, {pruned} pruned; "
        f"{len(missing_art)} ids with no art in this version (the editor draws a badge for those)"
    )
    if unresolved:
        print(f"  {len(unresolved)} table rows named a graphic this script could not resolve")
    if not args.dry_run:
        print(f"out: {os.path.abspath(args.out)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
