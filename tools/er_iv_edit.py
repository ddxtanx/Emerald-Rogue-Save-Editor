#!/usr/bin/env python3
"""Pokemon Emerald Rogue (v2.x EX) save editor.

Layout (verified against Pokabbie/pokeemerald-rogue include/save.h, src/save.c,
include/pokemon.h, src/pokemon.c):

  File = 32 x 4096-byte flash sectors (+ optional 16-byte mGBA trailer)
  Sector footer @4084: u16 id, u16 checksum, u32 security(0x08012025), u32 counter
  Sector ids rotate within a slot: physical sector p holds id = (10+p) % 14
    slot1 = sectors 0-13, slot2 = 14-27 (ids 14..27 in the merged view)
  SaveBlock1 (party lives here) = merged ids 1..4 -> offset 0x0 of merged buffer
  struct Pokemon = 104 bytes (BoxPokemon 80 + status/level/... 24), party @0x238
  BoxPokemon.secure (bytes 32..80): 4 x 12-byte substructs, XOR-obfuscated with
    key = otId ^ personality, then physically permuted by personality % 24.
  box checksum = sum of u16 over the 48 *de-obfuscated, permuted* secure bytes.
  sector checksum = ((sum>>16)+sum) & 0xFFFF over u32 of the 4084 data bytes.
"""
import struct, sys, shutil
from operator import xor

SECSZ, DATASZ = 0x1000, 4084
SECURITY = 0x08012025
PARTY_COUNT_OFF, PARTY_OFF, MONSZ = 0x234, 0x238, 104
SLOT_SECTORS = 14

SUBSTRUCT_ORDER = {
     0:(0,1,2,3),  1:(0,1,3,2),  2:(0,2,1,3),  3:(0,3,1,2),  4:(0,2,3,1),  5:(0,3,2,1),
     6:(1,0,2,3),  7:(1,0,3,2),  8:(2,0,1,3),  9:(3,0,1,2), 10:(2,0,3,1), 11:(3,0,2,1),
    12:(1,2,0,3), 13:(1,3,0,2), 14:(2,1,0,3), 15:(3,1,0,2), 16:(2,3,0,1), 17:(3,2,0,1),
    18:(1,2,3,0), 19:(1,3,2,0), 20:(2,1,3,0), 21:(3,1,2,0), 22:(2,3,1,0), 23:(3,2,1,0),
}

SPECIES = {2: "Ivysaur", 25: "Pikachu", 133: "Eevee", 136: "Flareon", 154: "Meganium",
           155: "Cyndaquil", 157: "Typhlosion", 158: "Totodile", 229: "Houndoom",
           244: "Suicune", 250: "Ho-Oh"}
SPECIES_ARCANINE = 59
IV_NAMES = ["HP", "Attack", "Defense", "Speed", "Sp.Atk", "Sp.Def"]


# ---------------------------------------------------------------- save <-> sectors
class Save:
    def __init__(self, path):
        self.raw = bytearray(open(path, 'rb').read())
        nsec = len(self.raw) // SECSZ
        self.trailer = b''
        if nsec == 32 and len(self.raw) == 32*SECSZ + 16:
            # mGBA appends a 16-byte save-block footer (magic 0x6ACAB1BC) after the raw save
            self.trailer = bytes(self.raw[32*SECSZ:])
            self.raw = self.raw[:32*SECSZ]
            assert self.trailer[8:12] == b'\xbc\xb1\xca\x6a', "bad mGBA trailer magic"
        assert len(self.raw) // SECSZ == 32, "unexpected size %d" % len(self.raw)

    def sector(self, p):
        base = p*SECSZ
        data = bytes(self.raw[base:base+DATASZ])
        sid, csum = struct.unpack_from('<2H', self.raw, base+DATASZ)
        sec, ctr = struct.unpack_from('<2I', self.raw, base+DATASZ+4)
        return data, sid, csum, sec, ctr

    @staticmethod
    def crc(data):
        s = 0
        for i in range(0, len(data), 4):
            s += int.from_bytes(data[i:i+4], 'little')
        return ((s >> 16) + s) & 0xFFFF

    def expected_rotation(self, slotbase):
        """Map logical sector id (0..13) -> physical sector for this slot."""
        pos = {}
        for p in range(slotbase, slotbase+SLOT_SECTORS):
            data, sid, csum, sec, ctr = self.sector(p)
            if sec == SECURITY and sid < SLOT_SECTORS:
                pos[sid] = p
        return pos

    def merged(self, slotbase=0):
        """Rebuild the contiguous save-block stream for a slot."""
        pos = self.expected_rotation(slotbase)
        out = bytearray()
        meta = []
        for k in range(SLOT_SECTORS):
            p = pos[k]
            data, sid, csum, sec, ctr = self.sector(p)
            meta.append((p, sid, csum, self.crc(data), ctr))
            out += data
        return out, meta


