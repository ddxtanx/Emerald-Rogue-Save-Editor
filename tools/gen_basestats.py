#!/usr/bin/env python3
"""Generate BASE_STATS table from expansion species_info/*.h"""
import os
import re
import urllib.request, json

tree = json.load(open(os.environ.get('TREE_JSON','tree.json')))
paths = [t['path'] for t in tree['tree'] if t['path'].startswith('src/data/pokemon/species_info/')]
base = {}
species_ids = {}
# species ids from ex_species.h
for line in open(os.environ.get('SPECIES_H','ex_species.h')):
    m = re.match(r'#define\s+(SPECIES_\w+)\s+(\d+)', line.strip())
    if m: species_ids[m.group(1)] = int(m.group(2))

for p in paths:
    url = 'https://raw.githubusercontent.com/Pokabbie/pokeemerald-rogue/expansion/' + p
    try:
        data = urllib.request.urlopen(url, timeout=30).read().decode('utf-8', 'replace')
    except Exception as e:
        print("FAIL", p, e); continue
    # split into [SPECIES_X] = { ... } blocks
    for m in re.finditer(r'\[(SPECIES_\w+)\]\s*=\s*\{(.*?)\n    },', data, re.S):
        name, body = m.group(1), m.group(2)
        if name not in species_ids: continue
        def g(f):
            mm = re.search(rf'\.{f}\s*=\s*(\d+)', body)
            return int(mm.group(1)) if mm else None
        vals = [g('baseHP'), g('baseAttack'), g('baseDefense'),
                g('baseSpeed'), g('baseSpAttack'), g('baseSpDefense')]
        if None in vals: continue
        base[species_ids[name]] = vals

out = "// Auto-generated from Pokabbie/pokeemerald-rogue expansion species_info\npub fn base_stats(sp: u16) -> Option<[u16; 6]> {\n    match sp {\n"
for sp, v in sorted(base.items()):
    out += "        {} => Some([{}, {}, {}, {}, {}, {}]),\n".format(sp, *v)
out += "        _ => None,\n    }\n}\n"
open(os.environ.get('OUT_RS','src/basestats.rs'),'w').write(out)
print(f"wrote {len(base)} species base stats")
