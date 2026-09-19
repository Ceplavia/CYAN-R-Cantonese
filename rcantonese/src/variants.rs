// Character standard / variant conversion — mirrors CharacterStandard.cpp.
#![allow(dead_code)]

use crate::db::ImeDatabase;
use crate::phrases::{MUTILATED_PHRASES, PRC_GENERAL_PHRASES};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharacterVariant {
        Traditional = 1,
        HongKong = 2,
        Taiwan = 3,
        Simplified = 4,
}

impl CharacterVariant {
        pub fn from_raw(value: u32) -> Self {
                match value {
                        2 => Self::HongKong,
                        3 => Self::Taiwan,
                        4 => Self::Simplified,
                        _ => Self::Traditional,
                }
        }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharacterStandard {
        Preset = 1,
        Custom = 2,
        Inherited = 3,
        Etymology = 4,
        OpenCC = 5,
        HongKong = 6,
        Taiwan = 7,
        PRCGeneral = 8,
        AncientBooksPublishing = 9,
        Mutilated = 51,
}

impl CharacterStandard {
        pub fn variant_table_name(&self) -> &'static str {
                match self {
                        Self::Inherited => "variant_old",
                        Self::HongKong => "variant_hk",
                        Self::Taiwan => "variant_tw",
                        Self::PRCGeneral => "variant_prc",
                        Self::AncientBooksPublishing => "variant_abp",
                        Self::Mutilated => "variant_sim",
                        _ => "",
                }
        }

        pub fn is_mutilated(&self) -> bool {
                *self == Self::Mutilated
        }
}

pub fn standard_for_variant(variant: CharacterVariant) -> CharacterStandard {
        match variant {
                CharacterVariant::Traditional => CharacterStandard::Preset,
                CharacterVariant::HongKong => CharacterStandard::HongKong,
                CharacterVariant::Taiwan => CharacterStandard::Taiwan,
                CharacterVariant::Simplified => CharacterStandard::Mutilated,
        }
}

pub fn is_ideographic_code_point(code_point: u32) -> bool {
        matches!(
                code_point,
                0x4E00..=0x9FFF
                        | 0x3400..=0x4DBF
                        | 0x20000..=0x2A6DF
                        | 0x2A700..=0x2B73F
                        | 0x2B740..=0x2B81F
                        | 0x2B820..=0x2CEAF
                        | 0x2CEB0..=0x2EBEF
                        | 0x30000..=0x3134F
                        | 0x31350..=0x323AF
                        | 0x2EBF0..=0x2EE5F
                        | 0x323B0..=0x33479
                        | 0x3007
                        | 0x2E80..=0x2E99
                        | 0x2E9B..=0x2EF3
                        | 0x2F00..=0x2FD5
                        | 0xF900..=0xFA6D
                        | 0xFA70..=0xFAD9
                        | 0x2F800..=0x2FA1D
        )
}

const MIN_PHRASE_LENGTH: usize = 2;

fn find_phrase<'a>(text: &str, phrases: &'a [(&'a str, &'a str)]) -> Option<&'a str> {
        phrases.iter().find(|(source, _)| *source == text).map(|(_, target)| *target)
}

fn max_phrase_length(phrases: &[(&str, &str)]) -> usize {
        phrases
                .iter()
                .map(|(source, _)| source.chars().count())
                .max()
                .unwrap_or(0)
                .max(MIN_PHRASE_LENGTH)
}

fn convert_segment(database: &ImeDatabase, standard: CharacterStandard, c: char) -> String {
        let code = c as u32;
        if !is_ideographic_code_point(code) {
                return c.to_string();
        }
        match database.query_variant_target(standard, code) {
                Some(target) => char::from_u32(target).map(|t| t.to_string()).unwrap_or_else(|| c.to_string()),
                None => c.to_string(),
        }
}

/// Greedy phrase-aware conversion: match longest phrase first, then per-scalar lookup.
fn greedy_phrase_convert(
        database: &ImeDatabase,
        standard: CharacterStandard,
        chars: &[char],
        phrases: &[(&str, &str)],
) -> String {
        let max_len = max_phrase_length(phrases);
        let mut result = String::new();
        let mut index = 0;
        while index < chars.len() {
                let remaining = chars.len() - index;
                let limit = max_len.min(remaining);
                let mut matched = false;
                for length in (MIN_PHRASE_LENGTH..=limit).rev() {
                        let candidate: String = chars[index..index + length].iter().collect();
                        if let Some(replacement) = find_phrase(&candidate, phrases) {
                                result.push_str(replacement);
                                index += length;
                                matched = true;
                                break;
                        }
                }
                if !matched {
                        result.push_str(&convert_segment(database, standard, chars[index]));
                        index += 1;
                }
        }
        result
}

/// Convert candidate text to the desired character standard.
/// Preset/Custom/Etymology/OpenCC are pass-through.
pub fn convert_text(database: &ImeDatabase, text: &str, standard: CharacterStandard) -> String {
        match standard {
                CharacterStandard::Preset | CharacterStandard::Custom | CharacterStandard::Etymology | CharacterStandard::OpenCC => {
                        return text.to_string();
                }
                _ => {}
        }

        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
                return text.to_string();
        }

        match standard {
                CharacterStandard::PRCGeneral | CharacterStandard::Mutilated => {
                        let phrases = if standard == CharacterStandard::PRCGeneral { PRC_GENERAL_PHRASES } else { MUTILATED_PHRASES };
                        if chars.len() == 1 {
                                return convert_segment(database, standard, chars[0]);
                        }
                        if let Some(replacement) = find_phrase(text, phrases) {
                                return replacement.to_string();
                        }
                        if chars.len() == 2 {
                                return chars.iter().map(|c| convert_segment(database, standard, *c)).collect();
                        }
                        greedy_phrase_convert(database, standard, &chars, phrases)
                }
                _ => chars.iter().map(|c| convert_segment(database, standard, *c)).collect(),
        }
}
