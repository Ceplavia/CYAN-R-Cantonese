// Processor — port of CCompositionProcessorEngine.
// Owns input keys, settings, engine, memory, preserved keys, and the language bar item.
#![allow(dead_code)]

use std::sync::Mutex;

use windows::core::*;
use windows::Win32::UI::TextServices::*;

use crate::compartment::Compartment;
use crate::engine::CoreImeEngine;
use crate::globals;
use crate::keytable::KeystrokeEngine;
use crate::langbar::LangBarItem;
use crate::memory::InputMemory;
use crate::settings::*;
use crate::types::*;
use crate::variants::{standard_for_variant, CharacterStandard, CharacterVariant};

fn reverse_lookup_method_from_keys(keys: &[VirtualInputKey]) -> ReverseLookupMethod {
        keys.first().map(|k| ReverseLookupMethod::from_first_key(*k)).unwrap_or(ReverseLookupMethod::None)
}

fn is_pinyin_or_structure(method: ReverseLookupMethod) -> bool {
        matches!(method, ReverseLookupMethod::Pinyin | ReverseLookupMethod::Structure)
}

fn contains_capitalized_event(events: &[BasicInputEvent]) -> bool {
        events.iter().any(|e| e.is_capitalized())
}

fn is_peculiar_input(method: ReverseLookupMethod, keys: &[VirtualInputKey], events: &[BasicInputEvent]) -> bool {
        match method {
                ReverseLookupMethod::None => {
                        contains_capitalized_event(events) || keys.iter().any(|k| !k.is_syllable_letter())
                }
                ReverseLookupMethod::Pinyin => {
                        contains_capitalized_event(events) || keys.iter().any(|k| !k.is_letter())
                }
                ReverseLookupMethod::Structure => {
                        keys.len() > 1 && keys[1..].iter().any(|k| !k.is_syllable_letter())
                }
                _ => false,
        }
}

fn keyboard_open_from_mode(mode: InputMethodMode) -> bool {
        mode == InputMethodMode::Cantonese
}
fn input_method_mode_from_open(open: bool) -> InputMethodMode {
        if open { InputMethodMode::Cantonese } else { InputMethodMode::Abc }
}
fn full_width_from_form(form: CharacterForm) -> bool {
        form == CharacterForm::FullWidth
}
fn character_form_from_full_width(full: bool) -> CharacterForm {
        if full { CharacterForm::FullWidth } else { CharacterForm::HalfWidth }
}
fn cantonese_punctuation_from_form(form: PunctuationForm) -> bool {
        form == PunctuationForm::Cantonese
}
fn punctuation_form_from_cantonese(cantonese: bool) -> PunctuationForm {
        if cantonese { PunctuationForm::Cantonese } else { PunctuationForm::English }
}

/// A displayed candidate list item (text + comment + consumed input count).
#[derive(Clone)]
pub struct CandidateItem {
        pub text: String,
        pub comment: String,
        pub input_count: usize,
        /// Separator row — renders as a divider line, never numbered or
        /// selectable. Used by the options menu between option groups.
        pub separator: bool,
}

struct PreservedKey {
        guid: GUID,
        vkey: u32,
        modifiers: u32,
        description: String,
}

/// Compartment change kinds signaled back to the service.
pub enum CompartmentChange {
        None,
        Private,
        Conversion,
        KeyboardOpen,
}

pub struct Processor {
        pub keys: KeystrokeEngine,
        pub engine: Option<CoreImeEngine>,
        pub memory: Mutex<InputMemory>,

        input_keys: Vec<VirtualInputKey>,
        input_events: Vec<BasicInputEvent>,

        pub settings: ImeSettings,
        client_id: u32,

        pub lang_bar: Option<ComObject<LangBarItem>>,
        preserved_keys: Vec<PreservedKey>,

        cached_input_text: String,
        cached_method: ReverseLookupMethod,
        cached_suggestions: Vec<Candidate>,
        selected_memory: Vec<Lexicon>,

        is_applying_settings: bool,
        is_mirroring_conversion: bool,
}

