// InputMemory — user frequency database. Port of InputMemory.cpp.
#![allow(dead_code)]

use std::path::PathBuf;

use crate::db::ImeDatabase;
use crate::globals;
use crate::segmenter::Segmenter;
use crate::types::*;

const MAX_CHAR_COUNT: usize = 9;

#[derive(Clone)]
struct MemoryLexicon {
        word: String,
        romanization: String,
        frequency: i64,
        latest: i64,
        input: String,
        input_count: usize,
        mark: String,
}

impl MemoryLexicon {
        fn new(word: String, romanization: String, frequency: i64, latest: i64, input: String, mark: String) -> Self {
                let input_count = input.chars().count();
                Self {
                        word,
                        romanization,
                        frequency,
                        latest,
                        input,
                        input_count,
                        mark,
                }
        }
        fn to_lexicon(&self, number: i64) -> Lexicon {
                Lexicon::cantonese(self.word.clone(), self.romanization.clone(), self.input.clone(), Some(self.mark.clone()), number)
        }
        fn replaced(&self, input: String, mark: String) -> Self {
                Self::new(self.word.clone(), self.romanization.clone(), self.frequency, self.latest, input, mark)
        }
}

fn regular_sorted(items: &[MemoryLexicon], is_ordered: bool) -> Vec<MemoryLexicon> {
        let mut frequency_preferred = items.to_vec();
        if !is_ordered {
                frequency_preferred.sort_by(|a, b| b.frequency.cmp(&a.frequency));
        }
        let mut date_preferred = items.to_vec();
        date_preferred.sort_by(|a, b| b.latest.cmp(&a.latest));

        let mut result = Vec::new();
        result.extend(frequency_preferred.iter().take(3).cloned());
        result.extend(date_preferred.iter().take(5).cloned());
        result.extend(frequency_preferred.iter().cloned());
        // distinct by (word, romanization)
        let mut seen = Vec::new();
        result.retain(|item| {
                let key = (item.word.clone(), item.romanization.clone());
                if seen.contains(&key) {
                        false
                } else {
                        seen.push(key);
                        true
                }
        });
        result
}

fn peculiar_sorted(items: &[MemoryLexicon]) -> Vec<MemoryLexicon> {
        let mut input_counts: Vec<usize> = Vec::new();
        for item in items {
                if !input_counts.contains(&item.input_count) {
                        input_counts.push(item.input_count);
                }
        }
        input_counts.sort_by(|a, b| b.cmp(a));
        let mut result = Vec::new();
        for count in input_counts {
                let group: Vec<MemoryLexicon> = items.iter().filter(|i| i.input_count == count).cloned().collect();
                result.extend(regular_sorted(&group, false));
        }
        result
}

fn tail_anchor_text(keys: &[VirtualInputKey]) -> String {
        keys.iter()
                .map(|key| if key.is_y() { LETTER_J.character } else { key.character })
                .collect()
}

fn suffix_anchor_text(romanization: &str, prefix_char_count: usize) -> String {
        if prefix_char_count > romanization.chars().count() {
                return String::new();
        }
        let suffix: String = romanization.chars().skip(prefix_char_count).collect();
        crate::engine::split(&suffix, ' ')
                .iter()
                .filter_map(|s| s.chars().next())
                .collect()
}

fn last_tone_free_syllable(romanization: &str) -> Option<String> {
        crate::engine::split(romanization, ' ').last().map(|s| stripped_tones(s))
}

struct MemorySerialFields {
        char_count: i64,
        letter_count: i64,
        complexity: i64,
        anchors: i64,
        spell: i64,
}

fn derive_serial_fields(word: &str, romanization: &str) -> MemorySerialFields {
        let mut tone_free = String::new();
        let mut letters = String::new();
        for c in romanization.chars() {
                if !c.is_ascii_digit() {
                        tone_free.push(c);
                }
                if is_lowercase_basic_latin_letter(c) {
                        letters.push(c);
                }
        }
        let phones = crate::engine::split(&tone_free, ' ');
        let mut complexity = 0i64;
        let mut anchor_keys = Vec::new();
        for phone in &phones {
                complexity = complexity * 10 + phone.chars().count() as i64;
                if let Some(first) = phone.chars().next() {
                        if let Some(key) = VirtualInputKey::for_character(first) {
                                anchor_keys.push(key);
                        }
                }
        }
        let spell_keys = input_keys_from_text(&letters);
        MemorySerialFields {
                char_count: word.chars().count() as i64,
                letter_count: spell_keys.len() as i64,
                complexity,
                anchors: combined_code(&anchor_keys),
                spell: combined_code(&spell_keys),
        }
}

