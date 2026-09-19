// Stub for rcantonese/src/settings.rs — the shared settings types without
// the registry layer. Keep in sync with the IME's settings.rs.
#![allow(dead_code)]

use crate::variants::CharacterVariant;

pub const CURRENT_SETTINGS_VERSION: u32 = 1;
pub const MINIMUM_CANDIDATE_FONT_SIZE: u32 = 11;
pub const MAXIMUM_CANDIDATE_FONT_SIZE: u32 = 24;
pub const DEFAULT_CANDIDATE_FONT_SIZE: u32 = 16;
pub const DEFAULT_CANDIDATE_NUMBER_FONT_SIZE: u32 = 13;
pub const DEFAULT_CANDIDATE_COMMENT_FONT_SIZE: u32 = 13;
pub const DEFAULT_CANDIDATE_PAGE_SIZE: u32 = 7;
pub const MINIMUM_CANDIDATE_PAGE_SIZE: u32 = 1;
pub const MAXIMUM_CANDIDATE_PAGE_SIZE: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMethodMode {
        Cantonese = 1,
        Abc = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharacterForm {
        HalfWidth = 1,
        FullWidth = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PunctuationForm {
        Cantonese = 1,
        English = 2,
}

#[derive(Clone, Debug)]
pub struct ImeSettings {
        pub version: u32,
        pub input_method_mode: InputMethodMode,
        pub character_form: CharacterForm,
        pub punctuation_form: PunctuationForm,
        pub character_variant: CharacterVariant,
        pub candidate_page_size: u32,
        pub candidate_font_size: u32,
        pub candidate_number_font_size: u32,
        pub candidate_comment_font_size: u32,
        /// (VK, TF_MOD_*) pairs from `options_menu_keys` in settings.toml.
        pub options_menu_keys: Vec<(u32, u32)>,
        /// COLORREF (0x00BBGGRR).
        pub candidate_text_color: u32,
        pub candidate_back_color: u32,
        pub candidate_select_color: u32,
        pub candidate_comment_color: u32,
        pub ui_language: String,
}

impl Default for ImeSettings {
        fn default() -> Self {
                Self {
                        version: CURRENT_SETTINGS_VERSION,
                        input_method_mode: InputMethodMode::Cantonese,
                        character_form: CharacterForm::HalfWidth,
                        punctuation_form: PunctuationForm::Cantonese,
                        character_variant: CharacterVariant::Traditional,
                        candidate_page_size: DEFAULT_CANDIDATE_PAGE_SIZE,
                        candidate_font_size: DEFAULT_CANDIDATE_FONT_SIZE,
                        candidate_number_font_size: DEFAULT_CANDIDATE_NUMBER_FONT_SIZE,
                        candidate_comment_font_size: DEFAULT_CANDIDATE_COMMENT_FONT_SIZE,
                        options_menu_keys: vec![(0xC0, crate::globals::TF_MOD_CONTROL)],
                        candidate_text_color: 0x00000000,
                        candidate_back_color: 0x00FFFFFF,
                        candidate_select_color: 0x00F0D8B0,
                        candidate_comment_color: 0x00606060,
                        ui_language: "auto".into(),
                }
        }
}

pub fn candidate_page_size_from_raw(value: u32) -> u32 {
        value.clamp(MINIMUM_CANDIDATE_PAGE_SIZE, MAXIMUM_CANDIDATE_PAGE_SIZE)
}
pub fn candidate_font_size_from_raw(value: u32) -> u32 {
        value.clamp(MINIMUM_CANDIDATE_FONT_SIZE, MAXIMUM_CANDIDATE_FONT_SIZE)
}