impl Processor {
        /// Port of CCompositionProcessorEngine::SetupLanguageProfile.
        pub fn new(thread_mgr: &ITfThreadMgr, client_id: u32) -> Option<Self> {
                let mut settings = load_settings();
                let mut processor = Self {
                        keys: KeystrokeEngine::new(),
                        engine: None,
                        memory: Mutex::new(InputMemory::new()),
                        input_keys: Vec::new(),
                        input_events: Vec::new(),
                        settings,
                        client_id,
                        lang_bar: None,
                        preserved_keys: Vec::new(),
                        cached_input_text: String::new(),
                        cached_method: ReverseLookupMethod::None,
                        cached_suggestions: Vec::new(),
                        selected_memory: Vec::new(),
                        is_applying_settings: false,
                        is_mirroring_conversion: false,
                };

                let _thread_unknown: IUnknown = thread_mgr.cast().ok()?;
                let skip = |name: &str| cfg!(debug_assertions) && std::env::var(name).is_ok();
                processor.is_applying_settings = true;
                if skip("RCANTONESE_SKIP_COMPARTMENTS") {
                        crate::globals::log("Processor::new compartments skipped");
                } else {
                        processor.apply_settings_to_compartments(thread_mgr);
                }
                processor.is_applying_settings = false;
                crate::globals::log("Processor::new after compartments");

                processor.keys.set_candidate_list_range(processor.settings.candidate_page_size);
                if skip("RCANTONESE_SKIP_PRESERVED") {
                        crate::globals::log("Processor::new preserved skipped");
                } else {
                        processor.setup_preserved(thread_mgr);
                }
                crate::globals::log("Processor::new after preserved");
                if skip("RCANTONESE_SKIP_LANGBAR") {
                        crate::globals::log("Processor::new langbar skipped");
                } else {
                        processor.setup_language_bar(thread_mgr, false);
                }
                crate::globals::log("Processor::new after langbar");
                if cfg!(debug_assertions) && std::env::var("RCANTONESE_SKIP_ENGINE").is_ok() {
                        crate::globals::log("Processor::new engine skipped");
                } else {
                        processor.engine = CoreImeEngine::prepare();
                }
                crate::globals::log(&format!(
                        "Processor::new engine={} db={}",
                        processor.engine.is_some(),
                        crate::globals::default_database_path().display()
                ));
                crate::globals::log("Processor::new before memory.lock");
                if skip("RCANTONESE_SKIP_MEMORY") {
                        crate::globals::log("Processor::new memory skipped");
                } else if let Ok(mut memory) = processor.memory.lock() {
                        crate::globals::log("Processor::new before memory.prepare");
                        memory.prepare();
                        crate::globals::log("Processor::new after memory.prepare");
                }
                crate::globals::log("Processor::new done");
                Some(processor)
        }

        // -- preserved keys --------------------------------------------------

