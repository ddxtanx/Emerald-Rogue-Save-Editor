//! er-save-editor — PKHeX-like editor for Pokémon Emerald Rogue (v2.x EX) saves.
//!
//! Format grounded in Pokabbie/pokeemerald-rogue (expansion branch):
//!   include/save.h          sector layout (4084+12 footer, security 0x08012025)
//!   include/pokemon.h       BoxPokemon(80)/Pokemon(104), substruct bitfields
//!   include/pokemon_storage_system.h  14 boxes x 30, names, wallpapers
//!   src/save.c              CalculateChecksum
//!   src/pokemon.c           EncryptBoxMon/DecryptBoxMon/GetSubstruct/CalculateBoxMonChecksum
//!
//! Module map:
//!   format.rs             sector geometry + byte helpers (offsets, endians, codecs)
//!   game_data.rs          game tables and lookups (names, seasons, config, pockets)
//!   mon.rs                Pokemon structs: decode/encode/stat math
//!   savefile.rs           the open save: slots, merged stream, accessors, write-back
//!   app/                  the GUI (layout in app/mod.rs, per-tab UIs alongside)
//!
//! Run:  er-save-editor FILE.sav  (or --open FILE.sav)  -> opens read-only in the GUI.
//! Nothing is written to disk unless Save / Save As is pressed (see README).

mod abilities;
mod app;
mod area_popup;
mod basestats;
mod deco_variants;
mod format;
mod game_data;
mod icons;
mod item_pockets;
mod lookup;
mod mon;
mod move_pp;
mod savefile;
mod searchable_combo;
mod species_meta;
mod tables;
mod tables_extra;

#[cfg(test)]
mod save_tests;

use app::Editor;

/// write a line to stdout without panicking on broken pipes (e.g. `| head`)
macro_rules! say {
    ($($a:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($a)*);
    }};
}

fn main() -> eframe::Result<()> {
    // The CLI does exactly one thing: pass a save to open it in the GUI.
    //   er-save-editor FILE.sav
    //   er-save-editor --open FILE.sav     (explicit, equivalent)
    // All editing happens in the GUI, and opening is read-only: nothing is
    // written to disk unless the user presses Save / Save As.
    let args: Vec<String> = std::env::args().collect();
    let mut open_path: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--open" => {
                if i + 1 < args.len() {
                    open_path = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            "--help" | "-h" => {
                say!("usage: er-save-editor [FILE.sav | --open FILE.sav]   open a save in the GUI");
                return Ok(());
            }
            s if s.starts_with('-') => {
                eprintln!("unknown option: {s} (try --help)");
                std::process::exit(1);
            }
            s => {
                open_path = Some(s.to_string());
            }
        }
        i += 1;
    }

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1150.0, 780.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Emerald Rogue Save Editor",
        opts,
        Box::new(move |_cc| {
            let mut ed = Editor::default();
            if let Some(p) = open_path.clone() {
                ed.open_path(p);
            }
            Box::new(ed)
        }),
    )
}
