// Core input-method types: VirtualInputKey, Syllable, Lexicon, Candidate,
// and the radix-100 coding helpers shared by the segmenter and the engines.
#![allow(dead_code)]

use std::fmt;

pub const COMPOUND_NUMBER_STEP: i64 = 1_000_000;
pub const MAX_CHAR_COUNT: usize = 9;

// ---------------------------------------------------------------------
// VirtualInputKey
// ---------------------------------------------------------------------

/// A key that can be part of an input sequence.
/// code layout: digits 0-9 -> 10-19, letters a-z -> 20-45, '\'' -> 47, '`' -> 48.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct VirtualInputKey {
        pub character: char,
        pub text: &'static str,
        pub code: i64,
        pub key_code: u32,
}

/// `stringify!('n')` produces "'n'" — strip the quotes to get the plain "n".
const fn vik_text(lit: &'static str) -> &'static str {
        let bytes = lit.as_bytes();
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(bytes.as_ptr().add(1), bytes.len() - 2)) }
}

macro_rules! vik {
        ($c:literal, $code:expr, $key:expr) => {
                VirtualInputKey {
                        character: $c,
                        text: vik_text(stringify!($c)),
                        code: $code,
                        key_code: $key,
                }
        };
}

pub const VK_0: u32 = 0x30;
pub const VK_OEM_3: u32 = 0xC0;
pub const VK_OEM_7: u32 = 0xDE;

pub static DIGIT_SET: [VirtualInputKey; 10] = [
        vik!('0', 10, 0x30),
        vik!('1', 11, 0x31),
        vik!('2', 12, 0x32),
        vik!('3', 13, 0x33),
        vik!('4', 14, 0x34),
        vik!('5', 15, 0x35),
        vik!('6', 16, 0x36),
        vik!('7', 17, 0x37),
        vik!('8', 18, 0x38),
        vik!('9', 19, 0x39),
];

pub static ALPHABET_SET: [VirtualInputKey; 26] = [
        vik!('a', 20, 0x41),
        vik!('b', 21, 0x42),
        vik!('c', 22, 0x43),
        vik!('d', 23, 0x44),
        vik!('e', 24, 0x45),
        vik!('f', 25, 0x46),
        vik!('g', 26, 0x47),
        vik!('h', 27, 0x48),
        vik!('i', 28, 0x49),
        vik!('j', 29, 0x4A),
        vik!('k', 30, 0x4B),
        vik!('l', 31, 0x4C),
        vik!('m', 32, 0x4D),
        vik!('n', 33, 0x4E),
        vik!('o', 34, 0x4F),
        vik!('p', 35, 0x50),
        vik!('q', 36, 0x51),
        vik!('r', 37, 0x52),
        vik!('s', 38, 0x53),
        vik!('t', 39, 0x54),
        vik!('u', 40, 0x55),
        vik!('v', 41, 0x56),
        vik!('w', 42, 0x57),
        vik!('x', 43, 0x58),
        vik!('y', 44, 0x59),
        vik!('z', 45, 0x5A),
];

pub const APOSTROPHE_KEY: VirtualInputKey = VirtualInputKey {
        character: '\'',
        text: "'",
        code: 47,
        key_code: VK_OEM_7,
};
pub const GRAVE_KEY: VirtualInputKey = VirtualInputKey {
        character: '`',
        text: "`",
        code: 48,
        key_code: VK_OEM_3,
};

pub const LETTER_A: VirtualInputKey = ALPHABET_SET[0];
pub const LETTER_J: VirtualInputKey = ALPHABET_SET[9];
pub const LETTER_M: VirtualInputKey = ALPHABET_SET[12];
pub const LETTER_O: VirtualInputKey = ALPHABET_SET[14];
pub const LETTER_Q: VirtualInputKey = ALPHABET_SET[16];
pub const LETTER_R: VirtualInputKey = ALPHABET_SET[17];
pub const LETTER_V: VirtualInputKey = ALPHABET_SET[21];
pub const LETTER_X: VirtualInputKey = ALPHABET_SET[23];
pub const LETTER_Y: VirtualInputKey = ALPHABET_SET[24];

