s = open('src/save_tests.rs').read()
s = s.replace('''#![allow(unused_imports)]
use crate::format::*;
use crate::game_data::*;
use crate::mon::*;
use crate::savefile::*;''', '''#![allow(unused_imports)]
use crate::basestats::base_stats;
use crate::deco_variants;
use crate::format::*;
use crate::game_data::*;
use crate::item_pockets;
use crate::mon::*;
use crate::savefile::*;
use crate::tables::NATURES;''')
open('src/save_tests.rs', 'w').write(s)

s = open('src/savefile.rs').read()
s = s.replace('    fn bag_slots_raw(&self, stash: bool)',
              '    pub(crate) fn bag_slots_raw(&self, stash: bool)')
open('src/savefile.rs', 'w').write(s)
print('ok')
