// Pinyin segmenter + pinyin reverse lookup — port of PinyinSegmenter.cpp / InputEnginePinyin.cpp.
#![allow(dead_code)]

use crate::engine::*;
use crate::types::*;

const MAX_SYLLABLE_KEY_COUNT: usize = 6;

// ---------------------------------------------------------------------
// PinyinSegmenter
// ---------------------------------------------------------------------

struct PinyinSplitEdge {
        syllable: PinyinSyllable,
        end_index: usize,
}

struct PinyinSplitNode {
        syllable: PinyinSyllable,
        previous_index: Option<usize>,
        length: usize,
        count: usize,
}

impl CoreImeEngine {
        pub fn pinyin_segment(&self, keys: &[VirtualInputKey]) -> PinyinSegmentation {
                let letter_keys: Vec<VirtualInputKey> = keys.iter().copied().filter(|k| k.is_letter()).collect();
                let input_length = letter_keys.len();
                if input_length == 0 || self.pinyin_syllables.is_empty() {
                        return Vec::new();
                }

                let mut edges: Vec<Vec<PinyinSplitEdge>> = (0..input_length).map(|_| Vec::new()).collect();
                for start_index in 0..input_length {
                        let mut code = 0i64;
                        let end_limit = input_length.min(start_index + MAX_SYLLABLE_KEY_COUNT);
                        for end_index in start_index..end_limit {
                                code = code * 100 + letter_keys[end_index].code;
                                if let Some(syllable) = self.pinyin_syllables.get(&code) {
                                        edges[start_index].push(PinyinSplitEdge {
                                                syllable: syllable.clone(),
                                                end_index: end_index + 1,
                                        });
                                }
                        }
                }

                if edges.is_empty() || edges[0].is_empty() {
                        return Vec::new();
                }

                let mut nodes: Vec<PinyinSplitNode> = Vec::new();
                let mut frontier: Vec<usize> = Vec::new();
                for edge in &edges[0] {
                        nodes.push(PinyinSplitNode {
                                syllable: edge.syllable.clone(),
                                previous_index: None,
                                length: edge.end_index,
                                count: 1,
                        });
                        frontier.push(nodes.len() - 1);
                }

                while !frontier.is_empty() {
                        let mut next_frontier = Vec::new();
                        for node_index in frontier.drain(..) {
                                let node_length = nodes[node_index].length;
                                let node_count = nodes[node_index].count;
                                if node_length >= input_length {
                                        continue;
                                }
                                for edge in &edges[node_length] {
                                        nodes.push(PinyinSplitNode {
                                                syllable: edge.syllable.clone(),
                                                previous_index: Some(node_index),
                                                length: edge.end_index,
                                                count: node_count + 1,
                                        });
                                        next_frontier.push(nodes.len() - 1);
                                }
                        }
                        frontier = next_frontier;
                }

                struct Candidate {
                        scheme: PinyinScheme,
                        length: usize,
                }

                let mut candidates: Vec<Candidate> = nodes
                        .iter()
                        .enumerate()
                        .map(|(index, _)| {
                                let mut syllables = Vec::with_capacity(nodes[index].count);
                                let mut current = Some(index);
                                while let Some(i) = current {
                                        syllables.push(nodes[i].syllable.clone());
                                        current = nodes[i].previous_index;
                                }
                                syllables.reverse();
                                Candidate {
                                        scheme: syllables,
                                        length: nodes[index].length,
                                }
                        })
                        .collect();

                candidates.sort_by(|a, b| b.length.cmp(&a.length));
                candidates.into_iter().map(|c| c.scheme).collect()
        }
}

// ---------------------------------------------------------------------
// PinyinLexicon + pinyin search
// ---------------------------------------------------------------------

#[derive(Clone)]
struct PinyinLexicon {
        text: String,
        pinyin: String,
        input: String,
        input_count: usize,
        mark: String,
        number: i64,
}

impl PinyinLexicon {
        fn new(text: String, pinyin: String, input: String, mark: Option<String>, number: i64) -> Self {
                let input_count = input.chars().count();
                let mark = mark.unwrap_or_else(|| input.clone());
                Self {
                        text,
                        pinyin,
                        input,
                        input_count,
                        mark,
                        number,
                }
        }
        fn replaced_input(&self, new_input: String) -> Self {
                Self::new(self.text.clone(), self.pinyin.clone(), new_input, Some(self.mark.clone()), self.number)
        }
}

impl PartialEq for PinyinLexicon {
        fn eq(&self, other: &Self) -> bool {
                self.text == other.text && self.pinyin == other.pinyin
        }
}
impl Eq for PinyinLexicon {}
impl PartialOrd for PinyinLexicon {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
        }
}
impl Ord for PinyinLexicon {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other.input_count.cmp(&self.input_count).then(self.number.cmp(&other.number))
        }
}

