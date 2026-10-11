#!/usr/bin/env python3
"""Locate all Pokemon structs (party-size 104B and box-size 80B) with valid
checksums in both saves: er.sav (out-of-adventure) vs er2.sav (in-adventure)."""
import os, struct, sys

DATASZ=4084; SECSZ=0x1000; SECURITY=0x08012025
SUB=[(0,1,2,3),(0,1,3,2),(0,2,1,3),(0,3,1,2),(0,2,3,1),(0,3,2,1),
     (1,0,2,3),(1,0,3,2),(2,0,1,3),(3,0,1,2),(2,0,3,1),(3,0,2,1),
     (1,2,0,3),(1,3,0,2),(2,1,0,3),(3,1,0,2),(2,3,0,1),(3,2,0,1),
     (1,2,3,0),(1,3,2,0),(2,1,3,0),(3,1,2,0),(2,3,1,0),(3,2,1,0)]

SPECIES={59:'Arcanine',376:'Metagross',609:'Larvesta',461:'Weavile',607:'Litwick',
         608:'Chandelure',374:'Metang',373:'Beldum',375:'Metagross?'}

def load(path):
    b=open(path,'rb').read()
    foot=lambda p: struct.unpack_from('<HHII',b,p*SECSZ+DATASZ)
    s0=max(foot(p)[3] for p in range(14) if foot(p)[2]==SECURITY)
    s1=max(foot(p)[3] for p in range(14,28) if foot(p)[2]==SECURITY)
    lo=0 if s0>=s1 else 14
    sids=sorted((p for p in range(lo,lo+14) if foot(p)[2]==SECURITY), key=lambda p: foot(p)[0])
    merged=b''.join(b[p*SECSZ:p*SECSZ+DATASZ] for p in sids)
    return b, merged

def mon_at(merged, off, size):
    raw=merged[off:off+size]
    if len(raw)<size: return None
    pid,otid=struct.unpack_from('<II',raw,0)
    sp=0; 
    kb=struct.pack('<I',otid^pid)
    slots=[bytes(x^kb[i%4] for i,x in enumerate(raw[32+s*12:32+s*12+12])) for s in range(4)]
    perm=SUB[pid%24]
    t=[slots[perm[i]] for i in range(4)]
    csum=0
    for s in slots:
        for w in range(0,12,2): csum=(csum+struct.unpack_from('<H',s,w)[0])&0xFFFFFFFF
    stored=struct.unpack_from('<H',raw,28)[0]
    if (csum&0xFFFF)!=stored: return None
    sp=struct.unpack_from('<I',t[0],0)[0]&0x7FF
    if sp==0 or sp>1025: return None
    lvl=raw[84] if size==104 else 0
    ot=''.join(chr(c-0xBB+65) if 0xBB<=c<=0xD4 else '' for c in raw[20:27])
    return (sp,lvl,ot)

def scan(merged):
    hits=[]
    for off in range(0,len(merged)-104):
        m=mon_at(merged,off,104)
        if m and m[0]!=0:
            hits.append((off,m))
    return hits

def dedupe(hits):
    # keep 104B hits; also record box-size 80B standalone by scanning every 4 bytes
    return hits

for path,tag in [(os.environ.get('SAV_OUT','save-out-of-adventure.sav'),'OUT-OF-ADV'),
                 (os.environ.get('SAV_IN','save-in-adventure.sav'),'IN-ADV')]:
    b,merged=load(path)
    print(f"\n===== {tag}: {path} (merged {len(merged)} bytes) =====")
    # party count fields
    SB1=DATASZ
    print("  party_count at SB1+0x234:", merged[SB1+0x234])
    # scan for 104B mons at every 4-byte alignment
    found=[]
    off=0
    while off < len(merged)-104:
        m=mon_at(merged,off,104)
        if m:
            found.append((off,m))
            off+=104
        else:
            off+=4
    for off,(sp,lvl,ot) in found:
        name=SPECIES.get(sp,str(sp))
        region='SB2' if off<DATASZ else ('SB1' if off<5*DATASZ else 'STORAGE')
        print(f"  off 0x{off:05x} [{region}] {name:12} lvl {lvl:3} OT={ot}")
