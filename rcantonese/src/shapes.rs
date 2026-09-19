// Shape-based reverse lookups — port of InputEngineCangjie.cpp / InputEngineStroke.cpp / InputEngineStructure.cpp.
#![allow(dead_code)]

use crate::db::{CangjieVariant, ShapeRow, StructureRow};
use crate::engine::{split, CoreImeEngine};
use crate::types::*;

fn starts_with(text: &str, prefix: &str) -> bool {
        text.len() >= prefix.len() && text.starts_with(prefix)
}
fn ends_with(text: &str, suffix: &str) -> bool {
        text.len() >= suffix.len() && text.ends_with(suffix)
}

// ---------------------------------------------------------------------
// Cangjie / Quick
// ---------------------------------------------------------------------

#[derive(Clone)]
struct ShapeLexicon {
        text: String,
        input: String,
        mark: String,
        complex: i64,
        number: i64,
}

impl PartialEq for ShapeLexicon {
        fn eq(&self, other: &Self) -> bool {
                self.text == other.text
        }
}
impl Eq for ShapeLexicon {}
impl PartialOrd for ShapeLexicon {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
        }
}
impl Ord for ShapeLexicon {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.complex.cmp(&other.complex).then(self.number.cmp(&other.number))
        }
}

fn shape_lexicons_from_rows(rows: Vec<ShapeRow>, input: &str, mark: &str, complex_override: Option<i64>) -> Vec<ShapeLexicon> {
        rows.into_iter()
                .map(|row| ShapeLexicon {
                        text: row.word,
                        input: input.to_string(),
                        mark: mark.to_string(),
                        complex: complex_override.unwrap_or(row.complex),
                        number: row.row_id,
                })
                .collect()
}

fn shape_distinct(items: Vec<ShapeLexicon>) -> Vec<ShapeLexicon> {
        let mut result: Vec<ShapeLexicon> = Vec::with_capacity(items.len());
        for item in items {
                if !result.contains(&item) {
                        result.push(item);
                }
        }
        result
}

fn is_quick_variant(variant: CangjieVariant) -> bool {
        matches!(variant, CangjieVariant::Quick5 | CangjieVariant::Quick3)
}

fn cangjie_root_character(key: VirtualInputKey) -> Option<char> {
        const ROOTS: [char; 26] = [
                '\u{65E5}', '\u{6708}', '\u{91D1}', '\u{6728}', '\u{6C34}', '\u{706B}', '\u{571F}', '\u{7AF9}', '\u{6208}', '\u{5341}',
                '\u{5927}', '\u{4E2D}', '\u{4E00}', '\u{5F13}', '\u{4EBA}', '\u{5FC3}', '\u{624B}', '\u{53E3}', '\u{5C38}', '\u{5EFF}',
                '\u{5C71}', '\u{5973}', '\u{7530}', '\u{96E3}', '\u{535C}', '\u{91CD}',
        ];
        if !key.is_letter() {
                return None;
        }
        let index = (key.character as u32 - 'a' as u32) as usize;
        ROOTS.get(index).copied()
}

fn cangjie_root_mark(keys: &[VirtualInputKey]) -> Option<String> {
        if keys.is_empty() {
                return None;
        }
        keys.iter().map(|key| cangjie_root_character(*key)).collect()
}

impl CoreImeEngine {
        pub fn cangjie_reverse_lookup(&self, keys: &[VirtualInputKey], variant: CangjieVariant) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                let Some(mark) = cangjie_root_mark(keys) else { return Vec::new() };
                let text = text_from_keys(keys);

                let mut shapes = Vec::new();
                if let Some(code) = char_code_from_text(&text) {
                        let mut exact_rows = if is_quick_variant(variant) {
                                self.database.query_quick_by_exact_code(variant, code)
                        } else {
                                self.database.query_cangjie_by_exact_code(variant, code)
                        };
                        if is_quick_variant(variant) {
                                let input_length = text.chars().count() as i64;
                                exact_rows.retain(|row| row.complex == input_length);
                        }
                        shapes.extend(shape_lexicons_from_rows(exact_rows, &text, &mark, Some(text.chars().count() as i64)));
                }

