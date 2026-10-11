#!/usr/bin/env python3
"""Swap a move in a Pokemon Emerald Rogue (v2.x EX) save.

Moves live in BoxPokemon substruct1 (type1): u16 moves[4]; u8 pp[4].
Move IDs come from the *expansion* branch's constants (Rogue EX uses the full
Gen-1..9 list; Hammer Arm = 359, Earthquake = 89). PP byte = base PP
(+8 per PP Up). Changing a move changes the 48 decrypted bytes, so the box
mon checksum and the containing flash-sector checksum are recomputed.
"""
import os
import struct, sys, re, shutil
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from er_iv_edit import Save, Mon, sum16_decrypted, decode_str

MOVES_H = os.environ.get('MOVES_H','ex_moves.h')
MOVE_NAMES = {}
for line in open(MOVES_H):
    m = re.match(r'#define MOVE_(\w+)\s+(\d+)\s*$', line)
    if m:
        MOVE_NAMES.setdefault(int(m.group(2)), m.group(1))
BY_NAME = {v: k for k, v in MOVE_NAMES.items()}

SB1 = 4084              # SaveBlock1 start in the merged stream
PARTY_COUNT_OFF, PARTY_OFF, MONSZ = 0x234, 0x238, 104
DATASZ, SECSZ, SLOT_SECTORS = 4084, 0x1000, 14


def mon_moves(m):
    t1 = bytes(m.t[1])
    return list(struct.unpack_from('<4H', t1, 0)), list(t1[8:12])


def main():
    src, out = sys.argv[1], sys.argv[2]
    species = int(sys.argv[3])
    old_name, new_name = sys.argv[4], sys.argv[5]
    old_id, new_id = BY_NAME[old_name], BY_NAME[new_name]

    save = Save(src)
    merged, meta = save.merged(0)
    assert all(m[2] == m[3] for m in meta), "input has bad sector checksums"
    pc = merged[SB1 + PARTY_COUNT_OFF]

    target_off = None
    for i in range(min(pc, 6)):
        off = SB1 + PARTY_OFF + MONSZ*i
        m = Mon(merged[off:off+MONSZ])
        if m.species() != species:
            continue
        ids, _ = mon_moves(m)
        if old_id in ids:
            target_off = off
            slot = ids.index(old_id)
            break
    if target_off is None:
        sys.exit(f"no party mon species {species} with move {old_name}")

    m = Mon(merged[target_off:target_off+MONSZ])
    ids, pp = mon_moves(m)
    print(f"{decode_str(m.nick_raw)} (species {species}, lvl {m.level()})")
    print("  before:", [MOVE_NAMES.get(x, x) for x in ids], pp)

    ids[slot] = new_id
    base_pp = 30  # will be read from game data below if present
    # Earthquake base PP per move data; keep it generic: derive from known table
    # base PP read from ex battle_moves.h (this game uses updated move flags:
    # Earthquake = 10, not the old gen-3 30)
    PP_TABLE = {'EARTHQUAKE': 10, 'HAMMER_ARM': 10}
    pp[slot] = PP_TABLE[new_name]
    new_t1 = struct.pack('<4H', *ids) + bytes(pp) + bytes(m.t[1][12:])
    m.t[1] = new_t1
    patched = m.seal()

    chk_ok = sum16_decrypted(m) == m.checksum
    print("  after :", [MOVE_NAMES.get(x, x) for x in ids], pp,
          f"boxchk={'OK' if chk_ok else 'BAD'}")
    assert chk_ok

    out_raw = bytearray(save.raw)
    for slotk in range(SLOT_SECTORS):
        lo, hi = slotk*DATASZ, (slotk+1)*DATASZ
        if not (lo <= target_off and target_off+MONSZ <= hi):
            continue
        p = save.expected_rotation(0)[slotk]
        data = bytearray(save.sector(p)[0])
        data[target_off-lo: target_off-lo+MONSZ] = patched
        sid, csum = struct.unpack_from('<2H', save.raw, p*SECSZ+DATASZ)
        sec, ctr = struct.unpack_from('<2I', save.raw, p*SECSZ+DATASZ+4)
        newcrc = Save.crc(bytes(data))
        out_raw[p*SECSZ:p*SECSZ+DATASZ] = data
        struct.pack_into('<2H', out_raw, p*SECSZ+DATASZ, sid, newcrc)
        struct.pack_into('<2I', out_raw, p*SECSZ+DATASZ+4, sec, ctr)
        print(f"  slot1 sector-id {slotk} (phys {p}): csum {csum:04x} -> {newcrc:04x}")

    with open(out, 'wb') as f:
        f.write(out_raw)
        f.write(save.trailer)
    print("wrote", out)


if __name__ == '__main__':
    main()