fn pinyin_distinct(items: Vec<PinyinLexicon>) -> Vec<PinyinLexicon> {
        let mut result: Vec<PinyinLexicon> = Vec::with_capacity(items.len());
        for item in items {
                if !result.contains(&item) {
                        result.push(item);
                }
        }
        result
}

fn pinyin_first(items: Vec<PinyinLexicon>, count: usize) -> Vec<PinyinLexicon> {
        items.into_iter().take(count).collect()
}

fn pinyin_concatenate(left: &PinyinLexicon, right: &PinyinLexicon) -> PinyinLexicon {
        PinyinLexicon::new(
                format!("{}{}", left.text, right.text),
                format!("{} {}", left.pinyin, right.pinyin),
                format!("{}{}", left.input, right.input),
                Some(format!("{} {}", left.mark, right.mark)),
                (left.number + COMPOUND_NUMBER_STEP) + (right.number + COMPOUND_NUMBER_STEP),
        )
}

fn pinyin_lexicons_from_rows(rows: Vec<crate::db::LexiconRow>, input: &str, mark: Option<String>) -> Vec<PinyinLexicon> {
        rows.into_iter()
                .map(|row| PinyinLexicon::new(row.word, row.romanization.clone(), input.to_string(), Some(mark.clone().unwrap_or(row.romanization)), row.row_id))
                .collect()
}

fn pinyin_contains_scheme_length(segmentation: &PinyinSegmentation, length: usize) -> bool {
        segmentation.iter().any(|scheme| pinyin_scheme_length(scheme) == length)
}

fn pinyin_contains_input_count(items: &[PinyinLexicon], input_count: usize) -> bool {
        items.iter().any(|item| item.input_count == input_count)
}

fn pinyin_distinct_input_counts(items: &[PinyinLexicon]) -> Vec<usize> {
        let mut result = Vec::new();
        for item in items {
                if !result.contains(&item.input_count) {
                        result.push(item.input_count);
                }
        }
        result
}

fn pinyin_find_with_input_count<'a>(items: &'a [PinyinLexicon], input_count: usize) -> Option<&'a PinyinLexicon> {
        items.iter().find(|item| item.input_count == input_count)
}

fn pinyin_starts_with(text: &str, prefix: &str) -> bool {
        text.len() >= prefix.len() && text.starts_with(prefix)
}
fn pinyin_ends_with(text: &str, suffix: &str) -> bool {
        text.len() >= suffix.len() && text.ends_with(suffix)
}

fn pinyin_split(text: &str, separator: char) -> Vec<String> {
        text.split(separator).filter(|p| !p.is_empty()).map(|p| p.to_string()).collect()
}

impl CoreImeEngine {
        fn pinyin_rows_by_spell(&self, keys: &[VirtualInputKey], complexity: i64, input: Option<String>, mark: Option<String>, limit: Option<i32>) -> Vec<PinyinLexicon> {
                let input_text = input.unwrap_or_else(|| text_from_keys(keys));
                let rows = self
                        .database
                        .query_pinyin_by_spell(combined_code(keys), complexity, limit.unwrap_or(-1));
                pinyin_lexicons_from_rows(rows, &input_text, mark)
        }

        fn pinyin_rows_by_anchors(&self, keys: &[VirtualInputKey], input: Option<String>, limit: Option<i32>) -> Vec<PinyinLexicon> {
                let code = combined_code(keys);
                let text = input.unwrap_or_else(|| text_from_keys(keys));
                let rows = self.database.query_pinyin_by_anchors(code, keys.len(), limit.unwrap_or(100));
                pinyin_lexicons_from_rows(rows, &text, Some(text.clone()))
        }

        fn pinyin_modify(item: &PinyinLexicon, text: &str, input_length: usize) -> PinyinLexicon {
                if item.input_count == input_length {
                        return item.clone();
                }
                if pinyin_starts_with(&stripped_spaces(&item.pinyin), text) {
                        return PinyinLexicon::new(item.text.clone(), item.pinyin.clone(), text.to_string(), Some(text.to_string()), item.number);
                }
                let syllables = pinyin_split(&item.pinyin, ' ');
                if syllables.is_empty() || !pinyin_ends_with(text, syllables.last().unwrap()) {
                        return item.clone();
                }
                let is_matched = (syllables.len() - 1 + syllables.last().unwrap().chars().count()) == input_length;
                if is_matched {
                        PinyinLexicon::new(item.text.clone(), item.pinyin.clone(), text.to_string(), Some(text.to_string()), item.number)
                } else {
                        item.clone()
                }
        }