fn current_time_milliseconds() -> i64 {
        std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0)
}

fn user_data_directory() -> Option<PathBuf> {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
                return Some(PathBuf::from(local).join("RCantonese"));
        }
        std::env::temp_dir().parent().map(|p| p.join("RCantonese")).or_else(|| Some(std::env::temp_dir().join("RCantonese")))
}

pub struct InputMemory {
        database: Option<ImeDatabase>,
}

impl InputMemory {
        pub fn new() -> Self {
                Self { database: None }
        }

        pub fn prepare(&mut self) -> bool {
                self.database = None;
                let Some(directory) = user_data_directory() else {
                        globals::log("InputMemory: no user data directory");
                        return false;
                };
                if std::fs::create_dir_all(&directory).is_err() {
                        globals::log_error("InputMemory: cannot create directory");
                        return false;
                }
                let path = directory.join("memory.sqlite3");
                let Some(database) = ImeDatabase::open_readwrite(&path) else {
                        globals::log_error("InputMemory: cannot open database");
                        return false;
                };
                if !Self::prepare_schema(&database) {
                        globals::log_error("InputMemory: schema prepare failed");
                        return false;
                }
                self.database = Some(database);
                true
        }

        fn prepare_schema(database: &ImeDatabase) -> bool {
                let statements = [
                        "BEGIN IMMEDIATE;",
                        "CREATE TABLE IF NOT EXISTS memory2608 (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, romanization TEXT NOT NULL, frequency INTEGER NOT NULL, latest INTEGER NOT NULL, char_count INTEGER NOT NULL, letter_count INTEGER NOT NULL, complexity INTEGER NOT NULL, anchors INTEGER NOT NULL, spell INTEGER NOT NULL, UNIQUE (word, romanization));",
                        "CREATE INDEX IF NOT EXISTS ix2608_frequency ON memory2608 (frequency);",
                        "CREATE INDEX IF NOT EXISTS ix2608_anchors ON memory2608 (anchors, char_count, frequency DESC);",
                        "CREATE INDEX IF NOT EXISTS ix2608_spell ON memory2608 (spell, letter_count, complexity, frequency DESC);",
                        "CREATE INDEX IF NOT EXISTS ix2608_word ON memory2608 (word, frequency DESC);",
                        "CREATE INDEX IF NOT EXISTS ix2608_lexicon ON memory2608 (word, romanization);",
                ];
                let mut ok = true;
                for sql in statements {
                        if !database.execute(sql) {
                                ok = false;
                                break;
                        }
                }
                if ok {
                        let version = database
                                .prepare("PRAGMA user_version;")
                                .and_then(|st| if st.step() { Some(st.column_i64(0)) } else { None });
                        match version {
                                Some(v) if v < 2608 => {
                                        let has_legacy = database
                                                .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'core_memory' LIMIT 1;")
                                                .map(|st| st.step())
                                                .unwrap_or(false);
                                        if has_legacy {
                                                ok = Self::migrate_legacy(database);
                                        }
                                        if ok {
                                                ok = database.execute("PRAGMA user_version = 2608;");
                                        }
                                }
                                Some(_) => {}
                                None => ok = false,
                        }
                }
                if ok {
                        database.execute("COMMIT;")
                } else {
                        database.execute("ROLLBACK;");
                        false
                }
        }

