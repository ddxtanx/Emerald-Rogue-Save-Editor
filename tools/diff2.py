#!/usr/bin/env python3
"""Locate every valid Pokemon struct in the whole merged save image, for both
the out-of-adventure save and the in-adventure quicksave, and report a
region-level diff. This is how the persistent vs in-adventure split gets mapped."""
import struct, json, sys

DATASZ, SECSZ, SECURITY = 4084, 0x1000, 0x08012025
SB2, SB1, STOR = 0, DATASZ, 5 * DATASZ

import os
NAMES = json.load(open(os.environ.get('SPECIES_JSON','species_names.json')))
def nm(sp):
    v = NAMES.get(str(sp))
    return v if v else str(sp)

# --- gen3 mon decode (shared by 80/104 byte structs) -------------------------
SUBSTRUCT = []
for a in range(4):
    for b in range(4):
        if b == a: continue
        for c in range(4):
            if c in (a, b): continue
            d = 6 - a - b - c
            if d in (a, b, c): continue
            SUBSTRUCT.append((a, b, c, d))
SUBSTRUCT = sorted(set(SUBSTRUCT))[:24]

def mon_decode(buf):
    """buf = 80 or 104 byte mon. Return dict or None if checksum bad."""
    if len(buf) < 32: return None
    pid, otid = struct.unpack_from('<II', buf, 0)
    chk = struct.unpack_from('<H', buf, 28)[0]
    key = pid ^ otid
    kb = key.to_bytes(4, 'little')
    raw = buf[32:80]
    dec = bytearray(48)
    for i in range(4):
        for j in range(12):
            dec[i * 12 + j] = raw[i * 12 + j] ^ kb[j % 4]
    perm = SUBSTRUCT[pid % 24]
    slots = [bytes(dec[p * 12:p * 12 + 12]) for p in perm]
    s = 0
    for sl in slots:
        for w in range(0, 12, 2):
            s = (s + struct.unpack_from('<H', sl, w)[0]) & 0xFFFFFFFF
    if (s & 0xFFFF) != chk:
        return None
    a, b, c, d = slots
    species = struct.unpack_from('<H', a, 0)[0]
    held = struct.unpack_from('<H', a, 2)[0]
    otid_sh, sid_sh = struct.unpack_from('<HH', a, 4)
    exp = struct.unpack_from('<I', a, 8)[0] & 0x1FFFFF
    h = struct.unpack_from('<I', b, 0)[0]
    level = h & 0x7F
    nickname = bytes(x for x in c[0:10] if x)
    def g(w):
        return [struct.unpack_from('<H', w, k)[0] for k in range(0, 12, 2)]
    wb = g(b)
    ivs = [wb[2] & 0x1F, wb[2] >> 5 & 0x1F, wb[2] >> 10 & 0x1F,
           wb[2] >> 15 & 0x1F, wb[3] & 0x1F, wb[3] >> 5 & 0x1F]
    return dict(pid=pid, otid=otid, species=species, held=held, level=level,
                exp=exp, nickname=nickname.decode('latin1'), ivs=ivs,
                cool=wb[0] & 0x1F if wb[0] != 0xFFFF else 0)

def merged(path):
    b = open(path, 'rb').read()
    def foot(p): return struct.unpack_from('<HHII', b, p * SECSZ + DATASZ)
    s0 = max(foot(p)[3] for p in range(14) if foot(p)[2] == SECURITY)
    s1 = max(foot(p)[3] for p in range(14, 28) if foot(p)[2] == SECURITY)
    lo = 0 if s0 >= s1 else 14
    ids = sorted((p for p in range(lo, lo + 14) if foot(p)[2] == SECURITY),
                 key=lambda p: foot(p)[0])
    return b''.join(b[p * SECSZ:p * SECSZ + DATASZ] for p in ids), [p - lo for p in ids]

def scan(m):
    """Find every offset (any alignment) where a valid mon struct starts."""
    hits = []
    for size in (104, 80):
        for off in range(0, len(m) - size):
            d = mon_decode(m[off:off + size])
            if d and 1 <= d['species'] <= 1200 and 1 <= d['level'] <= 100:
                hits.append((off, size, d))
    return hits

def region(tag):
    if tag < SB1: return 'SB2'
    if tag < STOR: return 'SB1'
    return 'STORAGE'

def main():
    A, aid = merged(os.environ.get('SAV_OUT','save-out-of-adventure.sav'))
    B, bid = merged(os.environ.get('SAV_IN','save-in-adventure.sav'))
    print(f"out-of-adv merged {len(A)} ids={aid}")
    print(f"in-adv    merged {len(B)} ids={bid}")

    # region-level diff: 64-byte windows
    print("\n=== differing 64-byte windows (abs offset, region, first/last) ===")
    runs = []
    cur = None
    for off in range(0, min(len(A), len(B)), 64):
        diff = A[off:off + 64] != B[off:off + 64]
        if diff and cur is None:
            cur = off
        elif not diff and cur is not None:
            runs.append((cur, off)); cur = None
    if cur is not None: runs.append((cur, min(len(A), len(B))))
    for s, e in runs:
        print(f"  {hex(s)}..{hex(e)} ({e-s:5d} B)  [{region(s)}]")

    print("\n=== valid mon structs: OUT ===")
    for off, size, d in scan(A):
        print(f"  {hex(off):>8} [{size:3d}] {region(off):8} {nm(d['species']):16} lvl {d['level']:3} nick={d['nickname']!r}")
    print("\n=== valid mon structs: IN ===")
    for off, size, d in scan(B):
        print(f"  {hex(off):>8} [{size:3d}] {region(off):8} {nm(d['species']):16} lvl {d['level']:3} nick={d['nickname']!r}")

main()
