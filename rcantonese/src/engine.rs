// CoreImeEngine — port of InputEngine.cpp / InputEngineFacade.cpp.
#![allow(dead_code)]

use crate::db::{CangjieVariant, ImeDatabase, LexiconRow, SymbolRow};
use crate::extra::EXTRA_ENTRIES;
use crate::segmenter::Segmenter;
use crate::types::*;
use crate::variants::{standard_for_variant, CharacterStandard, CharacterVariant};
use crate::globals;

fn query_limit(limit: Option<i32>, default_limit: i32) -> i32 {
        limit.unwrap_or(default_limit)
}

fn starts_with(text: &str, prefix: &str) -> bool {
        text.len() >= prefix.len() && text.starts_with(prefix)
}

fn ends_with(text: &str, suffix: &str) -> bool {
        text.len() >= suffix.len() && text.ends_with(suffix)
}

fn prefix_keys(keys: &[VirtualInputKey], count: usize) -> Vec<VirtualInputKey> {
        keys[..count.min(keys.len())].to_vec()
}

fn drop_first(keys: &[VirtualInputKey], count: usize) -> Vec<VirtualInputKey> {
        keys[count.min(keys.len())..].to_vec()
}

fn drop_last(keys: &[VirtualInputKey], count: usize) -> Vec<VirtualInputKey> {
        let length = keys.len() - count.min(keys.len());
        keys[..length].to_vec()
}

fn prefix_scheme(scheme: &Scheme, count: usize) -> Scheme {
        scheme[..count.min(scheme.len())].to_vec()
}

/// Split on a char, dropping empty parts (matches the C++ Split).
pub fn split(text: &str, separator: char) -> Vec<String> {
        text.split(separator).filter(|p| !p.is_empty()).map(|p| p.to_string()).collect()
}

fn split_keys(keys: &[VirtualInputKey], separator: VirtualInputKey) -> Vec<Vec<VirtualInputKey>> {
        let mut parts: Vec<Vec<VirtualInputKey>> = Vec::new();
        let mut part = Vec::new();
        for key in keys {
                if *key == separator {
                        if !part.is_empty() {
                                parts.push(std::mem::take(&mut part));
                        }
                        continue;
                }
                part.push(*key);
        }
        if !part.is_empty() {
                parts.push(part);
        }
        parts
}

fn last_tone_free_syllable(romanization: &str) -> Option<String> {
        let syllables = split(romanization, ' ');
        syllables.last().map(|s| stripped_tones(s))
}

fn tail_anchor_text(keys: &[VirtualInputKey]) -> String {
        keys.iter()
                .map(|key| if key.is_y() { LETTER_J.character } else { key.character })
                .collect()
}

fn suffix_anchor_text(romanization: &str, prefix_len_chars: usize) -> String {
        if prefix_len_chars > romanization.chars().count() {
                return String::new();
        }
        let suffix: String = romanization.chars().skip(prefix_len_chars).collect();
        split(&suffix, ' ')
                .iter()
                .filter_map(|s| s.chars().next())
                .collect()
}

pub fn distinct(items: Vec<Lexicon>) -> Vec<Lexicon> {
        let mut result: Vec<Lexicon> = Vec::with_capacity(items.len());
        for item in items {
                if !result.contains(&item) {
                        result.push(item);
                }
        }
        result
}

fn sorted(items: &mut Vec<Lexicon>) -> Vec<Lexicon> {
        items.sort();
        items.clone()
}

fn sorted_by_number(items: &mut Vec<Lexicon>) -> Vec<Lexicon> {
        items.sort_by_key(|i| i.number);
        items.clone()
}

fn first(items: Vec<Lexicon>, count: usize) -> Vec<Lexicon> {
        items.into_iter().take(count).collect()
}

fn should_map_emoji_skin_tone(category: i32) -> bool {
        category == 1 || category == 4
}

fn symbol_lexicons_from_rows(database: &ImeDatabase, rows: Vec<SymbolRow>, input: &str) -> Vec<Lexicon> {
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
                let mut code_point = row.code_point.clone();
                if should_map_emoji_skin_tone(row.category) {
                        if let Some(target) = database.query_emoji_skin_target(&code_point) {
                                code_point = target;
                        }
                }
                if let Some(symbol_text) = symbol_text_from_code_points(&code_point) {
                        result.push(Lexicon::emoji_or_symbol(
                                symbol_text,
                                row.cantonese.clone(),
                                row.romanization.clone(),
                                input.to_string(),
                                row.category < 10,
                        ));
                }
        }
        result
}

fn lexicons_from_rows(rows: Vec<LexiconRow>, input: &str, mark: Option<String>) -> Vec<Lexicon> {
        rows.into_iter()
                .map(|row| {
                        let mark = mark.clone().unwrap_or_else(|| stripped_tones(&row.romanization));
                        Lexicon::cantonese(row.word, row.romanization, input.to_string(), Some(mark), row.row_id)
                })
                .collect()
}

fn first_alias_count(segmentation: &Segmentation) -> usize {
        segmentation
                .first()
                .and_then(|scheme| scheme.first())
                .map(|s| s.alias.len())
                .unwrap_or(0)
}

fn contains_input_count(items: &[Lexicon], input_count: usize) -> bool {
        items.iter().any(|item| item.input_count == input_count)
}