        fn migrate_legacy(database: &ImeDatabase) -> bool {
                let Some(select) = database.prepare("SELECT word, romanization, frequency, latest FROM core_memory ORDER BY rowid;") else {
                        return false;
                };
                let Some(insert) = database.prepare(
                        "INSERT OR IGNORE INTO memory2608 (word, romanization, frequency, latest, char_count, letter_count, complexity, anchors, spell) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);",
                ) else {
                        return false;
                };
                while select.step() {
                        let word = select.column_text(0);
                        let romanization = select.column_text(1);
                        let frequency = select.column_i64(2);
                        let latest = select.column_i64(3);
                        let fields = derive_serial_fields(&word, &romanization);
                        insert.reset();
                        insert.bind_text(1, &word);
                        insert.bind_text(2, &romanization);
                        insert.bind_i64(3, frequency);
                        insert.bind_i64(4, latest);
                        insert.bind_i64(5, fields.char_count);
                        insert.bind_i64(6, fields.letter_count);
                        insert.bind_i64(7, fields.complexity);
                        insert.bind_i64(8, fields.anchors);
                        insert.bind_i64(9, fields.spell);
                        if !insert.step() {
                                // step() returns false for DONE too; check is ambiguous — treat as done.
                        }
                }
                true
        }

        pub fn is_prepared(&self) -> bool {
                self.database.is_some()
        }

        pub fn handle(&mut self, lexicon: &Lexicon) -> bool {
                let Some(database) = &self.database else { return false };
                if lexicon.is_not_cantonese() {
                        return false;
                }
                // find existing
                if let Some(find) = database.prepare("SELECT id, frequency FROM memory2608 WHERE word = ? AND romanization = ? LIMIT 1;") {
                        find.bind_text(1, &lexicon.text);
                        find.bind_text(2, &lexicon.romanization);
                        if find.step() {
                                let id = find.column_i64(0);
                                let frequency = find.column_i64(1);
                                if let Some(update) = database.prepare("UPDATE memory2608 SET frequency = ?, latest = ? WHERE id = ?;") {
                                        update.bind_i64(1, frequency + 1);
                                        update.bind_i64(2, current_time_milliseconds());
                                        update.bind_i64(3, id);
                                        update.step();
                                        return true;
                                }
                                return false;
                        }
                }
                let Some(insert) = database.prepare(
                        "INSERT INTO memory2608 (word, romanization, frequency, latest, char_count, letter_count, complexity, anchors, spell) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);",
                ) else {
                        return false;
                };
                let fields = derive_serial_fields(&lexicon.text, &lexicon.romanization);
                insert.bind_text(1, &lexicon.text);
                insert.bind_text(2, &lexicon.romanization);
                insert.bind_i64(3, 1);
                insert.bind_i64(4, current_time_milliseconds());
                insert.bind_i64(5, fields.char_count);
                insert.bind_i64(6, fields.letter_count);
                insert.bind_i64(7, fields.complexity);
                insert.bind_i64(8, fields.anchors);
                insert.bind_i64(9, fields.spell);
                insert.step();
                true
        }

        pub fn forget(&mut self, lexicon: &Lexicon) -> bool {
                let Some(database) = &self.database else { return false };
                if lexicon.is_not_cantonese() {
                        return false;
                }
                let Some(statement) = database.prepare("DELETE FROM memory2608 WHERE word = ? AND romanization = ?;") else {
                        return false;
                };
                statement.bind_text(1, &lexicon.text);
                statement.bind_text(2, &lexicon.romanization);
                statement.step();
                true
        }

        pub fn delete_all(&self) -> bool {
                match &self.database {
                        Some(db) => db.execute("DELETE FROM memory2608;"),
                        None => false,
                }
        }

