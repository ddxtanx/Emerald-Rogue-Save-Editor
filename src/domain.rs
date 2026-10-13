//! Typed views of the game's small finite fields.
//!
//! These exist so the compiler does the auditing. Before them, `homeStyles[0]`,
//! a season index, and a day counter were all `u8` passed to three different
//! setters, and `set_hub_season(4)` (a day, not a season) compiled silently. Now
//! each domain is its own type and mixing them is a type error.
//!
//! Every type keeps the on-disk value even when it is outside the game's enum,
//! because the editor must round-trip a real save byte for byte: Rogue ball ids
//! run past the vanilla table, ability slots depend on the save version, and a
//! corrupt save can hold any byte. So `from_raw` never rejects, `raw()` returns
//! what was read, and `is_known()` tells the UI whether to name the value or
//! warn about it. Nothing here clamps or substitutes on read — that would turn
//! a byte the game wrote into a byte we invented.

use crate::format::{HOME_STYLE_EXTERIOR, HOME_STYLE_INTERIOR, HOME_STYLE_PATH};
use crate::game_data::{
    BALLS, BATTLE_FORMATS, CONFIG_RANGES, CONFIG_TOGGLES, DIFF_PRESETS, GAME_MODES, REVISION_MODES,
    TRAINER_ORDERS,
};
use crate::game_data::{HUB_AREAS, HUB_UPGRADES, SEASONS};

/// The common shape of the domains whose values index a name table. Lets one
/// UI helper serve hub areas, upgrades, home styles and Lab config values
/// instead of a near-duplicate block per domain.
pub trait Domain: Copy + std::fmt::Display {
    fn from_raw(v: u8) -> Self;
    fn raw(self) -> u8;
    fn name(self) -> &'static str;
    fn is_known(self) -> bool;
    fn table_len() -> usize;
    fn all_vals() -> Vec<Self> {
        (0..Self::table_len() as u8).map(Self::from_raw).collect()
    }
}

/// Turbofish-free spelling of `<D as Domain>::all_vals()`.
pub fn values<D: Domain>() -> Vec<D> {
    D::all_vals()
}

/// An index-into-a-name-table domain (hub areas, upgrades, home styles, Lab
/// config slots and their option sets). The table is the domain: adding a
/// variant per row would duplicate it and drift.
macro_rules! indexed_domain {
    ($(#[$meta:meta])* $ty:ident, $table:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Default)]
        pub struct $ty(pub u8);
        impl $ty {
            pub fn table() -> &'static [&'static str] {
                $table
            }
            /// Accepts any value; use `is_known` to decide whether to name it.
            pub fn from_raw(v: u8) -> Self {
                Self(v)
            }
            pub fn raw(self) -> u8 {
                self.0
            }
            pub fn is_known(self) -> bool {
                Self::table().get(self.0 as usize).is_some()
            }
            /// The game's own name, or `"?"` when the value is outside the table.
            pub fn name(self) -> &'static str {
                Self::table().get(self.0 as usize).copied().unwrap_or("?")
            }
        }
        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                if self.is_known() {
                    f.write_str(self.name())
                } else {
                    write!(f, "unknown {} ({})", stringify!($ty), self.0)
                }
            }
        }
        impl crate::domain::Domain for $ty {
            fn from_raw(v: u8) -> Self {
                $ty::from_raw(v)
            }
            fn raw(self) -> u8 {
                $ty::raw(self)
            }
            fn name(self) -> &'static str {
                $ty::name(self)
            }
            fn is_known(self) -> bool {
                $ty::is_known(self)
            }
            fn table_len() -> usize {
                $ty::table().len()
            }
        }
    };
}

