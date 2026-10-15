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

**PP Ups live in one byte, 2 bits per move slot.** `gPPUpGetMask` / `gPPUpClearMask` /
`gPPUpAddValues` (`src/pokemon.c:424-426`) select one slot's 2-bit field, and the cap formula is
`base + (base * 20 * ups) / 100` (`CalculatePPWithBonus`, `:3412`). The stored field never exceeds 3:
the PP Up item requires `ups <= 2` (`:3638`) and PP Max sets the field to exactly 3 (`:3914-3918`). Both
items also refuse a move whose current max PP is under 5, which is how Sketch (1 PP) is excluded. When an
item raises the field it adds `newCap - oldCap` to the move's **current** PP (`:3643-3645`,
`:3920-3922`) rather than refilling it.

**The met-location byte is a region-map section id, not a map id.** The game stores
`GetCurrentRegionMapSectionId()` there at catch/hatch time (`src/pokemon.c:946-947`) and compares it with
the section the player is standing in elsewhere (`:3508`). The ids are the `MAPSEC_*` constants in
`include/constants/region_map_sections.h` — `0x00` POKEMON_HUB through `0xD5` NONE — so section **0 is a
real place**, not "unset", plus the three specials the game writes directly: `METLOC_SPECIAL_EGG` `0xFD`,
`METLOC_IN_GAME_TRADE` `0xFE`, `METLOC_FATEFUL_ENCOUNTER` `0xFF`. `tools/gen_map_secs.py` regenerates the
editor's id → name table from that header, which is what lets the mon panel print "Route 116" instead of
"loc 31".

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
| hubMap.areaCoords | 4 (`Coords8` + 2 pad, see §13) | 11 |
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