# ---------------------------------------------------------------- pokemon struct
def xor_bytes(b, key32):
    kb = struct.pack('<I', key32)
    return bytes(xor(x, y) for x, y in zip(b, (kb*4)[:len(b)]))


class Mon:
    def __init__(self, ba):
        self.ba = bytearray(ba)              # 104 bytes, raw (as stored)
        (self.pid, self.otid) = struct.unpack_from('<2I', self.ba, 0)
        self.nick_raw = bytes(self.ba[8:18])
        self.flags = self.ba[18]
        self.egg_species = self.ba[19]
        self.ot_raw = bytes(self.ba[20:27])
        self.markings = self.ba[27]
        self.checksum = struct.unpack_from('<H', self.ba, 28)[0]
        self.key = self.pid ^ self.otid
        self.perm = SUBSTRUCT_ORDER[self.pid % 24]   # slot i holds logical substruct perm[i]
        # stored (encrypted) 12-byte chunks, in physical order
        self.enc = [bytes(self.ba[32+12*i:44+12*i]) for i in range(4)]
        # clear, in physical slot order
        self.clear_slots = [xor_bytes(c, self.key) for c in self.enc]
        # logical order: GetSubstruct(type t) returns substructs[perm[t]]
        self.t = [self.clear_slots[self.perm[t]] for t in range(4)]

    # -- logical fields
    def f0(self): return struct.unpack_from('<I', self.t[0], 0)[0]
    def species(self): return self.f0() & 0x7FF
    def held_item(self): return self.f0() >> 11
    def exp(self): return struct.unpack_from('<I', self.t[0], 4)[0]
    def friendship(self): return self.t[0][8]
    def ivword(self): return struct.unpack_from('<I', self.t[3], 4)[0]
    def ivs(self):
        w = self.ivword()
        return [(w >> (5*i)) & 0x1F for i in range(6)]   # hp, atk, def, spe, spa, spd
    def ability_num(self): return (self.ivword() >> 30) & 1
    def evs(self): return list(self.t[2][0:6])
    def nature(self): return self.pid & 0x1F
    def level(self): return self.ba[84]

    # -- writes
    def set_iv(self, idx, val):
        assert 0 <= idx < 6 and 0 <= val <= 31
        w = self.ivword()
        w = (w & ~(0x1F << (5*idx))) | (val << (5*idx))
        self.t[3] = bytearray(self.t[3])
        self.t[3][4:8] = struct.pack('<I', w)

    def seal(self):
        """Re-pack: write logical t[] back into physical slots (still clear),
        compute checksum over decrypted physical bytes, then encrypt."""
        for i in range(4):
            self.clear_slots[self.perm[i]] = bytearray(bytes(self.t[i]))
        s = 0
        for chunk in self.clear_slots:
            for k in range(0, 12, 2):
                s += int.from_bytes(chunk[k:k+2], 'little')
        chk = s & 0xFFFF
        self.ba[28:30] = struct.pack('<H', chk)
        self.checksum = chk
        for i in range(4):
            self.ba[32+12*i:44+12*i] = xor_bytes(bytes(self.clear_slots[i]), self.key)
        return bytes(self.ba)