        fn setup_preserved(&mut self, thread_mgr: &ITfThreadMgr) {
                let key = |guid: GUID, vkey: u32, modifiers: u32, description: u32| PreservedKey {
                        guid,
                        vkey,
                        modifiers: modifiers & 0xffff,
                        description: crate::strings::text_or(description, "?").to_string(),
                };
                use crate::strings::*;
                self.preserved_keys = vec![
                        key(globals::GUID_PRESERVEDKEY_INPUT_MODE, 0x10, globals::TF_MOD_ON_KEYUP_SHIFT_ONLY, IDS_DESC_INPUT_MODE_TOGGLE),
                        key(globals::GUID_PRESERVEDKEY_CHARACTER_FORM, 0x20, globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_FORM_TOGGLE),
                        key(globals::GUID_PRESERVEDKEY_PUNCTUATION_FORM, 0xBE, globals::TF_MOD_CONTROL, IDS_DESC_PUNCTUATION_FORM_TOGGLE),
                        key(globals::GUID_PRESERVEDKEY_VARIANT_TRADITIONAL, 0x31, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_VARIANT_TRADITIONAL),
                        key(globals::GUID_PRESERVEDKEY_VARIANT_HONGKONG, 0x32, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_VARIANT_HONG_KONG),
                        key(globals::GUID_PRESERVEDKEY_VARIANT_TAIWAN, 0x33, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_VARIANT_TAIWAN),
                        key(globals::GUID_PRESERVEDKEY_VARIANT_SIMPLIFIED, 0x34, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_VARIANT_SIMPLIFIED),
                        key(globals::GUID_PRESERVEDKEY_CHARFORM_HALF, 0x35, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_FORM_HALF_WIDTH),
                        key(globals::GUID_PRESERVEDKEY_CHARFORM_FULL, 0x36, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_CHARACTER_FORM_FULL_WIDTH),
                        key(globals::GUID_PRESERVEDKEY_PUNCT_CANTONESE, 0x37, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_PUNCTUATION_FORM_CANTONESE),
                        key(globals::GUID_PRESERVEDKEY_PUNCT_ENGLISH, 0x38, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_PUNCTUATION_FORM_ENGLISH),
                        key(globals::GUID_PRESERVEDKEY_MODE_CANTONESE, 0x39, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_INPUT_MODE_CANTONESE),
                        key(globals::GUID_PRESERVEDKEY_MODE_ABC, 0x30, globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT, IDS_DESC_INPUT_MODE_ABC),
                ];

                if let Ok(keystroke_mgr) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                        for key in &self.preserved_keys {
                                let tf_key = TF_PRESERVEDKEY {
                                        uVKey: key.vkey,
                                        uModifiers: key.modifiers,
                                };
                                let desc_wide: Vec<u16> = key.description.encode_utf16().collect();
                                unsafe {
                                        let _ = keystroke_mgr.PreserveKey(self.client_id, &key.guid, &tf_key, &desc_wide);
                                }
                        }
                }
        }

        pub fn unpreserve_all(&self, thread_mgr: &ITfThreadMgr) {
                if let Ok(keystroke_mgr) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                        for key in &self.preserved_keys {
                                let tf_key = TF_PRESERVEDKEY {
                                        uVKey: key.vkey,
                                        uModifiers: key.modifiers,
                                };
                                unsafe {
                                        let _ = keystroke_mgr.UnpreserveKey(&key.guid, &tf_key);
                                }
                        }
                }
        }

        fn check_shift_key_only(&self, guid: &GUID) -> bool {
                const MASK: u32 = 0xffff0000;
                for key in &self.preserved_keys {
                        if key.guid != *guid {
                                continue;
                        }
                        let modifiers = key.modifiers | (if key.guid == globals::GUID_PRESERVEDKEY_INPUT_MODE { 0x00010000 } else { 0 });
                        if modifiers & MASK != 0 {
                                if modifiers & 0x00010000 != 0 && !globals::IS_SHIFT_KEY_DOWN_ONLY.load(std::sync::atomic::Ordering::Relaxed) {
                                        return false;
                                }
                                if modifiers & 0x00020000 != 0 && !globals::IS_CONTROL_KEY_DOWN_ONLY.load(std::sync::atomic::Ordering::Relaxed) {
                                        return false;
                                }
                                if modifiers & 0x00040000 != 0 && !globals::IS_ALT_KEY_DOWN_ONLY.load(std::sync::atomic::Ordering::Relaxed) {
                                        return false;
                                }
                        }
                }
                true
        }

        pub fn should_handle_input_method_mode_key(&self, guid: &GUID) -> bool {
                // Caps Lock is a hard English override — while it's on the
                // Shift toggle must not flip the compartment (the icon and
                // effective mode stay "en").
                *guid == globals::GUID_PRESERVEDKEY_INPUT_MODE
                        && self.check_shift_key_only(guid)
                        && !crate::keys::caps_lock_on()
        }

        pub fn is_character_variant_key(&self, guid: &GUID) -> bool {
                *guid == globals::GUID_PRESERVEDKEY_VARIANT_TRADITIONAL
                        || *guid == globals::GUID_PRESERVEDKEY_VARIANT_HONGKONG
                        || *guid == globals::GUID_PRESERVEDKEY_VARIANT_TAIWAN
                        || *guid == globals::GUID_PRESERVEDKEY_VARIANT_SIMPLIFIED
        }

        pub fn input_method_mode_for_key(&self, guid: &GUID) -> Option<InputMethodMode> {
                if *guid == globals::GUID_PRESERVEDKEY_MODE_CANTONESE {
                        Some(InputMethodMode::Cantonese)
                } else if *guid == globals::GUID_PRESERVEDKEY_MODE_ABC {
                        Some(InputMethodMode::Abc)
                } else {
                        None
                }
        }

        /// Port of OnPreservedKey — returns true when the key was eaten.
        pub fn on_preserved_key(&mut self, guid: &GUID, thread_mgr: &ITfThreadMgr) -> bool {
                if *guid == globals::GUID_PRESERVEDKEY_INPUT_MODE {
                        if !self.should_handle_input_method_mode_key(guid) {
                                return false;
                        }
                        return self.toggle_input_method_mode(thread_mgr).is_ok();
                }
                if *guid == globals::GUID_PRESERVEDKEY_CHARACTER_FORM {
                        if !self.check_shift_key_only(guid) {
                                return false;
                        }
                        let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, globals::GUID_COMPARTMENT_CHARACTER_FORM);
                        let Ok(full) = compartment.get_bool() else { return false };
                        if compartment.set_bool(!full).is_err() {
                                return false;
                        }
                        let form = character_form_from_full_width(!full);
                        self.settings.character_form = form;
                        save_settings(&self.settings);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_PUNCTUATION_FORM {
                        if !self.check_shift_key_only(guid) {
                                return false;
                        }
                        let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, globals::GUID_COMPARTMENT_PUNCTUATION_FORM);
                        let Ok(cantonese) = compartment.get_bool() else { return false };
                        if compartment.set_bool(!cantonese).is_err() {
                                return false;
                        }
                        let form = punctuation_form_from_cantonese(!cantonese);
                        self.settings.punctuation_form = form;
                        save_settings(&self.settings);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_VARIANT_TRADITIONAL {
                        self.set_character_variant(CharacterVariant::Traditional);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_VARIANT_HONGKONG {
                        self.set_character_variant(CharacterVariant::HongKong);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_VARIANT_TAIWAN {
                        self.set_character_variant(CharacterVariant::Taiwan);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_VARIANT_SIMPLIFIED {
                        self.set_character_variant(CharacterVariant::Simplified);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_CHARFORM_HALF {
                        self.set_character_form(CharacterForm::HalfWidth, thread_mgr);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_CHARFORM_FULL {
                        self.set_character_form(CharacterForm::FullWidth, thread_mgr);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_PUNCT_CANTONESE {
                        self.set_punctuation_form(PunctuationForm::Cantonese, thread_mgr);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_PUNCT_ENGLISH {
                        self.set_punctuation_form(PunctuationForm::English, thread_mgr);
                        return true;
                }
                if *guid == globals::GUID_PRESERVEDKEY_MODE_CANTONESE {
                        return self.set_input_method_mode(InputMethodMode::Cantonese, thread_mgr).is_ok();
                }
                if *guid == globals::GUID_PRESERVEDKEY_MODE_ABC {
                        return self.set_input_method_mode(InputMethodMode::Abc, thread_mgr).is_ok();
                }
                false
        }

        pub fn toggle_input_method_mode(&mut self, thread_mgr: &ITfThreadMgr) -> Result<()> {
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                let open = compartment.get_bool()?;
                compartment.set_bool(!open)?;
                let mode = input_method_mode_from_open(!open);
                self.settings.input_method_mode = mode;
                save_settings(&self.settings);
                Ok(())
        }

        pub fn set_input_method_mode(&mut self, mode: InputMethodMode, thread_mgr: &ITfThreadMgr) -> Result<()> {
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                compartment.set_bool(keyboard_open_from_mode(mode))?;
                self.settings.input_method_mode = mode;
                save_settings(&self.settings);
                Ok(())
        }

        // -- input buffer -----------------------------------------------------

        pub fn add_input_key(&mut self, key: VirtualInputKey, is_shifting: bool) -> bool {
                if key.code == 0 {
                        return false;
                }
                self.input_keys.push(key);
                self.input_events.push(BasicInputEvent::from_capitalized(key, is_shifting));
                true
        }

        pub fn remove_input_key(&mut self, index: usize) {
                if index < self.input_keys.len() {
                        self.input_keys.remove(index);
                        self.input_events.remove(index);
                }
        }

        pub fn clear_input_keys(&mut self) {
                self.input_keys.clear();
                self.input_events.clear();
                self.selected_memory.clear();
        }

        pub fn input_key_count(&self) -> usize {
                self.input_keys.len()
        }

        pub fn input_keys(&self) -> &[VirtualInputKey] {
                &self.input_keys
        }

        pub fn raw_input_text(&self) -> String {
                self.current_input_text()
        }

        pub fn current_input_text(&self) -> String {
                text_from_events(&self.input_events)
        }

        /// Port of GetCandidateTailInputEvents.
        pub fn candidate_tail_input_events(&self, input_count: usize) -> Vec<BasicInputEvent> {
                let offset = input_count.min(self.input_events.len());
                let is_reverse = reverse_lookup_method_from_keys(&self.input_keys) != ReverseLookupMethod::None;
                let mut tail: Vec<BasicInputEvent> = self.input_events[offset.min(self.input_events.len())..].to_vec();
                while !tail.is_empty() && tail[0].key.is_apostrophe() {
                        tail.remove(0);
                }
                if is_reverse && offset > 0 && !tail.is_empty() {
                        tail.insert(0, self.input_events[0]);
                }
                tail
        }

        pub fn set_input_events(&mut self, events: Vec<BasicInputEvent>) {
                self.input_keys = events.iter().map(|e| e.key).collect();
                self.input_events = events;
                self.cached_input_text.clear();
                self.cached_method = ReverseLookupMethod::None;
                self.cached_suggestions.clear();
        }

        pub fn is_reverse_lookup_buffer(&self) -> bool {
                self.current_reverse_lookup_method() != ReverseLookupMethod::None
        }

        pub fn current_reverse_lookup_method(&self) -> ReverseLookupMethod {
                reverse_lookup_method_from_keys(&self.input_keys)
        }

        fn reverse_lookup_query_keys(&self) -> Vec<VirtualInputKey> {
                if self.input_keys.is_empty() || self.current_reverse_lookup_method() == ReverseLookupMethod::None {
                        Vec::new()
                } else {
                        self.input_keys[1..].to_vec()
                }
        }

        fn reverse_lookup_reading_text(&self, suggestions: &[Candidate], is_peculiar: bool) -> String {
                let method = self.current_reverse_lookup_method();
                if suggestions.is_empty() || suggestions[0].lexicon.mark.is_empty() || suggestions[0].is_not_cantonese() {
                        return self.current_input_text();
                }
                if method == ReverseLookupMethod::Pinyin {
                        let input_text = self.current_input_text();
                        let tail_mark = if is_peculiar {
                                mark_formatted(&input_text.chars().skip(1).collect::<String>())
                        } else {
                                suggestions[0].lexicon.mark.clone()
                        };
                        return format!("{} {}", input_text.chars().next().unwrap_or('\0'), tail_mark);
                }
                if method == ReverseLookupMethod::Structure {
                        let input_text = self.current_input_text();
                        let tail_mark = if is_peculiar {
                                mark_formatted(&tone_converted(&input_text.chars().skip(1).collect::<String>()))
                        } else {
                                suggestions[0].lexicon.mark.clone()
                        };
                        return format!("{} {}", input_text.chars().next().unwrap_or('\0'), tail_mark);
                }
                suggestions[0].lexicon.mark.clone()
        }

        /// Port of IsNonAlphabeticInputKey — method-aware.
        pub fn is_non_alphabetic_input_key(&self, code: u32) -> bool {
                let method = self.current_reverse_lookup_method();
                if globals::modifiers_value() != 0 {
                        return false;
                }
                let Some(input_key) = VirtualInputKey::for_key_code(code) else { return false };
                if input_key.is_apostrophe() && (method == ReverseLookupMethod::None || is_pinyin_or_structure(method)) {
                        return true;
                }
                if method == ReverseLookupMethod::Stroke && input_key.is_tone_number() {
                        return true;
                }
                false
        }

        /// Port of GetReadingStrings — at most one string.
        pub fn reading_string(&mut self) -> Option<String> {
                if self.input_keys.is_empty() {
                        return None;
                }
                let current_input_text = self.current_input_text();
                let suggestions = self.input_suggestions().to_vec();
                if !suggestions.is_empty() {
                        let method = self.current_reverse_lookup_method();
                        let is_reverse = method != ReverseLookupMethod::None;
                        let input_length = current_input_text.chars().count();
                        let mut matched_input_count = suggestions[0].lexicon.input_count;
                        if is_reverse && input_length > 0 {
                                matched_input_count = matched_input_count.min(input_length - 1) + 1;
                        }
                        let is_peculiar = is_peculiar_input(method, &self.input_keys, &self.input_events);
                        let suggestions_first_not_cantonese = suggestions[0].is_not_cantonese();
                        if suggestions_first_not_cantonese {
                                return Some(current_input_text);
                        }
                        if !is_reverse && is_peculiar {
                                return Some(preview_mark_normalized(&self.input_events));
                        }
                        if is_reverse && is_peculiar {
                                return Some(self.reverse_lookup_reading_text(&suggestions, true));
                        }
                        if matched_input_count < input_length {
                                return Some(current_input_text);
                        }
                        let text = if is_reverse {
                                self.reverse_lookup_reading_text(&suggestions, false)
                        } else {
                                suggestions[0].lexicon.mark.clone()
                        };
                        if text.is_empty() {
                                Some(current_input_text)
                        } else {
                                Some(text)
                        }
                } else {
                        Some(current_input_text)
                }
        }

        /// Port of GetCandidateList / AppendInputEngineCandidates.
        pub fn candidate_list(&mut self) -> Vec<CandidateItem> {
                // "`" alone (pinyin prefix with no query yet) — offer the
                // grave/tilde symbol list so the key keeps its punctuation
                // menu behavior. Once letters follow, pinyin lookup takes over.
                if self.input_keys.len() == 1 && self.input_keys[0].is_grave() {
                        return crate::punctuation::grave_key()
                                .symbols(false)
                                .iter()
                                .map(|symbol| CandidateItem {
                                        text: symbol.text.to_string(),
                                        comment: symbol.comment.unwrap_or_default().to_string(),
                                        input_count: 1,
                        separator: false,
                                })
                                .collect();
                }
                let suggestions = self.input_suggestions().to_vec();
                if suggestions.is_empty() {
                        return Vec::new();
                }
                let is_reverse = self.is_reverse_lookup_buffer();
                let query_length = if is_reverse && !self.input_keys.is_empty() {
                        self.input_keys.len() - 1
                } else {
                        0
                };
                suggestions
                        .iter()
                        .map(|suggestion| CandidateItem {
                                text: suggestion.text.clone(),
                                comment: suggestion.comment.clone().unwrap_or_default(),
                                input_count: if is_reverse {
                                        suggestion.lexicon.input_count.min(query_length) + 1
                                } else {
                                        suggestion.lexicon.input_count
                                },
                                separator: false,
                        })
                        .collect()
        }

        fn candidate_at(&self, index: u32) -> Option<Lexicon> {
                if self.is_reverse_lookup_buffer() {
                        return None;
                }
                self.cached_suggestions.get(index as usize).map(|c| c.lexicon.clone())
        }

        pub fn append_selected_candidate_for_memory(&mut self, index: u32) {
                match self.candidate_at(index) {
                        Some(lexicon) if lexicon.is_cantonese() => self.selected_memory.push(lexicon),
                        _ => self.selected_memory.clear(),
                }
        }

        pub fn commit_selected_candidate_for_memory(&mut self, index: u32) {
                match self.candidate_at(index) {
                        Some(lexicon) if lexicon.is_cantonese() => {
                                self.selected_memory.push(lexicon);
                                if let Some(joined) = join_lexicons(&self.selected_memory) {
                                        if let Ok(mut memory) = self.memory.lock() {
                                                memory.handle(&joined);
                                        }
                                }
                                self.selected_memory.clear();
                        }
                        _ => self.selected_memory.clear(),
                }
        }

        pub fn forget_candidate_from_memory(&mut self, index: u32) -> bool {
                let Some(lexicon) = self.candidate_at(index) else { return false };
                if lexicon.is_not_cantonese() {
                        return false;
                }
                let result = self.memory.lock().map(|mut m| m.forget(&lexicon)).unwrap_or(false);
                self.cached_input_text.clear();
                self.cached_method = ReverseLookupMethod::None;
                self.cached_suggestions.clear();
                result
        }

        // -- suggestions ---------------------------------------------------------

        fn current_character_standard(&self) -> CharacterStandard {
                standard_for_variant(self.settings.character_variant)
        }

        /// Port of GetInputSuggestions — cached by input text + method.
        pub fn input_suggestions(&mut self) -> &[Candidate] {
                let input_text = text_from_keys(&self.input_keys);
                let method = reverse_lookup_method_from_keys(&self.input_keys);
                let Some(engine) = &self.engine else {
                        self.cached_input_text = input_text;
                        self.cached_method = method;
                        self.cached_suggestions.clear();
                        return &self.cached_suggestions;
                };
                if input_text != self.cached_input_text || method != self.cached_method {
                        self.cached_input_text = input_text;
                        self.cached_method = method;
                        if method == ReverseLookupMethod::None {
                                let memory_suggestions = {
                                        let memory = self.memory.lock().ok();
                                        match memory {
                                                Some(memory) => memory.suggest(
                                                        &self.input_keys,
                                                        &engine.segment(&self.input_keys),
                                                        &engine.segmenter,
                                                ),
                                                None => Vec::new(),
                                        }
                                };
                                let queried = engine.suggest(&self.input_keys, &engine.segment(&self.input_keys), true);
                                // Plain-text (English word) candidates only when no Chinese
                                // candidates matched — diverges from upstream which ranks them
                                // above queried lexicons.
                                let texts = if queried.is_empty() {
                                        engine.search_plain_texts(&self.input_keys)
                                } else {
                                        Vec::new()
                                };
                                let symbols = engine.search_symbols(&self.input_keys, &engine.segment(&self.input_keys));
                                self.cached_suggestions = crate::converter::dispatch(
                                        &engine.database,
                                        &memory_suggestions,
                                        &[],
                                        &texts,
                                        &symbols,
                                        &queried,
                                        RomanizationForm::Full,
                                        self.current_character_standard(),
                                );
                        } else {
                                let mut suggestions = crate::converter::transform(
                                        &engine.database,
                                        &engine.search_plain_texts(&self.input_keys),
                                        RomanizationForm::Full,
                                        self.current_character_standard(),
                                );
                                let query_keys = self.reverse_lookup_query_keys();
                                if !query_keys.is_empty() {
                                        suggestions.extend(crate::converter::transform(
                                                &engine.database,
                                                &engine.reverse_lookup(method, &query_keys),
                                                RomanizationForm::Full,
                                                self.current_character_standard(),
                                        ));
                                }
                                self.cached_suggestions = suggestions;
                        }
                }
                &self.cached_suggestions
        }

        // -- settings / compartments --------------------------------------------

        pub fn current_input_method_mode(&self) -> InputMethodMode {
                self.settings.input_method_mode
        }
        pub fn current_character_variant(&self) -> CharacterVariant {
                self.settings.character_variant
        }
        pub fn current_character_form(&self) -> CharacterForm {
                self.settings.character_form
        }
        pub fn current_punctuation_form(&self) -> PunctuationForm {
                self.settings.punctuation_form
        }
        pub fn current_candidate_page_size(&self) -> u32 {
                self.settings.candidate_page_size
        }
        pub fn current_candidate_font_size(&self) -> u32 {
                self.settings.candidate_font_size
        }
        pub fn current_candidate_number_font_size(&self) -> u32 {
                self.settings.candidate_number_font_size
        }
        pub fn current_candidate_comment_font_size(&self) -> u32 {
                self.settings.candidate_comment_font_size
        }

        pub fn set_character_variant(&mut self, variant: CharacterVariant) -> bool {
                if self.settings.character_variant == variant {
                        return false;
                }
                self.settings.character_variant = variant;
                save_settings(&self.settings);
                self.cached_input_text.clear();
                self.cached_suggestions.clear();
                true
        }

        pub fn set_candidate_page_size(&mut self, page_size: u32) -> bool {
                let normalized = candidate_page_size_from_raw(page_size);
                if self.settings.candidate_page_size == normalized {
                        return false;
                }
                self.settings.candidate_page_size = normalized;
                self.keys.set_candidate_list_range(normalized);
                save_settings(&self.settings);
                true
        }

        pub fn set_punctuation_form(&mut self, form: PunctuationForm, thread_mgr: &ITfThreadMgr) {
                if self.settings.punctuation_form == form {
                        return;
                }
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, globals::GUID_COMPARTMENT_PUNCTUATION_FORM);
                if compartment.set_bool(cantonese_punctuation_from_form(form)).is_err() {
                        return;
                }
                self.settings.punctuation_form = form;
                save_settings(&self.settings);
        }

        pub fn set_character_form(&mut self, form: CharacterForm, thread_mgr: &ITfThreadMgr) {
                if self.settings.character_form == form {
                        return;
                }
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, globals::GUID_COMPARTMENT_CHARACTER_FORM);
                if compartment.set_bool(full_width_from_form(form)).is_err() {
                        return;
                }
                self.settings.character_form = form;
                save_settings(&self.settings);
        }

        pub fn set_candidate_font_size(&mut self, size: u32) -> bool {
                let normalized = candidate_font_size_from_raw(size);
                if self.settings.candidate_font_size == normalized {
                        return false;
                }
                self.settings.candidate_font_size = normalized;
                save_settings(&self.settings);
                true
        }
        pub fn set_candidate_number_font_size(&mut self, size: u32) -> bool {
                let normalized = candidate_font_size_from_raw(size);
                if self.settings.candidate_number_font_size == normalized {
                        return false;
                }
                self.settings.candidate_number_font_size = normalized;
                save_settings(&self.settings);
                true
        }
        pub fn set_candidate_comment_font_size(&mut self, size: u32) -> bool {
                let normalized = candidate_font_size_from_raw(size);
                if self.settings.candidate_comment_font_size == normalized {
                        return false;
                }
                self.settings.candidate_comment_font_size = normalized;
                save_settings(&self.settings);
                true
        }

        fn apply_settings_to_compartments(&mut self, thread_mgr: &ITfThreadMgr) {
                let thread_unknown: IUnknown = thread_mgr.cast().unwrap();
                let open = Compartment::new(&thread_unknown, self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                let _ = open.set_bool(keyboard_open_from_mode(self.settings.input_method_mode));
                let form = Compartment::new(&thread_unknown, self.client_id, globals::GUID_COMPARTMENT_CHARACTER_FORM);
                let _ = form.set_bool(full_width_from_form(self.settings.character_form));
                let punct = Compartment::new(&thread_unknown, self.client_id, globals::GUID_COMPARTMENT_PUNCTUATION_FORM);
                let _ = punct.set_bool(cantonese_punctuation_from_form(self.settings.punctuation_form));
        }

        pub fn apply_persisted_settings(&mut self, thread_mgr: &ITfThreadMgr) {
                self.settings = load_settings();
                self.keys.set_candidate_list_range(self.settings.candidate_page_size);
                self.is_applying_settings = true;
                self.apply_settings_to_compartments(thread_mgr);
                self.private_compartments_updated(thread_mgr);
                self.keyboard_open_compartment_updated(thread_mgr);
                self.is_applying_settings = false;
        }

        /// Port of PrivateCompartmentsUpdated.
        pub fn private_compartments_updated(&mut self, thread_mgr: &ITfThreadMgr) {
                let thread_unknown: IUnknown = match thread_mgr.cast() {
                        Ok(u) => u,
                        Err(_) => return,
                };
                let conversion = Compartment::new(&thread_unknown, self.client_id, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION);
                let Ok(mut conversion_mode) = conversion.get_dword() else { return };
                let prev = conversion_mode;

                let char_form = Compartment::new(&thread_unknown, self.client_id, globals::GUID_COMPARTMENT_CHARACTER_FORM);
                if let Ok(full) = char_form.get_bool() {
                        if !full && conversion_mode & 0x08 != 0 {
                                // TF_CONVERSIONMODE_FULLSHAPE = 0x08
                                conversion_mode &= !0x08;
                        } else if full && conversion_mode & 0x08 == 0 {
                                conversion_mode |= 0x08;
                        }
                }
                let punct = Compartment::new(&thread_unknown, self.client_id, globals::GUID_COMPARTMENT_PUNCTUATION_FORM);
                if let Ok(cantonese) = punct.get_bool() {
                        // TF_CONVERSIONMODE_SYMBOL = 0x0400
                        if !cantonese && conversion_mode & 0x0400 != 0 {
                                conversion_mode &= !0x0400;
                        } else if cantonese && conversion_mode & 0x0400 == 0 {
                                conversion_mode |= 0x0400;
                        }
                }
                if conversion_mode != prev {
                        let _ = conversion.set_dword(conversion_mode);
                }
                if !self.is_applying_settings && !self.is_mirroring_conversion {
                        self.persist_character_form(thread_mgr);
                        self.persist_punctuation_form(thread_mgr);
                }
        }

        /// Port of ConversionModeCompartmentUpdated.
        pub fn conversion_mode_compartment_updated(&mut self, thread_mgr: &ITfThreadMgr) {
                if self.is_applying_settings {
                        return;
                }
                let thread_unknown: IUnknown = match thread_mgr.cast() {
                        Ok(u) => u,
                        Err(_) => return,
                };
                let conversion = Compartment::new(&thread_unknown, self.client_id, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION);
                let Ok(conversion_mode) = conversion.get_dword() else { return };

                self.is_mirroring_conversion = true;
                let char_form = Compartment::new(&thread_unknown, self.client_id, globals::GUID_COMPARTMENT_CHARACTER_FORM);
                if let Ok(full) = char_form.get_bool() {
                        if !full && conversion_mode & 0x08 != 0 {
                                let _ = char_form.set_bool(true);
                        } else if full && conversion_mode & 0x08 == 0 {
                                let _ = char_form.set_bool(false);
                        }
                }
                let punct = Compartment::new(&thread_unknown, self.client_id, globals::GUID_COMPARTMENT_PUNCTUATION_FORM);
                if let Ok(cantonese) = punct.get_bool() {
                        if !cantonese && conversion_mode & 0x0400 != 0 {
                                let _ = punct.set_bool(true);
                        } else if cantonese && conversion_mode & 0x0400 == 0 {
                                let _ = punct.set_bool(false);
                        }
                }
                let open = Compartment::new(&thread_unknown, self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                if let Ok(is_open) = open.get_bool() {
                        // TF_CONVERSIONMODE_NATIVE = 0x01
                        if is_open && conversion_mode & 0x01 == 0 {
                                let _ = open.set_bool(false);
                        } else if !is_open && conversion_mode & 0x01 != 0 {
                                let _ = open.set_bool(true);
                        }
                }
                self.is_mirroring_conversion = false;
        }

        /// Port of KeyboardOpenCompartmentUpdated.
        pub fn keyboard_open_compartment_updated(&mut self, thread_mgr: &ITfThreadMgr) {
                let thread_unknown: IUnknown = match thread_mgr.cast() {
                        Ok(u) => u,
                        Err(_) => return,
                };
                let conversion = Compartment::new(&thread_unknown, self.client_id, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION);
                let Ok(mut conversion_mode) = conversion.get_dword() else { return };
                let prev = conversion_mode;

                let open = Compartment::new(&thread_unknown, self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                if let Ok(is_open) = open.get_bool() {
                        if is_open && conversion_mode & 0x01 == 0 {
                                conversion_mode |= 0x01;
                        } else if !is_open && conversion_mode & 0x01 != 0 {
                                conversion_mode &= !0x01;
                        }
                }
                if conversion_mode != prev {
                        let _ = conversion.set_dword(conversion_mode);
                }
                if !self.is_applying_settings && !self.is_mirroring_conversion {
                        self.persist_input_method_mode(thread_mgr);
                }
        }

        fn persist_input_method_mode(&mut self, thread_mgr: &ITfThreadMgr) {
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                if let Ok(open) = compartment.get_bool() {
                        let mode = input_method_mode_from_open(open);
                        self.settings.input_method_mode = mode;
                        save_settings(&self.settings);
                }
        }
        fn persist_character_form(&mut self, thread_mgr: &ITfThreadMgr) {
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, globals::GUID_COMPARTMENT_CHARACTER_FORM);
                if let Ok(full) = compartment.get_bool() {
                        let form = character_form_from_full_width(full);
                        self.settings.character_form = form;
                        save_settings(&self.settings);
                }
        }
        fn persist_punctuation_form(&mut self, thread_mgr: &ITfThreadMgr) {
                let compartment = Compartment::new(&thread_mgr.cast::<IUnknown>().unwrap(), self.client_id, globals::GUID_COMPARTMENT_PUNCTUATION_FORM);
                if let Ok(cantonese) = compartment.get_bool() {
                        let form = punctuation_form_from_cantonese(cantonese);
                        self.settings.punctuation_form = form;
                        save_settings(&self.settings);
                }
        }

        // -- language bar --------------------------------------------------------

        fn setup_language_bar(&mut self, thread_mgr: &ITfThreadMgr, is_secure_mode: bool) {
                let item: ComObject<LangBarItem> = LangBarItem::new(
                        // Must be the reserved GUID_LBI_INPUTMODE — since
                        // Windows 8 the tray only renders the item whose
                        // GetInfo returns this guid; a custom guid registers
                        // fine but never displays.
                        windows::Win32::UI::TextServices::GUID_LBI_INPUTMODE,
                        crate::strings::text_or(crate::strings::IDS_LANGBAR_INPUT_METHOD_MODE, "Input Mode"),
                        crate::strings::text_or(crate::strings::IDS_LANGBAR_INPUT_MODE_TOOLTIP, "Input Mode"),
                        globals::IDI_INPUT_MODE_CANTONESE,
                        globals::IDI_INPUT_MODE_ABC,
                        is_secure_mode,
                )
                .into();
                // Register the compartment before adding the item — the shell
                // queries GetIcon immediately on AddItem and needs the
                // compartment to be readable by then.
                LangBarItem::register_compartment(&item, thread_mgr, self.client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                LangBarItem::add_item(&item, thread_mgr);
                self.lang_bar = Some(item);
        }

        /// Port of ~CLangBarItemButton cleanup — unadvise the compartment sink
        /// and remove the item so reactivation can add it cleanly.
        pub fn teardown_language_bar(&self, thread_mgr: &ITfThreadMgr) {
                if let Some(item) = &self.lang_bar {
                        LangBarItem::unadvise_compartment_sink(item);
                        LangBarItem::remove_item(item, thread_mgr);
                }
        }

        pub fn set_language_bar_status(&self, status: u32, is_set: bool) {
                if let Some(item) = &self.lang_bar {
                        item.set_status(status, is_set);
                }
        }
        pub fn show_all_language_bar_icons(&self) {
                self.set_language_bar_status(TF_LBI_STATUS_HIDDEN, false);
        }
        pub fn hide_all_language_bar_icons(&self) {
                self.set_language_bar_status(TF_LBI_STATUS_HIDDEN, true);
        }

        /// Called when document focus changes — refresh the mode icon.
        pub fn update_language_bar(&self, thread_mgr: Option<&ITfThreadMgr>, client_id: u32, _has_focus: bool) {
                let Some(thread_mgr) = thread_mgr else { return };
                let Some(item) = &self.lang_bar else { return };
                let thread_unknown: IUnknown = match thread_mgr.cast() {
                        Ok(u) => u,
                        Err(_) => return,
                };
                let open = Compartment::new(&thread_unknown, client_id, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
                if let Ok(is_open) = open.get_bool() {
                        item.update(is_open);
                        crate::tray::update_mode(is_open);
                }
        }

        pub fn on_activated(&mut self, thread_mgr: Option<&ITfThreadMgr>, client_id: u32) {
                // Port of ActiveLanguageProfileNotifySink::OnActivated — the
                // shell surfaces the tray icon in response to this update.
                self.show_all_language_bar_icons();
                if let Some(tm) = thread_mgr {
                        self.apply_persisted_settings(tm);
                }
                self.update_language_bar(thread_mgr, client_id, true);
        }
        pub fn on_deactivated(&self) {
                self.hide_all_language_bar_icons();
        }
        pub fn on_thread_focus(&self, thread_mgr: Option<&ITfThreadMgr>, client_id: u32) {
                self.update_language_bar(thread_mgr, client_id, true);
        }
}