impl VirtualInputKey {
        pub fn is_number(&self) -> bool {
                DIGIT_SET.contains(self)
        }
        pub fn is_tone_number(&self) -> bool {
                (11..=16).contains(&self.code)
        }
        pub fn is_letter(&self) -> bool {
                ALPHABET_SET.contains(self)
        }
        pub fn is_tone_letter(&self) -> bool {
                *self == LETTER_V || *self == LETTER_X || *self == LETTER_Q
        }
        pub fn is_syllable_letter(&self) -> bool {
                self.is_letter() && !self.is_tone_letter()
        }
        pub fn is_tone_input_key(&self) -> bool {
                self.is_tone_letter() || self.is_tone_number()
        }
        pub fn is_reverse_lookup_trigger(&self) -> bool {
                *self == GRAVE_KEY || *self == LETTER_V || *self == LETTER_X || *self == LETTER_Q
        }
        pub fn is_y(&self) -> bool {
                *self == LETTER_Y
        }
        pub fn is_m(&self) -> bool {
                *self == LETTER_M
        }
        pub fn is_apostrophe(&self) -> bool {
                self.code == 47
        }
        pub fn is_grave(&self) -> bool {
                self.code == 48
        }
        pub fn digit(&self) -> i64 {
                if self.is_number() { self.code - 10 } else { -1 }
        }

        pub fn for_key_code(key_code: u32) -> Option<VirtualInputKey> {
                if key_code == VK_OEM_7 {
                        return Some(APOSTROPHE_KEY);
                }
                if key_code == VK_OEM_3 {
                        return Some(GRAVE_KEY);
                }
                for key in ALPHABET_SET.iter().chain(DIGIT_SET.iter()) {
                        if key.key_code == key_code {
                                return Some(*key);
                        }
                }
                None
        }

        pub fn for_code(code: i64) -> Option<VirtualInputKey> {
                if code == 47 {
                        return Some(APOSTROPHE_KEY);
                }
                if code == 48 {
                        return Some(GRAVE_KEY);
                }
                for key in ALPHABET_SET.iter().chain(DIGIT_SET.iter()) {
                        if key.code == code {
                                return Some(*key);
                        }
                }
                None
        }

        /// Whether a virtual-key code maps to an alphabet letter key.
        pub fn is_matched_letter(key_code: u32) -> bool {
                (0x41..=0x5A).contains(&key_code)
        }

        pub fn for_character(character: char) -> Option<VirtualInputKey> {
                let lower = character.to_ascii_lowercase();
                for key in ALPHABET_SET
                        .iter()
                        .chain(DIGIT_SET.iter())
                        .chain([APOSTROPHE_KEY, GRAVE_KEY].iter())
                {
                        if key.character == lower {
                                return Some(*key);
                        }
                }
                None
        }
}

// ---------------------------------------------------------------------
// Coding helpers (base-100 serial codes, wrapping like the original i64)
// ---------------------------------------------------------------------

pub fn radix100_combined(codes: &[i64]) -> i64 {
        codes.iter().fold(0i64, |value, code| value.wrapping_mul(100).wrapping_add(*code))
}

pub fn combined_code(keys: &[VirtualInputKey]) -> i64 {
        keys.iter().fold(0i64, |value, key| value.wrapping_mul(100).wrapping_add(key.code))
}

/// Same as combined_code, but y is encoded as j (Cantonese anchors normalize y->j).
pub fn anchors_code(keys: &[VirtualInputKey]) -> i64 {
        keys.iter().fold(0i64, |value, key| {
                let key = if key.is_y() { &LETTER_J } else { key };
                value.wrapping_mul(100).wrapping_add(key.code)
        })
}

pub fn char_code_from_text(text: &str) -> Option<i64> {
        let mut result = 0i64;
        for c in text.chars() {
                let code = if c.is_ascii_lowercase() {
                        i64::from(u32::from(c) - u32::from('a') + 20)
                } else {
                        return None;
                };
                result = result.wrapping_mul(100).wrapping_add(code);
        }
        Some(result)
}

