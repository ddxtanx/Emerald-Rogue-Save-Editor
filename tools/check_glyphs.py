#!/usr/bin/env python3
"""Report font coverage for every non-ASCII code point used in src/.

egui 0.27.2 bundles subsetted fonts; a character that is not in the font's
cmap draws as a tofu box. This tool parses the cmap subtables (formats 0, 4,
6 and 12) of the exact .ttf files egui/epaint 0.27.2 ships -- no fonttools,
pure stdlib -- and answers, for every non-ASCII character found in this
crate's src/, whether the PROPORTIONAL chain (Ubuntu-Light, NotoEmoji,
emoji-icon: what labels, buttons and body text use) and the MONOSPACE chain
(Hack, NotoEmoji, emoji-icon: what monospace text uses) cover it.

It is the tool that produced the coverage facts behind src/glyph_guard.rs's
allow-list. Future sessions must not re-guess font coverage from memory:
run this.

Exit status: 0 if every non-ASCII code point in src/ is covered by the
proportional chain, 1 if any is missing (that character renders as an empty
box in labels), 2 if the fonts or sources cannot be read.

Usage: python3 tools/check_glyphs.py [--fonts <epaint fonts dir>]
"""
import argparse
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.normpath(os.path.join(HERE, "..", "src"))

DEFAULT_FONTS = [
    os.path.normpath(os.path.join(
        HERE, "..", "..", ".cargo", "registry", "src",
        "index.crates.io-1949cf8c6b5b557f", "epaint-0.27.2", "fonts")),
    # overridable; --fonts or EPHAINT_FONTS
]

PROPORTIONAL = ["Ubuntu-Light.ttf", "NotoEmoji-Regular.ttf", "emoji-icon-font.ttf"]
MONOSPACE = ["Hack-Regular.ttf", "NotoEmoji-Regular.ttf", "emoji-icon-font.ttf"]


def find_cmap(data):
    """Locate the cmap table: sfnt directory -> (offset, length)."""
    num_tables = struct.unpack(">H", data[4:6])[0]
    for i in range(num_tables):
        ent = data[12 + 16 * i: 12 + 16 * (i + 1)]
        if len(ent) < 16:
            break
        tag = ent[:4]
        off, ln = struct.unpack(">II", ent[8:16])
        if tag == b"cmap":
            return off, ln
    raise ValueError("no cmap table")


def cmap_subtables(data, off, ln):
    """All (format, ABSOLUTE subtable offset) pairs in the cmap.

    The encoding-record offsets are relative to the START OF THE CMAP TABLE,
    not to the start of the file: reading them as file offsets (the first
    version of this tool did exactly that) silently reports wrong coverage.
    """
    ver, n = struct.unpack(">HH", data[off:off + 4])
    seen = set()
    out = []
    for i in range(n):
        rec = data[off + 4 + 8 * i: off + 12 + 8 * i]
        if len(rec) < 8:
            break
        _plat, _enc, sub = struct.unpack(">HHI", rec)
        abs_sub = off + sub
        fmt = struct.unpack(">H", data[abs_sub:abs_sub + 2])[0] if abs_sub + 2 <= len(data) else 0xff
        if (fmt, abs_sub) in seen:
            continue
        seen.add((fmt, abs_sub))
        out.append((fmt, abs_sub))
    return out


def codepoints_format0(data, sub, end):
    length = struct.unpack(">H", data[sub + 2:sub + 4])[0]
    tbl = data[sub:sub + length]
    return {i for i, b in enumerate(tbl[6:6 + 256]) if b != 0}


def codepoints_format4(data, sub, end):
    seg2 = struct.unpack(">H", data[sub + 6:sub + 8])[0]
    seg = seg2 // 2
    o = sub + 14
    ends = struct.unpack(f">{seg}H", data[o:o + seg2])
    o += seg2 + 2  # reservedPad
    starts = struct.unpack(f">{seg}H", data[o:o + seg2])
    o += seg2
    deltas = struct.unpack(f">{seg}h", data[o:o + seg2])
    o += seg2
    ranges_start = o
    ranges = struct.unpack(f">{seg}H", data[o:o + seg2])
    out = set()
    for i in range(seg):
        if starts[i] == 0xFFFF:
            continue
        # The spec defines the glyph id for c as the u16 found at
        #   &idRangeOffset[i] + idRangeOffset[i] + 2*(c - startCode)
        # i.e. based at THIS segment's idRangeOffset entry, not at the end of
        # the whole array (basing it on the post-array offset silently drops
        # most of the coverage).
        seg_glyph_base = ranges_start + 2 * i + ranges[i]
        for c in range(starts[i], min(ends[i], 0xFFFF) + 1):
            if ranges[i] == 0:
                g = (c + deltas[i]) & 0xFFFF
                if g != 0:
                    out.add(c)
            else:
                gi = seg_glyph_base + 2 * (c - starts[i])
                if gi + 2 <= len(data):
                    g = struct.unpack(">H", data[gi:gi + 2])[0]
                    if g != 0:
                        out.add(c)
    return out