        fn memory_anchors_match(&self, keys: &[VirtualInputKey], input: Option<String>, limit: i64) -> Vec<MemoryLexicon> {
                let Some(database) = &self.database else { return Vec::new() };
                let Some(statement) = database.prepare(
                        "SELECT word, romanization, frequency, latest FROM memory2608 WHERE anchors = ? AND char_count = ? ORDER BY frequency DESC LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, anchors_code(keys));
                statement.bind_i64(2, keys.len() as i64);
                statement.bind_i64(3, limit);
                let user_input = input.unwrap_or_else(|| text_from_keys(keys));
                let mut rows = Vec::new();
                while statement.step() {
                        let romanization = statement.column_text(1);
                        rows.push(MemoryLexicon::new(
                                statement.column_text(0),
                                romanization.clone(),
                                statement.column_i64(2),
                                statement.column_i64(3),
                                user_input.clone(),
                                user_input.clone(),
                        ));
                }
                rows
        }

        fn memory_spell_match(&self, keys: &[VirtualInputKey], complexity: i64, input: Option<String>, mark: Option<String>, limit: i64) -> Vec<MemoryLexicon> {
                let Some(database) = &self.database else { return Vec::new() };
                let Some(statement) = database.prepare(
                        "SELECT word, romanization, frequency, latest FROM memory2608 WHERE spell = ? AND letter_count = ? AND complexity = ? ORDER BY frequency DESC LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, combined_code(keys));
                statement.bind_i64(2, keys.len() as i64);
                statement.bind_i64(3, complexity);
                statement.bind_i64(4, limit);
                let user_input = input.unwrap_or_else(|| text_from_keys(keys));
                let mut rows = Vec::new();
                while statement.step() {
                        let romanization = statement.column_text(1);
                        let mark = mark.clone().unwrap_or_else(|| stripped_tones(&romanization));
                        rows.push(MemoryLexicon::new(
                                statement.column_text(0),
                                romanization,
                                statement.column_i64(2),
                                statement.column_i64(3),
                                user_input.clone(),
                                mark,
                        ));
                }
                rows
        }

        fn memory_perform(&self, scheme: &[Syllable], limit: i64) -> Vec<MemoryLexicon> {
                self.memory_spell_match(
                        &scheme_origin_keys(scheme),
                        scheme_complexity(scheme),
                        Some(scheme_alias_text(scheme)),
                        Some(scheme_mark(scheme)),
                        limit,
                )
        }

        pub fn suggest(&self, keys: &[VirtualInputKey], segmentation: &Segmentation, segmenter: &Segmenter) -> Vec<Lexicon> {
                if !self.is_prepared() || keys.is_empty() {
                        return Vec::new();
                }
                let has_apostrophe = keys.iter().any(|k| k.is_apostrophe());
                let has_tone = keys.iter().any(|k| k.is_tone_input_key());
                if !has_apostrophe && !has_tone {
                        return self.search(keys, segmentation, segmenter);
                }
                let syllable_keys = syllable_keys(keys);
                let candidates = self.search(&syllable_keys, segmentation, segmenter);
                if has_apostrophe && has_tone {
                        let input_text = text_from_keys(keys);
                        let text = tone_converted(&input_text);
                        return candidates
                                .into_iter()
                                .filter(|item| text.starts_with(&item.romanization))
                                .map(|item| item.replaced_input(input_text.clone()))
                                .collect();
                }
                if has_tone {
                        return self.filter_tone_suggestions(keys, candidates);
                }
                self.filter_apostrophe_suggestions(keys, candidates)
        }

        fn search(&self, keys: &[VirtualInputKey], segmentation: &Segmentation, segmenter: &Segmenter) -> Vec<Lexicon> {
                let input_length = keys.len();
                let text = text_from_keys(keys);

                let ideal_schemes: Vec<&Scheme> = segmentation
                        .iter()
                        .filter(|scheme| scheme_length(scheme) == input_length)
                        .collect();

                let mut queried: Vec<MemoryLexicon> = Vec::new();
                if ideal_schemes.is_empty() {
                        for scheme in segmentation {
                                queried.extend(self.memory_perform(scheme, 5));
                        }
                } else {
                        for scheme in &ideal_schemes {
                                for count in (1..=scheme.len()).rev() {
                                        let limit = if count == scheme.len() { 20 } else { 5 };
                                        queried.extend(self.memory_perform(&scheme[..count], limit));
                                }
                        }
                }

                let mut ideal_queried = Vec::new();
                let mut not_ideal_queried = Vec::new();
                for item in queried.iter().cloned() {
                        if item.input_count >= input_length {
                                ideal_queried.push(item);
                        } else {
                                not_ideal_queried.push(item);
                        }
                }

                let mut ideal: Vec<Lexicon> = regular_sorted(&ideal_queried, false).iter().map(|i| i.to_lexicon(-1)).collect();
                let not_ideal: Vec<Lexicon> = peculiar_sorted(&not_ideal_queried).iter().map(|i| i.to_lexicon(-2)).collect();
                let anchors_matched = self.memory_anchors_match(keys, None, if queried.is_empty() { 20 } else { 5 });
                let anchors: Vec<Lexicon> = regular_sorted(&anchors_matched, true).iter().map(|i| i.to_lexicon(-1)).collect();
                if !ideal.is_empty() || !anchors.is_empty() {
                        ideal.extend(anchors);
                        ideal.extend(not_ideal);
                        return ideal;
                }

                if input_length <= 2 || input_length >= 25 {
                        return not_ideal;
                }

                let should_partially_match = ideal_schemes.is_empty()
                        || keys.last() == Some(&LETTER_M)
                        || keys.first() == Some(&LETTER_M);
                if !should_partially_match {
                        return not_ideal;
                }

                let mut prefix_matched: Vec<MemoryLexicon> = Vec::new();
                for scheme in segmentation {
                        if scheme.is_empty() || scheme.len() > MAX_CHAR_COUNT {
                                continue;
                        }
                        let tail = &keys[scheme_length(scheme).min(keys.len())..];
                        if tail.is_empty() {
                                continue;
                        }
                        let scheme_anchors = scheme_alias_anchors(scheme);
                        let mut conjoined = scheme_anchors.clone();
                        conjoined.extend_from_slice(tail);
                        let scheme_syllable_text = scheme_syllable_text(scheme);
                        let mark = format!("{} {}", scheme_mark(scheme), text_from_keys(tail));
                        let tail_as_anchor_text = tail_anchor_text(tail);

                        for item in self.memory_anchors_match(&conjoined, None, 100) {
                                let tone_free = stripped_tones(&item.romanization);
                                if tone_free.starts_with(&scheme_syllable_text)
                                        && suffix_anchor_text(&tone_free, scheme_syllable_text.chars().count()) == tail_as_anchor_text
                                {
                                        prefix_matched.push(item.replaced(text.clone(), mark.clone()));
                                }
                        }

                        let transformed_tail: String = tail
                                .iter()
                                .enumerate()
                                .map(|(i, k)| if i == 0 && k.is_y() { LETTER_J.character } else { k.character })
                                .collect();
                        let mut anchors_keys = scheme_anchors;
                        anchors_keys.push(tail[0]);
                        let syllable_text = format!("{} {}", scheme_syllable_text, transformed_tail);
                        for item in self.memory_anchors_match(&anchors_keys, None, 100) {
                                if stripped_tones(&item.romanization).starts_with(&syllable_text) {
                                        prefix_matched.push(item.replaced(text.clone(), mark.clone()));
                                }
                        }
                }

                let mut gained_matched: Vec<MemoryLexicon> = Vec::new();
                for number in (1..input_length).rev() {
                        if number > MAX_CHAR_COUNT {
                                continue;
                        }
                        for item in self.memory_anchors_match(&keys[..number], None, 100) {
                                let tail_start = if item.input_count > 0 { item.input_count - 1 } else { 0 };
                                let tail = &keys[tail_start.min(keys.len())..];
                                if tail.len() > 6 {
                                        continue;
                                }
                                let converted = item.replaced(text.clone(), text.clone());
                                if latin_letter_only(&item.romanization).starts_with(&text) {
                                        gained_matched.push(converted);
                                        continue;
                                }
                                let Some(last_syllable) = last_tone_free_syllable(&item.romanization) else {
                                        continue;
                                };
                                if let Some(tail_syllable) = segmenter.syllable_text(tail) {
                                        if last_syllable == tail_syllable {
                                                gained_matched.push(converted);
                                        }
                                } else if last_syllable.starts_with(&text_from_keys(tail)) {
                                        gained_matched.push(converted);
                                }
                        }
                }

                let mut partial = prefix_matched;
                partial.extend(gained_matched);
                let mut result: Vec<Lexicon> = peculiar_sorted(&partial).iter().take(5).map(|i| i.to_lexicon(-1)).collect();
                result.extend(not_ideal);
                result
        }

        fn filter_tone_suggestions(&self, keys: &[VirtualInputKey], candidates: Vec<Lexicon>) -> Vec<Lexicon> {
                let input_text = text_from_keys(keys);
                let text = tone_converted(&input_text);
                let text_tones = tone_digit_only(&text);
                let text_chars: Vec<char> = text.chars().collect();

                let mut qualified = Vec::new();
                for item in candidates {
                        let syllable_text = stripped_spaces(&item.romanization);
                        if syllable_text == text {
                                qualified.push(item.replaced_input(input_text.clone()));
                                continue;
                        }
                        let tones = tone_digit_only(&syllable_text);
                        if text_tones.chars().count() == 1 && tones.chars().count() == 1 {
                                if text_chars.len() == item.input_count + 1
                                        && text_chars.last().map(|c| is_cantonese_tone_digit(*c)).unwrap_or(false)
                                        && text_tones == tones
                                {
                                        qualified.push(item.replaced_input(input_text.clone()));
                                }
                                continue;
                        }
                        if text_tones.chars().count() == 1 && tones.chars().count() == 2 {
                                let is_tone_last = text_chars.last().map(|c| is_cantonese_tone_digit(*c)).unwrap_or(false);
                                if is_tone_last {
                                        let has_matching = tones.chars().last() == text_tones.chars().next();
                                        let correct_pos = item.input_count < text_chars.len()
                                                && is_cantonese_tone_digit(text_chars[item.input_count]);
                                        if has_matching && correct_pos {
                                                qualified.push(item.replaced_input(input_text.clone()));
                                        }
                                } else if tones.chars().next() == text_tones.chars().next() {
                                        qualified.push(item.replaced_input(input_text.clone()));
                                }
                                continue;
                        }
                        if text_tones.chars().count() == 2 && tones.chars().count() == 2 {
                                if text_chars.last().map(|c| is_cantonese_tone_digit(*c)).unwrap_or(false)
                                        && text_tones == tones
                                        && item.input_count == text_chars.len() - 2
                                {
                                        qualified.push(item.replaced_input(input_text.clone()));
                                }
                                continue;
                        }
                        if input_text == syllable_text {
                                qualified.push(item.replaced_input(input_text.clone()));
                        }
                }
                qualified
        }

        fn filter_apostrophe_suggestions(&self, keys: &[VirtualInputKey], candidates: Vec<Lexicon>) -> Vec<Lexicon> {
                if keys.is_empty() || keys[0].is_apostrophe() {
                        return Vec::new();
                }
                let is_trailing = keys.last().map(|k| k.is_apostrophe()).unwrap_or(false);
                let sep_count = keys.iter().filter(|k| k.is_apostrophe()).count();
                let input_length = keys.len();
                let text = text_from_keys(keys);
                let text_parts = crate::engine::split(&text, '\'');

                let mut qualified = Vec::new();
                for item in candidates {
                        let syllables = crate::engine::split(&stripped_tones(&item.romanization), ' ');
                        if syllables == text_parts {
                                qualified.push(item.replaced_input(text.clone()));
                                continue;
                        }
                        if sep_count == 1 && is_trailing {
                                if syllables.len() == 1 && item.input_count == input_length - 1 {
                                        qualified.push(item.replaced_input(text.clone()));
                                }
                                continue;
                        }
                        if sep_count == 1 {
                                if syllables.len() != 2 || text_parts.len() < 2 {
                                        continue;
                                }
                                let mut is_matched = true;
                                if input_length != 3 && syllables[0] != text_parts[0] {
                                        is_matched = text_parts[0].chars().count() == 1
                                                && !syllables[0].is_empty()
                                                && text_parts[0].chars().next() == syllables[0].chars().next()
                                                && text_parts[1].starts_with(&syllables[1]);
                                }
                                if is_matched {
                                        qualified.push(item.replaced_input(text.clone()));
                                }
                                continue;
                        }
                        if sep_count == 2 && is_trailing {
                                if syllables.len() != 2 || text_parts.len() < 2 || item.input_count != input_length - 2 {
                                        continue;
                                }
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
                                continue;
                        }
                        let has_three_part = ((sep_count == 2 && input_length == 5) || (sep_count == 3 && input_length == 6)) && text_parts.len() == 3;
                        if has_three_part && syllables.len() == 3 {
                                qualified.push(item.replaced_input(text.clone()));
                        }
                }
                qualified
        }
}