indexed_domain!(
    /// `areaBuiltFlags` / `areaCoords` index (`HUB_AREA_*`, `HUB_AREA_COUNT`).
    HubArea,
    HUB_AREAS
);
indexed_domain!(
    /// `upgradeFlags` bit index, named after the upgrade it unlocks.
    Upgrade,
    HUB_UPGRADES
);
indexed_domain!(
    /// `homeStyles[0]` (`EXTERIOR_STYLE_*`), not interchangeable with the other
    /// two style slots.
    ExteriorStyle,
    HOME_STYLE_EXTERIOR
);
indexed_domain!(
    /// `homeStyles[1]` (`INTERIOR_STYLE_*`).
    InteriorStyle,
    HOME_STYLE_INTERIOR
);
indexed_domain!(
    /// `homeStyles[2]` (`PATH_STYLE_*`).
    PathStyle,
    HOME_STYLE_PATH
);
indexed_domain!(
    /// Lab difficulty-config toggle bit (`CONFIG_TOGGLE_*`).
    ConfigToggle,
    CONFIG_TOGGLES
);
indexed_domain!(
    /// Lab difficulty-config range slot (`CONFIG_RANGE_*`). Which value domain
    /// applies is per slot — see [`ConfigRange::options`].
    ConfigRange,
    CONFIG_RANGES
);
indexed_domain!(
    /// Value of the battle-format range slot.
    BattleFormat,
    BATTLE_FORMATS
);
indexed_domain!(
    /// Value of the game-mode range slot.
    GameMode,
    GAME_MODES
);
indexed_domain!(
    /// Value of the difficulty-preset range slot.
    DiffPreset,
    DIFF_PRESETS
);
indexed_domain!(
    /// Value of the adventure-revision-mode range slot.
    RevisionMode,
    REVISION_MODES
);
indexed_domain!(
    /// Value of the trainer-order range slot.
    TrainerOrder,
    TRAINER_ORDERS
);

impl ConfigRange {
    /// The option list this slot's value is drawn from, empty for slots holding
    /// a plain number. This is the one place the slot-to-domain mapping lives:
    /// it used to be a `match i { 3 => .., 5 => .. }` at the call site.
    pub fn options(self) -> &'static [&'static str] {
        match self.0 {
            3 => BATTLE_FORMATS,
            5 => GAME_MODES,
            6 => DIFF_PRESETS,
            7 => REVISION_MODES,
            8 => TRAINER_ORDERS,
            _ => &[],
        }
    }
    /// Whether this slot's value is a signed offset (trainer level shift).
    pub fn is_signed(self) -> bool {
        self.0 == 0
    }
}

indexed_domain!(
    /// `homeDecorations[].decorVariant`. Ids come from the game's own
    /// `DecorationExporter` enum, reproduced by `tools/gen_deco_variants.py`, so an
    /// out-of-range id is a real possibility in a hand-edited save.
    DecorVariant,
    crate::deco_variants::DECOR_VARIANTS
);

/// `ball` field. Unlike the indexed domains this is an *id*, not a table index:
/// the vanilla table runs 1..=15 with gaps and Rogue adds ids past it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Ball(pub u16);
impl Ball {
    pub fn from_raw(v: u16) -> Self {
        Self(v)
    }
    pub fn raw(self) -> u16 {
        self.0
    }
    pub fn is_known(self) -> bool {
        BALLS.iter().any(|(id, _)| *id == self.0)
    }
    /// The ball's name, or `"?"` for an id the table has no entry for.
    pub fn name(self) -> &'static str {
        BALLS
            .iter()
            .find(|(id, _)| *id == self.0)
            .map(|(_, n)| *n)
            .unwrap_or("?")
    }
}
impl std::fmt::Display for Ball {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_known() {
            f.write_str(self.name())
        } else {
            write!(f, "unknown ball ({})", self.0)
        }
    }
}

