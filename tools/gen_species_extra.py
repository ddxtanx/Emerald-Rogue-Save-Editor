#!/usr/bin/env python3
"""Regenerate SPECIES_EXTRA (Rogue form ids above the base dex) in src/tables_extra.rs.

Why this exists separately from gen_tables_v2.py: that generator's id resolver only
understood `NAME +- N`, so 141 species macros never resolved (the Scatterbug/Mothim
cloaks, Shadow Lugia, and the whole tail of the form block), and NUM_SPECIES resolved
to nothing -- leaving ids 1434..1573 with no name, which the editor rendered as
"??????". Its name scan then took a text segment up to the next "\\n    [" and grabbed
the first .speciesName in it, so a block containing nested initializers handed the
NEXT species' name to this id: id 990 (SPECIES_DARMANITAN_GALARIAN) printed "Maractus",
954 printed "Groudon" for Primal Kyogre, 1023 printed "Pikachu" for Spiky-Eared Pichu.

Here the ids come from a full arithmetic resolver over the #define chain, and the names
come from brace-balanced designated initializers. Only the SPECIES_EXTRA table is
rewritten; ITEMS_EXTRA and QUEST_NAMES in the same file are left byte-identical.

Usage: python3 tools/gen_species_extra.py [--source <pokeemerald-rogue checkout>]
"""
import argparse
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_SOURCE = os.path.join(os.path.dirname(REPO), "rogue-src")
OUT = os.path.join(REPO, "src", "tables_extra.rs")
BASE_DEX_MAX = 905  # FORMS_START in include/constants/species.h


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", default=DEFAULT_SOURCE)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    src = a.source
    if not os.path.isdir(src):
        sys.exit(f"source checkout not found: {src}")

    def read(rel):
        with open(os.path.join(src, rel), encoding="utf-8", errors="replace") as f:
            return f.read()

    # ---------- ids: resolve the whole #define chain, arithmetic included ----------
    spec_h = read("include/constants/species.h")
    defs = {}
    for line in spec_h.splitlines():
        m = re.match(
            r"\s*#define\s+([A-Za-z_][A-Za-z_0-9]*)\s+(.+?)\s*(?://.*)?$", line.strip()
        )
        if not m:
            continue
        name, val = m.group(1), m.group(2).strip()
        if val.startswith("(") and val.endswith(")"):
            val = val[1:-1].strip()
        defs.setdefault(name, val)

    memo, resolving = {}, set()

    def arith(s):
        """Tiny recursive-descent evaluator for `int + - * / ( )` only: the header
        chains use nothing else, and no eval() runs over repository text."""
        toks = re.findall(r"\d+|[-+*/()]", s)  # whitespace dropped: peek() must see operators
        i = 0

        def peek():
            return toks[i] if i < len(toks) else None

        def num():
            nonlocal i
            t = peek()
            if t == "(":
                i += 1
                v = expr()
                if peek() == ")":
                    i += 1
                return v
            if t and t.isdigit():
                i += 1
                return int(t)
            raise ValueError("bad token " + str(t))

        def term():
            nonlocal i
            v = num()
            while peek() in ("*", "/"):
                op = toks[i]
                i += 1
                rhs = num()
                v = v * rhs if op == "*" else v // rhs
            return v

        def expr():
            nonlocal i
            v = term()
            while peek() in ("+", "-"):
                op = toks[i]
                i += 1
                rhs = term()
                v = v + rhs if op == "+" else v - rhs
            return v

        return expr() if toks else None


    def resolve(name):
        """Integer value of a macro chain: ints, identifiers, + - * / ( )."""
        if name in memo:
            return memo[name]
        if name in resolving or name not in defs:
            return None
        resolving.add(name)
        expr = defs[name]
        v = None
        if re.fullmatch(r"\d+", expr):
            v = int(expr)
        else:
            e, ok = expr, True
            for tok in sorted(
                set(re.findall(r"[A-Za-z_][A-Za-z_0-9]*", e)), key=len, reverse=True
            ):
                rv = resolve(tok)
                if rv is None:
                    ok = False
                    break
                e = re.sub(r"\b%s\b" % re.escape(tok), str(rv), e)
            if ok and re.fullmatch(r"[\d\s+\-*/()%]+", e or ""):
                v = arith(e)
        resolving.discard(name)
        memo[name] = v
        return v

    ids = {n: resolve(n) for n in defs if n.startswith("SPECIES_")}
    unresolved = sorted(n for n, v in ids.items() if v is None)
    ids = {k: v for k, v in ids.items() if v is not None}
    num_species = resolve("NUM_SPECIES") or max(ids.values()) + 1
    forms_start = resolve("FORMS_START") or BASE_DEX_MAX

    # ---------- names: brace-balanced [SPECIES_X] = { ... } blocks ----------
    def block_at(text, start):
        i = text.find("{", start)
        if i < 0:
            return ""
        depth, j, n = 0, i, len(text)
        while j < n:
            c = text[j]
            if c == '"':
                j += 1
                while j < n and text[j] != '"':
                    j += 2 if text[j] == "\\" else 1
            elif c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    return text[i : j + 1]
            j += 1
        return text[i:]

    def macro_body(text, name):
        """Body of an object-like #define, joining backslash continuations."""
        m = re.search(r"^[ \t]*#define[ \t]+%s\b" % re.escape(name), text, re.M)
        if not m:
            return ""
        out, i = [], m.end()
        while i < len(text):
            nl = text.find("\n", i)
            if nl < 0:
                out.append(text[i:])
                break
            line = text[i:nl]
            cont = line.rstrip().endswith("\\")
            out.append(line.rstrip()[:-1] if cont else line)
            if not cont:
                break
            i = nl + 1
        return " ".join(out)

    NAME_RE = re.compile(r'\.speciesName\s*=\s*_\("([^"]*)"\)')
    base_name = {}  # SPECIES_* macro -> the string the game displays
    for gen in range(1, 12):
        try:
            t = read(f"src/data/pokemon/species_info/gen_{gen}.h")
        except (FileNotFoundError, OSError):
            continue
        # shared *_MISC_INFO macros carry the name for whole families (Arceus,
        # Genesect, Xerneas, Zygarde, Meloetta ...), so expand them by name lookup
        for m in re.finditer(r"^[ \t]*#define[ \t]+([A-Z0-9_]*INFO[A-Z0-9_]*)\b", t, re.M):
            found = NAME_RE.search(macro_body(t, m.group(1)))
            if found:
                base_name.setdefault(m.group(1), found.group(1))
        for m in re.finditer(r"\[(SPECIES_\w+)\]\s*=", t):
            key = m.group(1)
            if key in base_name:
                continue
            tail = t[m.end() : m.end() + 400]
            if not tail.lstrip().startswith("{"):
                # Some entries delegate to a macro instead of a brace block
                # (`[SPECIES_MINIOR_ORANGE] = MINIOR_ORANGE_INFO,`). Falling through
                # to block_at() here would read the NEXT entry's block and name this
                # one after an unrelated species (Minior came out as "Komala").
                mm = re.match(r"\s*([A-Z0-9_]+)", tail)
                if mm:
                    nm2 = mm.group(1)
                    if nm2 in base_name:
                        base_name[key] = base_name[nm2]
                    else:
                        found = NAME_RE.search(macro_body(t, nm2))
                        if found:
                            base_name[key] = found.group(1)
                continue
            blk = block_at(t, m.end())
            found = NAME_RE.search(blk)
            if found and found.group(1).strip("?"):
                base_name[key] = found.group(1)
            else:
                for mi in re.findall(r"\b([A-Z0-9_]*INFO[A-Z0-9_]*)\b", blk):
                    if mi in base_name:
                        base_name[key] = base_name[mi]
                        break

    def prettify(mac):
        s = mac[len("SPECIES_"):] if mac.startswith("SPECIES_") else mac
        return s.replace("_", " ").title()

    SUFFIXES = ["_MEGA_X", "_MEGA_Y", "_MEGA_Z", "_MEGA", "_GIGANTAMAX"]

    def split_tag(mac):
        for suf in SUFFIXES:
            if mac.endswith(suf):
                tag = suf.lstrip("_").replace("_", " ").title()
                if suf == "_MEGA_Z":
                    tag = "Mega-Z"
                return mac[: -len(suf)], tag
        return mac, None

    # One number can carry several symbols (83 of them here, e.g. SPECIES_CASTFORM and
    # SPECIES_CASTFORM_NORMAL, or SPECIES_DEOXYS / SPECIES_DEOXYS_NORMAL). Pick the
    # canonical symbol: whichever one the game initializes in species_info, else the
    # first in species.h order. Picking the last would rename a real species (id 1163
    # came out "Yveltal" from a form alias whose canonical entry is Xerneas).
    order = {n: i for i, n in enumerate(defs)}
    by_val = {}
    for mac, v in ids.items():
        by_val.setdefault(v, []).append(mac)
    canonical = {}
    for v, macs in by_val.items():
        known = [m for m in macs if m in base_name]
        pool = known or macs
        canonical[v] = sorted(pool, key=lambda m: (m not in base_name, order.get(m, 1 << 30)))[0]

    extra = {}
    for v, mac in sorted(canonical.items()):
        if not (forms_start < v < num_species):
            continue
        # Sentinels, not species. Anchored on purpose: a substring test matched
        # SPECIES_EXEGGUTOR_ALOLAN ("EGG" inside "EXEGGUTOR") and dropped a real id.
        stem_body = mac[len("SPECIES_"):]
        if stem_body in ("NONE", "EGG") or stem_body.startswith("DEX_"):
            continue
        stem, tag = split_tag(mac)
        name = f"{base_name.get(stem) or prettify(stem)} ({tag})" if tag else base_name.get(stem) or prettify(stem)
        if not tag:
            # This ROM names form entries after their BASE species (Alolan Sandshrew's
            # block says just "Sandshrew"), losing the qualifier the id encodes. Add
            # the words the macro carries and the name does not.
            words = mac[len("SPECIES_"):].split("_")
            for cut in range(len(words) - 1, 0, -1):
                cand = "SPECIES_" + "_".join(words[:cut])
                if cand in base_name:
                    rest = [w for w in words[cut:] if w and w.title() not in name]
                    if rest:
                        name = f"{base_name[cand]} ({' '.join(w.title() for w in rest)})"
                    break
        extra[v] = name

    def ruststr(s):
        return s.replace("\\", "\\\\").replace('"', '\\"')

    body = "".join(f'    ({k}, "{ruststr(extra[k])}"),\n' for k in sorted(extra))
    new_block = f"pub const SPECIES_EXTRA: &[(u16, &str)] = &[\n{body}];"

    cur = open(OUT, encoding="utf-8").read()
    start = cur.index("pub const SPECIES_EXTRA:")
    end = cur.index("];", start) + 2
    updated = cur[:start] + new_block + cur[end:]
    print(
        f"source={src}\nnum_species={num_species} forms_start={forms_start} "
        f"unresolved_macros={len(unresolved)} extra_ids={len(extra)} "
        f"range={min(extra)}-{max(extra)} names_from_species_info="
        f"{sum(1 for k in extra if extra[k])}"
    )
    if unresolved:
        print("first unresolved macros:", unresolved[:10])
    if a.dry_run:
        print(f"dry run: would write {len(new_block)} chars to {OUT}")
        return
    with open(OUT, "w", encoding="utf-8") as f:
    # The table this script used to emit is CSV now (data/species_names_extra.csv), parsed at start-up by
    # src/tables_extra.rs; re-running this generator would replace that loader with literals
    # and break every accessor. Add a CSV emitter before removing this stop.
    raise SystemExit(
        "SUPERSEDED: SPECIES_EXTRA now lives in data/species_names_extra.csv.\n"
        "Edit that file, or port this generator to write it, before re-running."
    )
        f.write(updated)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