**An editor note on hand-added spawn entries.** Pushing a caught mon to the Safari Zone roster cannot copy the
record: these 20 bytes hold species, the six IVs, nature, ability slot, gender, ball, shiny and a nickname, and
nothing else - no level, EXP, EVs, moves, OT/PID or stats. The editor fills exactly those fields and sets
`priorityCounter` to 100 (above the values the game's own spawn tree uses), so the entry it adds is actually
rolled. Adding to a full roster is refused rather than overwriting a live entry.

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
in-run and out-of-run daycare are separate. That is why the editor's "add a copy to daycare" writes one roster
and not both: it picks the one the save is currently showing (`stash.ok` tells it which) and says so in the
status line - filling both would put the same mon into two states the game treats as different. Quest state is `RogueQuestState` bitfields
(`stateFlags:16`, `highestCompleteDifficulty:3`, `highestCollectedRewardDifficulty:3`).

## 13. Hub map fields (v2.2.0)

**`areaCoords[11]` has a 4-byte stride, not 2.** `include/rogue.h` declares
`struct Coords8 areaCoords[AREA_COUNT]` (`s8 x; s8 y;` = 2 bytes) and `SerializeArray` uses
`sizeof(struct Coords8)` — but a stride-2 read desynchronises every field after it
(`upgradeFlags`, `homeDecorations`, styles, wander slots, weather, time, season, safari, daycare,
difficulty, …) in both real saves, and the game's own accessor
(`src/rogue_hub.c:4212`) reads `x + (i + 1) * 4` while walking neighbours. The observed layout is
`s8 x, s8 y` followed by two zero pad bytes. Brute-forcing x/y per area against `er2.sav` and a
second independent save pinned this: stride 4 validates all downstream era pins, stride 2 does not.
Consequences: coords are **signed 8-bit** (−128..127), and the built flag and coordinate are
**separate state** — every area in both saves carries a coordinate, built or not (unbuilt areas
hold sentinel positions such as `(-1,0)` that the game ignores).

**Reachability model.** The player moves between areas by walking onto edge tiles whose neighbour
exists: `RogueHub_GetAreaAtConnection` looks up the adjacent coordinate in `gRogueHubAreas` and
returns NULL unless that area's built bit is set. There is no warp table in the save. So a
coordinate is only meaningful if the tile is 4-connected to another built area, and two built
areas on the same tile are one unreachable area. The editor validates exactly these three failure
modes (collision, island, stranded neighbour) before applying a `hub-coord` edit. The game's own
builder uses a spiral search for a free adjacent tile (`RogueHub_BuildArea` →
`FindAdjacentTileForArea`); `RogueHub_ClearProgress` then hard-codes the canonical layout and
zeroes coords 4..10, confirming the coord table is per-save state and not ROM data.

**`weatherState` is a raw u16 seed, not a weather id.** The game folds it through a per-season modulo
in `Rogue_ModifyOverworldMapWeather` (`src/rogue_controller.c:1918-2011`) to choose the weather an
outdoor map shows:

| season | modulo | buckets → weather |
|---|---|---|
| spring | `% 5` | 0 Rain, 1 Rain Thunderstorm, 2 Fog Diagonal (misty terrain), 3-4 none |
| summer | `% 5` | 0 Rain Thunderstorm, 1-4 none |
| autumn | `% 6` | 0 Rain, 1 Rain Thunderstorm, 2-3 Leaves (grassy terrain), 4-5 none |
| winter | `% 6` | 0-2 Snow, 3 Shade, 4-5 none |

The names are the `WEATHER_*` ids in `include/constants/weather.h`; `tools/gen_weather_names.py`
regenerates them and `game_data::hub_weather_for` mirrors the table above, so the editor can print the
weather instead of the bucket number. An earlier revision of this document attributed the buckets to
`RogueToD_UpdateWeather` with a `W_SUNNY/W_RAIN/W_STORM/W_FOG/W_SUNRISE` set — neither that function nor
those names exist in the v2.2.0-EX tree. Two more paths bypass the seed entirely: during a run the route
rooms use `VAR_ROGUE_DESIRED_WEATHER` (:1929-1933), and before the tutorial finishes the game forces no
weather (:1934-1936).

`0xFFFF` is the "no save data yet" sentinel: `65535 % 5 = 0` and `65535 % 6 = 3`, so a sentinel save is
not "no weather" but a fixed one — Rain in spring, Rain Thunderstorm in summer, Leaves in autumn, Shade
in winter. Setting `0xFFFF` therefore restores the game's default behaviour.

**`statueLevel` is never serialized.** `SerializeRogueBlockInternal` writes `weatherState`,
`timeOfDayMinutes`, `seasonCounter` but no statue field — it is derived from quest-completion state
on load (`RogueHub_SetStatueLevel` after the version-notify call). Edit quest state instead.

**`homeWanderingMonSpecies[10]` (u16).** A follow-mon's shininess is encoded in the same u16:
`FollowMon_GetMonGraphics` compares against `species + FOLLOWMON_SHINY_OFFSET`, and the game sets
`SHINY_FOLLOWS = 1` in `include/config/rogue.h` (v2.2.0). The offset is **10000**
(`include/rogue_hub.h:211`), so a shiny Haxorus (2728) is stored as 12728. `0` means
`SPECIES_NONE`, not the first species. Follow-mons only spawn inside the player's house.

**`homeStyles[3]` (u8).** Indices are the order `RogueHub_InitHouseStyles` uses: 0 = exterior
(`gRogueHouseExteriorStyles`, 4 variants, first entry has `species = 0xFFFF`), 1 = interior
(`gRogueHouseInteriorStyles`, 1 variant — the game forces any out-of-range value back to 0),
2 = path (`gRogueHousePathStyles`, 6 variants). Out-of-range values are clamped on load, so write
only in-range ids.

**`homeDecorations[192]` (4 B each: `u8 x, y, decorVariant, active:1`).** Slot ranges from
`include/constants/rogue_hub.h`: 0..159 are the **outside** yard (`HOME_DECOR_OUTSIDE_COUNT 160`,
x/y limited to 21..106), 160..191 are **inside** the house (`HOME_DECOR_INSIDE_COUNT 32`, x/y 5..20
/ 5..15). `decorVariant` indexes the ROM table `sDecorationVariants` (`src/rogue_hub.c:237`) —
`0` means no decoration; the game skips the first entry when cycling. Valid ids observed in the
table run up to ~118 (`DECOR_VARIANT_WARDROBE_3`). `active` is a 1-bit flag — write 0 or 1, not the
whole byte. Reference point: `RogueHub_ClearProgress` seeds slot 0 at (21,19) with
`DECOR_VARIANT_OUTFIT_CHANGING_WARDROBE` (id 104), and that is exactly what a real save carries.

## 13b. Typed field domains (`src/domain.rs`)

The game declares most of these fields as anonymous enums. The editor mirrors them
as distinct Rust types so the compiler catches unit errors that raw integers allow:
`set_season_day(SeasonDay)` cannot take a season index, and the three `homeStyles`
slots are three separate types (an exterior id is meaningless indoors, and vice
versa: exterior has 5 entries, interior 6, path 5).

Every type keeps the byte it read. `from_raw` never rejects, `raw()` returns what
was on disk, and `is_known()` tells the UI whether to name the value or warn. That
is a format requirement, not a style choice: the ball field is a 5-bit **item id**
whose numbering is branch-specific (see below), ability slot 2 only exists in the
expansion build's 2-bit field, and the effective nature (`pid % 25` XOR the stored
modifier) can exceed 24. An enum that
clamped on read would turn a byte the game wrote into a byte the editor invented,
which breaks the byte-for-byte round-trip the tests hold.

