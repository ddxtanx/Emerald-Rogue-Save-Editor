import re
data = open('ex_rogue_items.h').read()
# find start of gRogueItems array
start = data.index('gRogueItems[')
arr = data[start:]
# each entry: [XXX - ITEM_ROGUE_ITEM_FIRST] = { .name = _("Name"), ... }
# take the FIRST .name in each entry (ROGUE_EXPANSION branch)
entries = []
pos = 0
pat = re.compile(r'\[([A-Za-z0-9_]+)\s*-\s*ITEM_ROGUE_ITEM_FIRST\]\s*=\s*\{')
for m in pat.finditer(arr):
    body = arr[m.end():m.end()+800]
    nm = re.search(r'\.name\s*=\s*_\("([^"]+)"\)', body)
    entries.append((827 + len(entries), nm.group(1) if nm else m.group(1)))
print("rogue items parsed:", len(entries))
print(entries[:12])
print([e for e in entries if e[0] in (828,830,831,832,833,834,841,856,865,867)])
src = open('er-save-editor/src/tables.rs').read()
mt = re.search(r'pub const ITEMS[^=]+&\[(.*?)\];', src, re.S)
have = set(int(x) for x in re.findall(r'\((\d+),', mt.group(1)))
new = [(i, n) for i, n in entries if i not in have]
ins = "".join("    (%d, \"%s\"),\n" % (i, n) for i, n in new)
src = src[:mt.end(1)] + ins + src[mt.end(1):]
    # The table this script used to emit is CSV now (data/item_names_extra.csv), parsed at start-up by
    # src/tables_extra.rs; re-running this generator would replace that loader with literals
    # and break every accessor. Add a CSV emitter before removing this stop.
    raise SystemExit(
        "SUPERSEDED: ITEMS_EXTRA now lives in data/item_names_extra.csv.\n"
        "Edit that file, or port this generator to write it, before re-running."
    )
open('er-save-editor/src/tables.rs', 'w').write(src)
print("added", len(new), "rogue items")