def sum16_decrypted(m):
    """CalculateBoxMonChecksum: sum u16 over the 48 decrypted substruct bytes
    in PHYSICAL (permuted) order -- exactly what GetSubstruct+raw[] yields."""
    s = 0
    for chunk in m.clear_slots:
        for i in range(0, 12, 2):
            s += int.from_bytes(chunk[i:i+2], 'little')
    return s & 0xFFFF


def decode_str(b):
    out = ""
    for c in b:
        if c == 0xFF: break
        elif c == 0x00: out += ' '   # CHAR_SPACE is 0x00 in Gen3 encoding
        elif 0xA1 <= c <= 0xAA: out += '0123456789'[c-0xA1]
        elif c == 0xAB: out += '!'
        elif c == 0xAC: out += '?'
        elif c == 0xAD: out += '.'
        elif c == 0xAE: out += '-'
        elif c == 0xB0: out += '['
        elif c == 0xB1: out += ']'
        elif 0xBB <= c <= 0xD4: out += "ABCDEFGHIJKLMNOPQRSTUVWXYZ"[c-0xBB]
        elif 0xD5 <= c <= 0xEE: out += "abcdefghijklmnopqrstuvwxyz"[c-0xD5]
        else: out += f"<{c:02x}>"
    return out.strip()


NAMES = ["Hardy","Lonely","Brave","Adament","Naughty","Bold","Docile","Relaxed",
         "Impish","Lax","Timid","Hasty","Serious","Jolly","Naive","Modest",
         "Mild","Quiet","Bashful","Rash","Calm","Gentle","Sassy","Careful","Quirky"]