| Type | Field | Domain |
|---|---|---|
| `Nature` | nature (pid XOR modifier) | 0..=24 named, `Unexpected(u8)` otherwise |
| `GenderFlag` | `genderFlag` | 0/1, `Unexpected` on older-struct misreads |
| `AbilitySlot` | `abilityNum` | 0/1/2 (2 = hidden, 2-bit field only) |
| `StatIdx` | IV/EV column order | hp, atk, def, spe, spa, spd |
| `Season` / `SeasonDay` | `seasonCounter` | the field stores DAYS (2 per season, wraps over 8) |
| `ExteriorStyle`/`InteriorStyle`/`PathStyle` | `homeStyles[0..3]` | separate tables, separate types |
| `HubArea` / `Upgrade` | `areaBuiltFlags`, `upgradeFlags` | named by `HUB_AREAS` / `HUB_UPGRADES` |
| `DecorVariant` | `homeDecorations[].decorVariant` | `DecorationExporter` enum order |
| `Ball` | `pokeball:5` | a 5-bit **item id**: `FIRST_BALL..=LAST_BALL` = 1..=28 here (`ITEM_POKE_BALL` .. `ITEM_ROGUE_BALL`). Names come from the item table itself (`game_data::balls()`), never a second hand-written list |

### Ball ids: the field is an item id, and the numbering is branch-specific

`/*0x0A*/ u16 pokeball:5; //31 balls` (`include/pokemon.h:120`). A ball IS an item to
the game: `value = ITEM_POKE_BALL; SetBoxMonData(boxMon, MON_DATA_POKEBALL, &value)`
(`src/pokemon.c:950-951`), a ball check reads
`GetMonData(.., MON_DATA_POKEBALL) == ITEM_LUXURY_BALL` (`src/pokemon.c:3506`), and the
summary screen converts the stored id back to a sprite with `ItemIdToBallId`
(`src/pokemon_summary_screen.c:4645`). So a mon's ball value is an id into the item
table, and the same value is the item icon key.

Two trees number that range differently (`FIRST_BALL`/`LAST_BALL`,
`include/constants/items.h:39-40` in the expansion tree, `:24-25` in the 2.2.07-era
tree). The game's own header flags the hazard —
`// Note: If moving ball IDs around, updating FIRST_BALL/LAST_BALL is not sufficient`.

| tree | ITEM_POKE_BALL | ITEM_MASTER_BALL | ITEM_ULTRA_BALL | ITEM_ROGUE_BALL | `FIRST_BALL` |
|---|---|---|---|---|---|
| expansion / EX (this editor, verified live) | 1 | 4 | 3 | 28 | `ITEM_POKE_BALL` |
| 2.2.07-era tree (`rogue-src-2207`) | 4 | 1 | 2 | 13 | `ITEM_MASTER_BALL` |

A hand-written ball list carrying the 2.2.07-era numbering shipped in this editor
and mislabelled real saves: a save holding 17 mons at id 1 displayed them as Master
Balls, ids 23 (Safari Ball) and 27 (Cherish Ball) had no entry at all, and the bag tab
called those same ids what they really are. `balls()` now filters the generated item
table to `FIRST_BALL..=LAST_BALL`, with tests pinning id 1 = Poke Ball / id 4 = Master
Ball / id 28 = Rogue Ball, requiring the range to be contiguous (a gap means the branch
reordered items), requiring every ball name to equal the item name for that id, and
checking every ball in a real save names a known ball.
| `Pocket` | item pocket | id-keyed; `POCKET_NONE` items cannot be bagged |
| `ConfigToggle` / `ConfigRange` | Lab difficulty config | toggle bit / range slot |
| `BattleFormat`..`TrainerOrder` | range slot values | one option table per slot |

## 14. Verification checklist for any editor

1. Pick the live slot by footer counter; validate every sector checksum.
2. Decode with the *Rogue* struct sizes (104 for party).
3. After any edit: re-seal the mon (checksum + encryption) **and** the sector checksum.
4. Preserve sector rotation, footers, and the mGBA trailer byte-for-byte. A `noop` resave must be byte-identical —
   this project tests exactly that with `cmp`.
5. Never commit `.sav` files: they carry player names and TID/SID.
