// TOML-backed settings — replaces the registry store.
// File: %LOCALAPPDATA%\RCantonese\settings.toml
//
// Minimal hand-rolled parse/write: the DLL is injected into every process, so
// we avoid pulling the serde/toml ecosystem. Only flat `key = value` lines,
// quoted strings, integers and string arrays are supported — enough for the
// IME's settings schema.
#![allow(dead_code)]

use std::path::PathBuf;

use crate::globals;
use crate::settings::{CharacterForm, ImeSettings, InputMethodMode, PunctuationForm};
use crate::variants::CharacterVariant;

pub fn settings_path() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("RCantonese").join("settings.toml")
}

fn input_method_mode_name(mode: InputMethodMode) -> &'static str {
        match mode {
                InputMethodMode::Cantonese => "cantonese",
                InputMethodMode::Abc => "abc",
        }
}
fn input_method_mode_from_name(name: &str) -> Option<InputMethodMode> {
        match name {
                "cantonese" => Some(InputMethodMode::Cantonese),
                "abc" => Some(InputMethodMode::Abc),
                _ => None,
        }
}
fn character_form_name(form: CharacterForm) -> &'static str {
        match form {
                CharacterForm::HalfWidth => "half",
                CharacterForm::FullWidth => "full",
        }
}
fn character_form_from_name(name: &str) -> Option<CharacterForm> {
        match name {
                "half" => Some(CharacterForm::HalfWidth),
                "full" => Some(CharacterForm::FullWidth),
                _ => None,
        }
}
fn punctuation_form_name(form: PunctuationForm) -> &'static str {
        match form {
                PunctuationForm::Cantonese => "cantonese",
                PunctuationForm::English => "english",
        }
}
fn punctuation_form_from_name(name: &str) -> Option<PunctuationForm> {
        match name {
                "cantonese" => Some(PunctuationForm::Cantonese),
                "english" => Some(PunctuationForm::English),
                _ => None,
        }
}
fn character_variant_name(variant: CharacterVariant) -> &'static str {
        match variant {
                CharacterVariant::Traditional => "traditional",
                CharacterVariant::HongKong => "hongkong",
                CharacterVariant::Taiwan => "taiwan",
                CharacterVariant::Simplified => "simplified",
        }
}
fn character_variant_from_name(name: &str) -> Option<CharacterVariant> {
        match name {
                "traditional" => Some(CharacterVariant::Traditional),
                "hongkong" | "hong_kong" | "hk" => Some(CharacterVariant::HongKong),
                "taiwan" | "tw" => Some(CharacterVariant::Taiwan),
                "simplified" | "sim" => Some(CharacterVariant::Simplified),
                _ => None,
        }
}

// -- minimal TOML value parsing ------------------------------------------

fn strip_comment(line: &str) -> &str {
        // '#' starts a comment unless inside quotes — our values never
        // contain '#', so a plain split is fine.
        line.split('#').next().unwrap_or("").trim_end()
}

fn unquote(value: &str) -> Option<String> {
        let v = value.trim();
        let inner = v.strip_prefix('"')?.strip_suffix('"')?;
        Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn quote(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn parse_u32(value: &str) -> Option<u32> {
        value.trim().parse().ok()
}

/// Colors are written as `0xRRGGBB` (web order, human-friendly) and stored in
/// settings as COLORREF (0x00BBGGRR) — convert on read/write.
pub fn parse_color(value: &str) -> Option<u32> {
        let v = value.trim().trim_start_matches("0x").trim_start_matches("0X");
        let rgb = u32::from_str_radix(v, 16).ok()?;
        let r = rgb & 0xFF;
        let g = (rgb >> 8) & 0xFF;
        let b = (rgb >> 16) & 0xFF;
        Some(r | (g << 8) | (b << 16))
}

pub fn color_to_hex(colorref: u32) -> String {
        let r = colorref & 0xFF;
        let g = (colorref >> 8) & 0xFF;
        let b = (colorref >> 16) & 0xFF;
        format!("0x{b:02X}{g:02X}{r:02X}")
}

fn parse_string_array(value: &str) -> Vec<String> {
        let v = value.trim();
        let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
                return Vec::new();
        };
        inner
                .split(',')
                .filter_map(|part| unquote(part))
                .collect()
}

// -- hotkey parsing --------------------------------------------------------
//
// "ctrl+`" / "ctrl+shift+f4" → (VK, TF_MOD_*). The last '+'-separated token
// is the key name, the rest are modifiers.

fn key_name_to_vk(name: &str) -> Option<u32> {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
                "space" => return Some(0x20),
                "tab" => return Some(0x09),
                "enter" | "return" => return Some(0x0D),
                "esc" | "escape" => return Some(0x1B),
                "backspace" => return Some(0x08),
                "`" | "oem_3" | "backquote" => return Some(0xC0),
                "'" | "oem_7" => return Some(0xDE),
                _ => {}
        }
        if let Some(f) = lower.strip_prefix('f') {
                if let Ok(n) = f.parse::<u32>() {
                        if (1..=12).contains(&n) {
                                return Some(0x6F + n); // VK_F1 = 0x70
                        }
                }
        }
        let mut chars = lower.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
                match c {
                        'a'..='z' => return Some(c.to_ascii_uppercase() as u32),
                        '0'..='9' => return Some(c as u32),
                        _ => {}
                }
        }
        None
}