def codepoints_format6(data, sub, end):
    first, cnt = struct.unpack(">HH", data[sub + 6:sub + 10])
    out = set()
    for i in range(cnt):
        gi = sub + 10 + 2 * i  # format 6's glyphIdArray is u16, not u8
        if gi + 2 <= len(data) and struct.unpack(">H", data[gi:gi + 2])[0] != 0:
            out.add(first + i)
    return out


def codepoints_format12(data, sub, end):
    ngroups = struct.unpack(">I", data[sub + 12:sub + 16])[0]
    out = set()
    o = sub + 16
    for _ in range(ngroups):
        s_c, e_c, g = struct.unpack(">III", data[o:o + 12])
        o += 12
        # the glyph id is implicit: every char in the range maps (id != 0 for
        # a mapped range; ranges present in a cmap subtable ARE the coverage).
        out.update(range(s_c, e_c + 1))
    return out


PARSERS = {0: codepoints_format0, 4: codepoints_format4,
           6: codepoints_format6, 12: codepoints_format12}


def font_codepoints(path):
    data = open(path, "rb").read()
    off, ln = find_cmap(data)
    cps = set()
    for fmt, sub in cmap_subtables(data, off, ln):
        p = PARSERS.get(fmt)
        if p is None:
            continue  # formats we do not parse cannot add coverage claims
        cps |= p(data, sub, off + ln)
    return cps


def chain_codepoints(fonts_dir, names):
    cps = set()
    missing = []
    for n in names:
        path = os.path.join(fonts_dir, n)
        if not os.path.exists(path):
            missing.append(path)
            continue
        cps |= font_codepoints(path)
    return cps, missing


def used_non_ascii(src_dir):
    chars = {}
    for root, _dirs, files in os.walk(src_dir):
        for fn in sorted(files):
            if not fn.endswith(".rs"):
                continue
            p = os.path.join(root, fn)
            text = open(p, encoding="utf-8", errors="replace").read()
            for ch in text:
                if ord(ch) > 127:
                    chars.setdefault(ch, []).append(p)
    return chars


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fonts", default=None,
                    help="epaint 0.27.2 fonts directory (contains the four .ttf files)")
    ap.add_argument("--codepoints", default=None,
                    help="comma-separated code points to query (0x2642, U+2205, 9733) "
                         "instead of scanning src/ -- for choosing a replacement glyph")
    args = ap.parse_args()
    fonts_dir = None
    for cand in ([args.fonts] if args.fonts else []) + \
            [os.environ.get("EPHAINT_FONTS", "")] + DEFAULT_FONTS:
        if cand and os.path.isdir(cand):
            fonts_dir = cand
            break
    if fonts_dir is None:
        print("cannot locate the epaint 0.27.2 fonts directory; pass --fonts", file=sys.stderr)
        return 2
    if not os.path.isdir(SRC):
        print(f"cannot read sources at {SRC}", file=sys.stderr)
        return 2

    prop, miss_p = chain_codepoints(fonts_dir, PROPORTIONAL)
    mono, miss_m = chain_codepoints(fonts_dir, MONOSPACE)
    print(f"fonts: {fonts_dir}")
    for m in sorted(set(miss_p) | set(miss_m)):
        print(f"MISSING FONT: {m}", file=sys.stderr)
    if miss_p or miss_m:
        return 2
    print(f"proportional chain {' + '.join(PROPORTIONAL)}: {len(prop)} code points")
    print(f"monospace chain    {' + '.join(MONOSPACE)}: {len(mono)} code points")
    print()

    if args.codepoints:
        wanted = []
        for tok in args.codepoints.replace("U+", "0x").split(","):
            tok = tok.strip()
            if tok:
                wanted.append(int(tok, 0))
        per_font = {n: font_codepoints(os.path.join(fonts_dir, n))
                    for n in sorted(set(PROPORTIONAL) | set(MONOSPACE))}
        print(f"{'codepoint':>10}  {'char':<4}  prop  mono  first font providing it")
        for cp in wanted:
            src = next((n for n in PROPORTIONAL if cp in per_font[n]), "-")
            srcm = next((n for n in MONOSPACE if cp in per_font[n]), "-")
            ch = chr(cp) if cp >= 0x20 else "?"
            print(f"  U+{cp:04X}  {ch:<4}  {'yes' if cp in prop else 'NO ':<4} "
                  f"{'yes' if cp in mono else 'NO ':<4}  prop={src} mono={srcm}")
        return 0

    used = used_non_ascii(SRC)
    bad = 0
    print(f"{'codepoint':>10}  {'char':<4}  prop  mono  uses")
    for ch in sorted(used, key=lambda c: ord(c)):
        cp = ord(ch)  # the chains are sets of code points (ints), not chars
        p, m = cp in prop, cp in mono
        bad += 0 if p else 1
        print(f"  U+{cp:04X}  {ch:<4}  {'yes' if p else 'NO ':<4} "
              f"{'yes' if m else 'NO ':<4}  {len(used[ch])} file(s)")
    print()
    if bad:
        print(f"FAIL: {bad} code point(s) used in src/ are NOT covered by the "
              f"proportional chain and render as tofu boxes.")
        return 1
    print("OK: every non-ASCII code point in src/ is covered by the proportional chain.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
