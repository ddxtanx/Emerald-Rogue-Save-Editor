import struct, sys

p = sys.argv[1] if len(sys.argv) > 1 else "save.sav"
data = open(p, 'rb').read()
print("size", len(data), hex(len(data)))
print("first 32 bytes:", data[:32].hex())

SECTOR = 0x1000
for off in (0, 16):
    print(f"\n=== parsing with offset {off} ===")
    ok = True
    for n in range(0, 32):
        i = off + n*SECTOR
        if i+SECTOR > len(data): break
        sec = data[i:i+SECTOR]
        sec_id, tag1, tag2, tag3 = struct.unpack_from("<4H", sec, 0)
        footer = sec[SECTOR-16:]
        pcRC, pcSecID, pcCookie, pcSaveIdx = struct.unpack_from("<4I", footer, 0)
        fsecid, ftag1, ftag2, ftag3 = struct.unpack_from("<4H", footer, 8)
        if n < 6 or n == 31:
            print(f"sec {n:2d} id={sec_id:04x} tags={tag1:04x}{tag2:04x}{tag3:04x} | foot id={fsecid:04x} ftags={ftag1:04x}{ftag2:04x}{ftag3:04x} pcSaveIdx={pcSaveIdx:08x} pcSecID={pcSecID:08x} pcRC={pcRC:08x}")
        if sec_id != fsecid:
            ok = False
    print("headers/footers consistent:", ok)