/// `Nature` (`enum Nature`, `pid % 25`, plus the hidden-nature modifier the
/// expansion branch allows). Named variants so call sites read as
/// `Nature::Adamant`; the display name comes from the shared table, so the two
/// cannot drift apart.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Nature {
    Hardy,
    Lonely,
    Brave,
    Adamant,
    Naughty,
    Bold,
    Docile,
    Relaxed,
    Impish,
    Lax,
    Timid,
    Hasty,
    Serious,
    Jolly,
    Naive,
    Modest,
    Mild,
    Quiet,
    Bashful,
    Rash,
    Calm,
    Gentle,
    Sassy,
    Careful,
    Quirky,
    /// Outside 0..=24. Kept, not clamped: an edited save may legitimately hold it.
    Unexpected(u8),
}
impl Nature {
    /// Accepts any value; `Unexpected` carries the byte through unchanged.
    pub fn from_raw(v: u8) -> Self {
        match v {
            0 => Self::Hardy,
            1 => Self::Lonely,
            2 => Self::Brave,
            3 => Self::Adamant,
            4 => Self::Naughty,
            5 => Self::Bold,
            6 => Self::Docile,
            7 => Self::Relaxed,
            8 => Self::Impish,
            9 => Self::Lax,
            10 => Self::Timid,
            11 => Self::Hasty,
            12 => Self::Serious,
            13 => Self::Jolly,
            14 => Self::Naive,
            15 => Self::Modest,
            16 => Self::Mild,
            17 => Self::Quiet,
            18 => Self::Bashful,
            19 => Self::Rash,
            20 => Self::Calm,
            21 => Self::Gentle,
            22 => Self::Sassy,
            23 => Self::Careful,
            24 => Self::Quirky,
            other => Self::Unexpected(other),
        }
    }
    pub fn raw(self) -> u8 {
        match self {
            Self::Hardy => 0,
            Self::Lonely => 1,
            Self::Brave => 2,
            Self::Adamant => 3,
            Self::Naughty => 4,
            Self::Bold => 5,
            Self::Docile => 6,
            Self::Relaxed => 7,
            Self::Impish => 8,
            Self::Lax => 9,
            Self::Timid => 10,
            Self::Hasty => 11,
            Self::Serious => 12,
            Self::Jolly => 13,
            Self::Naive => 14,
            Self::Modest => 15,
            Self::Mild => 16,
            Self::Quiet => 17,
            Self::Bashful => 18,
            Self::Rash => 19,
            Self::Calm => 20,
            Self::Gentle => 21,
            Self::Sassy => 22,
            Self::Careful => 23,
            Self::Quirky => 24,
            Self::Unexpected(v) => v,
        }
    }
    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unexpected(_))
    }
    /// Display name from the shared nature table (keyed on the raw value).
    pub fn name(self) -> &'static str {
        crate::tables::NATURE_IDS
            .iter()
            .find(|(id, _)| *id == self.raw() as u16)
            .map(|(_, n)| *n)
            .unwrap_or("?")
    }
}
impl std::fmt::Display for Nature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
impl Domain for Nature {
    fn from_raw(v: u8) -> Self {
        Nature::from_raw(v)
    }
    fn raw(self) -> u8 {
        Nature::raw(self)
    }
    fn name(self) -> &'static str {
        Nature::name(self)
    }
    fn is_known(self) -> bool {
        Nature::is_known(self)
    }
    fn table_len() -> usize {
        crate::tables::NATURE_IDS.len()
    }
}

/// Item pocket (`POCKET_*`). Ids are sparse and the display names live in
/// `POCKET_ORDER`, so this is an id wrapper rather than a positional enum.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Pocket(pub u8);
impl Pocket {
    /// `POCKET_NONE`: the id has no pocket, and the game refuses to bag it.
    pub const NONE: Pocket = Pocket(0);
    pub fn from_raw(v: u8) -> Self {
        Self(v)
    }
    pub fn raw(self) -> u8 {
        self.0
    }
    pub fn is_known(self) -> bool {
        self.0 == 0
            || crate::item_pockets::POCKET_ORDER
                .iter()
                .any(|(_, p)| *p == self.0)
    }
    /// Human name from `POCKET_ORDER` (which stores display names, not the C
    /// constant names), falling back to the id.
    pub fn name(self) -> String {
        match crate::item_pockets::POCKET_ORDER
            .iter()
            .find(|(_, p)| *p == self.0)
        {
            Some((n, _)) => (*n).to_string(),
            None => format!("pocket {}", self.0),
        }
    }
}
impl std::fmt::Display for Pocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

