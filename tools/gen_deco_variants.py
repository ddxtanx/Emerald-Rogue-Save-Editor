#!/usr/bin/env python3
"""Reproduce the DECOR_VARIANT_* enum the game's own DecorationExporter emits.

The variant ids stored in `homeDecorations[].decorVariant` are indices into a
generated enum (tools/Pokabbie/Build/CustomJson/DecorationExporter.cpp), so they
are not readable from the save alone. This walks the same JSON inputs in the
same order and prints `id\tNAME` for every ungated variant.

Order rules replicated from the C++:
  * ExpandCommonArrayGroup: inline groups first, then includes, each included
    group keyed with a zero-padded counter (so include order survives).
  * nlohmann::json objects are std::map -> keys iterate SORTED, so group keys are
    visited in lexicographic order at every level.
  * Decorations are visited in array order; each contributes one entry per
    `variants` element, or exactly one ("Default") when it has no variants.
  * A decoration with "#if" compiles in/out entirely; gated entries do not take
    enum slots when the condition is false.
  * Duplicate pretty names get "_1", "_2", ... suffixes.
"""

import json
import os
import re
import sys

ROOT = sys.argv[1] if len(sys.argv) > 1 else "."
DATA = os.path.join(ROOT, "src/data/rogue/decorations.json")

ELIPSIS = "\u2026"


def format_unique_id(name: str) -> str:
    s = name.upper()
    for a, b in [
        (" ", "_"),
        ("-", "_"),
        ("é", "E"),
        ("!", "EMARK"),
        ("?", "QMARK"),
        (",", ""),
        (".", ""),
        ('"', ""),
        ("\\", ""),
        ("/", ""),
        ("[", ""),
        ("]", ""),
        ("{", ""),
        ("}", ""),
        ("(", ""),
        (")", ""),
        ("'", ""),
        ("+", "PLUS"),
        ("#", "_"),
        (ELIPSIS, ""),
    ]:
        s = s.replace(a, b)
    return s


def read_json(path):
    with open(path, encoding="utf-8-sig") as fh:
        return json.load(fh)


def expand(path, raw, group, counter):
    """ExpandCommonArrayGroupInternal, preserving nlohmann sorted-key order."""
    out = {}
    condition = raw.get("condition", "")
    for gname, groups in (raw.get(group) or {}).items():
        key = ("#if %s // " % condition) + gname if condition else gname
        bucket = out.setdefault(key, [])
        for source_group in groups:
            dest = {}
            if "defaults" in raw:
                dest.update(raw["defaults"])
            dest.update(source_group)
            bucket.append(dest)
    for inc in raw.get("includes") or []:
        full = os.path.normpath(os.path.join(os.path.dirname(path), inc))
        inner = expand(full, read_json(full), group, counter)
        for src_name, groups in inner[group].items():
            prefix = "%06d_" % counter[0]
            counter[0] += 1
            out[prefix + src_name + " // [" + full + "]"] = groups
    return {group: out}


# --- config defines, so #if conditions can be evaluated ---------------------
def load_defines():
    defines = {}
    for dirpath, _dirs, files in os.walk(os.path.join(ROOT, "include")):
        for fn in files:
            if not fn.endswith(".h"):
                continue
            p = os.path.join(dirpath, fn)
            try:
                txt = open(p, encoding="utf-8-sig", errors="replace").read()
            except OSError:
                continue
            for m in re.finditer(r"^\s*#define\s+([A-Za-z_]\w*)(?:\s+([^\n/]*?))?\s*$", txt, re.M):
                k, v = m.group(1), (m.group(2) or "1").strip()
                defines.setdefault(k, v)
    return defines


DEFINES = load_defines()
TRUE, FALSE = "1", "0"