        fn pinyin_query(&self, input_length: usize, segmentation: &PinyinSegmentation, limit: Option<i32>) -> Vec<PinyinLexicon> {
                let ideal_schemes: Vec<&PinyinScheme> = segmentation
                        .iter()
                        .filter(|scheme| pinyin_scheme_length(scheme) == input_length)
                        .collect();
                let mut result = Vec::new();
                if ideal_schemes.is_empty() {
                        for scheme in segmentation {
                                result.extend(self.pinyin_rows_by_spell(
                                        &pinyin_scheme_keys(scheme),
                                        pinyin_scheme_complexity(scheme),
                                        Some(pinyin_scheme_text(scheme)),
                                        Some(pinyin_scheme_mark(scheme)),
                                        limit,
                                ));
                        }
                        return result;
                }
                for scheme in ideal_schemes {
                        if scheme.len() == 1 {
                                result.extend(self.pinyin_rows_by_spell(
                                        &pinyin_scheme_keys(scheme),
                                        pinyin_scheme_complexity(scheme),
                                        Some(pinyin_scheme_text(scheme)),
                                        Some(pinyin_scheme_mark(scheme)),
                                        limit,
                                ));
                        } else {
                                for count in (1..=scheme.len()).rev() {
                                        let slice = &scheme[..count.min(scheme.len())].to_vec();
                                        result.extend(self.pinyin_rows_by_spell(
                                                &pinyin_scheme_keys(slice),
                                                pinyin_scheme_complexity(slice),
                                                Some(pinyin_scheme_text(slice)),
                                                Some(pinyin_scheme_mark(slice)),
                                                limit,
                                        ));
                                }
                        }
                }
                result
        }

        fn process_pinyin_slices(&self, keys: &[VirtualInputKey], text: &str, limit: Option<i32>) -> Vec<PinyinLexicon> {
                let adjusted_limit = if limit.is_some() { 100 } else { 300 };
                let input_length = keys.len();
                let mut result = Vec::new();

                for number in 0..input_length {
                        let leading_len = input_length - number;
                        let leading_keys = &keys[..leading_len];
                        let leading_text = text_from_keys(leading_keys);

                        for item in self.pinyin_rows_by_spell(leading_keys, leading_len as i64, Some(leading_text.clone()), None, limit) {
                                result.push(Self::pinyin_modify(&item, text, input_length));
                        }

                        let mut anchors_matched: Vec<PinyinLexicon> = self
                                .pinyin_rows_by_anchors(leading_keys, Some(leading_text), Some(adjusted_limit))
                                .iter()
                                .map(|item| Self::pinyin_modify(item, text, input_length))
                                .collect();
                        anchors_matched.sort();
                        anchors_matched = pinyin_first(anchors_matched, 72);
                        result.extend(anchors_matched);
                }
                let mut result = pinyin_distinct(result);
                result.sort();
                result
        }