fn contains_scheme_length(segmentation: &Segmentation, length: usize) -> bool {
        segmentation.iter().any(|scheme| scheme_length(scheme) == length)
}

fn distinct_input_counts(items: &[Lexicon]) -> Vec<usize> {
        let mut result = Vec::new();
        for item in items {
                if !result.contains(&item.input_count) {
                        result.push(item.input_count);
                }
        }
        result
}

fn count_apostrophes(keys: &[VirtualInputKey]) -> usize {
        keys.iter().filter(|k| k.is_apostrophe()).count()
}

fn contains_apostrophe(keys: &[VirtualInputKey]) -> bool {
        keys.iter().any(|k| k.is_apostrophe())
}

fn contains_tone_input_key(keys: &[VirtualInputKey]) -> bool {
        keys.iter().any(|k| k.is_tone_input_key())
}

fn non_syllable_input_text(keys: &[VirtualInputKey]) -> String {
        keys.iter().filter(|k| !k.is_syllable_letter()).map(|k| k.text).collect()
}

fn leading_syllable_and_tone_input_text(keys: &[VirtualInputKey]) -> String {
        let mut leading: Vec<VirtualInputKey> = Vec::new();
        for key in keys {
                if key.is_syllable_letter() {
                        leading.push(*key);
                } else {
                        break;
                }
        }
        for key in &keys[leading.len()..] {
                if !key.is_syllable_letter() {
                        leading.push(*key);
                } else {
                        break;
                }
        }
        text_from_keys(&leading)
}

fn find_with_input_count(items: &[Lexicon], input_count: usize) -> Option<&Lexicon> {
        items.iter().find(|item| item.input_count == input_count)
}

// ---------------------------------------------------------------------
// ExtraEntry
// ---------------------------------------------------------------------

fn extra_entry_search(keys: &[VirtualInputKey]) -> Vec<Lexicon> {
        let spell = combined_code(keys);
        let complex = keys.len();
        let input = text_from_keys(keys);
        EXTRA_ENTRIES
                .iter()
                .filter(|(_, _, c, s)| *c == complex && *s == spell)
                .map(|(word, romanization, _, _)| {
                        Lexicon::cantonese(
                                word.to_string(),
                                romanization.to_string(),
                                input.clone(),
                                Some(stripped_tones(romanization)),
                                0,
                        )
                })
                .collect()
}

// ---------------------------------------------------------------------
// CoreImeEngine
// ---------------------------------------------------------------------

pub struct Emoji {
        pub category: i32,
        pub unique_number: i64,
        pub unicode_version: i32,
        pub text: String,
        pub cantonese: String,
        pub romanization: String,
}

pub struct CoreImeEngine {
        pub database: ImeDatabase,
        pub segmenter: Segmenter,
        pub pinyin_syllables: std::collections::HashMap<i64, PinyinSyllable>,
}

#[derive(Clone)]
pub struct PinyinSyllable {
        pub code: i64,
        pub keys: Vec<VirtualInputKey>,
        pub text: String,
}

impl PartialEq for PinyinSyllable {
        fn eq(&self, other: &Self) -> bool {
                self.code == other.code
        }
}
impl Eq for PinyinSyllable {}

pub type PinyinScheme = Vec<PinyinSyllable>;
pub type PinyinSegmentation = Vec<PinyinScheme>;

pub fn pinyin_scheme_length(scheme: &PinyinScheme) -> usize {
        scheme.iter().map(|s| s.keys.len()).sum()
}
pub fn pinyin_scheme_complexity(scheme: &PinyinScheme) -> i64 {
        scheme.iter().fold(0i64, |v, s| v * 10 + s.keys.len() as i64)
}
pub fn pinyin_scheme_keys(scheme: &PinyinScheme) -> Vec<VirtualInputKey> {
        scheme.iter().flat_map(|s| s.keys.iter().copied()).collect()
}
pub fn pinyin_scheme_text(scheme: &PinyinScheme) -> String {
        scheme.iter().map(|s| s.text.as_str()).collect()
}
pub fn pinyin_scheme_mark(scheme: &PinyinScheme) -> String {
        scheme.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
}

impl CoreImeEngine {
        pub fn prepare() -> Option<Self> {
                globals::log("engine::prepare before open_default");
                let database = ImeDatabase::open_default()?;
                globals::log("engine::prepare after open_default");
                if cfg!(debug_assertions) && std::env::var("RCANTONESE_SKIP_ENGINE").is_ok() {
                        globals::log("engine::prepare skipped (RCANTONESE_SKIP_ENGINE)");
                        return None;
                }
                Self::prepare_with(database)
        }

        pub fn prepare_path(path: &std::path::Path) -> Option<Self> {
                let database = ImeDatabase::open(path)?;
                Self::prepare_with(database)
        }

        fn prepare_with(database: ImeDatabase) -> Option<Self> {
                let mut segmenter = Segmenter::new();
                globals::log("engine::prepare_with before segmenter");
                if !segmenter.prepare(&database) {
                        globals::log_error("R-Cantonese segmenter prepare failed");
                        return None;
                }
                globals::log("engine::prepare_with after segmenter");
                let mut pinyin_syllables = std::collections::HashMap::new();
                for row in database.query_pinyin_syllables() {
                        pinyin_syllables.insert(
                                row.code,
                                PinyinSyllable {
                                        code: row.code,
                                        keys: input_keys_from_code(row.code),
                                        text: row.syllable,
                                },
                        );
                }
                globals::log("engine::prepare_with done");
                Some(Self {
                        database,
                        segmenter,
                        pinyin_syllables,
                })
        }

