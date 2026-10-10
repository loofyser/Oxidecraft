//! The enchantment name table and the roman numerals the tooltip reads.
//!
//! Values and names from the source's enchantment registry
//! (`Enchantment.java`:21-87 for the ids, the `setName` sites for the lang
//! keys, `Enchantment.getTranslatedName`:217-221 for the composition) and the
//! en_US strings for those keys plus `enchantment.level.1..10` (short value
//! tokens only — the table carries the strings). No source text is copied.
//!
//! [`enchant_line`] answers one `ench`-list entry's line, or `None` for an
//! id the registry never registered — the source's `getEnchantmentById(k) !=
//! null` guard (`ItemStack.java`:697-714) skips those entries.

/// One row of the 1.8 enchantment registry: the effect id and the en_US
/// display string its lang key carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnchantEntry {
    /// The effect id (`Enchantment.effectId`).
    pub id: u16,
    /// The en_US display string (`enchantment.<key>`).
    pub name: &'static str,
}

/// The 1.8 registry's twenty-five rows, ascending by id. Ids 9-15, 22-31,
/// 36-47 and 52-60 hold no registration.
pub const ENCHANTS: &[EnchantEntry] = &[
    EnchantEntry {
        id: 0,
        name: "Protection",
    },
    EnchantEntry {
        id: 1,
        name: "Fire Protection",
    },
    EnchantEntry {
        id: 2,
        name: "Feather Falling",
    },
    EnchantEntry {
        id: 3,
        name: "Blast Protection",
    },
    EnchantEntry {
        id: 4,
        name: "Projectile Protection",
    },
    EnchantEntry {
        id: 5,
        name: "Respiration",
    },
    EnchantEntry {
        id: 6,
        name: "Aqua Affinity",
    },
    EnchantEntry {
        id: 7,
        name: "Thorns",
    },
    EnchantEntry {
        id: 8,
        name: "Depth Strider",
    },
    EnchantEntry {
        id: 16,
        name: "Sharpness",
    },
    EnchantEntry {
        id: 17,
        name: "Smite",
    },
    EnchantEntry {
        id: 18,
        name: "Bane of Arthropods",
    },
    EnchantEntry {
        id: 19,
        name: "Knockback",
    },
    EnchantEntry {
        id: 20,
        name: "Fire Aspect",
    },
    EnchantEntry {
        id: 21,
        name: "Looting",
    },
    EnchantEntry {
        id: 32,
        name: "Efficiency",
    },
    EnchantEntry {
        id: 33,
        name: "Silk Touch",
    },
    EnchantEntry {
        id: 34,
        name: "Unbreaking",
    },
    EnchantEntry {
        id: 35,
        name: "Fortune",
    },
    EnchantEntry {
        id: 48,
        name: "Power",
    },
    EnchantEntry {
        id: 49,
        name: "Punch",
    },
    EnchantEntry {
        id: 50,
        name: "Flame",
    },
    EnchantEntry {
        id: 51,
        name: "Infinity",
    },
    EnchantEntry {
        id: 61,
        name: "Luck of the Sea",
    },
    EnchantEntry {
        id: 62,
        name: "Lure",
    },
];

/// The registry's display string for an effect id, or `None` past it.
pub fn enchant_name(id: u16) -> Option<&'static str> {
    ENCHANTS
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.name)
}

/// The level suffix (`enchantment.level.<level>`, `en_US.lang`:1383-1392):
/// the roman numeral for levels 1-10, else the key itself — the source's
/// `StatCollector` returns a missing key verbatim, so an out-of-range level
/// renders as `enchantment.level.<level>`, never clamped.
pub fn roman_numeral(level: i16) -> String {
    match level {
        1 => String::from("I"),
        2 => String::from("II"),
        3 => String::from("III"),
        4 => String::from("IV"),
        5 => String::from("V"),
        6 => String::from("VI"),
        7 => String::from("VII"),
        8 => String::from("VIII"),
        9 => String::from("IX"),
        10 => String::from("X"),
        _ => format!("enchantment.level.{level}"),
    }
}

/// One `ench`-list entry's line (`Enchantment.getTranslatedName`: the name,
/// a space, the roman level), or `None` for an unregistered id.
pub fn enchant_line(id: u16, level: i16) -> Option<String> {
    Some(format!("{} {}", enchant_name(id)?, roman_numeral(level)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_the_sources_own_names() {
        assert_eq!(enchant_name(0), Some("Protection"));
        assert_eq!(enchant_name(2), Some("Feather Falling"));
        assert_eq!(enchant_name(16), Some("Sharpness"));
        assert_eq!(enchant_name(18), Some("Bane of Arthropods"));
        assert_eq!(enchant_name(34), Some("Unbreaking"));
        assert_eq!(enchant_name(48), Some("Power"));
        assert_eq!(enchant_name(61), Some("Luck of the Sea"));
        assert_eq!(enchant_name(62), Some("Lure"));
        assert_eq!(ENCHANTS.len(), 25, "the 1.8 registry's own count");
    }

    #[test]
    fn gaps_hold_no_registration() {
        for id in [9, 15, 22, 31, 36, 47, 52, 60, 63, 255] {
            assert_eq!(enchant_name(id), None, "id {id} is no enchantment");
        }
    }

    #[test]
    fn the_numerals_run_one_to_ten() {
        let numerals = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
        for (level, numeral) in numerals.iter().enumerate() {
            assert_eq!(
                roman_numeral(level as i16 + 1),
                String::from(*numeral),
                "level {}",
                level + 1
            );
        }
    }

    #[test]
    fn out_of_range_levels_pass_the_key_through() {
        assert_eq!(roman_numeral(0), "enchantment.level.0");
        assert_eq!(roman_numeral(11), "enchantment.level.11");
        assert_eq!(roman_numeral(-1), "enchantment.level.-1");
    }

    #[test]
    fn lines_join_the_name_and_the_numeral() {
        assert_eq!(enchant_line(16, 3), Some(String::from("Sharpness III")));
        assert_eq!(enchant_line(34, 2), Some(String::from("Unbreaking II")));
        assert_eq!(enchant_line(0, 4), Some(String::from("Protection IV")));
        assert_eq!(enchant_line(51, 1), Some(String::from("Infinity I")));
    }

    #[test]
    fn unknown_ids_have_no_line() {
        assert_eq!(enchant_line(9, 1), None);
        assert_eq!(enchant_line(255, 5), None);
    }
}
