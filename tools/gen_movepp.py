#!/usr/bin/env python3
"""Regenerate src/move_pp.rs from the game tree (rogue-src-2207)."""
import re, sys
root = sys.argv[1] if len(sys.argv) > 1 else '/workspace/hermes/rogue-src-2207'
c = open(f'{root}/include/constants/moves.h', errors='ignore').read()
name2id = {m.group(1): int(m.group(2)) for m in re.finditer(r'#define\s+(MOVE_\w+)\s+(\d+)', c)}
h = open(f'{root}/src/data/battle_moves.h', errors='ignore').read()
d = {}
for b in re.finditer(r'\[(MOVE_\w+)\]\s*=\s*\{(.*?)\n    \},', h, re.S):
    mm = re.search(r'\.pp\s*=\s*([\w ()|]+?),', b.group(2))
    if mm and re.fullmatch(r'\d+', mm.group(1).strip()):
        mid = name2id.get(b.group(1))
        if mid is not None:
            d[mid] = int(mm.group(1))
assert d.get(1) == 35 and d.get(6) == 15, f"anchor moves wrong: {d.get(1)}, {d.get(6)}"
lines = [f"    ({k}, {v})," for k, v in sorted(d.items())]
open('src/move_pp.rs','w').write(
"""// Auto-generated from rogue-src-2207 (include/constants/moves.h +
// src/data/battle_moves.h gBattleMoves_Mainline). Do not edit by hand.
#![allow(dead_code)]

/// Base PP per move id exactly as the game's move table has it.
/// max_pp = base + base/5 * pp_ups (2 bits per move slot in the PP Bonuses byte,
/// game-capped at 3 ups: pokemon.c gPPUpGetMask / item_use.c).
pub const MOVE_PP: &[(u16, u8)] = &[
""" + "\n".join(lines) + "\n];\n")
print(f'move_pp: {len(d)} entries, max id {max(d)}')