        pub fn is_prepared(&self) -> bool {
                self.segmenter.is_prepared()
        }

        pub fn convert_text(&self, text: &str, standard: CharacterStandard) -> String {
                crate::variants::convert_text(&self.database, text, standard)
        }

        pub fn convert_text_for_variant(&self, text: &str, variant: CharacterVariant) -> String {
                self.convert_text(text, standard_for_variant(variant))
        }

        pub fn segment(&self, keys: &[VirtualInputKey]) -> Segmentation {
                self.segmenter.segment(keys)
        }

        pub fn search_plain_texts(&self, keys: &[VirtualInputKey]) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                let text = text_from_keys(keys);
                self.database
                        .query_plain_texts_by_spell(combined_code(keys), keys.len())
                        .into_iter()
                        .map(|word| Lexicon::plain_text(text.clone(), word))
                        .collect()
        }

        pub fn search_symbols(&self, keys: &[VirtualInputKey], segmentation: &Segmentation) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                let syllable_keys = syllable_keys(keys);
                if syllable_keys.is_empty() {
                        return Vec::new();
                }
                let syllable_length = syllable_keys.len();
                let input = text_from_keys(keys);
                let mut result = Vec::new();
                for scheme in segmentation {
                        if scheme_length(scheme) == syllable_length {
                                let rows = self.database.query_symbols_by_spell(
                                        combined_code(&scheme_origin_keys(scheme)),
                                        scheme_complexity(scheme),
                                );
                                result.extend(symbol_lexicons_from_rows(&self.database, rows, &input));
                        }
                }
                distinct(result)
        }

        pub fn fetch_emoji_sequence(&self, category: Option<i32>) -> Vec<Emoji> {
                self.database
                        .query_emoji_sequence()
                        .into_iter()
                        .filter(|row| category.map(|c| row.category == c).unwrap_or(true))
                        .filter_map(|row| {
                                symbol_text_from_code_points(&row.code_point).map(|text| Emoji {
                                        category: row.category,
                                        unique_number: 10000 + row.row_id,
                                        unicode_version: row.unicode_version,
                                        text,
                                        cantonese: row.cantonese,
                                        romanization: row.romanization,
                                })
                        })
                        .collect()
        }

        pub fn fetch_default_frequent_emojis(&self) -> Vec<Emoji> {
                self.database
                        .query_default_frequent_emojis()
                        .into_iter()
                        .filter_map(|row| {
                                symbol_text_from_code_points(&row.code_point).map(|text| Emoji {
                                        category: 0,
                                        unique_number: 5000 + row.row_id,
                                        unicode_version: row.unicode_version,
                                        text,
                                        cantonese: row.cantonese,
                                        romanization: row.romanization,
                                })
                        })
                        .collect()
        }

        // -- main suggest pipeline ----------------------------------------

        pub fn suggest(&self, keys: &[VirtualInputKey], segmentation: &Segmentation, deep_search: bool) -> Vec<Lexicon> {
                if !self.is_prepared() {
                        return Vec::new();
                }
                match keys.len() {
                        0 => Vec::new(),
                        1 => {
                                let key = keys[0];
                                if key == LETTER_A {
                                        let mut result = self.spell_match(keys, 1, "a".to_string(), Some("a".to_string()), None);
                                        result.extend(self.spell_match(&[LETTER_A, LETTER_A], 2, "a".to_string(), Some("a".to_string()), None));
                                        result.extend(self.anchors_match(keys, Some("a".to_string()), None));
                                        return result;
                                }
                                if key == LETTER_O || key == LETTER_M {
                                        let text = text_from_keys(keys);
                                        let mut result = self.spell_match(keys, 1, text.clone(), Some(text.clone()), None);
                                        result.extend(self.anchors_match(keys, Some(text), None));
                                        return result;
                                }
                                self.anchors_match(keys, None, None)
                        }
                        _ => self.dispatch(keys, segmentation, deep_search),
                }
        }

        fn dispatch(&self, keys: &[VirtualInputKey], segmentation: &Segmentation, deep_search: bool) -> Vec<Lexicon> {
                let syllable_keys = syllable_keys(keys);
                let syllable_text = text_from_keys(&syllable_keys);
                let alias_count = first_alias_count(segmentation);

                let mut lexicons;
                if alias_count == 0 && deep_search {
                        lexicons = extra_entry_search(keys);
                        lexicons.extend(self.process_slices(&syllable_keys, &syllable_text, None));
                } else if alias_count == 0 {
                        lexicons = extra_entry_search(keys);
                        lexicons.extend(self.anchors_match(&syllable_keys, Some(syllable_text), None));
                } else if (alias_count == 1 && syllable_keys.len() > 1) || syllable_keys.len() != keys.len() {
                        lexicons = self.search(&syllable_keys, segmentation, None, deep_search);
                        lexicons.extend(self.process_slices(&syllable_keys, &syllable_text, None));
                } else {
                        lexicons = self.search(&syllable_keys, segmentation, None, deep_search);
                }

                let has_apostrophe = contains_apostrophe(keys);
                let has_tone = contains_tone_input_key(keys);
                if has_apostrophe && has_tone {
                        return self.filter_apostrophe_and_tone_suggestions(keys, lexicons);
                }
                if has_tone {
                        return self.filter_tone_suggestions(keys, lexicons);
                }
                if has_apostrophe {
                        return self.filter_apostrophe_suggestions(keys, lexicons);
                }
                lexicons
        }

        fn search(&self, keys: &[VirtualInputKey], segmentation: &Segmentation, limit: Option<i32>, deep_search: bool) -> Vec<Lexicon> {
                let input_length = keys.len();
                let text = text_from_keys(keys);

                let anchors_matched = self.anchors_match(keys, Some(text.clone()), limit);
                let queried = self.query(input_length, segmentation, limit);

                let mut should_match_prefixes = false;
                if deep_search && input_length > 2 && input_length < 25 {
                        should_match_prefixes = keys.last() == Some(&LETTER_M) || keys.first() == Some(&LETTER_M);
                        if !should_match_prefixes {
                                should_match_prefixes =
                                        !contains_input_count(&queried, input_length) && !contains_scheme_length(segmentation, input_length);
                        }
                }

                let mut prefix_matched: Vec<Lexicon> = Vec::new();
                if should_match_prefixes {
                        let prefixes_limit = if limit.is_some() { 200 } else { 500 };
                        for scheme in segmentation {
                                if scheme.is_empty() || scheme.len() > MAX_CHAR_COUNT {
                                        continue;
                                }
                                let tail = drop_first(keys, scheme_length(scheme));
                                if tail.is_empty() {
                                        continue;
                                }
                                let scheme_anchors = scheme_alias_anchors(scheme);
                                let mut conjoined = scheme_anchors.clone();
                                conjoined.extend_from_slice(&tail);
                                let mut anchors = scheme_anchors;
                                anchors.push(tail[0]);

                                let scheme_syllable_text = scheme_syllable_text(scheme);
                                let mark = format!("{} {}", scheme_mark(scheme), text_from_keys(&tail));
                                let tail_as_anchor_text = tail_anchor_text(&tail);

                                for item in self.anchors_match(&conjoined, None, Some(prefixes_limit)) {
                                        let tone_free = stripped_tones(&item.romanization);
                                        if !starts_with(&tone_free, &scheme_syllable_text) {
                                                continue;
                                        }
                                        if suffix_anchor_text(&tone_free, scheme_syllable_text.chars().count()) == tail_as_anchor_text {
                                                prefix_matched.push(Lexicon::cantonese(
                                                        item.text,
                                                        item.romanization,
                                                        text.clone(),
                                                        Some(mark.clone()),
                                                        item.number,
                                                ));
                                        }
                                }

                                let transformed_tail: String = tail
                                        .iter()
                                        .enumerate()
                                        .map(|(i, k)| if i == 0 && k.is_y() { LETTER_J.character } else { k.character })
                                        .collect();
                                let syllables = format!("{} {}", scheme_syllable_text, transformed_tail);
                                for item in self.anchors_match(&anchors, None, Some(prefixes_limit)) {
                                        if starts_with(&stripped_tones(&item.romanization), &syllables) {
                                                prefix_matched.push(Lexicon::cantonese(
                                                        item.text,
                                                        item.romanization,
                                                        text.clone(),
                                                        Some(mark.clone()),
                                                        item.number,
                                                ));
                                        }
                                }
                        }
                }

                let mut gained_matched: Vec<Lexicon> = Vec::new();
                if should_match_prefixes {
                        for number in (1..input_length).rev() {
                                if number > MAX_CHAR_COUNT {
                                        continue;
                                }
                                let leading_keys = prefix_keys(keys, number);
                                let leading_text = text_from_keys(&leading_keys);
                                for item in self.anchors_match(&leading_keys, Some(leading_text), Some(300)) {
                                        let tail_start = if item.input_count > 0 { item.input_count - 1 } else { 0 };
                                        let tail = drop_first(keys, tail_start);
                                        if tail.len() > 6 {
                                                continue;
                                        }
                                        let converted =
                                                Lexicon::cantonese(item.text.clone(), item.romanization.clone(), text.clone(), Some(text.clone()), item.number);
                                        if starts_with(&latin_letter_only(&item.romanization), &text) {
                                                gained_matched.push(converted);
                                                continue;
                                        }
                                        let Some(last_syllable) = last_tone_free_syllable(&item.romanization) else {
                                                continue;
                                        };
                                        if let Some(tail_syllable) = self.segmenter.syllable_text(&tail) {
                                                if last_syllable == tail_syllable {
                                                        gained_matched.push(converted);
                                                }
                                        } else if starts_with(&last_syllable, &text_from_keys(&tail)) {
                                                gained_matched.push(converted);
                                        }
                                }
                        }
                }

                let mut ideal_queried = Vec::new();
                let mut not_ideal_queried = Vec::new();
                for item in queried {
                        if item.input_count == input_length {
                                ideal_queried.push(item);
                        } else if item.input_count < input_length {
                                not_ideal_queried.push(item);
                        }
                }
                ideal_queried = distinct(sorted_by_number(&mut ideal_queried));
                not_ideal_queried = distinct(sorted(&mut not_ideal_queried));

                let mut full_input = Vec::new();
                full_input.extend(ideal_queried.iter().cloned());
                if ideal_queried.is_empty() {
                        full_input.extend(extra_entry_search(keys));
                }
                full_input.extend(anchors_matched);
                full_input.extend(prefix_matched);
                full_input.extend(gained_matched);
                let full_input = distinct(full_input);

                let mut fetched = Vec::new();
                fetched.extend(first(full_input.clone(), 10));
                fetched.extend(first(sorted(&mut full_input.clone()), 10));
                fetched.extend(first(not_ideal_queried.clone(), 10));
                fetched.extend(first(sorted_by_number(&mut not_ideal_queried.clone()), 10));
                fetched.extend(full_input.iter().cloned());
                fetched.extend(not_ideal_queried.iter().cloned());
                let fetched = distinct(fetched);

                if fetched.is_empty() {
                        return if deep_search {
                                self.process_slices(keys, &text, limit)
                        } else {
                                self.anchors_match(keys, Some(text), limit)
                        };
                }

                let first_input_count = fetched[0].input_count;
                if first_input_count >= input_length {
                        return fetched;
                }
                if !deep_search {
                        return fetched;
                }

                let mut concatenated: Vec<Lexicon> = Vec::new();
                for head_length in distinct_input_counts(&fetched) {
                        let tail_keys = drop_first(keys, head_length);
                        let tail_lexicons = self.search(&tail_keys, &self.segmenter.segment(&tail_keys), Some(50), deep_search);
                        let Some(head_lexicon) = find_with_input_count(&fetched, head_length) else {
                                continue;
                        };
                        if tail_lexicons.is_empty() {
                                continue;
                        }
                        if let Some(lexicon) = concatenate(head_lexicon, &tail_lexicons[0]) {
                                concatenated.push(lexicon);
                        }
                }

                let mut concatenated = first(sorted(&mut distinct(concatenated)), 1);
                concatenated.extend(fetched);
                concatenated
        }

        fn query(&self, input_length: usize, segmentation: &Segmentation, limit: Option<i32>) -> Vec<Lexicon> {
                let ideal_schemes: Vec<&Scheme> = segmentation
                        .iter()
                        .filter(|scheme| scheme_length(scheme) == input_length)
                        .collect();

                let mut result = Vec::new();
                if ideal_schemes.is_empty() {
                        for scheme in segmentation {
                                if scheme.len() <= MAX_CHAR_COUNT {
                                        result.extend(self.perform(scheme, limit));
                                }
                        }
                        return result;
                }

                for scheme in ideal_schemes {
                        if scheme.len() == 1 {
                                result.extend(self.perform(scheme, limit));
                        } else {
                                for count in (1..=scheme.len()).rev() {
                                        if count <= MAX_CHAR_COUNT {
                                                result.extend(self.perform(&prefix_scheme(scheme, count), limit));
                                        }
                                }
                        }
                }
                result
        }

        fn perform(&self, scheme: &Scheme, limit: Option<i32>) -> Vec<Lexicon> {
                self.spell_match(
                        &scheme_origin_keys(scheme),
                        scheme_complexity(scheme),
                        scheme_alias_text(scheme),
                        Some(scheme_mark(scheme)),
                        limit,
                )
        }

        fn process_slices(&self, keys: &[VirtualInputKey], text: &str, limit: Option<i32>) -> Vec<Lexicon> {
                let adjusted_limit = if limit.is_some() { 100 } else { 300 };
                let input_length = keys.len();
                let mut result = Vec::new();

                for leading_length in (1..=input_length).rev() {
                        if leading_length > MAX_CHAR_COUNT {
                                continue;
                        }
                        let leading_keys = prefix_keys(keys, leading_length);
                        let mut anchors_matched: Vec<Lexicon> = self
                                .anchors_match(&leading_keys, None, Some(adjusted_limit))
                                .into_iter()
                                .map(|item| self.modify(item, keys, text, input_length))
                                .collect();
                        anchors_matched = first(sorted(&mut anchors_matched), 50);
                        result.extend(anchors_matched);
                }
                sorted(&mut distinct(result))
        }

        fn modify(&self, item: Lexicon, keys: &[VirtualInputKey], text: &str, input_length: usize) -> Lexicon {
                if input_length <= 1 || item.input_count == input_length {
                        return item;
                }
                let converted = Lexicon::cantonese(item.text.clone(), item.romanization.clone(), text.to_string(), Some(text.to_string()), item.number);
                if starts_with(&latin_letter_only(&item.romanization), text) {
                        return converted;
                }
                let Some(last_syllable) = last_tone_free_syllable(&item.romanization) else {
                        return item;
                };
                let tail_start = if item.input_count > 0 { item.input_count - 1 } else { 0 };
                let tail = drop_first(keys, tail_start);
                if tail.len() > 6 {
                        return item;
                }
                if let Some(tail_syllable) = self.segmenter.syllable_text(&tail) {
                        return if last_syllable == tail_syllable { converted } else { item };
                }
                if starts_with(&last_syllable, &text_from_keys(&tail)) {
                        converted
                } else {
                        item
                }
        }

        pub fn anchors_match(&self, keys: &[VirtualInputKey], input: Option<String>, limit: Option<i32>) -> Vec<Lexicon> {
                if keys.is_empty() || keys.len() > MAX_CHAR_COUNT {
                        return Vec::new();
                }
                let code = anchors_code(keys);
                let text = input.unwrap_or_else(|| text_from_keys(keys));
                let rows = self
                        .database
                        .query_lexicon_by_anchors(code, keys.len(), query_limit(limit, 100));
                lexicons_from_rows(rows, &text, Some(text.clone()))
        }

        pub fn spell_match(&self, keys: &[VirtualInputKey], complexity: i64, input: String, mark: Option<String>, limit: Option<i32>) -> Vec<Lexicon> {
                let rows = self
                        .database
                        .query_lexicon_by_spell(combined_code(keys), complexity, query_limit(limit, -1));
                lexicons_from_rows(rows, &input, mark)
        }

        // -- tone / apostrophe filters --------------------------------------

        fn filter_tone_suggestions(&self, keys: &[VirtualInputKey], lexicons: Vec<Lexicon>) -> Vec<Lexicon> {
                let input_text = text_from_keys(keys);
                let tone_input = non_syllable_input_text(keys);
                let text = tone_converted(&input_text);
                let text_tones = tone_digit_only(&text);
                let text_chars: Vec<char> = text.chars().collect();

                let mut qualified = Vec::new();
                for item in lexicons {
                        let syllable_text = stripped_spaces(&item.romanization);
                        if starts_with(&syllable_text, &text) {
                                qualified.push(item.replaced_input(input_text.clone()));
                                continue;
                        }

                        let tones = tone_digit_only(&syllable_text);
                        if text_tones.chars().count() == 1 && tones.chars().count() == 1 {
                                let is_correct_position = item.input_count < text_chars.len()
                                        && is_cantonese_tone_digit(text_chars[item.input_count]);
                                if text_tones == tones && is_correct_position {
                                        qualified.push(item.replaced_input(format!("{}{}", item.input, tone_input)));
                                }
                                continue;
                        }

                        if text_tones.chars().count() == 1 && tones.chars().count() == 2 {
                                let is_tone_last = text_chars.last().map(|c| is_cantonese_tone_digit(*c)).unwrap_or(false);
                                if is_tone_last {
                                        let has_matching_tone = ends_with(&tones, &text_tones);
                                        let is_correct_position = item.input_count < text_chars.len()
                                                && is_cantonese_tone_digit(text_chars[item.input_count]);
                                        if has_matching_tone && is_correct_position {
                                                qualified.push(item.replaced_input(input_text.clone()));
                                        }
                                } else if starts_with(&tones, &text_tones) {
                                        qualified.push(item.replaced_input(format!("{}{}", item.input, tone_input)));
                                }
                                continue;
                        }

                        if text_tones.chars().count() == 2 && tones.chars().count() == 1 {
                                let is_correct_position = item.input_count < text_chars.len()
                                        && is_cantonese_tone_digit(text_chars[item.input_count]);
                                if starts_with(&text_tones, &tones) && is_correct_position {
                                        qualified.push(item.replaced_input(leading_syllable_and_tone_input_text(keys)));
                                }
                                continue;
                        }

                        if text_tones.chars().count() == 2 && tones.chars().count() == 2 {
                                if text_tones != tones {
                                        continue;
                                }
                                let is_tone_last = text_chars.last().map(|c| is_cantonese_tone_digit(*c)).unwrap_or(false);
                                if is_tone_last {
                                        if item.input_count == text_chars.len() - 2 {
                                                qualified.push(item.replaced_input(input_text.clone()));
                                        }
                                } else {
                                        let tail_start = item.input_count + 1;
                                        let is_correct_position = tail_start < text_chars.len()
                                                && text_tones.chars().last() == Some(text_chars[tail_start]);
                                        if is_correct_position {
                                                qualified.push(item.replaced_input(format!("{}{}", item.input, tone_input)));
                                        }
                                }
                                continue;
                        }

                        if starts_with(&input_text, &syllable_text) {
                                qualified.push(item.replaced_input(syllable_text));
                        }
                }
                let mut qualified = qualified;
                qualified.sort_by(|a, b| b.input_count.cmp(&a.input_count));
                qualified
        }

        fn filter_apostrophe_and_tone_suggestions(&self, keys: &[VirtualInputKey], lexicons: Vec<Lexicon>) -> Vec<Lexicon> {
                let input_text = text_from_keys(keys);
                let text = tone_converted(&input_text);
                lexicons
                        .into_iter()
                        .filter(|item| starts_with(&text, &item.romanization))
                        .map(|item| item.replaced_input(input_text.clone()))
                        .collect()
        }

        fn filter_apostrophe_suggestions(&self, keys: &[VirtualInputKey], lexicons: Vec<Lexicon>) -> Vec<Lexicon> {
                if keys.is_empty() || keys[0].is_apostrophe() {
                        return Vec::new();
                }
                let is_trailing_separator = keys.last().map(|k| k.is_apostrophe()).unwrap_or(false);
                let input_separator_count = count_apostrophes(keys);
                let input_length = keys.len();
                let text = text_from_keys(keys);
                let text_parts = split(&text, '\'');

                let mut qualified = Vec::new();
                for item in &lexicons {
                        let syllables = split(&stripped_tones(&item.romanization), ' ');
                        if syllables == text_parts {
                                qualified.push(item.replaced_input(text.clone()));
                                continue;
                        }

                        if input_separator_count == 1 && is_trailing_separator {
                                if syllables.len() == 1 && item.input_count == input_length - 1 {
                                        qualified.push(item.replaced_input(text.clone()));
                                }
                                continue;
                        }

                        if input_separator_count == 1 {
                                if syllables.len() == 1 {
                                        if !text_parts.is_empty() && item.input_count == text_parts[0].chars().count() {
                                                qualified.push(item.replaced_input(format!("{}'", item.input)));
                                        }
                                } else if syllables.len() == 2 && text_parts.len() >= 2 {
                                        let mut is_matched = true;
                                        if input_length != 3 && syllables[0] != text_parts[0] {
                                                is_matched = text_parts[0].chars().count() == 1
                                                        && !syllables[0].is_empty()
                                                        && text_parts[0].chars().next() == syllables[0].chars().next()
                                                        && starts_with(&text_parts[1], &syllables[1]);
                                        }
                                        if is_matched {
                                                qualified.push(item.replaced_input(format!("{}'", item.input)));
                                        }
                                }
                                continue;
                        }

                        if input_separator_count == 2 && is_trailing_separator {
                                if syllables.len() == 1 {
                                        if !text_parts.is_empty() && item.input_count == text_parts[0].chars().count() {
                                                qualified.push(item.replaced_input(format!("{}'", item.input)));
                                        }
                                } else if syllables.len() == 2 && text_parts.len() >= 2 && item.input_count == input_length - 2 {
                                        let mut is_matched = true;
                                        if input_length != 4 && syllables[0] != text_parts[0] {
                                                is_matched = text_parts[0].chars().count() == 1
                                                        && !syllables[0].is_empty()
                                                        && text_parts[0].chars().next() == syllables[0].chars().next()
                                                        && text_parts[1] == syllables[1];
                                        }
                                        if is_matched {
                                                qualified.push(item.replaced_input(text.clone()));
                                        }
                                }
                                continue;
                        }

                        if ((input_separator_count == 2 && input_length == 5) || (input_separator_count == 3 && input_length == 6))
                                && text_parts.len() == 3
                        {
                                match syllables.len() {
                                        1 => {
                                                if item.input_count == 1 {
                                                        qualified.push(item.replaced_input(format!("{}'", item.input)));
                                                }
                                        }
                                        2 => {
                                                if item.input_count == 2 {
                                                        qualified.push(item.replaced_input(format!("{}''", item.input)));
                                                }
                                        }
                                        3 => qualified.push(item.replaced_input(text.clone())),
                                        _ => {}
                                }
                                continue;
                        }

                        let text_part_count = text_parts.len();
                        let syllable_count = syllables.len();
                        if syllable_count < text_part_count && syllable_count > 0 {
                                let is_matched = (0..syllable_count).all(|i| syllables[i] == text_parts[i]);
                                if is_matched {
                                        qualified.push(item.replaced_input(format!("{}{}", item.input, "i".repeat(syllable_count - 1))));
                                }
                        }
                }

                if !qualified.is_empty() {
                        let mut q = distinct(qualified);
                        q.sort_by(|a, b| b.input_count.cmp(&a.input_count));
                        return q;
                }

                // Fallback: match anchors of apostrophe-separated parts.
                let mut anchor_keys = Vec::new();
                for part in split_keys(keys, APOSTROPHE_KEY) {
                        if let Some(first_key) = part.first() {
                                anchor_keys.push(*first_key);
                        }
                }
                let anchor_count = anchor_keys.len();
                let mut anchors_matched = Vec::new();
                for item in self.anchors_match(&anchor_keys, None, None) {
                        let syllables = split(&item.romanization, ' ');
                        if syllables.len() != anchor_count || text_parts.len() != anchor_count {
                                continue;
                        }
                        let mut is_matched = true;
                        for index in 0..anchor_count {
                                let syllable = stripped_tones(&syllables[index]);
                                let part = &text_parts[index];
                                let is_anchor_only = part.chars().count() == 1;
                                if (is_anchor_only && !starts_with(&syllable, part)) || (!is_anchor_only && syllable != *part) {
                                        is_matched = false;
                                        break;
                                }
                        }
                        if is_matched {
                                anchors_matched.push(item.replaced_input(text.clone()));
                        }
                }
                anchors_matched
        }

        // -- reverse lookup --------------------------------------------------

        pub fn reverse_lookup(&self, method: ReverseLookupMethod, keys: &[VirtualInputKey]) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                match method {
                        ReverseLookupMethod::Pinyin => self.pinyin_reverse_lookup(keys),
                        ReverseLookupMethod::Cangjie => self.cangjie_reverse_lookup(keys, CangjieVariant::Cangjie5),
                        ReverseLookupMethod::Stroke => self.stroke_reverse_lookup(keys),
                        ReverseLookupMethod::Structure => self.structure_reverse_lookup(keys),
                        ReverseLookupMethod::None => Vec::new(),
                }
        }

        pub fn reverse_lookup_word(&self, word: &str, input: &str, mark: Option<String>) -> Vec<Lexicon> {
                if word.is_empty() {
                        return Vec::new();
                }
                let exact = self.database.lookup_romanizations_for_word(word);
                if !exact.is_empty() {
                        return exact
                                .into_iter()
                                .map(|r| Lexicon::cantonese(word.to_string(), r, input.to_string(), mark.clone(), 0))
                                .collect();
                }
                if word.chars().count() <= 1 {
                        return Vec::new();
                }
                // Greedy longest-prefix romanization lookup.
                let chars: Vec<char> = word.chars().collect();
                let mut romanizations = Vec::new();
                let mut index = 0;
                while index < chars.len() {
                        let mut matched: Option<(String, usize)> = None;
                        let mut length = chars.len() - index;
                        while length > 0 {
                                let leading: String = chars[index..index + length].iter().collect();
                                let found = self.database.lookup_romanizations_for_word(&leading);
                                if !found.is_empty() {
                                        matched = Some((found[0].clone(), length));
                                        break;
                                }
                                length -= 1;
                        }
                        let Some((romanization, matched_len)) = matched else {
                                romanizations.clear();
                                break;
                        };
                        romanizations.push(romanization);
                        index += matched_len;
                }
                if romanizations.is_empty() {
                        return Vec::new();
                }
                vec![Lexicon::cantonese(word.to_string(), romanizations.join(" "), input.to_string(), mark, 0)]
        }
}

