#!/usr/bin/env python3
"""One-off: split src/main.rs into focused modules (format, game_data, mon,
savefile, app/). Run once; afterwards edit the modules directly."""
import os

src = open('src/main.rs', encoding='utf-8').read().splitlines(keepends=True)


def seg(a, b):  # 1-based inclusive
    return ''.join(src[a - 1:b])


os.makedirs('src/app', exist_ok=True)
mods = {}

mods['format.rs'] = (
    '//! Save-container format primitives: sector geometry, merged-stream offsets,\n'
    '//! endian helpers, Gen-3 string codec, and the substruct permutation table.\n'
    '//! Offsets verified against the game source and live saves (see docs/FORMAT.md).\n\n'
    + seg(31, 155) + '\n' + seg(156, 159) + '\n'
    + seg(326, 344) + '\n' + seg(345, 389) + '\n'
)

mods['game_data.rs'] = (
    '//! Game-side data tables and their lookups (names, pockets, difficulty config,\n'
    '//! seasons, hub areas/upgrades). These mirror constants from the game tree; the\n'
    '//! big static tables live in tables.rs / tables_extra.rs / move_pp.rs.\n\n'
    'use crate::move_pp::MOVE_PP;\nuse crate::tables;\nuse crate::tables_extra;\n'
    'use std::sync::OnceLock;\n\n'
    + seg(160, 324) + '\n' + seg(390, 464) + '\n'
)

mods['mon.rs'] = (
    '//! Pokemon structs (80B BoxPokemon / 104B party Pokemon): decode, encode,\n'
    '//! bitfield accessors, and stat math. Level derives from exp (Rogue uses a\n'
    '//! flat exp schedule), see EXP_PER_LEVEL.\n\n'
    'use crate::format::*;\nuse crate::game_data::*;\n\n'
    + seg(465, 822) + '\n' + seg(2249, 2276) + '\n'
)

mods['savefile.rs'] = (
    '//! The open save file: sector container, active-slot selection, merged stream,\n'
    '//! party/box/item accessors, Rogue block walk, stash, safari/unique mons, and\n'
    '//! write-back with checksum re-sealing (mGBA trailer preserved).\n\n'
    'use crate::format::*;\nuse crate::game_data::*;\nuse crate::mon::*;\n\n'
    + seg(823, 2197) + '\n'
)

mods['save_tests.rs'] = (
    '#![allow(unused_imports)]\n'
    'use crate::format::*;\nuse crate::game_data::*;\nuse crate::mon::*;\n'
    'use crate::savefile::*;\n\n'
    + seg(2279, 3089) + '\n'
)

mods['app/mod.rs'] = (
    '//! The eframe application: layout, tab dispatch, selection model, and file\n'
    '//! open/save plumbing. Per-tab UIs live in the sibling modules as `impl Editor`\n'
    '//! blocks — edit those files, not this one, for tab-specific changes.\n\n'
    'use crate::format::*;\nuse crate::game_data::*;\nuse crate::mon::*;\nuse crate::savefile::*;\n\n'
    + seg(3090, 3286) + '\n' + seg(4747, 4857)
)

mods['app/rogue_ui.rs'] = (
    '//! Rogue-tab UI: adventure state, hub map, quests, difficulty, daycare,\n'
    '//! safari roster, unique mons.\n\n'
    'use super::*;\n\nimpl Editor {\n' + seg(3287, 4162) + '\n}\n'
)
mods['app/mon_ui.rs'] = (
    '//! Central Pokemon editor pane (fields of the selected mon).\n\n'
    'use super::*;\n\nimpl Editor {\n' + seg(4165, 4505) + '\n}\n'
)
mods['app/items_ui.rs'] = (
    '//! Items tab: bag/PC/stash editing, sort, add row.\n\n'
    'use super::*;\n\nimpl Editor {\n' + seg(4507, 4661) + '\n}\n'
)
mods['app/trainer_ui.rs'] = (
    '//! Trainer tab: TID/SID, name, playtime.\n\n'
    'use super::*;\n\nimpl Editor {\n' + seg(4663, 4744) + '\n}\n'
)

for p, body in mods.items():
    path = os.path.join('src', p)
    open(path, 'w', encoding='utf-8').write(body)
    print('wrote', path, len(body.splitlines()), 'lines')

open('src/main.rs', 'w', encoding='utf-8').write(
    '//! er-save-editor — PKHeX-like editor for Pokémon Emerald Rogue (v2.x EX) saves.\n'
    '//!\n'
    '//! Format grounded in Pokabbie/pokeemerald-rogue (expansion branch):\n'
    '//!   include/save.h          sector layout (4084+12 footer, security 0x08012025)\n'
    '//!   include/pokemon.h       BoxPokemon(80)/Pokemon(104), substruct bitfields\n'
    '//!   include/pokemon_storage_system.h  14 boxes x 30, names, wallpapers\n'
    '//!   src/save.c              CalculateChecksum\n'
    '//!   src/pokemon.c           EncryptBoxMon/DecryptBoxMon/GetSubstruct/CalculateBoxMonChecksum\n'
    '//!\n'
    '//! Module map:\n'
    '//!   format.rs             sector geometry + byte helpers (offsets, endians, codecs)\n'
    '//!   game_data.rs          game tables and lookups (names, seasons, config, pockets)\n'
    '//!   mon.rs                Pokemon structs: decode/encode/stat math\n'
    '//!   savefile.rs           the open save: slots, merged stream, accessors, write-back\n'
    '//!   app/                  the GUI (layout in app/mod.rs, per-tab UIs alongside)\n'
    '//!   searchable_combo.rs   searchable dropdown widget (ComboBox replacement)\n'
    '//!\n'
    '//! Run:  er-save-editor FILE.sav  (or --open FILE.sav)  -> opens read-only in the GUI.\n'
    '//! Nothing is written to disk unless Save / Save As is pressed (see README).\n\n'
    'mod app;\nmod basestats;\nmod deco_variants;\nmod format;\nmod game_data;\n'
    'mod item_pockets;\nmod mon;\nmod move_pp;\nmod savefile;\nmod searchable_combo;\n'
    'mod tables;\nmod tables_extra;\n\n'
    '#[cfg(test)]\nmod save_tests;\n\n'
    'use app::Editor;\n\n'
    '/// write a line to stdout without panicking on broken pipes (e.g. `| head`)\n'
    'macro_rules! say {\n'
    '    ($($a:tt)*) => {{\n'
    '        use std::io::Write as _;\n'
    '        let _ = writeln!(std::io::stdout(), $($a)*);\n'
    '    }};\n'
    '}\n\n'
    + seg(2198, 2248)
)
print('wrote src/main.rs')