fn modifier_name_to_flag(name: &str) -> Option<u32> {
        match name.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some(globals::TF_MOD_CONTROL),
                "shift" => Some(globals::TF_MOD_SHIFT),
                "alt" => Some(globals::TF_MOD_ALT),
                _ => None,
        }
}

/// Parse "ctrl+shift+`" → (0xC0, TF_MOD_CONTROL | TF_MOD_SHIFT).
pub fn parse_hotkey(spec: &str) -> Option<(u32, u32)> {
        let mut parts: Vec<&str> = spec.split('+').collect();
        if parts.len() < 2 {
                // bare key, no modifier — allowed ("f4")
                let vk = key_name_to_vk(parts.first()?.trim())?;
                return Some((vk, 0));
        }
        // Trailing '+' keeps the literal '+' key: "ctrl++" → key is "+".
        let key_token = if parts.last().map(|s| s.is_empty()).unwrap_or(false) && parts.len() >= 2 {
                parts.pop();
                "+"
        } else {
                parts.pop()?
        };
        let vk = key_name_to_vk(key_token)?;
        let mut modifiers = 0u32;
        for part in parts {
                modifiers |= modifier_name_to_flag(part)?;
        }
        Some((vk, modifiers))
}

pub fn hotkey_to_spec(vk: u32, modifiers: u32) -> String {
        let mut spec = String::new();
        if modifiers & globals::TF_MOD_CONTROL != 0 {
                spec.push_str("ctrl+");
        }
        if modifiers & globals::TF_MOD_SHIFT != 0 {
                spec.push_str("shift+");
        }
        if modifiers & globals::TF_MOD_ALT != 0 {
                spec.push_str("alt+");
        }
        let key = match vk {
                0x20 => "space".to_string(),
                0x09 => "tab".to_string(),
                0x0D => "enter".to_string(),
                0x1B => "esc".to_string(),
                0x08 => "backspace".to_string(),
                0xC0 => "`".to_string(),
                0xDE => "'".to_string(),
                0x70..=0x7B => format!("f{}", vk - 0x6F),
                0x30..=0x39 => format!("{}", vk - 0x30),
                0x41..=0x5A => format!("{}", char::from_u32(vk).unwrap_or('?').to_ascii_lowercase()),
                _ => format!("0x{vk:x}"),
        };
        spec.push_str(&key);
        spec
}

// -- load / save -----------------------------------------------------------

/// Load settings from the TOML file; falls back to defaults for missing or
/// unknown values. Unknown keys are ignored so newer configs don't break
/// older builds.
pub fn load() -> ImeSettings {
        let mut settings = ImeSettings::default();
        let path = settings_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
                return settings;
        };
        let mut hotkeys: Vec<(u32, u32)> = Vec::new();
        for line in content.lines() {
                let line = strip_comment(line);
                if line.is_empty() || line.starts_with('[') {
                        continue;
                }
                let Some((key, value)) = line.split_once('=') else { continue };
                let key = key.trim();
                let value = value.trim();
                match key {
                        "version" => {
                                if let Some(v) = parse_u32(value) {
                                        settings.version = v;
                                }
                        }
                        "input_method_mode" => {
                                if let Some(v) = unquote(value).as_deref().and_then(input_method_mode_from_name) {
                                        settings.input_method_mode = v;
                                }
                        }
                        "character_form" => {
                                if let Some(v) = unquote(value).as_deref().and_then(character_form_from_name) {
                                        settings.character_form = v;
                                }
                        }
                        "punctuation_form" => {
                                if let Some(v) = unquote(value).as_deref().and_then(punctuation_form_from_name) {
                                        settings.punctuation_form = v;
                                }
                        }
                        "character_variant" => {
                                if let Some(v) = unquote(value).as_deref().and_then(character_variant_from_name) {
                                        settings.character_variant = v;
                                }
                        }
                        "candidate_page_size" => {
                                if let Some(v) = parse_u32(value) {
                                        settings.candidate_page_size = crate::settings::candidate_page_size_from_raw(v);
                                }
                        }
                        "candidate_font_size" => {
                                if let Some(v) = parse_u32(value) {
                                        settings.candidate_font_size = crate::settings::candidate_font_size_from_raw(v);
                                }
                        }
                        "candidate_number_font_size" => {
                                if let Some(v) = parse_u32(value) {
                                        settings.candidate_number_font_size = crate::settings::candidate_font_size_from_raw(v);
                                }
                        }
                        "candidate_comment_font_size" => {
                                if let Some(v) = parse_u32(value) {
                                        settings.candidate_comment_font_size = crate::settings::candidate_font_size_from_raw(v);
                                }
                        }
                        "candidate_text_color" => {
                                if let Some(v) = parse_color(value) {
                                        settings.candidate_text_color = v;
                                }
                        }
                        "candidate_back_color" => {
                                if let Some(v) = parse_color(value) {
                                        settings.candidate_back_color = v;
                                }
                        }
                        "candidate_select_color" => {
                                if let Some(v) = parse_color(value) {
                                        settings.candidate_select_color = v;
                                }
                        }
                        "candidate_comment_color" => {
                                if let Some(v) = parse_color(value) {
                                        settings.candidate_comment_color = v;
                                }
                        }
                        "options_menu_keys" => {
                                hotkeys = parse_string_array(value)
                                        .iter()
                                        .filter_map(|spec| parse_hotkey(spec))
                                        .collect();
                        }
                        "ui_language" => {
                                if let Some(v) = unquote(value) {
                                        if matches!(v.as_str(), "auto" | "en" | "zh") {
                                                settings.ui_language = v;
                                        }
                                }
                        }
                        _ => {}
                }
        }
        if !hotkeys.is_empty() {
                settings.options_menu_keys = hotkeys;
        }
        settings
}