def resolve(expr, depth=0):
    expr = expr.strip()
    if depth > 12:
        raise RuntimeError("define expansion too deep: %r" % expr)
    out, i = [], 0
    while i < len(expr):
        m = re.match(r"[A-Za-z_]\w*", expr[i:])
        if m:
            tok = m.group(0)
            if tok in ("defined",):
                out.append(tok)
                i += len(tok)
                continue
            if tok in DEFINES:
                out.append("(%s)" % resolve(DEFINES[tok], depth + 1))
            else:
                out.append("0")  # preprocessor: undefined identifier -> 0
            i += len(tok)
            continue
        m = re.match(r"\d+", expr[i:])
        if m:
            out.append(m.group(0))
            i += len(m.group(0))
            continue
        out.append(expr[i])
        i += 1
    return "".join(out)


def cond_true(cond):
    try:
        expr = resolve(cond)
        expr = re.sub(r"defined\s*\(?(\w+)\)?", lambda m: "1" if m.group(1) in DEFINES else "0", expr)
        expr = expr.replace("||", " or ").replace("&&", " and ")
        expr = re.sub(r"(?<![<>!])=(?!=)", "==", expr)
        return bool(eval(expr, {"__builtins__": {}}, {}))
    except Exception as exc:  # noqa: BLE001
        print("!! cannot evaluate %r: %s" % (cond, exc), file=sys.stderr)
        return False


def main():
    raw = read_json(DATA)
    expanded = expand(DATA, raw, "decorations", [0])[group_key]
    name_count = {}
    variants = []
    skipped = []
    for gkey in sorted(expanded):
        for decor in expanded[gkey]:
            uid = format_unique_id(decor["name"])
            if uid not in name_count:
                name_count[uid] = 1
            else:
                suffix = name_count[uid]
                name_count[uid] += 1
                uid += "_%d" % suffix
            cond = decor.get("#if")
            active = cond is None or cond_true(cond)
            if decor.get("variants"):
                vs = [v.get("name", "Unnamed") for v in decor["variants"]]
            else:
                vs = ["Default"]
            for v in vs:
                label = "DECOR_VARIANT_%s_%s" % (uid, format_unique_id(v))
                if active:
                    variants.append(label)
                else:
                    skipped.append((cond, label))

    if "--rust" in sys.argv:
        out = sys.argv[sys.argv.index("--rust") + 1]
        with open(out, "w") as fh:
            fh.write(
                "// Generated from %s by tools/gen_deco_variants.py. Do not edit.\n"
                "// Index == the value stored in homeDecorations[].decorVariant; the enum\n"
                "// comes from DecorationExporter.cpp, which is why the ids are not readable\n"
                "// from the save alone. Cross-check: id 119 is the wardrobe the game seeds in\n"
                "// RogueHub_ClearProgress, and a real v2.2.0 save carries exactly that.\n"
                "\n"
                "pub const DECOR_VARIANTS: &[&str] = &[\n" % DATA
            )
            for label in variants:
                pretty = label[len("DECOR_VARIANT_"):].lower().replace("_", " ")
                fh.write('    "%s",\n' % pretty)
            fh.write("];\n\n")
            fh.write(
                "// The raw enum names, index-aligned with DECOR_VARIANTS, for cross-referencing\n"
                "// the game's generated sDecorationVariants table.\n"
                "pub const DECOR_VARIANT_NAMES: &[&str] = &[\n"
            )
            for label in variants:
                fh.write('    "%s",\n' % label)
            fh.write("];\n")
            fh.write("\n// The 'remove decoration' menu action, never stored in the save.\n")
            fh.write(
                "pub const DECOR_VARIANT_REMOVE: usize = %d;\n"
                % variants.index("DECOR_VARIANT_REMOVE_DECOR_DEFAULT")
            )
        print("# wrote %s (%d variants)" % (out, len(variants)), file=sys.stderr)
        return

    for i, label in enumerate(variants):
        print("%d\t%s" % (i, label))
    print("# DECOR_VARIANT_COUNT = %d" % len(variants), file=sys.stderr)
    for cond, label in skipped:
        print("# gated out (%s): %s" % (cond, label), file=sys.stderr)


group_key = "decorations"
if __name__ == "__main__":
    main()
