//! Species-level derived data: gender and ability resolution.
//!
//! Two fields a save stores as small indices/flags cannot be displayed from the
//! mon alone:
//!
//! * `genderFlag` is 3 bits, but for fixed-gender species the game IGNORES it
//!   entirely and returns the species' fixed value. Showing the raw bit made
//!   Beldum/Metagross read as "Female" (`0xE0` = flag bits set, but
//!   `genderRatio == MON_GENDERLESS` decides the answer).
//! * `abilityNum` is only a SLOT INDEX into `gBaseStats[species].abilities[]`,
//!   so the same number means a different ability per species.
//!
//! Both helpers below mirror the game's functions rather than inventing a
//! policy, because the point of the editor is to show what the game will show.

use crate::abilities::ABILITY_NAMES;
use crate::species_meta::SPECIES_META;

/// `MON_MALE` / `MON_FEMALE` / `MON_GENDERLESS` (include/constants/pokemon.h).
/// These are the same constants the generated species table was emitted with,
/// so they are aliased rather than re-declared: one source of truth.
pub use crate::species_meta::{
    GENDER_FEMALE as MON_FEMALE, GENDER_GENDERLESS as MON_GENDERLESS, GENDER_MALE as MON_MALE,
};

pub const MAX_ABILITY_SLOT: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gender {
    Male,
    Female,
    Genderless,
}

impl Gender {
    pub fn label(self) -> &'static str {
        match self {
            Gender::Male => "Male",
            Gender::Female => "Female",
            Gender::Genderless => "Genderless",
        }
    }
    /// Compact mark for roster rows: ♂ / ♀ / ∅ (U+2205 empty set — "no gender").
    pub fn glyph(self) -> char {
        match self {
            Gender::Male => '♂',
            Gender::Female => '♀',
            Gender::Genderless => '\u{2205}',
        }
    }
}

/// `(genderRatio, [abilities; 3])` for a species id, or `None` when the species
/// is outside the generated table (a handful of cosmetic extra forms).
pub fn species_meta(sp: u16) -> Option<(u8, [u16; 3])> {
    SPECIES_META
        .iter()
        .find(|(s, _, _)| *s == sp)
        .map(|(_, r, a)| (*r, *a))
}

pub fn ability_name(id: u16) -> Option<&'static str> {
    ABILITY_NAMES
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
}

/// The ability a species' slot `slot` resolves to, following
/// `GetAbilityBySpecies_ForRevised` (src/pokemon.c:3183) exactly:
///
/// ```c
/// if (abilityNum < NUM_ABILITY_SLOTS) gLastUsedAbility = abilities[abilityNum];
/// if (abilityNum >= NUM_NORMAL_ABILITY_SLOTS)          // only for slot 2
///     for (i = 2; i < 3 && gLastUsedAbility == ABILITY_NONE; i++)
///         gLastUsedAbility = abilities[i];
/// for (i = 0; i < 3 && gLastUsedAbility == ABILITY_NONE; i++)
///     gLastUsedAbility = abilities[i];
/// ```
///
/// Order matters: an empty *normal* slot falls back through slots 0,1,2, while
/// an empty *hidden* slot is rescued by the hidden slot first. So for
/// `abilities = [NONE, X, Y]` slot 0 resolves to X, not to the hidden Y.
///
/// Two overrides in the game are deliberately not modelled here: a traded mon
/// (`IsOtherTrainer(otId)`) can have its whole ability list replaced by a custom
/// gift mon, and revision mode can override a species' abilities from
/// `gRoguePokemonProfiles_Revised`. Both are noted in docs/FORMAT.md.
pub fn ability_for(sp: u16, slot: u8) -> Option<u16> {
    let ab = species_meta(sp)?.1;
    let slot = slot as usize;
    let mut a = ab.get(slot).copied().unwrap_or(0);
    if slot >= 2 && a == 0 {
        a = ab[2];
    }
    if a == 0 {
        a = ab.into_iter().find(|&x| x != 0).unwrap_or(0);
    }
    (a != 0).then_some(a)
}

/// Human-readable ability for a slot: name when known, otherwise an explicit
/// "slot N (empty)" so the UI never silently shows the wrong name.
pub fn ability_label(sp: u16, slot: u8) -> String {
    match ability_for(sp, slot).and_then(ability_name) {
        Some(n) => n.to_string(),
        None => format!("slot {slot} (empty)"),
    }
}

/// Whether a species has a fixed gender, i.e. whether `genderFlag` is ignored.
/// `None` = species not in the table (ratio unknown).
pub fn gender_is_fixed(sp: u16) -> Option<bool> {
    let r = species_meta(sp)?.0;
    Some(matches!(r, MON_MALE | MON_FEMALE | MON_GENDERLESS))
}