                let prefix_rows = if is_quick_variant(variant) {
                        self.database.query_quick_by_prefix(variant, &text, 100)
                } else {
                        self.database.query_cangjie_by_prefix(variant, &text, 100)
                };
                shapes.extend(shape_lexicons_from_rows(prefix_rows, &text, &mark, None));

                shapes = shape_distinct(shapes);
                shapes.sort();

                let mut result = Vec::new();
                for shape in shapes {
                        result.extend(self.reverse_lookup_word(&shape.text, &shape.input, Some(shape.mark)));
                }
                result
        }
}

// ---------------------------------------------------------------------
// Stroke
// ---------------------------------------------------------------------

#[derive(Clone, Copy)]
struct StrokeKey {
        code: i64,
        mark: char,
        is_wildcard: bool,
}

fn stroke_key_from_input_key(key: VirtualInputKey) -> Option<StrokeKey> {
        Some(match key.character {
                'w' | 'h' | 't' | 'j' | '1' => StrokeKey { code: 1, mark: '\u{2F00}', is_wildcard: false },
                's' | 'k' | '2' => StrokeKey { code: 2, mark: '\u{2F01}', is_wildcard: false },
                'a' | 'p' | 'l' | '3' => StrokeKey { code: 3, mark: '\u{2F03}', is_wildcard: false },
                'd' | 'n' | 'u' | '4' => StrokeKey { code: 4, mark: '\u{2F02}', is_wildcard: false },
                'z' | 'i' | '5' => StrokeKey { code: 5, mark: '\u{4E5B}', is_wildcard: false },
                'x' | 'o' | '6' => StrokeKey { code: 6, mark: '\u{FF0A}', is_wildcard: true },
                _ => return None,
        })
}

impl CoreImeEngine {
        pub fn stroke_reverse_lookup(&self, keys: &[VirtualInputKey]) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                let stroke_keys: Vec<StrokeKey> = keys.iter().filter_map(|k| stroke_key_from_input_key(*k)).collect();

                let input: String = stroke_keys.iter().map(|k| char::from(b'0' + k.code as u8)).collect();
                let mark: String = stroke_keys.iter().map(|k| k.mark).collect();
                let mut pattern = String::new();
                for k in &stroke_keys {
                        if k.is_wildcard {
                                pattern.push_str("[12345]");
                        } else {
                                pattern.push(char::from(b'0' + k.code as u8));
                        }
                }
                let has_wildcard = stroke_keys.iter().any(|k| k.is_wildcard);

                let mut shapes = Vec::new();
                if has_wildcard {
                        shapes.extend(shape_lexicons_from_rows(
                                self.database.query_stroke_by_pattern(&pattern, true, 100),
                                &input,
                                &mark,
                                None,
                        ));
                } else {
                        let code = stroke_keys.iter().fold(0i64, |v, k| v * 10 + k.code);
                        shapes.extend(shape_lexicons_from_rows(
                                self.database.query_stroke_by_code(code, stroke_keys.len() as i64),
                                &input,
                                &mark,
                                Some(stroke_keys.len() as i64),
                        ));
                }

                let prefix_pattern = format!("{}*", pattern);
                shapes.extend(shape_lexicons_from_rows(
                        self.database.query_stroke_by_pattern(&prefix_pattern, false, 100),
                        &input,
                        &mark,
                        None,
                ));

                shapes = shape_distinct(shapes);
                shapes.sort();

                let mut result = Vec::new();
                for shape in shapes {
                        result.extend(self.reverse_lookup_word(&shape.text, &shape.input, Some(shape.mark)));
                }
                result
        }
}

// ---------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------

fn structure_distinct_words(rows: Vec<StructureRow>) -> Vec<StructureRow> {
        let mut result: Vec<StructureRow> = Vec::with_capacity(rows.len());
        for row in rows {
                if !result.iter().any(|item| item.word == row.word) {
                        result.push(row);
                }
        }
        result
}

