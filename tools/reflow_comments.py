#!/usr/bin/env python3
"""Reflow comment-only lines to the Rust style guide's comment margin.

The guide (doc.rust-lang.org/style-guide, "Comments"): a source line that is
entirely a comment must stay within 80 characters of comment (sigils included,
indentation excluded), or the 100-character line limit, whichever is smaller.
rustfmt does not rewrap comments (`wrap_comments` is unstable), so this is the
mechanical part of a style pass that has to be done by hand or by script.

Conservative by construction:

  * only own-line comments are touched; trailing comments stay where they are;
  * a run is skipped if it contains box-drawing or aligned text (layout notes
    about save offsets are the project's most valuable comments and reflowing
    them would destroy the alignment);
  * doc-comment code examples (4-space indented after the sigil, or inside a
    ``` fence) and markdown bullets are never merged into prose. An over-long
    bullet is wrapped with a hanging indent instead;
  * indentation is never changed, only the break points inside the text.

Usage: python3 tools/reflow_comments.py [--check] [paths...]
"""
import re
import sys

BOX = "│└┌─├→←↑↓═╗╚╔╣┼╬─▌░"
CODE_FENCE = "```"


def sigil(line):
    s = line.strip()
    for sig in ("//!", "///", "//"):
        if s.startswith(sig):
            return sig
    return None


def body_of(line, sig):
    """The comment text after the sigil, with exactly one leading space removed."""
    i = line.index(sig) + len(sig)
    rest = line[i:]
    if rest.startswith(" "):
        rest = rest[1:]
    return rest


def is_table_or_diagram(body):
    if any(c in body for c in BOX):
        return True
    # Aligned columns: two or more spaces separating non-space runs, a common
    # shape in the offset tables.
    return bool(re.search(r"\S {2,}\S", body))


def is_code_example(body):
    return body.startswith("    ") or body.startswith("\t")


def is_bullet(body):
    return bool(re.match(r"[-*+]\s|^\d+[.)]\s", body))


def wrap(text, width):
    words = text.split()
    out, cur = [], ""
    for w in words:
        if cur and len(cur) + 1 + len(w) > width:
            out.append(cur)
            cur = w
        else:
            cur = f"{cur} {w}".strip()
    if cur:
        out.append(cur)
    return out


def limit(indent):
    """Comment budget for a line at `indent`: sigils+text <= 80, total <= 100."""
    return min(80, 100 - indent)


def process(lines):
    out = []
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i]
        sig = sigil(line)
        if sig is None:
            out.append(line)
            i += 1
            continue
        indent = len(line) - len(line.lstrip())
        # Collect the run of same-sigil, same-indent own-line comments.
        run = [line]
        j = i + 1
        while j < n:
            s2 = sigil(lines[j])
            ind2 = len(lines[j]) - len(lines[j].lstrip())
            if s2 == sig and ind2 == indent and lines[j].strip():
                run.append(lines[j])
                j += 1
            else:
                break
        bodies = [body_of(r, sig) for r in run]
        fenced = False
        new_run = []
        for b in bodies:
            if b.strip().startswith(CODE_FENCE):
                fenced = not fenced
                new_run.append((b, "keep"))
                continue
            if fenced or is_code_example(b):
                new_run.append((b, "keep"))
                continue
            if not b.strip():
                new_run.append((b, "blank"))
                continue
            if is_table_or_diagram(b):
                new_run.append((b, "keep"))
                continue
            if is_bullet(b):
                new_run.append((b, "bullet"))
                continue
            new_run.append((b, "prose"))

        # Merge consecutive prose entries and re-wrap; keep everything else as is.
        emitted = []
        k = 0
        while k < len(new_run):
            kind = new_run[k][1]
            if kind == "prose":
                text = ""
                while k < len(new_run) and new_run[k][1] == "prose":
                    piece = new_run[k][0].strip()
                    text = f"{text} {piece}".strip()
                    k += 1
                budget = limit(indent) - len(sig) - 1
                for w in wrap(text, budget):
                    emitted.append(f"{' ' * indent}{sig} {w}")
            elif kind == "blank":
                # A blank comment line is the author's paragraph break; it ends a
                # prose run rather than joining across it.
                emitted.append(f"{' ' * indent}{sig}")
                k += 1
            elif kind == "bullet":
                body = new_run[k][0]
                k += 1
                budget = limit(indent) - len(sig) - 1
                if len(body) > budget:
                    words = wrap(body, budget)
                    emitted.append(f"{' ' * indent}{sig} {words[0]}")
                    cont = " " * (len(sig) + 1)
                    for w in words[1:]:
                        emitted.append(f"{' ' * indent}{sig}{cont}{w}")
                else:
                    emitted.append(f"{' ' * indent}{sig} {body.rstrip()}")
            else:
                body = new_run[k][0]
                k += 1
                emitted.append(f"{' ' * indent}{sig} {body.rstrip()}" if body.strip() else f"{' ' * indent}{sig}")
        out.extend(emitted)
        i = j
    return out


def main():
    check = "--check" in sys.argv
    paths = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not paths:
        import os

        paths = []
        for root, _dirs, fs in os.walk("src"):
            paths += [os.path.join(root, f) for f in fs if f.endswith(".rs")]
    changed = 0
    for p in sorted(paths):
        src = open(p).read()
        lines = src.split("\n")
        new = process(lines)
        # A line that was already within budget and unbroken stays byte-identical.
        if new != lines:
            over = sum(
                1
                for l in new
                if sigil(l)
                and len(l) - (len(l) - len(l.lstrip())) + 0 > 0
                and len(l.lstrip()) > min(80, 100 - (len(l) - len(l.lstrip())))
            )
            if check:
                print(f"{p}: would reflow ({over} comment lines still over margin)")
            else:
                open(p, "w").write("\n".join(new))
                print(f"{p}: reflowed ({over} over-margin lines remain)")
            changed += 1
    print(f"files touched: {changed}")


if __name__ == "__main__":
    main()