# ---------------------------------------------------------------- main
def main():
    src = sys.argv[1]
    mon_idx = int(sys.argv[2]) if len(sys.argv) > 2 else None
    save = Save(src)

    for slotbase in (0, 14):
        merged, meta = save.merged(slotbase)
        print(f"--- slot {slotbase//14+1} (sectors {slotbase}-{slotbase+13}) ---")
        for p, sid, csum, calc, ctr in meta:
            print(f"  phys {p:2d} id {sid:2d} stored={csum:04x} calc={calc:04x} match={csum==calc} ctr={ctr}")

    merged, meta = save.merged(0)
    bad = [m for m in meta if m[2] != m[3]]
    if bad:
        print("!! slot1 sector checksum mismatches:", bad)

    # SaveBlock1 begins at merged offset DATASZ (sector id 1); party offsets are
    # relative to SaveBlock1's start (struct SaveBlock1 in global.h).
    SB1 = DATASZ
    pc = merged[SB1 + PARTY_COUNT_OFF]
    print("\nplayerPartyCount =", pc)

    mons = []
    for i in range(min(pc, 6)):
        off = SB1 + PARTY_OFF + MONSZ*i
        m = Mon(merged[off:off+MONSZ])
        sp = m.species()
        chk_ok = sum16_decrypted(m) == m.checksum
        mons.append((off, m))
        print(f"party[{i}] off=0x{off:05x} species={sp:<3} lvl={m.level():<3} name={decode_str(m.nick_raw)!r:<14} "
              f"OT={decode_str(m.ot_raw)!r:<10} nature={NAMES[m.nature()]:<9} "
              f"IVs={m.ivs()} EVs={m.evs()} boxchk={'OK' if chk_ok else 'BAD'}")

    if mon_idx is None:
        return

    # find Arcanine (or use the requested slot)
    target = None
    for i, (off, m) in enumerate(mons):
        if m.species() == SPECIES_ARCANINE:
            target = i
            break
    if mon_idx is not None and mon_idx >= 0:
        target = mon_idx
    if target is None:
        print("no Arcanine in party"); return
    print("\nEditing party slot", target)

    off = SB1 + PARTY_OFF + MONSZ*target
    m = Mon(merged[off:off+MONSZ])
    print("  before:", decode_str(m.nick_raw), "species", m.species(), "IVs", m.ivs())
    # back-solve the cached base stat from the game's own stored value so we can
    # refresh the cached stat in struct Pokemon after the IV change (Emerald
    # only recomputes stats at level-up; offsets 0x86..0x98 hold cached stats).
    ba = bytearray(merged[off:off+MONSZ])
    lvl = ba[84]
    nature = m.nature()
    NEG_NATURES_ATK = {5, 10, 15, 20}  # Bold, Timid, Modest, Calm lower Attack
    POS_NATURES_ATK = {1, 2, 3, 4}     # Lonely, Brave, Adamant, Naughty raise Attack
    def atkmul(nat):
        if nat in NEG_NATURES_ATK: return 90
        if nat in POS_NATURES_ATK: return 110
        return 100
    CACHED_ATK_OFF = 86 + 4  # hp@86,maxhp@88,atk@8A
    old_cached_atk = struct.unpack_from('<H', ba, CACHED_ATK_OFF)[0]
    old_iv_atk = m.ivs()[1]
    base_atk = None
    for B in range(30, 256):
        v = ((2*B + old_iv_atk) * lvl // 100 + 5)
        v = (v * atkmul(nature)) // 100
        if v == old_cached_atk:
            base_atk = B; break
    m.set_iv(1, 31)                     # index 1 == Attack
    if base_atk is not None:
        newv = ((2*base_atk + 31) * lvl // 100 + 5)
        newv = (newv * atkmul(nature)) // 100
        ba = bytearray(m.ba)
        ba[CACHED_ATK_OFF:CACHED_ATK_OFF+2] = struct.pack('<H', newv)
        m.ba = ba
        print(f"  cached base atk {base_atk}: stat {old_cached_atk} -> {newv} (lvl {lvl}, nature {NAMES[nature]})")
    patched = m.seal()
    print("  after :", "IVs", m.ivs(), "new box checksum %04x" % m.checksum)

    out = bytearray(save.raw)

    def patch_slot(slotbase):
        rot = save.expected_rotation(slotbase)
        for k in range(SLOT_SECTORS):
            sec_lo, sec_hi = k*DATASZ, (k+1)*DATASZ
            if not (sec_lo <= off and off+MONSZ <= sec_hi):
                continue
            p = rot[slotbase+k]
            data = bytearray(save.sector(p)[0])
            data[off-sec_lo: off-sec_lo+MONSZ] = patched
            sid, csum = struct.unpack_from('<2H', save.raw, p*SECSZ+DATASZ)
            sec, ctr = struct.unpack_from('<2I', save.raw, p*SECSZ+DATASZ+4)
            newcrc = Save.crc(bytes(data))
            out[p*SECSZ:p*SECSZ+DATASZ] = data
            struct.pack_into('<2H', out, p*SECSZ+DATASZ, sid, newcrc)
            struct.pack_into('<2I', out, p*SECSZ+DATASZ+4, sec, ctr)
            print(f"  slot{slotbase//14+1}: sector-id {slotbase+k} (phys {p}): csum {csum:04x} -> {newcrc:04x}")

    patch_slot(0)
    # mirror into slot 2 if it carries the same party data (keeps both slots valid)
    merged2, _ = save.merged(14)
    if merged2[off:off+MONSZ] == merged[off:off+MONSZ]:
        patch_slot(14)
    else:
        print("  slot2 party differs (older backup) -- leaving slot2 untouched")

    dst = sys.argv[3]
    shutil.copy2(src, dst + ".bak") if len(sys.argv) > 3 else None
    with open(dst, 'wb') as f:
        f.write(out)
        f.write(save.trailer)
    print("\nwrote", dst)


if __name__ == '__main__':
    main()