fn filter_apostrophe_and_tone(rows: Vec<StructureRow>, converted_text: &str) -> Vec<StructureRow> {
        let text_tones = tone_digit_only(converted_text);
        if text_tones.chars().count() != 1 {
                return Vec::new();
        }
        let is_tone_in_tail = converted_text.chars().last().map(|c| is_cantonese_tone_digit(c)).unwrap_or(false);
        rows.into_iter()
                .filter(|row| {
                        let tones = tone_digit_only(&row.romanization);
                        if is_tone_in_tail { ends_with(&tones, &text_tones) } else { starts_with(&tones, &text_tones) }
                })
                .collect()
}

fn filter_tone_only(rows: Vec<StructureRow>, converted_text: &str) -> Vec<StructureRow> {
        let text_tones = tone_digit_only(converted_text);
        match text_tones.chars().count() {
                1 => {
                        let is_tone_in_tail = converted_text.chars().last().map(|c| is_cantonese_tone_digit(c)).unwrap_or(false);
                        rows.into_iter()
                                .filter(|row| {
                                        if starts_with(&stripped_spaces(&row.romanization), converted_text) {
                                                return true;
                                        }
                                        let tones = tone_digit_only(&row.romanization);
                                        if is_tone_in_tail { ends_with(&tones, &text_tones) } else { starts_with(&tones, &text_tones) }
                                })
                                .collect()
                }
                2 => rows
                        .into_iter()
                        .filter(|row| {
                                starts_with(&stripped_spaces(&row.romanization), converted_text)
                                        || text_tones == tone_digit_only(&row.romanization)
                        })
                        .collect(),
                _ => rows
                        .into_iter()
                        .filter(|row| starts_with(&stripped_spaces(&row.romanization), converted_text))
                        .collect(),
        }
}

fn filter_apostrophe_only(rows: Vec<StructureRow>, input_text: &str) -> Vec<StructureRow> {
        let text_parts = split(input_text, '\'');
        rows.into_iter()
                .filter(|row| split(&stripped_tones(&row.romanization), ' ') == text_parts)
                .collect()
}

fn filter_structure_rows(keys: &[VirtualInputKey], input_text: &str, rows: Vec<StructureRow>) -> Vec<StructureRow> {
        let has_apostrophe = keys.iter().any(|k| k.is_apostrophe());
        let has_tone_letter = keys.iter().any(|k| k.is_tone_letter());
        if has_apostrophe && has_tone_letter {
                return filter_apostrophe_and_tone(rows, &tone_converted(input_text));
        }
        if has_tone_letter {
                return filter_tone_only(rows, &tone_converted(input_text));
        }
        if has_apostrophe {
                return filter_apostrophe_only(rows, input_text);
        }
        rows
}

fn structure_mark(keys: &[VirtualInputKey], input_text: &str, segmentation: &Segmentation) -> String {
        if keys.iter().any(|k| !k.is_syllable_letter()) {
                return mark_formatted(&tone_converted(input_text));
        }
        let Some(best_scheme) = segmentation.first() else { return input_text.to_string() };
        let leading_length = scheme_length(best_scheme);
        if leading_length < keys.len() {
                return format!("{} {}", scheme_mark(best_scheme), input_text.chars().skip(leading_length).collect::<String>());
        }
        scheme_mark(best_scheme)
}

impl CoreImeEngine {
        pub fn structure_reverse_lookup(&self, keys: &[VirtualInputKey]) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                let input_text = text_from_keys(keys);
                let syllable_keys = syllable_keys(keys);
                let segmentation = self.segmenter.segment(keys);

                let mut rows: Vec<StructureRow> = Vec::new();
                for scheme in &segmentation {
                        if scheme_length(scheme) == syllable_keys.len() {
                                rows.extend(self.database.query_structure_by_spell(
                                        combined_code(&scheme_origin_keys(scheme)),
                                        scheme_complexity(scheme),
                                        -1,
                                ));
                        }
                }
                if rows.is_empty() {
                        return Vec::new();
                }
                let rows = structure_distinct_words(filter_structure_rows(keys, &input_text, rows));
                if rows.is_empty() {
                        return Vec::new();
                }
                let mark = structure_mark(keys, &input_text, &segmentation);
                let mut result = Vec::new();
                for row in rows {
                        result.extend(self.reverse_lookup_word(&row.word, &input_text, Some(mark.clone())));
                }
                result
        }
}