/// `PokemonSubstruct3.genderFlag`. Width 3 in the expansion branch, 1 on older
/// structs; values above 1 on a 1-bit struct mean the layout assumption is
/// wrong for that save, which is worth distinguishing from "female".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenderFlag {
    Male,
    Female,
    /// Not a legal flag value on this struct version.
    Unexpected(u8),
}
impl GenderFlag {
    pub fn from_raw(v: u8) -> Self {
        match v {
            0 => Self::Male,
            1 => Self::Female,
            other => Self::Unexpected(other),
        }
    }
    pub fn raw(self) -> u8 {
        match self {
            Self::Male => 0,
            Self::Female => 1,
            Self::Unexpected(v) => v,
        }
    }
    pub fn is_expected(self) -> bool {
        !matches!(self, Self::Unexpected(_))
    }
}

impl std::fmt::Display for GenderFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Male => f.write_str("male"),
            Self::Female => f.write_str("female"),
            Self::Unexpected(v) => write!(f, "gender flag {v} (not 0/1 on this struct)"),
        }
    }
}

/// Which of the species' ability slots the mon uses. Slot 2 is the hidden
/// ability and only exists in the expansion build's 2-bit field; a save written
/// by an older struct cannot hold it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbilitySlot {
    First,
    Second,
    Hidden,
    Unexpected(u8),
}
impl AbilitySlot {
    pub fn from_raw(v: u8) -> Self {
        match v {
            0 => Self::First,
            1 => Self::Second,
            2 => Self::Hidden,
            other => Self::Unexpected(other),
        }
    }
    pub fn raw(self) -> u8 {
        match self {
            Self::First => 0,
            Self::Second => 1,
            Self::Hidden => 2,
            Self::Unexpected(v) => v,
        }
    }
}

impl std::fmt::Display for AbilitySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::First => f.write_str("ability slot 1"),
            Self::Second => f.write_str("ability slot 2"),
            Self::Hidden => f.write_str("hidden ability"),
            Self::Unexpected(v) => write!(f, "ability slot {v} (out of range)"),
        }
    }
}

/// Stat index, so `set_iv` call sites cannot pass an off-by-one column. The
/// struct packs IVs in this order (hp, atk, def, spe, spa, spd).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum StatIdx {
    Hp,
    Atk,
    Def,
    Spe,
    Spa,
    Spd,
}
impl StatIdx {
    pub const ALL: [StatIdx; 6] = [
        Self::Hp,
        Self::Atk,
        Self::Def,
        Self::Spe,
        Self::Spa,
        Self::Spd,
    ];
    pub fn index(self) -> usize {
        match self {
            Self::Hp => 0,
            Self::Atk => 1,
            Self::Def => 2,
            Self::Spe => 3,
            Self::Spa => 4,
            Self::Spd => 5,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Hp => "HP",
            Self::Atk => "Atk",
            Self::Def => "Def",
            Self::Spe => "Spe",
            Self::Spa => "SpA",
            Self::Spd => "SpD",
        }
    }
}

/// `seasonCounter` interpreted as the season index it derives from
/// (`days / DAYS_PER_SEASON`, wrapping over four seasons). Storing a *season*
/// here is a unit error the type now prevents: the field holds days.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
    Unexpected(u8),
}
impl Season {
    pub fn from_index(i: u8) -> Self {
        match i {
            0 => Self::Spring,
            1 => Self::Summer,
            2 => Self::Autumn,
            3 => Self::Winter,
            other => Self::Unexpected(other),
        }
    }
    pub fn index(self) -> u8 {
        match self {
            Self::Spring => 0,
            Self::Summer => 1,
            Self::Autumn => 2,
            Self::Winter => 3,
            Self::Unexpected(v) => v,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Unexpected(_) => "?",
            s => SEASONS[s.index() as usize],
        }
    }
}

