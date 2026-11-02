#!/usr/bin/env python3
"""Fix rustdoc list-item lints clippy raises on this crate's doc comments.

Two lints, one root cause. `doc_overindented_list_items` and
`doc_lazy_continuation` both come from how continuation lines sit relative to a
bullet in a `///` / `//!` block:

    /// - one bullet
    ///     continued at four spaces      <- overindented (rustdoc wants 2)
    /// a flush paragraph right after     <- lazy continuation: rustdoc folds
    ///                                    this into the bullet's text

`tools/reflow_comments.py` used a bullet hanging indent wider than rustdoc's, so
its output tripped both. This script rewrites only the comment lines:

  * a continuation indented deeper than the bullet's text column moves to that
    column (clippy's own suggestion);
  * a non-bullet line flush against the marker, following a bullet group, starts
    a new paragraph instead of being swallowed by the last bullet - it gets a
    blank doc line before it.

Nothing outside comments is touched; run it, then prove that with
`--prove-code-unchanged`, which strips comment and blank lines and compares the
code to the same files at HEAD.
"""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = re.compile(r"^(\s*)(//[/!]{1,2})(.*)$")
BULLET = re.compile(r"^(\s*)([-*+])\s+")


def fix_block(lines, path_holder, flagged):
    """Fix one contiguous doc-comment block; return the new lines."""
    out = []
    text_col = None  # column bullet continuations should align to
    for i, line in enumerate(lines):
        m = DOC.match(line)
        if not m:
            out.append(line)
            continue
        indent, marker, rest = m.groups()
        content = rest[1:] if rest.startswith(" ") else rest
        stripped = content.rstrip()
        base = len(indent) + len(marker)

        # blank doc line: closes the bullet group
        if not stripped.strip():
            text_col = None
            out.append(line)
            continue

        b = BULLET.match(content)
        if b:
            bindent = len(b.group(1))
            text_col = base + bindent + 2
            out.append(line)
            continue

        cur_indent = len(content) - len(content.lstrip())
        if text_col is not None and cur_indent > text_col:
            # overindented continuation -> align with the bullet text
            out.append(" " * text_col + content.lstrip())
            continue
        if text_col is not None and cur_indent == 0:
            # Flush text after a bullet group is a lazy continuation. It is NOT
            # safe to insert a blank line here: this script cannot tell a new
            # paragraph from a sentence that simply wrapped, and an early version
            # split sentences mid-line ("The game's" / "`RogueHub_BuildArea()`
            # writes both"). Flag it for a human instead.
            flagged.append((path_holder["p"], len(out) + 1, "flush text after a bullet"))
            out.append(line)
            continue
        out.append(line)
    return out


def fix_file(path):
    text = path.read_text()
    out = []
    block = []
    flagged: list = []
    path_holder: dict = {"p": str(path)}
    for line in text.split("\n"):
        if DOC.match(line):
            block.append(line)
        else:
            if block:
                out.extend(fix_block(block, {"p": str(path)}, flagged))
                block = []
            out.append(line)
    if block:
        out.extend(fix_block(block, {"p": str(path)}, flagged))
    if flagged:
        for f in flagged:
            print('  flag:', f)
    new = "\n".join(out)
    if new != text:
        path.write_text(new)
        return True
    return False


def code_only(text):
    """Lines that are not comments and not blank."""
    keep = []
    for line in text.split("\n"):
        s = line.strip()
        if not s or s.startswith("//"):
            continue
        keep.append(line.rstrip())
    return "\n".join(keep)


def main():
    args = sys.argv[1:]
    prove = "--prove-code-unchanged" in args
    files = [p for p in (ROOT / "src").rglob("*.rs")]
    if prove:
        bad = []
        head = subprocess.run(
            ["git", "-C", str(ROOT), "diff", "--name-only"], capture_output=True, text=True
        ).stdout.split()
        for rel in head:
            p = ROOT / rel
            if not p.exists():
                continue
            old = subprocess.run(
                ["git", "-C", str(ROOT), "show", f"HEAD:{rel}"], capture_output=True, text=True
            ).stdout
            if code_only(old) != code_only(p.read_text()):
                bad.append(rel)
        print("files whose CODE changed (want none):", bad or "none")
        return 0 if not bad else 1
    changed = [p for p in files if fix_file(p)]
    print(f"rewrote {len(changed)} file(s):")
    for p in changed:
        print("  ", p.relative_to(ROOT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