// ---------------------------------------------------------------------
// Tests — run with `cargo test -p r-cantonese`. Requires ime.sqlite3 next to
// the crate manifest (run `cargo run -p preparing` first).
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
        use super::*;
        use crate::types::*;
        use std::path::PathBuf;

        fn db_path() -> PathBuf {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ime.sqlite3")
        }

        fn keys(text: &str) -> Vec<VirtualInputKey> {
                text.chars().filter_map(VirtualInputKey::for_character).collect()
        }

        fn suggest(engine: &CoreImeEngine, input: &str) -> Vec<Lexicon> {
                let keys = keys(input);
                assert_eq!(keys.len(), input.chars().count());
                let segmentation = engine.segment(&keys);
                engine.suggest(&keys, &segmentation, true)
        }

        fn texts(suggestions: &[Lexicon]) -> Vec<String> {
                suggestions.iter().map(|l| l.text.clone()).collect()
        }

        #[test]
        fn engine_prepares() {
                assert!(db_path().exists(), "run `cargo run -p preparing` first");
                let engine = CoreImeEngine::prepare_path(&db_path()).expect("engine prepare");
                assert!(engine.is_prepared());
        }

        #[test]
        fn segment_neihou() {
                let engine = CoreImeEngine::prepare_path(&db_path()).unwrap();
                let seg = engine.segment(&keys("neihou"));
                assert!(!seg.is_empty());
                // best scheme should be nei + hou
                let first: Vec<String> = seg[0].iter().map(|s| s.origin_text()).collect();
                assert_eq!(first, vec!["nei", "hou"]);
        }

        #[test]
        fn suggests_neihou() {
                let engine = CoreImeEngine::prepare_path(&db_path()).unwrap();
                let suggestions = suggest(&engine, "neihou");
                assert!(texts(&suggestions).iter().any(|t| t == "你好"),
                        "expected 你好, got {:?}", &texts(&suggestions)[..suggestions.len().min(10)]);
        }

        #[test]
        fn suggests_single_syllable() {
                let engine = CoreImeEngine::prepare_path(&db_path()).unwrap();
                let suggestions = suggest(&engine, "nei");
                assert!(texts(&suggestions).iter().any(|t| t == "你"),
                        "expected 你, got {:?}", &texts(&suggestions)[..suggestions.len().min(10)]);
        }

        #[test]
        fn suggests_with_anchor_initials() {
                let engine = CoreImeEngine::prepare_path(&db_path()).unwrap();
                // "nh" — first-letter shorthand should still find 你好
                let suggestions = suggest(&engine, "nh");
                assert!(texts(&suggestions).iter().any(|t| t == "你好"),
                        "expected 你好 for anchor input, got {:?}", &texts(&suggestions)[..suggestions.len().min(10)]);
        }

        #[test]
        fn reverse_lookup_by_pinyin() {
                let engine = CoreImeEngine::prepare_path(&db_path()).unwrap();
                // `r` prefix triggers pinyin reverse lookup in upstream keymap;
                // here we exercise the underlying query directly.
                let rows = engine.database.lookup_romanizations_for_word("你");
                assert!(rows.iter().any(|r| r.contains("nei")), "got {:?}", rows);
        }

        #[test]
        fn lookup_cangjie_by_character() {
                let engine = CoreImeEngine::prepare_path(&db_path()).unwrap();
                let rows = engine.database.query_cangjie_by_prefix(crate::db::CangjieVariant::Cangjie5, "onf", 10);
                assert!(!rows.is_empty(), "expected cangjie rows for prefix onf");
        }
}


#[cfg(test)]
mod debug_tests {
        use crate::db::ImeDatabase;
        use crate::types::serial_code;
        use std::path::PathBuf;

        #[test]
        fn inspect_neihou_rows() {
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ime.sqlite3");
                let db = ImeDatabase::open(&path).unwrap();
                // direct: what rows exist for word=你好
                let st = db.prepare("SELECT rowid, word, romanization, char_count, complexity, anchors, spell FROM lexicon_core WHERE word = \'你好\';").unwrap();
                let mut n = 0;
                while st.step() {
                        n += 1;
                        eprintln!("row: word={} rom={} chars={} complexity={} anchors={} spell={}",
                                st.column_text(1), st.column_text(2), st.column_i64(3),
                                st.column_i64(4), st.column_i64(5), st.column_i64(6));
                }
                eprintln!("count={} expected_spell={} expected_complexity=33", n, serial_code("neihou"));
                // what the query path computes
                let rows = db.query_lexicon_by_spell(serial_code("neihou"), 33, -1);
                eprintln!("spell query rows: {}", rows.len());
                for r in rows.iter().take(5) { eprintln!("  {} {}", r.word, r.romanization); }
        }
}