pub fn anchors_code_from_text(text: &str) -> Option<i64> {
        let mut result = 0i64;
        for c in text.chars() {
                let lower = if c == 'y' { 'j' } else { c };
                if !lower.is_ascii_lowercase() {
                        return None;
                }
                let code = i64::from(u32::from(lower) - u32::from('a') + 20);
                result = result.wrapping_mul(100).wrapping_add(code);
        }
        Some(result)
}

/// The letter code used by the dictionary generator (a-z -> 20..45).
pub fn serial_code(text: &str) -> i64 {
        text.chars()
                .filter_map(|c| c.is_ascii_lowercase().then(|| i64::from(u32::from(c) - u32::from('a') + 20)))
                .fold(0i64, |value, code| value.wrapping_mul(100).wrapping_add(code))
}

pub fn decimal_code(values: impl IntoIterator<Item = i64>) -> i64 {
        values.into_iter().fold(0i64, |value, digit| value.wrapping_mul(10).wrapping_add(digit))
}

pub fn input_keys_from_code(mut code: i64) -> Vec<VirtualInputKey> {
        let mut keys = Vec::new();
        while code > 0 {
                let part = code % 100;
                code /= 100;
                if let Some(key) = VirtualInputKey::for_code(part) {
                        keys.push(key);
                }
        }
        keys.reverse();
        keys
}

pub fn input_keys_from_text(text: &str) -> Vec<VirtualInputKey> {
        text.chars().filter_map(VirtualInputKey::for_character).collect()
}

pub fn text_from_keys(keys: &[VirtualInputKey]) -> String {
        keys.iter().map(|key| key.text).collect()
}

pub fn syllable_keys(keys: &[VirtualInputKey]) -> Vec<VirtualInputKey> {
        keys.iter().copied().filter(|key| key.is_syllable_letter()).collect()
}

pub fn letter_keys(keys: &[VirtualInputKey]) -> Vec<VirtualInputKey> {
        keys.iter().copied().filter(|key| key.is_letter()).collect()
}

// ---------------------------------------------------------------------
// Text helpers on romanization strings
// ---------------------------------------------------------------------

pub fn is_lowercase_basic_latin_letter(c: char) -> bool {
        c.is_ascii_lowercase()
}
pub fn is_uppercase_basic_latin_letter(c: char) -> bool {
        c.is_ascii_uppercase()
}
pub fn is_basic_latin_letter(c: char) -> bool {
        c.is_ascii_alphabetic()
}
pub fn is_cantonese_tone_digit(c: char) -> bool {
        matches!(c, '1'..='6')
}

/// Convert tone letters to digits: v->1, x->2, q->3, vv->4, xx->5, qq->6.
pub fn tone_converted(text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut result = String::new();
        let mut i = 0;
        while i < chars.len() {
                let c = chars[i];
                match c {
                        'v' | 'x' | 'q' => {
                                let single = match c {
                                        'v' => '1',
                                        'x' => '2',
                                        _ => '3',
                                };
                                if i + 1 < chars.len() && chars[i + 1] == c {
                                        let doubled = match c {
                                                'v' => '4',
                                                'x' => '5',
                                                _ => '6',
                                        };
                                        result.push(doubled);
                                        i += 2;
                                } else {
                                        result.push(single);
                                        i += 1;
                                }
                        }
                        _ => {
                                result.push(c);
                                i += 1;
                        }
                }
        }
        result
}

/// Insert a space after every non-Latin-letter character.
pub fn mark_formatted(text: &str) -> String {
        let mut result = String::new();
        for c in text.chars() {
                result.push(c);
                if !is_basic_latin_letter(c) {
                        result.push(' ');
                }
        }
        result
}

pub fn stripped_tones(text: &str) -> String {
        text.chars().filter(|c| !is_cantonese_tone_digit(*c)).collect()
}
pub fn stripped_spaces(text: &str) -> String {
        text.chars().filter(|c| *c != ' ').collect()
}
pub fn tone_digit_only(text: &str) -> String {
        text.chars().filter(|c| is_cantonese_tone_digit(*c)).collect()
}
pub fn latin_letter_only(text: &str) -> String {
        text.chars().filter(|c| is_basic_latin_letter(*c)).collect()
}