        fn pinyin_search(&self, keys: &[VirtualInputKey], segmentation: &PinyinSegmentation, limit: Option<i32>) -> Vec<PinyinLexicon> {
                let input_length = keys.len();
                let text = text_from_keys(keys);

                let anchors_matched = self.pinyin_rows_by_anchors(keys, Some(text.clone()), limit);
                let queried = self.pinyin_query(input_length, segmentation, limit);

                let should_match_prefixes = !pinyin_contains_input_count(&queried, input_length)
                        && !pinyin_contains_scheme_length(segmentation, input_length);

                let mut prefix_matched = Vec::new();
                if should_match_prefixes {
                        let prefixes_limit = if limit.is_some() { 200 } else { 500 };
                        for scheme in segmentation {
                                let scheme_len = pinyin_scheme_length(scheme);
                                let tail = &keys[scheme_len.min(keys.len())..];
                                if tail.is_empty() {
                                        continue;
                                }
                                let mut scheme_anchors: Vec<VirtualInputKey> = Vec::with_capacity(scheme.len());
                                let mut scheme_texts: Vec<&str> = Vec::with_capacity(scheme.len());
                                for syllable in scheme {
                                        if let Some(first) = syllable.keys.first() {
                                                scheme_anchors.push(*first);
                                        }
                                        scheme_texts.push(&syllable.text);
                                }
                                let mut conjoined = scheme_anchors.clone();
                                conjoined.extend_from_slice(tail);
                                let mut anchors = scheme_anchors;
                                anchors.push(tail[0]);

                                let scheme_mark = scheme_texts.join(" ");
                                let mark = format!("{} {}", scheme_mark, text_from_keys(tail));
                                let tail_anchors_text = text_from_keys(tail);

                                for item in self.pinyin_rows_by_anchors(&conjoined, None, Some(prefixes_limit)) {
                                        if !pinyin_starts_with(&item.pinyin, &scheme_mark) {
                                                continue;
                                        }
                                        let suffix: String = item.pinyin.chars().skip(scheme_mark.chars().count()).collect();
                                        let suffix_anchors: String = pinyin_split(&suffix, ' ')
                                                .iter()
                                                .filter_map(|s| s.chars().next())
                                                .collect();
                                        if suffix_anchors == tail_anchors_text {
                                                prefix_matched.push(PinyinLexicon::new(
                                                        item.text.clone(),
                                                        item.pinyin.clone(),
                                                        text.clone(),
                                                        Some(mark.clone()),
                                                        item.number,
                                                ));
                                        }
                                }

                                for item in self.pinyin_rows_by_anchors(&anchors, None, Some(prefixes_limit)) {
                                        if pinyin_starts_with(&item.pinyin, &mark) {
                                                prefix_matched.push(PinyinLexicon::new(
                                                        item.text.clone(),
                                                        item.pinyin.clone(),
                                                        text.clone(),
                                                        Some(mark.clone()),
                                                        item.number,
                                                ));
                                        }
                                }
                        }
                }

                let mut gained_matched = Vec::new();
                if should_match_prefixes {
                        for number in 1..input_length {
                                let leading_len = input_length - number;
                                let leading_keys = &keys[..leading_len];
                                let leading_text = text_from_keys(leading_keys);
                                for item in self.pinyin_rows_by_anchors(leading_keys, Some(leading_text), Some(300)) {
                                        if pinyin_starts_with(&stripped_spaces(&item.pinyin), &text) {
                                                gained_matched.push(PinyinLexicon::new(
                                                        item.text.clone(),
                                                        item.pinyin.clone(),
                                                        text.clone(),
                                                        Some(text.clone()),
                                                        item.number,
                                                ));
                                                continue;
                                        }
                                        let syllables = pinyin_split(&item.pinyin, ' ');
                                        if syllables.is_empty() || !pinyin_ends_with(&text, syllables.last().unwrap()) {
                                                continue;
                                        }
                                        let is_matched = (syllables.len() - 1 + syllables.last().unwrap().chars().count()) == input_length;
                                        if is_matched {
                                                gained_matched.push(PinyinLexicon::new(
                                                        item.text.clone(),
                                                        item.pinyin.clone(),
                                                        text.clone(),
                                                        Some(text.clone()),
                                                        item.number,
                                                ));
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
                ideal_queried.sort_by_key(|i| i.number);
                let ideal_queried = pinyin_distinct(ideal_queried);
                not_ideal_queried.sort();
                let not_ideal_queried = pinyin_distinct(not_ideal_queried);

                let mut full_input = Vec::new();
                full_input.extend(ideal_queried.iter().cloned());
                full_input.extend(anchors_matched);
                full_input.extend(prefix_matched);
                full_input.extend(gained_matched);
                let full_input = pinyin_distinct(full_input);

                let mut fetched = Vec::new();
                fetched.extend(pinyin_first(full_input.clone(), 10));
                let mut sorted_full = full_input.clone();
                sorted_full.sort();
                fetched.extend(pinyin_first(sorted_full, 10));
                fetched.extend(pinyin_first(not_ideal_queried.clone(), 10));
                let mut sorted_ni = not_ideal_queried.clone();
                sorted_ni.sort_by_key(|i| i.number);
                fetched.extend(pinyin_first(sorted_ni, 10));
                fetched.extend(full_input.iter().cloned());
                fetched.extend(not_ideal_queried.iter().cloned());
                let fetched = pinyin_distinct(fetched);

                if fetched.is_empty() {
                        return self.process_pinyin_slices(keys, &text, limit);
                }

                let first_input_count = fetched[0].input_count;
                if first_input_count >= input_length {
                        return fetched;
                }

                let mut concatenated = Vec::new();
                for head_length in pinyin_distinct_input_counts(&fetched) {
                        let tail_keys = &keys[head_length.min(keys.len())..];
                        let tail_seg = self.pinyin_segment(tail_keys);
                        let tail_lexicons = self.pinyin_search(tail_keys, &tail_seg, Some(50));
                        let Some(head_lexicon) = pinyin_find_with_input_count(&fetched, head_length) else {
                                continue;
                        };
                        if tail_lexicons.is_empty() {
                                continue;
                        }
                        concatenated.push(pinyin_concatenate(head_lexicon, &tail_lexicons[0]));
                }
                let mut concatenated = pinyin_distinct(concatenated);
                concatenated.sort();
                let mut concatenated = pinyin_first(concatenated, 1);
                concatenated.extend(fetched);
                concatenated
        }

        fn filter_pinyin_syllable_separators(&self, keys: &[VirtualInputKey], lexicons: Vec<PinyinLexicon>) -> Vec<PinyinLexicon> {
                if keys.is_empty() || keys[0].is_apostrophe() {
                        return Vec::new();
                }
                let is_trailing = keys.last().map(|k| k.is_apostrophe()).unwrap_or(false);
                let sep_count = keys.iter().filter(|k| k.is_apostrophe()).count();
                let input_length = keys.len();
                let text = text_from_keys(keys);
                let text_parts = pinyin_split(&text, '\'');

                let mut filtered = Vec::new();
                for item in lexicons {
                        let syllables = pinyin_split(&item.pinyin, ' ');
                        if syllables == text_parts {
                                filtered.push(item.replaced_input(text.clone()));
                                continue;
                        }
                        if sep_count == 1 && is_trailing {
                                if syllables.len() == 1 && item.input_count == input_length - 1 {
                                        filtered.push(item.replaced_input(text.clone()));
                                }
                                continue;
                        }
                        if sep_count == 1 {
                                if syllables.len() == 1 {
                                        if !text_parts.is_empty() && item.input_count == text_parts[0].chars().count() {
                                                filtered.push(item.replaced_input(format!("{}'", item.input)));
                                        }
                                } else if syllables.len() == 2 && text_parts.len() >= 2 {
                                        let mut is_matched = true;
                                        if input_length != 3 && syllables[0] != text_parts[0] {
                                                is_matched = text_parts[0].chars().count() == 1
                                                        && !syllables[0].is_empty()
                                                        && text_parts[0].chars().next() == syllables[0].chars().next()
                                                        && pinyin_starts_with(&text_parts[1], &syllables[1]);
                                        }
                                        if is_matched {
                                                filtered.push(item.replaced_input(format!("{}'", item.input)));
                                        }
                                }
                                continue;
                        }
                        if sep_count == 2 && is_trailing {
                                if syllables.len() == 1 {
                                        if !text_parts.is_empty() && item.input_count == text_parts[0].chars().count() {
                                                filtered.push(item.replaced_input(format!("{}'", item.input)));
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
                                                filtered.push(item.replaced_input(text.clone()));
                                        }
                                }
                                continue;
                        }
                        if ((sep_count == 2 && input_length == 5) || (sep_count == 3 && input_length == 6)) && text_parts.len() == 3 {
                                match syllables.len() {
                                        1 => {
                                                if item.input_count == 1 {
                                                        filtered.push(item.replaced_input(format!("{}'", item.input)));
                                                }
                                        }
                                        2 => {
                                                if item.input_count == 2 {
                                                        filtered.push(item.replaced_input(format!("{}''", item.input)));
                                                }
                                        }
                                        3 => filtered.push(item.replaced_input(text.clone())),
                                        _ => {}
                                }
                                continue;
                        }
                        if syllables.len() < text_parts.len() && !syllables.is_empty() {
                                let is_matched = (0..syllables.len()).all(|i| syllables[i] == text_parts[i]);
                                if is_matched {
                                        filtered.push(item.replaced_input(format!("{}{}", item.input, "i".repeat(syllables.len() - 1))));
                                }
                        }
                }
                filtered.sort();
                filtered
        }

        pub fn pinyin_reverse_lookup(&self, keys: &[VirtualInputKey]) -> Vec<Lexicon> {
                if keys.is_empty() {
                        return Vec::new();
                }
                let has_separators = keys.iter().any(|k| k.is_apostrophe());
                let search_keys: Vec<VirtualInputKey> = if has_separators {
                        keys.iter().copied().filter(|k| k.is_letter()).collect()
                } else {
                        keys.to_vec()
                };
                if search_keys.is_empty() {
                        return Vec::new();
                }

                let segmentation = self.pinyin_segment(&search_keys);
                let mut pinyin_lexicons = if segmentation.is_empty() {
                        self.process_pinyin_slices(&search_keys, &text_from_keys(&search_keys), None)
                } else {
                        self.pinyin_search(&search_keys, &segmentation, None)
                };

                if has_separators {
                        pinyin_lexicons = self.filter_pinyin_syllable_separators(keys, pinyin_lexicons);
                }

                let mut result = Vec::new();
                for item in pinyin_lexicons {
                        result.extend(self.reverse_lookup_word(&item.text, &item.input, Some(item.mark)));
                }
                result
        }
}
