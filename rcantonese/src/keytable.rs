// Keystroke categories / functions and IsVirtualKeyNeed — port of
// KeyStateCategory.h and CompositionProcessorEngineKeystrokes.cpp.
#![allow(dead_code)]

use crate::globals;
use crate::settings;
use crate::types::{ReverseLookupMethod, VirtualInputKey};

// virtual key codes used by the input method
pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21; // PageUp
pub const VK_NEXT: u32 = 0x22; // PageDown
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_DELETE: u32 = 0x2E;
pub const VK_PACKET: u32 = 0xE7;
pub const VK_OEM_MINUS: u32 = 0xBD;
pub const VK_OEM_PLUS: u32 = 0xBB;
pub const VK_OEM_1: u32 = 0xBA; // ;:
pub const VK_OEM_2: u32 = 0xBF; // /?
pub const VK_OEM_3: u32 = 0xC0; // `~
pub const VK_OEM_4: u32 = 0xDB; // [{
pub const VK_OEM_5: u32 = 0xDC; // \|
pub const VK_OEM_6: u32 = 0xDD; // ]}
pub const VK_OEM_7: u32 = 0xDE; // '"
pub const VK_OEM_COMMA: u32 = 0xBC;
pub const VK_OEM_PERIOD: u32 = 0xBE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeystrokeCategory {
        None,
        Composing,
        Candidate,
        Phrase,
        InvokeCompositionEditSession,
        InvokeQuickPhraseEditSession,
        InvokeWildcardCompositionEditSession,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeystrokeFunction {
        None,
        Input,
        Cancel,
        FinalizeTextStore,
        FinalizeTextstoreAndInput,
        FinalizeCandidateList,
        FinalizeCandidateListAndInput,
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        MovePageUp,
        MovePageDown,
        MovePageTop,
        MovePageBottom,
        SelectByNumber,
        Convert,
        ConvertWildcard,
        Backspace,
        PunctuationKey,
        DoubleSingleByte,
        CharacterForm,
        ForgetCandidate,
        SymbolQuery,
        ToggleOptions,
}

#[derive(Clone, Copy)]
pub struct KeystrokeState {
        pub category: KeystrokeCategory,
        pub function: KeystrokeFunction,
}

impl KeystrokeState {
        pub const NONE: Self = Self {
                category: KeystrokeCategory::None,
                function: KeystrokeFunction::None,
        };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CandidateMode {
        None,
        Original,
        Phrase,
        Incremental,
        WithNextComposition,
        Punctuation,
        Options,
}

#[derive(Clone, Copy)]
struct Keystroke {
        virtual_key: u32,
        modifiers: u32,
        function: KeystrokeFunction,
}

fn keystroke_table() -> [Keystroke; 27] {
        let mut table = [Keystroke {
                virtual_key: 0,
                modifiers: 0,
                function: KeystrokeFunction::Input,
        }; 27];
        for (i, key) in crate::types::ALPHABET_SET.iter().enumerate() {
                table[i] = Keystroke {
                        virtual_key: key.key_code,
                        modifiers: 0,
                        function: KeystrokeFunction::Input,
                };
        }
        // "`" starts a pinyin reverse-lookup composition (rime-cantonese style).
        table[26] = Keystroke {
                virtual_key: crate::types::GRAVE_KEY.key_code,
                modifiers: 0,
                function: KeystrokeFunction::Input,
        };
        table
}

fn is_no_modifier() -> bool {
        globals::check_modifiers(globals::modifiers_value(), 0)
}
fn is_shift_only_modifier() -> bool {
        globals::check_modifiers(globals::modifiers_value(), globals::TF_MOD_SHIFT)
}
fn is_control_shift_modifier() -> bool {
        globals::check_modifiers(globals::modifiers_value(), globals::TF_MOD_CONTROL | globals::TF_MOD_SHIFT)
}

fn set_state(state: &mut KeystrokeState, category: KeystrokeCategory, function: KeystrokeFunction) -> bool {
        state.category = category;
        state.function = function;
        true
}

fn try_candidate_forget_key(code: u32, state: &mut KeystrokeState) -> bool {
        if (code == VK_BACK || code == VK_DELETE) && is_control_shift_modifier() {
                return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::ForgetCandidate);
        }
        false
}

fn try_candidate_navigation_key(code: u32, category: KeystrokeCategory, state: &mut KeystrokeState) -> bool {
        match code {
                VK_TAB => {
                        if is_no_modifier() {
                                set_state(state, category, KeystrokeFunction::MoveDown)
                        } else if is_shift_only_modifier() {
                                set_state(state, category, KeystrokeFunction::MoveUp)
                        } else {
                                false
                        }
                }
                VK_UP => set_state(state, category, KeystrokeFunction::MoveUp),
                VK_DOWN => set_state(state, category, KeystrokeFunction::MoveDown),
                VK_LEFT | VK_PRIOR => set_state(state, category, KeystrokeFunction::MovePageUp),
                VK_RIGHT | VK_NEXT => set_state(state, category, KeystrokeFunction::MovePageDown),
                VK_HOME => set_state(state, category, KeystrokeFunction::MovePageTop),
                VK_END => set_state(state, category, KeystrokeFunction::MovePageBottom),
                VK_OEM_MINUS | VK_OEM_4 => {
                        if is_no_modifier() {
                                set_state(state, category, KeystrokeFunction::MovePageUp)
                        } else {
                                false
                        }
                }
                VK_OEM_PLUS | VK_OEM_6 => {
                        if is_no_modifier() {
                                set_state(state, category, KeystrokeFunction::MovePageDown)
                        } else {
                                false
                        }
                }
                _ => false,
        }
}

fn is_non_alphabetic_input_key(code: u32, method: ReverseLookupMethod) -> bool {
        // Port of CCompositionProcessorEngine::IsNonAlphabeticInputKey: in normal
        // Jyutping mode only the apostrophe is a non-alphabetic input key; digit
        // tone marks are only accepted in stroke reverse-lookup mode. Digits in a
        // candidate list select candidates instead.
        if globals::modifiers_value() != 0 {
                return false;
        }
        let Some(input_key) = VirtualInputKey::for_key_code(code) else {
                return false;
        };
        if input_key.is_apostrophe()
                && matches!(
                        method,
                        ReverseLookupMethod::None | ReverseLookupMethod::Pinyin | ReverseLookupMethod::Structure
                )
        {
                return true;
        }
        method == ReverseLookupMethod::Stroke && input_key.is_tone_number()
}

pub struct KeystrokeEngine {
        composition_keys: Vec<Keystroke>,
        candidate_index_range: Vec<u32>,
        phrase_modifier: u32,
}

impl KeystrokeEngine {
        pub fn new() -> Self {
                let mut engine = Self {
                        composition_keys: keystroke_table().to_vec(),
                        candidate_index_range: Vec::new(),
                        phrase_modifier: 0,
                };
                engine.set_candidate_list_range(settings::DEFAULT_CANDIDATE_PAGE_SIZE);
                engine
        }

        pub fn set_candidate_list_range(&mut self, page_size: u32) {
                self.candidate_index_range.clear();
                for i in 1..=page_size {
                        self.candidate_index_range.push(if i == 10 { 0 } else { i });
                }
        }

        pub fn candidate_list_index_range(&self) -> &[u32] {
                &self.candidate_index_range
        }

        pub fn set_phrase_modifier(&mut self, modifier: u32) {
                self.phrase_modifier = modifier;
        }
        pub fn phrase_modifier(&self) -> u32 {
                self.phrase_modifier
        }

        fn is_virtual_key_keystroke_composition(&self, code: u32, state: &mut KeystrokeState, function: KeystrokeFunction) -> bool {
                state.category = KeystrokeCategory::None;
                state.function = KeystrokeFunction::None;
                for keystroke in &self.composition_keys {
                        if keystroke.virtual_key == code && globals::check_modifiers(globals::modifiers_value(), keystroke.modifiers) {
                                if function == KeystrokeFunction::None || function == keystroke.function {
                                        state.category = KeystrokeCategory::Composing;
                                        state.function = keystroke.function;
                                        return true;
                                }
                        }
                }
                false
        }

        fn is_virtual_key_keystroke_candidate(&self, code: u32, state: &mut KeystrokeState, candidate_mode: CandidateMode) -> Option<bool> {
                // Returns Some(ret_code) when a candidate keystroke matched, else None.
                let _ = state;
                let _ = code;
                let _ = candidate_mode;
                None
        }

        fn is_key_in_range(&self, code: u32) -> bool {
                // Port of CCandidateRange::IsRange: digits 0-9 (0x30..0x39) and numpad (0x60..0x69).
                let value = code.wrapping_sub(0x30);
                self.candidate_index_range.iter().any(|range| {
                        *range == value || (0x60..=0x69).contains(&code) && (code - 0x60) == *range
                })
        }

        fn is_keystroke_range(&self, code: u32, state: &mut KeystrokeState, candidate_mode: CandidateMode) -> bool {
                state.category = KeystrokeCategory::None;
                state.function = KeystrokeFunction::None;
                if !self.is_key_in_range(code) {
                        return false;
                }
                match candidate_mode {
                        CandidateMode::Phrase => {
                                if (self.phrase_modifier == 0 && globals::modifiers_value() == 0)
                                        || (self.phrase_modifier != 0
                                                && globals::check_modifiers(globals::modifiers_value(), self.phrase_modifier))
                                {
                                        set_state(state, KeystrokeCategory::Phrase, KeystrokeFunction::SelectByNumber)
                                } else {
                                        state.category = KeystrokeCategory::InvokeCompositionEditSession;
                                        state.function = KeystrokeFunction::FinalizeTextstoreAndInput;
                                        false
                                }
                        }
                        CandidateMode::WithNextComposition => {
                                if (self.phrase_modifier == 0 && globals::modifiers_value() == 0)
                                        || (self.phrase_modifier != 0
                                                && globals::check_modifiers(globals::modifiers_value(), self.phrase_modifier))
                                {
                                        set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::SelectByNumber)
                                } else {
                                        false
                                }
                        }
                        CandidateMode::None => false,
                        _ => set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::SelectByNumber),
                }
        }

        /// Port of CCompositionProcessorEngine::IsVirtualKeyNeed.
        pub fn is_virtual_key_need(
                &self,
                code: u32,
                wch: u16,
                composing: bool,
                candidate_mode: CandidateMode,
                method: ReverseLookupMethod,
                state: &mut KeystrokeState,
        ) -> bool {
                state.category = KeystrokeCategory::None;
                state.function = KeystrokeFunction::None;

                if candidate_mode == CandidateMode::Punctuation {
                        match code {
                                VK_RETURN | VK_SPACE => {
                                        return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::FinalizeCandidateList)
                                }
                                VK_ESCAPE | VK_BACK => {
                                        return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Cancel)
                                }
                                _ => {}
                        }
                        if try_candidate_navigation_key(code, KeystrokeCategory::Candidate, state) {
                                return true;
                        }
                        if self.is_keystroke_range(code, state, candidate_mode) {
                                return true;
                        }
                        if VirtualInputKey::is_matched_letter(code) && (is_no_modifier() || is_shift_only_modifier()) {
                                return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::FinalizeCandidateListAndInput);
                        }
                        return false;
                }

                let mut composing = composing;
                if matches!(candidate_mode, CandidateMode::Original | CandidateMode::Phrase | CandidateMode::WithNextComposition) {
                        composing = false;
                }

                if composing || candidate_mode == CandidateMode::Incremental || candidate_mode == CandidateMode::None {
                        if (composing || candidate_mode == CandidateMode::Incremental) && is_non_alphabetic_input_key(code, method) {
                                state.category = KeystrokeCategory::Composing;
                                state.function = KeystrokeFunction::Input;
                                return true;
                        }
                        if VirtualInputKey::is_matched_letter(code)
                                && (globals::modifiers_value() == 0
                                        || globals::check_modifiers(globals::modifiers_value(), globals::TF_MOD_SHIFT))
                        {
                                state.category = KeystrokeCategory::Composing;
                                state.function = KeystrokeFunction::Input;
                                return true;
                        }
                        if self.is_virtual_key_keystroke_composition(code, state, KeystrokeFunction::None) {
                                return true;
                        }
                }

                if matches!(candidate_mode, CandidateMode::Original | CandidateMode::Phrase | CandidateMode::WithNextComposition) {
                        let category = if candidate_mode == CandidateMode::Phrase {
                                KeystrokeCategory::Phrase
                        } else {
                                KeystrokeCategory::Candidate
                        };
                        if candidate_mode != CandidateMode::Phrase && try_candidate_forget_key(code, state) {
                                return true;
                        }
                        if try_candidate_navigation_key(code, category, state) {
                                return true;
                        }
                        if let Some(ret) = self.is_virtual_key_keystroke_candidate(code, state, candidate_mode) {
                                return ret;
                        }
                        if self.is_virtual_key_keystroke_composition(code, state, KeystrokeFunction::Input) {
                                if candidate_mode != CandidateMode::Original {
                                        return true;
                                }
                                state.category = KeystrokeCategory::Candidate;
                                state.function = KeystrokeFunction::FinalizeCandidateListAndInput;
                                return true;
                        }
                } else if candidate_mode == CandidateMode::Incremental {
                        if try_candidate_forget_key(code, state) {
                                return true;
                        }
                        if try_candidate_navigation_key(code, KeystrokeCategory::Candidate, state) {
                                return true;
                        }
                        if let Some(ret) = self.is_virtual_key_keystroke_candidate(code, state, candidate_mode) {
                                return ret;
                        }
                }

                if !composing
                        && !matches!(candidate_mode, CandidateMode::Original | CandidateMode::Phrase | CandidateMode::WithNextComposition)
                        && self.is_virtual_key_keystroke_composition(code, state, KeystrokeFunction::Input)
                {
                        return true;
                }

                // System pre-defined keystrokes
                if composing {
                        if candidate_mode != CandidateMode::Incremental {
                                match code {
                                        VK_LEFT => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MoveLeft),
                                        VK_RIGHT => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MoveRight),
                                        VK_RETURN | VK_SPACE => {
                                                return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::FinalizeTextStore)
                                        }
                                        VK_ESCAPE => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::Cancel),
                                        VK_BACK => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::Backspace),
                                        VK_UP => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MoveUp),
                                        VK_DOWN => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MoveDown),
                                        VK_PRIOR => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MovePageUp),
                                        VK_NEXT => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MovePageDown),
                                        VK_HOME => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MovePageTop),
                                        VK_END => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::MovePageBottom),
                                        _ => {}
                                }
                        } else {
                                match code {
                                        VK_RETURN => {
                                                return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::FinalizeTextStore)
                                        }
                                        VK_ESCAPE => return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Cancel),
                                        VK_BACK => return set_state(state, KeystrokeCategory::Composing, KeystrokeFunction::Backspace),
                                        VK_SPACE => return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Convert),
                                        _ => {}
                                }
                        }
                }

                if matches!(candidate_mode, CandidateMode::Original | CandidateMode::WithNextComposition) {
                        match code {
                                VK_RETURN => {
                                        return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::FinalizeCandidateList)
                                }
                                VK_SPACE => return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Convert),
                                VK_BACK => return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Cancel),
                                VK_ESCAPE => {
                                        if candidate_mode == CandidateMode::WithNextComposition {
                                                state.category = KeystrokeCategory::InvokeCompositionEditSession;
                                                state.function = KeystrokeFunction::FinalizeTextStore;
                                                return true;
                                        }
                                        return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Cancel);
                                }
                                _ => {}
                        }
                        if candidate_mode == CandidateMode::WithNextComposition
                                && self.is_virtual_key_keystroke_composition(code, state, KeystrokeFunction::None)
                        {
                                state.category = KeystrokeCategory::Composing;
                                state.function = KeystrokeFunction::FinalizeTextstoreAndInput;
                                return true;
                        }
                }

                if candidate_mode == CandidateMode::Phrase {
                        match code {
                                VK_RETURN => {
                                        return set_state(state, KeystrokeCategory::Phrase, KeystrokeFunction::FinalizeCandidateList)
                                }
                                VK_SPACE => return set_state(state, KeystrokeCategory::Phrase, KeystrokeFunction::Convert),
                                VK_ESCAPE => return set_state(state, KeystrokeCategory::Phrase, KeystrokeFunction::Cancel),
                                VK_BACK => return set_state(state, KeystrokeCategory::Candidate, KeystrokeFunction::Cancel),
                                _ => {}
                        }
                }

                if self.is_keystroke_range(code, state, candidate_mode) {
                        return true;
                }
                if state.category != KeystrokeCategory::None {
                        return false;
                }

                let has_active_input = composing || candidate_mode != CandidateMode::None;
                if has_active_input && wch != 0 && !self.is_virtual_key_keystroke_composition(code, state, KeystrokeFunction::None) {
                        state.category = KeystrokeCategory::InvokeCompositionEditSession;
                        state.function = KeystrokeFunction::FinalizeTextStore;
                        return false;
                }

                false
        }
}
