// ImeSettings + registry-backed SettingsStore — mirrors Settings.cpp.
#![allow(dead_code)]

use crate::variants::CharacterVariant;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, RegCloseKey, RegCreateKeyExW, RegGetValueW,
        RegOpenKeyExW, RegSetValueExW,
};
use windows::core::w;

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
        /// Hotkeys that toggle the options menu — (VK, TF_MOD_*) pairs parsed
        /// from `options_menu_keys` in settings.toml. Default: Ctrl+`.
        pub options_menu_keys: Vec<(u32, u32)>,
        /// Candidate window colors — COLORREF (0x00BBGGRR).
        pub candidate_text_color: u32,
        pub candidate_back_color: u32,
        pub candidate_select_color: u32,
        pub candidate_comment_color: u32,
        /// UI language override: "auto" (system), "en", or "zh".
        pub ui_language: String,
        /// Show the standalone tray icon (r-cantonese-tray.exe). Weasel-style:
        /// off by default — the langbar item next to the input indicator is
        /// the primary icon; the tray icon only surfaces for balloons.
        pub display_tray_icon: bool,
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
                        display_tray_icon: false,
                }
        }
}

pub fn candidate_page_size_from_raw(value: u32) -> u32 {
        value.clamp(MINIMUM_CANDIDATE_PAGE_SIZE, MAXIMUM_CANDIDATE_PAGE_SIZE)
}
pub fn candidate_font_size_from_raw(value: u32) -> u32 {
        value.clamp(MINIMUM_CANDIDATE_FONT_SIZE, MAXIMUM_CANDIDATE_FONT_SIZE)
}

fn input_method_mode_from_raw(value: u32) -> InputMethodMode {
        if value == 2 { InputMethodMode::Abc } else { InputMethodMode::Cantonese }
}
fn character_form_from_raw(value: u32) -> CharacterForm {
        if value == 2 { CharacterForm::FullWidth } else { CharacterForm::HalfWidth }
}
fn punctuation_form_from_raw(value: u32) -> PunctuationForm {
        if value == 2 { PunctuationForm::English } else { PunctuationForm::Cantonese }
}

fn read_dword(key: HKEY, name: windows::core::PCWSTR) -> Option<u32> {
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let result = unsafe {
                RegGetValueW(
                        key,
                        None,
                        name,
                        windows::Win32::System::Registry::RRF_RT_REG_DWORD,
                        None,
                        Some(&mut value as *mut _ as *mut _),
                        Some(&mut size),
                )
        };
        if result == ERROR_SUCCESS { Some(value) } else { None }
}

fn write_dword(key: HKEY, name: windows::core::PCWSTR, value: u32) {
        unsafe {
                let _ = RegSetValueExW(key, name, Some(0), REG_DWORD, Some(std::slice::from_raw_parts(&value as *const _ as *const u8, 4)));
        }
}

fn with_settings_key(access: u32, f: impl FnOnce(HKEY)) {
        let mut key = HKEY::default();
        let result = unsafe {
                RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        w!("Software\\RCantonese\\Settings"),
                        Some(0),
                        None,
                        windows::Win32::System::Registry::REG_OPTION_NON_VOLATILE,
                        windows::Win32::System::Registry::REG_SAM_FLAGS(access),
                        None,
                        &mut key,
                        None,
                )
        };
        if result.is_ok() {
                f(key);
                unsafe {
                        let _ = RegCloseKey(key);
                }
        }
}

fn open_settings_key() -> Option<HKEY> {
        let mut key = HKEY::default();
        let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\RCantonese\\Settings"), Some(0), KEY_READ, &mut key) };
        if result.is_ok() { Some(key) } else { None }
}

fn load_settings_from_registry() -> ImeSettings {
        let mut settings = ImeSettings::default();
        if let Some(key) = open_settings_key() {
                if let Some(v) = read_dword(key, w!("Version")) {
                        settings.version = v;
                }
                if let Some(v) = read_dword(key, w!("InputMethodMode")) {
                        settings.input_method_mode = input_method_mode_from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("CharacterForm")) {
                        settings.character_form = character_form_from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("PunctuationForm")) {
                        settings.punctuation_form = punctuation_form_from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("CharacterVariant")) {
                        settings.character_variant = CharacterVariant::from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("CandidatePageSize")) {
                        settings.candidate_page_size = candidate_page_size_from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("CandidateFontSize")) {
                        settings.candidate_font_size = candidate_font_size_from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("LabelFontSize")) {
                        settings.candidate_number_font_size = candidate_font_size_from_raw(v);
                }
                if let Some(v) = read_dword(key, w!("CommentFontSize")) {
                        settings.candidate_comment_font_size = candidate_font_size_from_raw(v);
                }
                unsafe {
                        let _ = RegCloseKey(key);
                }
        }
        settings
}

/// Load settings: TOML first, then migrate registry values on first run.
pub fn load_settings() -> ImeSettings {
        let path = crate::config::settings_path();
        if path.exists() {
                return crate::config::load();
        }
        let settings = load_settings_from_registry();
        if open_settings_key().is_some() {
                // Registry values exist from a pre-TOML build — migrate them.
                let _ = crate::config::save(&settings);
        }
        settings
}

/// Persist the whole settings to the TOML file.
pub fn save_settings(settings: &ImeSettings) {
        let _ = crate::config::save(settings);
}

#[allow(dead_code)]
fn save_settings_registry(settings: &ImeSettings) {
        with_settings_key(KEY_WRITE.0, |key| {
                write_dword(key, w!("Version"), settings.version);
                write_dword(key, w!("InputMethodMode"), settings.input_method_mode as u32);
                write_dword(key, w!("CharacterForm"), settings.character_form as u32);
                write_dword(key, w!("PunctuationForm"), settings.punctuation_form as u32);
                write_dword(key, w!("CharacterVariant"), settings.character_variant as u32);
                write_dword(key, w!("CandidatePageSize"), candidate_page_size_from_raw(settings.candidate_page_size));
                write_dword(key, w!("CandidateFontSize"), candidate_font_size_from_raw(settings.candidate_font_size));
                write_dword(key, w!("LabelFontSize"), candidate_font_size_from_raw(settings.candidate_number_font_size));
                write_dword(key, w!("CommentFontSize"), candidate_font_size_from_raw(settings.candidate_comment_font_size));
        });
}

/// Persist a single setting value.
pub fn save_setting(name: windows::core::PCWSTR, value: u32) {
        with_settings_key(KEY_WRITE.0, |key| {
                write_dword(key, name, value);
        });
}