/// Mirrors `GetGenderForSpecies` (src/pokemon.c:3321) plus
/// `Rogue_ModifyGenderRatio` (src/rogue_pokemon.c:4057), which forces every
/// variable-ratio species to exactly 50% female -- so the flag alone decides and
/// no `pid`-based comparison is performed. Returns `None` when the species is
/// unknown, in which case the caller must fall back to the raw flag.
pub fn gender_for_species(sp: u16, flag: u8) -> Option<Gender> {
    let ratio = species_meta(sp)?.0;
    Some(match ratio {
        MON_MALE => Gender::Male,
        MON_FEMALE => Gender::Female,
        MON_GENDERLESS => Gender::Genderless,
        _ if flag == 0 => Gender::Male,
        _ => Gender::Female,
    })
}

/// The flag value the game keeps for a gender. Only meaningful for
/// variable-ratio species; for fixed-gender species the game leaves the stored
/// bits alone (a genderless mon's flag is whatever it was at generation, e.g.
/// `0xE0` with the upper ribbon bits around it).
pub fn flag_for_gender(g: Gender) -> u8 {
    match g {
        Gender::Male => 0,
        Gender::Female | Gender::Genderless => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_gender_species_ignore_the_stored_flag() {
        // Metagross (374) is genderless. The test save's party mon 2 stores
        // genderFlag = 0xE0, which the old raw-flag read reported as "Female".
        assert_eq!(gender_for_species(374, 0xE0 & 7), Some(Gender::Genderless));
        assert_eq!(gender_is_fixed(374), Some(true));
        // Magearna (801) likewise, and Arceus (493) whose whole family is
        // written through a function-like macro in the source.
        assert_eq!(gender_for_species(801, 1), Some(Gender::Genderless));
        assert_eq!(gender_for_species(493, 0), Some(Gender::Genderless));
    }

    #[test]
    fn variable_ratio_species_follow_the_flag() {
        // Pikachu (25) is 50/50 under Rogue_ModifyGenderRatio.
        assert_eq!(gender_is_fixed(25), Some(false));
        assert_eq!(gender_for_species(25, 0), Some(Gender::Male));
        assert_eq!(gender_for_species(25, 1), Some(Gender::Female));
        // Nidoran F (29) is fixed-female in the table (MON_FEMALE), not flag-driven.
        assert_eq!(gender_is_fixed(29), Some(true));
        assert_eq!(gender_for_species(29, 0), Some(Gender::Female));
    }

    #[test]
    fn signature_ability_species_map_slot_to_name() {
        // Slot 0 of Arceus is Multitype (121); slot 1 is empty by declaration
        // ({ MULTITYPE, NONE }), so the game substitutes rather than reading
        // past the initializer.
        assert_eq!(ability_for(493, 0), Some(121));
        assert_eq!(ability_name(121), Some("Multitype"));
        assert_eq!(ability_label(493, 0), "Multitype");
        // Genesect -> Download, Xerneas -> Fairy Aura, Zygarde -> Aura Break.
        assert_eq!(ability_name(ability_for(649, 0).unwrap()), Some("Download"));
        assert_eq!(
            ability_name(ability_for(716, 0).unwrap()),
            Some("Fairy Aura")
        );
        assert_eq!(
            ability_name(ability_for(718, 0).unwrap()),
            Some("Aura Break")
        );
    }

    #[test]
    fn ability_slot_meaning_depends_on_the_species() {
        // The test save's box mons 11 and 12 both store abilityNum = 2, but the
        // species behind those ids are Slurpuff (685) and Inkay (686), so slot 2
        // resolves to different abilities. Printing "2" hid this entirely.
        assert_eq!(ability_for(685, 2), Some(84));
        assert_eq!(ability_name(ability_for(685, 2).unwrap()), Some("Unburden"));
        assert_eq!(
            ability_name(ability_for(686, 2).unwrap()),
            Some("Infiltrator")
        );
        // Beldum (374) declares Clear Body in slot 0 and Light Metal in slot 2,
        // with slot 1 empty. The game's fallback walks slots 0,1,2 for an empty
        // *normal* slot -- so slot 1 reads as Clear Body, not the hidden Light
        // Metal. (For an empty *hidden* slot the hidden slot is tried first.)
        assert_eq!(
            ability_name(ability_for(374, 0).unwrap()),
            Some("Clear Body")
        );
        assert_eq!(
            ability_name(ability_for(374, 2).unwrap()),
            Some("Light Metal")
        );
        assert_eq!(
            ability_name(ability_for(374, 1).unwrap()),
            Some("Clear Body"),
            "an empty normal slot must fall through slots 0,1,2 in order"
        );
        // Magearna (801) declares one signature ability, so every slot resolves
        // to it.
        assert_eq!(ability_for(801, 0), ability_for(801, 2));
        assert_eq!(
            ability_name(ability_for(801, 2).unwrap()),
            Some("Soul-Heart")
        );
    }

    #[test]
    fn unknown_species_report_no_data_instead_of_guessing() {
        // ids the generator could not resolve (cosmetic extra forms, e.g.
        // 1438 Scatterbug polar cloak) must not be presented as known data.
        assert_eq!(species_meta(1438), None);
        assert_eq!(gender_for_species(1438, 1), None);
        assert_eq!(ability_for(1438, 0), None);
    }
}
