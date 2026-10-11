# Emerald Rogue (v2.x / EX) save format — reverse-engineering notes

Everything here was derived from the game's own source
([Pokabbie/pokeemerald-rogue](https://github.com/Pokabbie/pokeemerald-rogue), `expansion` branch) and confirmed
against real v2.2.0-EX saves. Paths in `←` markers refer to that repo.

The `expansion` branch is required for EX v2.2.0. Tables generated from `master` decode incorrectly.

## 1. File container

```
32 × 4096-byte flash sectors  = 131072 bytes
+ optional 16-byte mGBA trailer
```

Each sector: 4084 bytes of data + a 12-byte footer at offset 4084.

```c
struct SectorFooter {   // ← src/save.c, include/save.h
    u16 id;
    u16 checksum;   // sum of u32 words folded: ((sum>>16) + sum) & 0xFFFF
    u32 security;   // must equal 0x08012025
    u32 counter;    // monotonic; higher = newer write
};
```

`SECTOR_SECURITY_NUM = 0x08012025`. Sector IDs:

| id | meaning |
|----|---------|
| 0 | SaveBlock2 |
| 1–4 | SaveBlock1 (4 chunks) |
| 5–13 | Pokémon storage (9 chunks) |
| 28, 29 | Hall of Fame |
| 30 | Trainer Hill |
| 31 | Recorded battle |

There are two mirrored save slots of 14 sectors each: slot 1 = sectors 0–13, slot 2 = sectors 14–27. To read a save,
take the slot with the higher footer counter, then order that slot's sectors by footer `id` and concatenate their
4084-byte data regions:

```
merged = SB2 (4084) ‖ SB1 (4×4084 = 16336) ‖ STORAGE (9×4084 = 36756)  = 57176 bytes
```

Both slots validate on a healthy save; only the counter decides which is live.

## 2. Rogue's size deltas

Rogue enlarges the vanilla blocks (`← include/save.h` comments):

- SaveBlock1: **+464** bytes
- SaveBlock2: **+116** bytes
- PokémonStorage: **+1044** bytes

Consequently **vanilla Emerald offsets are wrong**, and PKHeX rejects these saves. The main cause is that the party
`Pokemon` struct is 104 bytes in Rogue instead of 80.

## 3. SaveBlock2 (`merged` offset 0)

```
SB2 + 0x4C : u32 encryptionKey      // XOR key for money + bag quantities
player name, TID, SID, playtime at vanilla-relative positions shifted by the +116 delta
```

## 4. SaveBlock1 (`merged` offset 4084)

Verified offsets, relative to the start of merged SaveBlock1 (`SB1`):

```
SB1 + 0x234 : u8 partyCount
SB1 + 0x238 : struct Pokemon party[6]      // 104 bytes each
SB1 + 0x4A8 : u32 money                    // XOR'd with encryptionKey
SB1 + 0x4B0 : PC item pouch (50 × u32: u16 id, u16 qty), raw
SB1 + 0x578 : bag items (BAG_ITEM_CAPACITY = 450); quantity XOR'd with low 16 bits of encryptionKey
```

`0x4A8 = 0x238 + 6*104` — the money field simply follows the enlarged party array. Cross-checked against the
community converter `jfzazo/pokeemerald-rogue_savconverter` and a live save.

## 5. Pokémon structs

`BoxPokemon` = 80 bytes, Rogue `Pokemon` = 104 bytes (80-byte body + 24-byte cached-stat extension).

Header (first 32 bytes, plaintext):

```
u32 pid
u32 otid          // TID | SID<<16
u8  nickname[10]  (gen3 encoding)
u16 otid_sh, sid_sh (in substruct A)
u16 checksum      // at offset 28
u8  isEgg/hatching flags etc.
```

Body: 48 bytes at offset 32 = four 12-byte substructs, permuted and XOR-encrypted.

```
perm        = SUBSTRUCT_ORDER[pid % 24]        // 24 permutations, ← src/pokemon.c GetSubstruct
key         = pid ^ otid                       // ← EncryptBoxMon
byte[i]     = plaintext[i] ^ key[(i%12)%4]     // per substruct, 4-byte key cycled
checksum    = sum of u16 words over the 4 decrypted substructs, masked to 16 bits
```

Substruct contents (logical A/B/C/D):

```
A: species(u16), heldItem(u16), otid_sh, sid_sh, exp(u32, 21 bits), …pokerus
B: hp_iv bits, level u8, happiness, status, …  IVs packed across 11 bits each in two u16s
C: nickname is in the header; moves[4] u16 + pp[4] u8 + maxPP/PP-up bits
D: experience/contest/ribbons etc.
```

Cached stat block for the 104-byte party variant (plaintext u16, offsets within the struct):

```
86 currentHP   88 maxHP   90 Attack   92 Defense   94 Speed   96 SpAtk   98 SpDef
```

**Any edit must re-seal**: rewrite substructs → recompute Pokémon checksum → re-encrypt → recompute the containing
sector checksum. Skipping the sector checksum makes the game discard the save.

### Stat formulas (`← src/pokemon.c`, confirmed against live saves)

```
HP    = ((2*base + IV + EV/4) * lvl) / 100 + lvl + 10
other = (((2*base + IV + EV/4) * lvl) / 100 + 5) * natureMod / 100     // integer division at each step
natureMod = 110 boosted, 100 neutral, 90 lowered
```

Nature index order (index 3 = **Adamant**, not Calm — a real trap when back-solving):

```
Hardy, Lonely, Brave, Adamant, Naughty,
Bold, Docile, Relaxed, Impish, Lax,
Timid, Hasty, Serious, Jolly, Naive,
Modest, Mild, Quiet, Bashful, Rash,
Calm, Gentle, Sassy, Careful, Quirky
```

Careful with back-solving base stats from a single observed stat: two different (base, nature) pairs can produce the
same value. Verify against the source's `species_info` table instead.

## 6. Pokémon storage (`merged` offset 5 × 4084 = 0x5000)

```c
struct PokemonStorage {              // ← include/pokemon_storage_system.h
    /*0x0000*/ u8  currentBox;
    /*0x0001*/ struct BoxPokemon boxes[ACTUAL_TOTAL_BOXES_COUNT][IN_BOX_COUNT];
    /*0x8344*/ u8  boxNames[ACTUAL_TOTAL_BOXES_COUNT][BOX_NAME_LENGTH + 1];
    /*0x83C2*/ u8  boxWallpapers[ACTUAL_TOTAL_BOXES_COUNT];
    /*0x8432*/ struct Pokemon fusions[MAX_FUSION_STORAGE];
};
```

```
ACTUAL_TOTAL_BOXES_COUNT = 14      TOTAL_BOXES_COUNT = 10 (the visible PC)
LEFTOVER_BOXES_COUNT     = 4       IN_BOX_COUNT      = 30 (5×6)
BOX_NAME_LENGTH          = 8       MAX_FUSION_STORAGE = 4
```

Mon in box `bx`, slot `s`: `STORAGE + 4 + (bx*30 + s) * 80`.
Box names: `STORAGE + 4 + 14*30*80 + bx*9` (stride is 9: `BOX_NAME_LENGTH + 1`).
Names are gen3-encoded, not ASCII — decode with the gen3 table or they look like garbage.

## 7. The Rogue save block

Boxes 10–13 are **not** extra storage. Rogue overlays its own save block there:

```c
// ← src/rogue_save.c
void* ptr = &gPokemonStoragePtr->boxes[TOTAL_BOXES_COUNT][0];
gRogueSaveBlock = (struct RogueSaveBlock*)ptr;

#define ROGUE_SAVE_BLOCK_CAPACITY (sizeof(struct BoxPokemon) * IN_BOX_COUNT * LEFTOVER_BOXES_COUNT)
STATIC_ASSERT(sizeof(struct RogueSaveBlock) < ROGUE_SAVE_BLOCK_CAPACITY, RogueSaveBlockSize);
```

So the block starts at `STORAGE + 4 + 10*30*80`, i.e. `0x5000 + 4 + 0x5DC0` in merged coordinates.

### Serialization framing

`SerializeRogueBlockInternal` (`← src/rogue_save.c`) writes a header first:

```
u16 saveVersion
u8  currentBlockFormat
u16 secretId            // expected 27615
```

Confirmed magic found in both test saves at merged offset `0xad8b`: version `7`, format `2`, `secretId 27615`.
Nothing after `currentBlockFormat` may be interpreted until the block format for that save version is applied
(the source says so explicitly). Save version enum runs `SAVE_VER_ID_1_X … SAVE_VER_ID_2_2_1`.

The block is serialized **unencrypted** in the observed path (`stream.encryptionKey = 0`), via a size-prefixed
stream: every array is written as `u16 serializedSize` followed by `serializedSize * elementSize` bytes. That framing,
not raw struct layout, is what must be walked to decode the payload.

Order after the header: `rogueVersion`, `gRngRogueValue`, `sRunRestoreBlock`, `gRogueRun`,
`gRogueAdvPath.currentRoomParams`, `gRogueAdvPath.currentRoomType`.

### Contents

```c
struct RogueSaveBlock {           // ← include/rogue.h
    u16 saveVersion;
    u8  currentBlockFormat;
    struct RogueQuestState       questStates[QUEST_SAVE_COUNT];
    struct RogueCampaignState    campaignData[ROGUE_CAMPAIGN_COUNT];
    struct RogueSafariMon        safariMons[ROGUE_SAFARI_TOTAL_MONS];       // 60
    struct RogueDaycarePokemon   daycarePokemon[DAYCARE_SLOT_COUNT];        // 3
    struct UniqueMon             dynamicUniquePokemon[DYNAMIC_UNIQUE_MON_COUNT]; // 5
    struct AdventureReplay       adventureReplay[ROGUE_ADVENTURE_REPLAY_COUNT];  // 2
    u32 safariMonCustomIds[ROGUE_SAFARI_TOTAL_CUSTOM_MONS];                 // 16
    u8  monMasteryFlags[MON_MASTERY_BYTE_COUNT];
    struct RogueHubMap           hubMap;
    struct RogueDifficultyConfig difficultyConfig;
    u16 timeOfDayMinutes;
    u16 lastKnownNumSpecies;
    u8  seasonCounter;
};
```

Campaigns: `LOW_BST, CLASSIC, MINIBOSS_BATTLER, AUTO_BATTLER, LATERMANNER, POKEBALL_LIMIT, ONE_HP`.

## 8. In-adventure vs out-of-adventure state

Rogue keeps a **run restore block** so the persistent overworld state survives while a run is active:

```c
static EWRAM_DATA struct RogueRunRestoreBlock sRunRestoreBlock = {0};   // ← src/rogue_save.c

struct RogueRunRestoreBlock {
    struct Pokemon           playerParty[PARTY_SIZE];      // 6
    struct ItemSlot          bagItems[BAG_ITEM_CAPACITY];  // 450
    struct RogueDaycarePokemon daycarePokemon[DAYCARE_SLOT_COUNT];
    struct RogueDifficultyConfig difficultyConfig;
    u32 money;
    u32 playTime;
};
```

`RogueSave_SaveHubStates()` stashes party/bag/daycare/difficulty/money/playtime into it; `RogueSave_LoadHubStates()`
restores them. The stashed copy is serialized inside the Rogue save block (it appears in the serialization order
above) — **not** in SaveBlock1.

Empirically, `SB1 + 0x234` / `+0x238` always hold *whichever* party is currently active:

| save | `SB1+0x234` partyCount | party contents |
|------|------------------------|----------------|
| out-of-adventure quicksave | 2 | Arcanine L54, Metagross L57 |
| in-adventure quicksave | 5 | Magearna L55, Arcanine L55, Scolipede L55, Metagross L55, Weavile L55 |

The PC storage region was identical in both. So the offset does not move — the *game swaps which state is loaded into
it* at the adventure boundary, and the other state rides along in `sRunRestoreBlock` inside the Rogue block. An editor
that wants to change both must decode the Rogue block, not just SaveBlock1.

## 9. Species space

Far bigger than vanilla. `include/constants/species.h` defines 1600+ species ids (max id ~1434), built from chained
anchors:

```
FORMS_START        = SPECIES_ENAMORUS_INCARNATE   (905)
GEN9_START         = SPECIES_ALCREMIE_RIBBON_RAINBOW_SWIRL
SPECIES_ROGUE_START= SPECIES_URSHIFU_RAPID_STRIKE_STYLE_GIGANTAMAX
NUM_SPECIES        = SPECIES_EGG
```

Mega forms (`FORMS_START + n`), Gigantamax forms, Gen 9 species, and Rogue's own invented forms
(`SPECIES_PIKIN_MEGA`, `SPECIES_WOBBUFFET_PUNCHING`, Rogue megas for Gen 6+ …) all occupy distinct ids. A table that
stops at vanilla's ~386 will render these as bare numbers — which is exactly what an early version of this editor did.

Names come from `.speciesName = _("Name")` in `src/data/pokemon/species_info/gen_N.h`. Note the many shared
`*_MISC_INFO` macros: several forms of one species share a block that defines the name outside the designated
initialiser, so a naive per-block regex misses them.

## 10. Items

```
ITEM_ROGUE_ITEM_FIRST = 827
ITEM_ROGUE_ITEM_LAST  = 1027      // 201 Rogue-defined items
```

Rogue item names live in `gRogueItems[]` using designated initialisers
(`[ITEM_PRESSURE_CURSE - ITEM_ROGUE_ITEM_FIRST] = { .name = _("Pressure Curse"), … }`) wrapped in
`#ifdef ROGUE_EXPANSION` blocks. Names include `Link Cable`, `Quest Book`, the curses, TRs, debug mints. Not every id
resolves from public headers — unresolved ones currently display as numeric ids.

## 11. Verified stream walk (v2.2.0 era, saveVersion 7)

Starting at `boxes[10][0]` (merged offset `0xad88`), after the 8-byte header each `SerializeArray` writes a
`u16` element count followed by `count × elemsize` bytes. The counts observed in live saves and pinned by the
test suite (`rogue_block_roundtrip_synth`):

| field | elemsize | count |
|-------|---------:|------:|
| questStates | 8 | 232 |
| campaignData | 4 | 7 |
| hubMap.areaBuiltFlags | 1 | 2 |
| hubMap.areaCoords | 4 (Coords16) | 11 |
| hubMap.upgradeFlags | 1 | 6 |
| hubMap.homeDecorations | 4 | 192 |
| hubMap.homeStyles | 1 | 3 |
| hubMap.homeWanderingMonSpecies | 2 | 10 |
| hubMap.weatherState | 2 (plain `SerializeData`) | — |
| timeOfDayMinutes / seasonCounter | 2 / 1 (plain) | — |
| safariMons | 20 | 60 |
| daycarePokemon | 84 | 3 |
| difficultyConfig.toggleBits | 1 | 3 (20 toggles) |
| difficultyConfig.rangeValues | 1 | 9 |
| dynamicUniquePokemon | 8 | 5 |
| safariMonCustomIds | 4 | 16 |
| monMasteryFlags | 1 | 72 (569 bits) |
| isDebug | 1 (plain) | — |
| debug cfg (only when isDebug) | — | skipped when 0 |
| RogueRideMonState | 7 (plain) | — |
| adventureReplay | 16 | 2 |
| gRngRogueValue | 4 (plain) | — |
| sRunRestoreBlock | 2696 (plain) | — |
| gRogueRun, currentRoomParams, currentRoomType | plain | after stash |

**Header size is version-dependent:** `lastKnownNumSpecies` (u16) is serialized only when
`saveVersion >= SAVE_VER_ID_2_1_0` (on-disk stamp 4+), so the payload starts at `boxes[10][0] + 8`
for v2.1.0-era saves and `+ 6` for older ones. The on-disk `saveVersion` equals
`SAVE_VER_ID_* - 1` (`RogueSave_GetVersionIdFor(v) = v + 1`): v2.2.0 saves carry **7**.

**seasonCounter is a day counter, not a season index.** `rogue_timeofday.c` defines
`DAYS_PER_SEASON 2`, `SEASON_COUNT 4`; `RogueToD_GetSeason() = seasonCounter / DAYS_PER_SEASON`,
and every write wraps `% (DAYS_PER_SEASON * SEASON_COUNT)` (i.e. % 8). Counter 4 = Autumn day 0.

**struct RogueSafariMon (20 B)** — bitfields confirmed against a real v2.2.0 roster (all IVs ≤31,
natures ≤24, shiny ∈{0,1}, priority 0..101) and pinned by a unit test:
```
w0: species:16 | hpIV:5 | attackIV:5 | defenseIV:5 | shinyFlag:1
w1: speedIV:5  | spAttackIV:5 | spDefenseIV:5 | pokeball:5 | nature:5 | abilityNum:2 | genderFlag:1 | unused0:4
then nickname[10] (Gen3 charset), priorityCounter u8, customMonLookup u8
```

**struct UniqueMon (8 B):** `u32 customMonId, u16 species, u16 countDown` (countDown 0 = none pending).

The stash (`sRunRestoreBlock`) is located by validity scan (sealed party mon checksums + sane money/playtime)
rather than a fixed offset, so tail-layout drift in newer builds still decodes. Quest names for the 232 slots
come from parsing `data/rogue/quests.json` with the exporter's include/#if rules (see `tools/gen_tables_v2.py`).

**Save-version migration is the game's job, not the editor's.** On load,
`RogueSave_OnSaveLoaded` calls `Rogue_NotifySaveVersionUpdated` when the stamp differs from
`ROGUE_SAVE_VERSION`; the game then re-serializes the block in its current layout (and applies
quest-level fixups from each quest's `addedInVersion` / `resetProgressInVersion`). The editor
therefore reports the stamp (`saveVersion 7 (2.2.0)`) and refuses to edit whenever the era pins
fail, instead of trying to rewrite old layouts itself.

## 12. Daycare / quests

```c
/*0x3030*/ struct DayCare daycare;      // vanilla daycare, in SaveBlock1
struct DayCare { struct DaycareMon mons[DAYCARE_MON_COUNT]; ... }
struct RogueDaycarePokemon { struct RogueBoxPokemonFacade boxMonFacade; /*80B*/ u8 unused:1; }
```

Rogue's daycare is `daycarePokemon[3]` inside the Rogue save block *and* mirrored in `sRunRestoreBlock`, so the
in-run and out-of-run daycare are separate. Quest state is `RogueQuestState` bitfields
(`stateFlags:16`, `highestCompleteDifficulty:3`, `highestCollectedRewardDifficulty:3`).

## 13. Verification checklist for any editor

1. Pick the live slot by footer counter; validate every sector checksum.
2. Decode with the *Rogue* struct sizes (104 for party).
3. After any edit: re-seal the mon (checksum + encryption) **and** the sector checksum.
4. Preserve sector rotation, footers, and the mGBA trailer byte-for-byte. A `noop` resave must be byte-identical —
   this project tests exactly that with `cmp`.
5. Never commit `.sav` files: they carry player names and TID/SID.