/// Write the whole settings file in canonical order.
pub fn save(settings: &ImeSettings) -> std::io::Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
        }
        let keys: Vec<String> = settings
                .options_menu_keys
                .iter()
                .map(|&(vk, m)| quote(&hotkey_to_spec(vk, m)))
                .collect();
        let content = format!(
                "# R-Cantonese settings — edited by hand or by the config center.\n\
                 version = {}\n\
                 input_method_mode = {}\n\
                 character_form = {}\n\
                 punctuation_form = {}\n\
                 character_variant = {}\n\
                 candidate_page_size = {}\n\
                 candidate_font_size = {}\n\
                 candidate_number_font_size = {}\n\
                 candidate_comment_font_size = {}\n\
                 candidate_text_color = {}\n\
                 candidate_back_color = {}\n\
                 candidate_select_color = {}\n\
                 candidate_comment_color = {}\n\
                 options_menu_keys = [{}]\n\
                 ui_language = {}\n",
                settings.version,
                quote(input_method_mode_name(settings.input_method_mode)),
                quote(character_form_name(settings.character_form)),
                quote(punctuation_form_name(settings.punctuation_form)),
                quote(character_variant_name(settings.character_variant)),
                settings.candidate_page_size,
                settings.candidate_font_size,
                settings.candidate_number_font_size,
                settings.candidate_comment_font_size,
                color_to_hex(settings.candidate_text_color),
                color_to_hex(settings.candidate_back_color),
                color_to_hex(settings.candidate_select_color),
                color_to_hex(settings.candidate_comment_color),
                keys.join(", "),
                quote(&settings.ui_language),
        );
        if let Err(e) = std::fs::write(&path, content) {
                globals::log_error(&format!("config save failed: {e:?}"));
                return Err(e);
        }
        Ok(())
}

#[cfg(test)]
mod tests {
        use super::*;

        #[test]
        fn hotkey_parse() {
                assert_eq!(parse_hotkey("ctrl+`"), Some((0xC0, globals::TF_MOD_CONTROL)));
                assert_eq!(
                        parse_hotkey("ctrl+shift+f4"),
                        Some((0x73, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT))
                );
                assert_eq!(parse_hotkey("f4"), Some((0x73, 0)));
                assert_eq!(parse_hotkey("alt+space"), Some((0x20, globals::TF_MOD_ALT)));
                assert_eq!(parse_hotkey("ctrl+shift+bad"), None);
        }

        #[test]
        fn hotkey_roundtrip() {
                assert_eq!(hotkey_to_spec(0xC0, globals::TF_MOD_CONTROL), "ctrl+`");
                assert_eq!(
                        hotkey_to_spec(0x73, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT),
                        "ctrl+shift+f4"
                );
        }

        #[test]
        fn parse_values() {
                assert_eq!(unquote("\"abc\""), Some("abc".to_string()));
                assert_eq!(parse_u32("7"), Some(7));
                assert_eq!(parse_string_array("[\"a\", \"b\"]"), vec!["a", "b"]);
                assert_eq!(strip_comment("a = 1 # c"), "a = 1");
        }
}