/// Decode a '.'-separated hex code-point list like "1F600.1F3FB" into text.
pub fn symbol_text_from_code_points(code_points: &str) -> Option<String> {
        if code_points.is_empty() || code_points.starts_with('.') || code_points.ends_with('.') {
                return None;
        }
        let mut result = String::new();
        for part in code_points.split('.') {
                if part.is_empty() {
                        return None;
                }
                let value = u32::from_str_radix(part, 16).ok()?;
                if value > 0x10FFFF || (0xD800..=0xDFFF).contains(&value) {
                        return None;
                }
                result.push(char::from_u32(value)?);
        }
        Some(result)
}

// ---------------------------------------------------------------------
// BasicInputEvent
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyboardCase {
        Lowercased = 1,
        Uppercased = 2,
        CapsLocked = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BasicInputEvent {
        pub key: VirtualInputKey,
        pub key_case: KeyboardCase,
}

impl BasicInputEvent {
        pub fn new(key: VirtualInputKey, key_case: KeyboardCase) -> Self {
                Self { key, key_case }
        }
        pub fn from_capitalized(key: VirtualInputKey, is_capitalized: bool) -> Self {
                Self {
                        key,
                        key_case: if is_capitalized { KeyboardCase::Uppercased } else { KeyboardCase::Lowercased },
                }
        }
        pub fn is_capitalized(&self) -> bool {
                self.key_case != KeyboardCase::Lowercased
        }
}

pub fn text_from_events(events: &[BasicInputEvent]) -> String {
        events
                .iter()
                .map(|event| {
                        if event.is_capitalized() {
                                event.key.character.to_ascii_uppercase().to_string()
                        } else {
                                event.key.text.to_string()
                        }
                })
                .collect()
}

/// Preview mark normalization (reading string shown in the composition).
pub fn preview_mark_normalized(events: &[BasicInputEvent]) -> String {
        let mut result = String::new();
        let count = events.len();
        let mut i = 0;
        while i < count {
                let event = events[i];
                let has_more = i + 1 < count;
                if event.key.is_tone_letter() {
                        // v/x/q single -> 1/2/3, doubled -> 4/5/6
                        if has_more && events[i + 1].key == event.key {
                                let digit = match event.key.character {
                                        'v' => '4',
                                        'x' => '5',
                                        _ => '6',
                                };
                                result.push(digit);
                                i += 2;
                        } else {
                                let digit = match event.key.character {
                                        'v' => '1',
                                        'x' => '2',
                                        _ => '3',
                                };
                                result.push(digit);
                                i += 1;
                        }
                        if i < count {
                                result.push(' ');
                        }
                        continue;
                }
                if event.key.is_letter() {
                        if event.is_capitalized() {
                                result.push(event.key.character.to_ascii_uppercase());
                        } else {
                                result.push(event.key.character);
                        }
                        i += 1;
                        continue;
                }
                // non-letters emitted as-is
                result.push(event.key.character);
                i += 1;
                if i < count {
                        result.push(' ');
                }
        }
        result
}

// ---------------------------------------------------------------------
// Syllable / Scheme / Segmentation
// ---------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Syllable {
        pub alias_code: i64,
        pub origin_code: i64,
        pub alias: Vec<VirtualInputKey>,
        pub origin: Vec<VirtualInputKey>,
}

impl Syllable {
        pub fn new(alias_code: i64, origin_code: i64) -> Self {
                Self {
                        alias: input_keys_from_code(alias_code),
                        origin: input_keys_from_code(origin_code),
                        alias_code,
                        origin_code,
                }
        }
        pub fn alias_text(&self) -> String {
                text_from_keys(&self.alias)
        }
        pub fn origin_text(&self) -> String {
                text_from_keys(&self.origin)
        }
}



pub type Scheme = Vec<Syllable>;
pub type Segmentation = Vec<Scheme>;

pub fn scheme_length(scheme: &[Syllable]) -> usize {
        scheme.iter().map(|s| s.alias.len()).sum()
}

pub fn scheme_complexity(scheme: &[Syllable]) -> i64 {
        scheme.iter().fold(0i64, |value, s| value.wrapping_mul(10).wrapping_add(s.origin.len() as i64))
}

pub fn scheme_origin_keys(scheme: &[Syllable]) -> Vec<VirtualInputKey> {
        scheme.iter().flat_map(|s| s.origin.iter().copied()).collect()
}