impl std::fmt::Display for Season {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A day counter: what `seasonCounter` actually stores. Knows the season it
/// falls in, so the arithmetic lives in one place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeasonDay(pub u8);
impl SeasonDay {
    pub fn from_raw(v: u8) -> Self {
        Self(v)
    }
    pub fn raw(self) -> u8 {
        self.0
    }
    /// `seasonCounter % DAYS_PER_SEASON` puts a day inside the first season, so
    /// the game's own wrap is over `DAYS_PER_SEASON * SEASON_COUNT` days.
    pub const fn cycle_len() -> u8 {
        crate::game_data::DAYS_PER_SEASON * crate::game_data::SEASON_COUNT as u8
    }
    pub fn season(self) -> Season {
        Season::from_index(self.0 % Self::cycle_len() / crate::game_data::DAYS_PER_SEASON)
    }
    pub fn day_in_season(self) -> u8 {
        self.0 % crate::game_data::DAYS_PER_SEASON
    }
    /// Advance by whole seasons, leaving the day inside the season alone. This is
    /// the operation the "Next season" button performs.
    pub fn advanced_seasons(self, n: i32) -> Self {
        let cycle = Self::cycle_len() as i32;
        let next = (self.0 as i32 + n * crate::game_data::DAYS_PER_SEASON as i32).rem_euclid(cycle);
        Self(next as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property that makes the typed layer safe to put in front of a save:
    /// every value, in and out of range, survives a round trip unchanged.
    #[test]
    fn every_domain_round_trips_its_raw_value() {
        for v in 0u8..=255 {
            assert_eq!(HubArea::from_raw(v).raw(), v);
            assert_eq!(Upgrade::from_raw(v).raw(), v);
            assert_eq!(ExteriorStyle::from_raw(v).raw(), v);
            assert_eq!(InteriorStyle::from_raw(v).raw(), v);
            assert_eq!(PathStyle::from_raw(v).raw(), v);
            assert_eq!(DecorVariant::from_raw(v).raw(), v);
            assert_eq!(ConfigToggle::from_raw(v).raw(), v);
            assert_eq!(ConfigRange::from_raw(v).raw(), v);
            assert_eq!(GenderFlag::from_raw(v).raw(), v);
            assert_eq!(AbilitySlot::from_raw(v).raw(), v);
            assert_eq!(SeasonDay::from_raw(v).raw(), v);
        }
        for v in 0u16..=200 {
            assert_eq!(Ball::from_raw(v).raw(), v);
        }
    }

    #[test]
    fn in_range_values_are_named_and_out_of_range_ones_are_not() {
        assert!(PathStyle::from_raw(0).is_known());
        assert_eq!(PathStyle::from_raw(4).name(), "Muddy Tracks");
        assert!(!PathStyle::from_raw(5).is_known());
        assert_eq!(PathStyle::from_raw(5).name(), "?");
        // A Rogue ball id past the vanilla table is legal and must not be
        // renamed or rejected, but it is not nameable either.
        assert!(!Ball::from_raw(200).is_known());
        assert_eq!(Ball::from_raw(4).name(), "Poké Ball");
        assert!(Ball::from_raw(4).is_known());
    }

    /// The bug the three style types exist to prevent: the slots hold different
    /// domains, so a value that is legal outdoors can be meaningless indoors.
    #[test]
    fn the_three_home_style_slots_are_distinct_domains() {
        let outdoor = values::<ExteriorStyle>().len();
        let indoor = values::<InteriorStyle>().len();
        let path = values::<PathStyle>().len();
        assert_eq!((outdoor, indoor, path), (5, 6, 5));
        // 5 is a legal interior id and an out-of-range exterior id.
        assert!(InteriorStyle::from_raw(5).is_known());
        assert!(!ExteriorStyle::from_raw(5).is_known());
    }

    #[test]
    fn gender_and_ability_flag_unexpected_values() {
        assert_eq!(GenderFlag::from_raw(2), GenderFlag::Unexpected(2));
        assert!(!GenderFlag::from_raw(2).is_expected());
        assert_eq!(GenderFlag::Unexpected(2).raw(), 2);
        assert_eq!(AbilitySlot::from_raw(2), AbilitySlot::Hidden);
        assert_eq!(AbilitySlot::from_raw(3), AbilitySlot::Unexpected(3));
    }

    #[test]
    fn season_day_units() {
        // Two days per season, four seasons: day 6 is winter's first day, and
        // the counter wraps rather than growing past the cycle.
        assert_eq!(SeasonDay::from_raw(6).season(), Season::Winter);
        assert_eq!(SeasonDay::from_raw(6).day_in_season(), 0);
        assert_eq!(SeasonDay::cycle_len(), 8);
        assert_eq!(
            SeasonDay::from_raw(7).advanced_seasons(1),
            SeasonDay::from_raw(1)
        );
        assert_eq!(
            SeasonDay::from_raw(0).advanced_seasons(-1),
            SeasonDay::from_raw(6)
        );
        // A season index handed to the day counter is detectable, not silent.
        assert_eq!(SeasonDay::from_raw(2).season(), Season::Summer);
    }

    #[test]
    fn lab_config_slots_carry_their_own_value_domains() {
        assert_eq!(ConfigRange::from_raw(3).options(), BATTLE_FORMATS);
        assert_eq!(ConfigRange::from_raw(8).options(), TRAINER_ORDERS);
        assert!(ConfigRange::from_raw(1).options().is_empty());
        assert!(ConfigRange::from_raw(0).is_signed());
        // Slot order is pinned to the game's CONFIG_RANGE_* enum.
        assert_eq!(ConfigRange::from_raw(3).name(), "Battle format");
        assert_eq!(<ConfigRange as Domain>::table_len(), 9);
    }

    #[test]
    fn stat_indices_cover_six_columns_in_struct_order() {
        assert_eq!(StatIdx::ALL[0], StatIdx::Hp);
        assert_eq!(StatIdx::ALL[5], StatIdx::Spd);
        assert_eq!(StatIdx::ALL.len(), 6);
        assert_eq!(StatIdx::Spa.index(), 4);
        assert_eq!(StatIdx::Spa.label(), "SpA");
    }

    #[test]
    fn named_natures_match_the_shared_nature_table() {
        assert_eq!(Nature::Adamant.raw(), 3);
        assert_eq!(Nature::from_raw(3), Nature::Adamant);
        assert_eq!(Nature::Sassy.name(), "Sassy");
        // 25 table rows, 25 named variants, in the same order.
        assert_eq!(values::<Nature>().len(), crate::tables::NATURE_IDS.len());
        for n in (0..=24u8).map(Nature::from_raw) {
            assert_eq!(
                crate::tables::NATURE_IDS[n.raw() as usize],
                (n.raw() as u16, n.name()),
                "nature table and Nature enum drifted at {}",
                n.raw()
            );
        }
        // Out of range survives as itself rather than becoming Quirky.
        assert_eq!(Nature::from_raw(99), Nature::Unexpected(99));
        assert_eq!(Nature::Unexpected(99).raw(), 99);
        assert_eq!(Nature::Unexpected(99).name(), "?");
    }

    #[test]
    fn pockets_are_ids_not_positions() {
        assert_eq!(Pocket::NONE.raw(), 0);
        // The display names in POCKET_ORDER are display names, so they must be
        // taken whole (slicing a prefix off "Items" once panicked).
        let listed: Vec<Pocket> = crate::item_pockets::POCKET_ORDER
            .iter()
            .map(|(_, p)| Pocket::from_raw(*p))
            .collect();
        assert!(!listed.is_empty());
        assert!(listed.iter().all(|p| !p.name().is_empty() && p.is_known()));
        assert!(Pocket::from_raw(1).is_known());
        assert_eq!(Pocket::from_raw(200).name(), "pocket 200");
    }
}