pub fn scheme_alias_text(scheme: &[Syllable]) -> String {
        scheme.iter().map(|s| s.alias_text()).collect()
}

pub fn scheme_origin_text(scheme: &[Syllable]) -> String {
        scheme.iter().map(|s| s.origin_text()).collect()
}

pub fn scheme_alias_anchors(scheme: &[Syllable]) -> Vec<VirtualInputKey> {
        scheme.iter().filter_map(|s| s.alias.first().copied()).collect()
}

pub fn scheme_origin_anchors(scheme: &[Syllable]) -> Vec<VirtualInputKey> {
        scheme.iter().filter_map(|s| s.origin.first().copied()).collect()
}

pub fn scheme_alias_anchors_text(scheme: &[Syllable]) -> String {
        text_from_keys(&scheme_alias_anchors(scheme))
}

pub fn scheme_origin_anchors_text(scheme: &[Syllable]) -> String {
        text_from_keys(&scheme_origin_anchors(scheme))
}

/// Alias text of each syllable joined by spaces — the user-facing mark.
pub fn scheme_mark(scheme: &[Syllable]) -> String {
        scheme
                .iter()
                .map(|s| s.alias_text())
                .collect::<Vec<_>>()
                .join(" ")
}

/// Origin text of each syllable joined by spaces.
pub fn scheme_syllable_text(scheme: &[Syllable]) -> String {
        scheme
                .iter()
                .map(|s| s.origin_text())
                .collect::<Vec<_>>()
                .join(" ")
}

// ---------------------------------------------------------------------
// Lexicon / Candidate
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LexiconType {
        Cantonese,
        Text,
        Emoji,
        Symbol,
        Composed,
}

#[derive(Clone, Debug)]
pub struct Lexicon {
        pub lexicon_type: LexiconType,
        pub text: String,
        pub romanization: String,
        pub input: String,
        pub input_count: usize,
        pub mark: String,
        pub number: i64,
        pub attached: Option<String>,
}

impl Lexicon {
        pub fn new(
                lexicon_type: LexiconType,
                text: String,
                romanization: String,
                input: String,
                mark: Option<String>,
                number: i64,
                attached: Option<String>,
        ) -> Self {
                let input_count = input.chars().count();
                Self {
                        lexicon_type,
                        text,
                        romanization,
                        input,
                        input_count,
                        mark: mark.unwrap_or_default(),
                        number,
                        attached,
                }
        }

        pub fn cantonese(text: String, romanization: String, input: String, mark: Option<String>, number: i64) -> Self {
                Self::new(LexiconType::Cantonese, text, romanization, input, mark, number, None)
        }

        pub fn plain_text(input: String, text: String) -> Self {
                let mark = text.clone();
                Self::new(LexiconType::Text, text, String::new(), input, Some(mark), 0, None)
        }

        pub fn emoji_or_symbol(text: String, cantonese: String, romanization: String, input: String, is_emoji: bool) -> Self {
                Self::new(
                        if is_emoji { LexiconType::Emoji } else { LexiconType::Symbol },
                        text,
                        romanization,
                        input,
                        Some(cantonese),
                        0,
                        None,
                )
        }

        pub fn is_cantonese(&self) -> bool {
                self.lexicon_type == LexiconType::Cantonese
        }
        pub fn is_not_cantonese(&self) -> bool {
                !self.is_cantonese()
        }
        pub fn is_emoji_or_symbol(&self) -> bool {
                matches!(self.lexicon_type, LexiconType::Emoji | LexiconType::Symbol)
        }
        pub fn is_compound(&self) -> bool {
                self.number > COMPOUND_NUMBER_STEP
        }
        pub fn is_input_memory(&self) -> bool {
                self.number < 0
        }
        pub fn is_ideal_input_memory(&self) -> bool {
                self.number == -1
        }
        pub fn is_not_ideal_input_memory(&self) -> bool {
                self.number == -2
        }

        pub fn replaced_input(&self, new_input: String) -> Self {
                let mut copy = self.clone();
                copy.input_count = new_input.chars().count();
                copy.input = new_input;
                copy
        }
}

impl PartialEq for Lexicon {
        fn eq(&self, other: &Self) -> bool {
                self.lexicon_type == other.lexicon_type
                        && self.text == other.text
                        && self.romanization == other.romanization
                        && self.input == other.input
                        && self.mark == other.mark
                        && self.number == other.number
                        && self.attached == other.attached
        }
}
impl Eq for Lexicon {}

/// inputCount descending, then number ascending (matches C++ operator<).
impl PartialOrd for Lexicon {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
        }
}
impl Ord for Lexicon {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other.input_count.cmp(&self.input_count).then(self.number.cmp(&other.number))
        }
}

pub fn concatenate(left: &Lexicon, right: &Lexicon) -> Option<Lexicon> {
        let mut result = left.clone();
        result.text.push_str(&right.text);
        if !left.romanization.is_empty() || !right.romanization.is_empty() {
                result.romanization = format!("{} {}", left.romanization, right.romanization).trim().to_string();
        }
        result.input.push_str(&right.input);
        result.input_count = result.input.chars().count();
        if !left.mark.is_empty() || !right.mark.is_empty() {
                result.mark = format!("{} {}", left.mark, right.mark).trim().to_string();
        }
        result.number = (left.number + COMPOUND_NUMBER_STEP).wrapping_add(right.number + COMPOUND_NUMBER_STEP);
        Some(result)
}

pub fn join_lexicons(lexicons: &[Lexicon]) -> Option<Lexicon> {
        let mut iter = lexicons.iter();
        let mut result = iter.next()?.clone();
        for item in iter {
                result = concatenate(&result, item)?;
        }
        Some(result)
}

// ---------------------------------------------------------------------
// Candidate
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RomanizationForm {
        Full,
        Toneless,
        Nothing,
}

#[derive(Clone, Debug)]
pub struct Candidate {
        pub text: String,
        pub comment: Option<String>,
        pub secondary_comment: Option<String>,
        pub lexicon: Lexicon,
}

impl Candidate {
        pub fn new(lexicon: Lexicon, display_text: Option<String>, romanization_form: RomanizationForm, composed_comment: Option<String>) -> Self {
                let text = display_text.unwrap_or_else(|| lexicon.text.clone());
                let mut comment = None;
                let mut secondary_comment = None;
                match lexicon.lexicon_type {
                        LexiconType::Cantonese => {
                                comment = match romanization_form {
                                        RomanizationForm::Full => Some(lexicon.romanization.clone()),
                                        RomanizationForm::Toneless => Some(stripped_tones(&lexicon.romanization)),
                                        RomanizationForm::Nothing => None,
                                };
                        }
                        LexiconType::Composed => {
                                comment = composed_comment;
                                if !lexicon.romanization.is_empty() {
                                        secondary_comment = Some(lexicon.romanization.clone());
                                }
                        }
                        _ => {}
                }
                Self {
                        text,
                        comment,
                        secondary_comment,
                        lexicon,
                }
        }

        pub fn is_cantonese(&self) -> bool {
                self.lexicon.is_cantonese()
        }

        pub fn is_not_cantonese(&self) -> bool {
                self.lexicon.is_not_cantonese()
        }
}

impl PartialEq for Candidate {
        fn eq(&self, other: &Self) -> bool {
                if self.is_cantonese() && other.is_cantonese() && self.comment.is_none() && other.comment.is_none() {
                        return self.text == other.text && stripped_tones(&self.lexicon.romanization) == stripped_tones(&other.lexicon.romanization);
                }
                self.text == other.text && self.comment == other.comment
        }
}
impl Eq for Candidate {}

impl fmt::Display for Candidate {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.text)
        }
}

// ---------------------------------------------------------------------
// Reverse lookup method
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReverseLookupMethod {
        None,
        Pinyin,
        Cangjie,
        Stroke,
        Structure,
}

impl ReverseLookupMethod {
        pub fn from_first_key(key: VirtualInputKey) -> Self {
                match key.character {
                        '`' => Self::Pinyin,
                        'v' => Self::Cangjie,
                        'x' => Self::Stroke,
                        'q' => Self::Structure,
                        _ => Self::None,
                }
        }
}
