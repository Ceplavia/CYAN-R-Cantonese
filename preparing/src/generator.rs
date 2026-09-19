use crate::database::Database;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::hash::Hash;
use std::io;
use std::path::{Path, PathBuf};

const TABLES: &[&str] = &[
        "CREATE TABLE plain_text_table (id INTEGER PRIMARY KEY AUTOINCREMENT, input TEXT NOT NULL, word TEXT NOT NULL, letter_count INTEGER NOT NULL, spell INTEGER NOT NULL)",
        "CREATE TABLE lexicon_core (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, romanization TEXT NOT NULL, char_count INTEGER NOT NULL, complexity INTEGER NOT NULL, anchors INTEGER NOT NULL, spell INTEGER NOT NULL)",
        "CREATE TABLE stroke_table (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, stroke TEXT NOT NULL, complex INTEGER NOT NULL, code INTEGER NOT NULL)",
        "CREATE TABLE cangjie_table (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, cangjie5 TEXT NOT NULL, c5complex INTEGER NOT NULL, c5code INTEGER NOT NULL, cangjie3 TEXT NOT NULL, c3complex INTEGER NOT NULL, c3code INTEGER NOT NULL)",
        "CREATE TABLE emoji_skin_map (id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT NOT NULL, target TEXT NOT NULL)",
        "CREATE TABLE pinyin_lexicon (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, romanization TEXT NOT NULL, char_count INTEGER NOT NULL, complexity INTEGER NOT NULL, anchors INTEGER NOT NULL, spell INTEGER NOT NULL)",
        "CREATE TABLE structure_table (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, romanization TEXT NOT NULL, char_count INTEGER NOT NULL, complexity INTEGER NOT NULL, spell INTEGER NOT NULL)",
        "CREATE TABLE quick_table (id INTEGER PRIMARY KEY AUTOINCREMENT, word TEXT NOT NULL, quick5 TEXT NOT NULL, q5complex INTEGER NOT NULL, q5code INTEGER NOT NULL, quick3 TEXT NOT NULL, q3complex INTEGER NOT NULL, q3code INTEGER NOT NULL)",
        "CREATE TABLE symbol_table (id INTEGER PRIMARY KEY AUTOINCREMENT, category INTEGER NOT NULL, unicode_version INTEGER NOT NULL, code_point TEXT NOT NULL, cantonese TEXT NOT NULL, romanization TEXT NOT NULL, complexity INTEGER NOT NULL, spell INTEGER NOT NULL)",
        "CREATE TABLE syllable_core_table (alias_code INTEGER PRIMARY KEY, origin_code INTEGER NOT NULL, alias TEXT NOT NULL, origin TEXT NOT NULL)",
        "CREATE TABLE syllable_pinyin_table (code INTEGER PRIMARY KEY, syllable TEXT NOT NULL)",
        "CREATE TABLE variant_abp (source INTEGER PRIMARY KEY, target INTEGER NOT NULL)",
        "CREATE TABLE variant_hk (source INTEGER PRIMARY KEY, target INTEGER NOT NULL)",
        "CREATE TABLE variant_old (source INTEGER PRIMARY KEY, target INTEGER NOT NULL)",
        "CREATE TABLE variant_prc (source INTEGER PRIMARY KEY, target INTEGER NOT NULL)",
        "CREATE TABLE variant_sim (source INTEGER PRIMARY KEY, target INTEGER NOT NULL)",
        "CREATE TABLE variant_tw (source INTEGER PRIMARY KEY, target INTEGER NOT NULL)",
];

const INDEXES: &[&str] = &[
        "CREATE INDEX ix_lexicon_core_anchors ON lexicon_core (anchors, char_count)",
        "CREATE INDEX ix_lexicon_core_spell ON lexicon_core (spell, complexity)",
        "CREATE INDEX ix_lexicon_core_word ON lexicon_core (word)",
        "CREATE INDEX ix_structure_spell ON structure_table (spell, complexity)",
        "CREATE INDEX ix_pinyin_anchors ON pinyin_lexicon (anchors, char_count)",
        "CREATE INDEX ix_pinyin_spell ON pinyin_lexicon (spell, complexity)",
        "CREATE INDEX ix_cangjie_cangjie5 ON cangjie_table (cangjie5, c5complex)",
        "CREATE INDEX ix_cangjie_c5code ON cangjie_table (c5code)",
        "CREATE INDEX ix_cangjie_cangjie3 ON cangjie_table (cangjie3, c3complex)",
        "CREATE INDEX ix_cangjie_c3code ON cangjie_table (c3code)",
        "CREATE INDEX ix_quick_quick5 ON quick_table (quick5, q5complex)",
        "CREATE INDEX ix_quick_q5code ON quick_table (q5code)",
        "CREATE INDEX ix_quick_quick3 ON quick_table (quick3, q3complex)",
        "CREATE INDEX ix_quick_q3code ON quick_table (q3code)",
        "CREATE INDEX ix_stroke_stroke ON stroke_table (stroke, complex)",
        "CREATE INDEX ix_stroke_code ON stroke_table (code, complex)",
        "CREATE INDEX ix_symbol_spell ON symbol_table (spell, complexity)",
        "CREATE INDEX ix_emoji_skin_map_source ON emoji_skin_map (source)",
        "CREATE INDEX ix_plain_text_spell ON plain_text_table (spell, letter_count)",
];

const VARIANT_TABLES: &[(&str, &str)] = &[
        ("CharacterVariant.AncientBooksPublishing.txt", "variant_abp"),
        ("CharacterVariant.HongKong.txt", "variant_hk"),
        ("CharacterVariant.Inherited.txt", "variant_old"),
        ("CharacterVariant.PRCGeneral.txt", "variant_prc"),
        ("CharacterVariant.Simplified.txt", "variant_sim"),
        ("CharacterVariant.Taiwan.txt", "variant_tw"),
];

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Debug)]
struct LexiconEntry {
        word: String,
        romanization: String,
        char_count: i64,
        complexity: i64,
        anchors: i64,
        spell: i64,
}

#[derive(Debug, Eq, Hash, PartialEq)]
struct CangjieEntry {
        word: String,
        cangjie5: String,
        c5complex: i64,
        c5code: i64,
        cangjie3: String,
        c3complex: i64,
        c3code: i64,
}

#[derive(Debug, Eq, Hash, PartialEq)]
struct QuickEntry {
        word: String,
        quick5: String,
        q5complex: i64,
        q5code: i64,
        quick3: String,
        q3complex: i64,
        q3code: i64,
}

#[derive(Debug, Eq, Hash, PartialEq)]
struct StrokeEntry {
        word: String,
        stroke: String,
        complex: i64,
        code: i64,
}

pub fn generate(resource_directory: &Path, output_path: &Path) -> Result<()> {
        let parent = output_path.parent().ok_or("output path has no parent directory")?;
        fs::create_dir_all(parent)?;
        let temporary_path = temporary_path(output_path)?;
        if temporary_path.exists() {
                fs::remove_file(&temporary_path)?;
        }
        let temporary_file = TemporaryFile::new(temporary_path.clone());

        {
                let database = Database::open(&temporary_path)?;
                database.execute("PRAGMA page_size = 16384")?;
                database.execute("PRAGMA journal_mode = DELETE")?;
                database.execute("BEGIN IMMEDIATE")?;
                for command in TABLES {
                        database.execute(command)?;
                }

                let jyutping_lines = source_lines(resource_directory, "jyutping.txt")?;
                let jyutping = convert_lexicon(&jyutping_lines, "jyutping.txt")?;
                insert_plain_text(&database, resource_directory)?;
                insert_lexicon(&database, "lexicon_core", &jyutping)?;

                let cangjie5 = source_map(resource_directory, "cangjie5.txt")?;
                let cangjie3 = source_map(resource_directory, "cangjie3.txt")?;
                insert_strokes(&database, resource_directory, &jyutping_lines)?;
                insert_cangjie(&database, &jyutping_lines, &cangjie5, &cangjie3)?;
                insert_skin_tone_map(&database, resource_directory)?;

                let pinyin_lines = source_lines(resource_directory, "pinyin.txt")?;
                insert_lexicon(&database, "pinyin_lexicon", &convert_lexicon(&pinyin_lines, "pinyin.txt")?)?;
                insert_structure(&database, resource_directory)?;
                insert_quick(&database, &jyutping_lines, &cangjie5, &cangjie3)?;
                insert_symbols(&database, resource_directory)?;
                insert_syllables(&database, resource_directory)?;
                insert_variants(&database, resource_directory)?;

                database.execute("COMMIT")?;
                for command in INDEXES {
                        database.execute(command)?;
                }
                database.execute("VACUUM")?;
                database.execute("ANALYZE")?;
        }

        replace_file(&temporary_path, output_path)?;
        temporary_file.keep();
        Ok(())
}

fn insert_plain_text(database: &Database, resource_directory: &Path) -> Result<()> {
        let mut seen_lines = HashSet::new();
        let mut seen_entries = HashSet::new();
        let mut statement = database.prepare("INSERT INTO plain_text_table (input, word, letter_count, spell) VALUES (?, ?, ?, ?)")?;
        for line in source_lines(resource_directory, "text.txt")? {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || !seen_lines.insert(line.to_owned()) {
                        continue;
                }
                let parts = fields(line);
                if parts.len() < 2 {
                        return invalid_data(format!("bad line format in text.txt: {line}"));
                }
                let input = parts[0];
                let word = parts[1];
                if !seen_entries.insert((input.to_owned(), word.to_owned())) {
                        continue;
                }
                statement.bind_text(1, input)?;
                statement.bind_text(2, word)?;
                statement.bind_i64(3, character_count(input))?;
                statement.bind_i64(4, serial_code(input))?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_lexicon(database: &Database, table: &str, entries: &[LexiconEntry]) -> Result<()> {
        let sql = format!("INSERT INTO {table} (word, romanization, char_count, complexity, anchors, spell) VALUES (?, ?, ?, ?, ?, ?)");
        let mut statement = database.prepare(&sql)?;
        for entry in entries {
                statement.bind_text(1, &entry.word)?;
                statement.bind_text(2, &entry.romanization)?;
                statement.bind_i64(3, entry.char_count)?;
                statement.bind_i64(4, entry.complexity)?;
                statement.bind_i64(5, entry.anchors)?;
                statement.bind_i64(6, entry.spell)?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_structure(database: &Database, resource_directory: &Path) -> Result<()> {
        let mut transformed = Vec::new();
        let mut seen = HashSet::new();
        for line in source_lines(resource_directory, "structure.txt")? {
                let parts = fields(&line);
                if parts.len() < 3 {
                        return invalid_data(format!("bad line format in structure.txt: {line}"));
                }
                let value = format!("{}\t{}", parts[0], parts[2]);
                if seen.insert(value.clone()) {
                        transformed.push(value);
                }
        }
        let entries = convert_lexicon(&transformed, "structure.txt")?;
        let mut statement = database.prepare("INSERT INTO structure_table (word, romanization, char_count, complexity, spell) VALUES (?, ?, ?, ?, ?)")?;
        for entry in entries {
                statement.bind_text(1, &entry.word)?;
                statement.bind_text(2, &entry.romanization)?;
                statement.bind_i64(3, entry.char_count)?;
                statement.bind_i64(4, entry.complexity)?;
                statement.bind_i64(5, entry.spell)?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_cangjie(database: &Database, jyutping_lines: &[String], cangjie5: &HashMap<String, Vec<String>>, cangjie3: &HashMap<String, Vec<String>>) -> Result<()> {
        let characters = jyutping_words(jyutping_lines, true)?;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for word in characters {
                let matches5 = cangjie5.get(&word).map(Vec::as_slice).unwrap_or_default();
                let matches3 = cangjie3.get(&word).map(Vec::as_slice).unwrap_or_default();
                for index in 0..matches5.len().max(matches3.len()) {
                        let code5 = matches5.get(index).map(String::as_str).unwrap_or("X");
                        let code3 = matches3.get(index).map(String::as_str).unwrap_or("X");
                        let entry = CangjieEntry {
                                word: word.clone(),
                                cangjie5: code5.to_owned(),
                                c5complex: character_count(code5),
                                c5code: serial_code(code5),
                                cangjie3: code3.to_owned(),
                                c3complex: character_count(code3),
                                c3code: serial_code(code3),
                        };
                        if seen.insert(entry.clone_key()) {
                                entries.push(entry);
                        }
                }
        }
        let mut statement = database.prepare("INSERT INTO cangjie_table (word, cangjie5, c5complex, c5code, cangjie3, c3complex, c3code) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
        for entry in entries {
                statement.bind_text(1, &entry.word)?;
                statement.bind_text(2, &entry.cangjie5)?;
                statement.bind_i64(3, entry.c5complex)?;
                statement.bind_i64(4, entry.c5code)?;
                statement.bind_text(5, &entry.cangjie3)?;
                statement.bind_i64(6, entry.c3complex)?;
                statement.bind_i64(7, entry.c3code)?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_quick(database: &Database, jyutping_lines: &[String], cangjie5: &HashMap<String, Vec<String>>, cangjie3: &HashMap<String, Vec<String>>) -> Result<()> {
        let words = jyutping_words(jyutping_lines, false)?;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for word in words {
                if word.chars().count() == 1 {
                        let matches5 = cangjie5.get(&word).map(Vec::as_slice).unwrap_or_default();
                        let matches3 = cangjie3.get(&word).map(Vec::as_slice).unwrap_or_default();
                        for index in 0..matches5.len().max(matches3.len()) {
                                let quick5 = quick_code(matches5.get(index).map(String::as_str).unwrap_or("X"));
                                let quick3 = quick_code(matches3.get(index).map(String::as_str).unwrap_or("X"));
                                push_quick_entry(&mut entries, &mut seen, &word, quick5, quick3);
                        }
                } else {
                        let quick5 = word.chars().map(|character| first_quick_code(cangjie5, character)).collect::<Vec<_>>().concat();
                        let quick3 = word.chars().map(|character| first_quick_code(cangjie3, character)).collect::<Vec<_>>().concat();
                        push_quick_entry(&mut entries, &mut seen, &word, quick5, quick3);
                }
        }
        let mut statement = database.prepare("INSERT INTO quick_table (word, quick5, q5complex, q5code, quick3, q3complex, q3code) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
        for entry in entries {
                statement.bind_text(1, &entry.word)?;
                statement.bind_text(2, &entry.quick5)?;
                statement.bind_i64(3, entry.q5complex)?;
                statement.bind_i64(4, entry.q5code)?;
                statement.bind_text(5, &entry.quick3)?;
                statement.bind_i64(6, entry.q3complex)?;
                statement.bind_i64(7, entry.q3code)?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_strokes(database: &Database, resource_directory: &Path, jyutping_lines: &[String]) -> Result<()> {
        let stroke_map = source_map(resource_directory, "stroke.txt")?;
        let characters = jyutping_words(jyutping_lines, true)?;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for word in characters {
                let Some(matches) = stroke_map.get(&word) else {
                        continue;
                };
                for matched in matches {
                        let mut codes = Vec::new();
                        for character in matched.chars() {
                                let code = match character {
                                        'w' | 'h' | '1' => 1,
                                        's' | '2' => 2,
                                        'a' | 'p' | '3' => 3,
                                        'd' | 'n' | '4' => 4,
                                        'z' | '5' => 5,
                                        _ => return invalid_data(format!("bad stroke format: {word} = {matched}")),
                                };
                                codes.push(code);
                        }
                        if codes.len() > 30 {
                                continue;
                        }
                        let stroke = codes.iter().map(i64::to_string).collect::<String>();
                        if seen.insert((word.clone(), stroke.clone())) {
                                entries.push(StrokeEntry { word: word.clone(), stroke, complex: i64::try_from(codes.len())?, code: decimal_code(codes) });
                        }
                }
        }
        let mut statement = database.prepare("INSERT INTO stroke_table (word, stroke, complex, code) VALUES (?, ?, ?, ?)")?;
        for entry in entries {
                statement.bind_text(1, &entry.word)?;
                statement.bind_text(2, &entry.stroke)?;
                statement.bind_i64(3, entry.complex)?;
                statement.bind_i64(4, entry.code)?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_symbols(database: &Database, resource_directory: &Path) -> Result<()> {
        let mut statement =
                database.prepare("INSERT INTO symbol_table (category, unicode_version, code_point, cantonese, romanization, complexity, spell) VALUES (?, ?, ?, ?, ?, ?, ?)")?;
        for line in source_lines(resource_directory, "symbol.txt")? {
                let parts = fields(&line);
                if parts.len() != 5 {
                        continue;
                }
                let romanization = parts[4];
                let complexity = decimal_code(romanization.split_whitespace().map(|phone| character_count(phone) - 1));
                statement.bind_i64(1, parts[0].parse()?)?;
                statement.bind_i64(2, parts[1].parse()?)?;
                statement.bind_text(3, parts[2])?;
                statement.bind_text(4, parts[3])?;
                statement.bind_text(5, romanization)?;
                statement.bind_i64(6, complexity)?;
                statement.bind_i64(7, serial_code(romanization))?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_skin_tone_map(database: &Database, resource_directory: &Path) -> Result<()> {
        let mut statement = database.prepare("INSERT INTO emoji_skin_map (source, target) VALUES (?, ?)")?;
        for line in source_lines(resource_directory, "skin-tone-map.txt")? {
                let parts = fields(&line);
                if parts.len() != 2 {
                        continue;
                }
                statement.bind_text(1, parts[0])?;
                statement.bind_text(2, parts[1])?;
                statement.insert()?;
        }
        Ok(())
}

fn insert_syllables(database: &Database, resource_directory: &Path) -> Result<()> {
        let mut core_statement = database.prepare("INSERT INTO syllable_core_table (alias_code, origin_code, alias, origin) VALUES (?, ?, ?, ?)")?;
        for line in source_lines(resource_directory, "syllable-core.txt")? {
                let parts = fields(&line);
                if parts.len() != 2 {
                        return invalid_data(format!("bad line format in syllable-core.txt: {line}"));
                }
                core_statement.bind_i64(1, serial_code(parts[0]))?;
                core_statement.bind_i64(2, serial_code(parts[1]))?;
                core_statement.bind_text(3, parts[0])?;
                core_statement.bind_text(4, parts[1])?;
                core_statement.insert()?;
        }

        let mut pinyin_statement = database.prepare("INSERT INTO syllable_pinyin_table (code, syllable) VALUES (?, ?)")?;
        for syllable in source_lines(resource_directory, "syllable-pinyin.txt")? {
                let syllable = syllable.trim();
                if syllable.is_empty() {
                        continue;
                }
                pinyin_statement.bind_i64(1, serial_code(syllable))?;
                pinyin_statement.bind_text(2, syllable)?;
                pinyin_statement.insert()?;
        }
        Ok(())
}

fn insert_variants(database: &Database, resource_directory: &Path) -> Result<()> {
        for (file_name, table_name) in VARIANT_TABLES {
                let variants = character_variants(resource_directory, file_name)?;
                let sql = format!("INSERT INTO {table_name} (source, target) VALUES (?, ?)");
                let mut statement = database.prepare(&sql)?;
                for (source, target) in variants {
                        statement.bind_i64(1, i64::from(source))?;
                        statement.bind_i64(2, i64::from(target))?;
                        statement.insert()?;
                }
        }
        Ok(())
}

fn convert_lexicon(lines: &[String], file_name: &str) -> Result<Vec<LexiconEntry>> {
        lines.iter()
                .map(|line| {
                        let parts = fields(line.trim());
                        if parts.len() != 2 {
                                return invalid_data(format!("bad line format in {file_name}: {line}"));
                        }
                        let word = parts[0];
                        let romanization = parts[1];
                        let phones = romanization.chars().filter(|character| !character.is_ascii_digit()).collect::<String>();
                        let complexity = decimal_code(phones.split_whitespace().map(character_count));
                        let anchors = phones.split_whitespace().filter_map(|phone| phone.chars().next()).collect::<String>();
                        Ok(LexiconEntry {
                                word: word.to_owned(),
                                romanization: romanization.to_owned(),
                                char_count: character_count(word),
                                complexity,
                                anchors: serial_code(&anchors),
                                spell: serial_code(romanization),
                        })
                })
                .collect()
}

fn character_variants(resource_directory: &Path, file_name: &str) -> Result<Vec<(u32, u32)>> {
        let mut unique_lines = HashSet::new();
        let mut variants = BTreeMap::new();
        for line in source_lines(resource_directory, file_name)? {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || !unique_lines.insert(line.to_owned()) {
                        continue;
                }
                let parts = fields(line);
                if parts.len() < 2 {
                        return invalid_data(format!("bad line format in {file_name}: {line}"));
                }
                let mut left_characters = parts[0].chars();
                let Some(left) = left_characters.next() else {
                        return invalid_data(format!("bad source character in {file_name}: {line}"));
                };
                if left_characters.next().is_some() {
                        return invalid_data(format!("bad source character in {file_name}: {line}"));
                }
                let Some(right) = parts[1].split_whitespace().next().and_then(|value| value.chars().next()) else {
                        return invalid_data(format!("bad target character in {file_name}: {line}"));
                };
                let source = u32::from(left);
                let target = u32::from(right);
                if !is_generic_cjkv(source) || !is_generic_cjkv(target) {
                        eprintln!("Skipping non-CJKV character variant in {file_name}: {line}");
                        continue;
                }
                if source == target {
                        if parts[1].chars().count() == 1 {
                                return invalid_data(format!("self-mapping character variant in {file_name}: {line}"));
                        }
                        continue;
                }
                variants.entry(source).or_insert(target);
        }
        Ok(variants.into_iter().collect())
}

fn source_map(resource_directory: &Path, file_name: &str) -> Result<HashMap<String, Vec<String>>> {
        let mut map = HashMap::<String, Vec<String>>::new();
        for line in source_lines(resource_directory, file_name)? {
                let parts = fields(&line);
                if parts.len() != 2 {
                        continue;
                }
                map.entry(parts[0].to_owned()).or_default().push(parts[1].to_owned());
        }
        Ok(map)
}

fn jyutping_words(lines: &[String], single_character_only: bool) -> Result<Vec<String>> {
        let mut words = Vec::new();
        let mut seen = HashSet::new();
        for line in lines {
                let parts = fields(line);
                let Some(word) = parts.first() else {
                        continue;
                };
                let word = word.trim();
                if single_character_only && word.chars().count() != 1 {
                        continue;
                }
                if seen.insert(word.to_owned()) {
                        words.push(word.to_owned());
                }
        }
        Ok(words)
}

fn push_quick_entry(entries: &mut Vec<QuickEntry>, seen: &mut HashSet<QuickEntry>, word: &str, quick5: String, quick3: String) {
        let entry = QuickEntry {
                word: word.to_owned(),
                q5complex: character_count(&quick5),
                q5code: serial_code(&quick5),
                q3complex: character_count(&quick3),
                q3code: serial_code(&quick3),
                quick5,
                quick3,
        };
        if seen.insert(entry.clone_key()) {
                entries.push(entry);
        }
}

fn first_quick_code(map: &HashMap<String, Vec<String>>, character: char) -> String {
        map.get(&character.to_string()).and_then(|matches| matches.first()).map(|value| quick_code(value)).unwrap_or_else(|| "X".to_owned())
}

fn quick_code(cangjie: &str) -> String {
        let characters = cangjie.chars().collect::<Vec<_>>();
        if characters.len() > 2 { format!("{}{}", characters[0], characters[characters.len() - 1]) } else { cangjie.to_owned() }
}

fn source_lines(resource_directory: &Path, file_name: &str) -> Result<Vec<String>> {
        let path = resource_directory.join(file_name);
        let content = fs::read_to_string(&path).map_err(|error| io::Error::new(error.kind(), format!("failed to read {}: {error}", path.display())))?;
        Ok(content.trim().lines().map(|line| line.trim_end_matches('\r').to_owned()).collect())
}

fn fields(line: &str) -> Vec<&str> {
        line.split('\t').map(str::trim).filter(|part| !part.is_empty()).collect()
}

fn serial_code(text: &str) -> i64 {
        text.chars().filter_map(letter_code).fold(0_i64, |value, code| value.wrapping_mul(100).wrapping_add(code))
}

fn letter_code(character: char) -> Option<i64> {
        character.is_ascii_lowercase().then(|| i64::from(u32::from(character) - u32::from('a') + 20))
}

fn decimal_code(values: impl IntoIterator<Item = i64>) -> i64 {
        values.into_iter().fold(0_i64, |value, digit| value.wrapping_mul(10).wrapping_add(digit))
}

fn character_count(text: &str) -> i64 {
        i64::try_from(text.chars().count()).expect("text length exceeds i64")
}

fn is_generic_cjkv(code: u32) -> bool {
        matches!(
                code,
                0x4E00..=0x9FFF
                        | 0x3400..=0x4DBF
                        | 0x20000..=0x2A6DF
                        | 0x2A700..=0x2B73F
                        | 0x2B740..=0x2B81F
                        | 0x2B820..=0x2CEAF
                        | 0x2CEB0..=0x2EBEF
                        | 0x30000..=0x3134F
                        | 0x31350..=0x323AF
                        | 0x2EBF0..=0x2EE5F
                        | 0x323B0..=0x33479
                        | 0x3007
                        | 0x2E80..=0x2E99
                        | 0x2E9B..=0x2EF3
                        | 0x2F00..=0x2FD5
                        | 0xF900..=0xFA6D
                        | 0xFA70..=0xFAD9
                        | 0x2F800..=0x2FA1D
        )
}

fn temporary_path(output_path: &Path) -> Result<PathBuf> {
        let file_name = output_path.file_name().and_then(|name| name.to_str()).ok_or("output filename is not valid UTF-8")?;
        Ok(output_path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id())))
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
        fs::rename(source, destination)?;
        Ok(())
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
        use std::os::windows::ffi::OsStrExt;

        const MOVE_FILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVE_FILE_WRITE_THROUGH: u32 = 0x8;

        #[link(name = "kernel32")]
        unsafe extern "system" {
                fn MoveFileExW(existing_filename: *const u16, new_filename: *const u16, flags: u32) -> i32;
        }

        let source_wide = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let destination_wide = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let result = unsafe { MoveFileExW(source_wide.as_ptr(), destination_wide.as_ptr(), MOVE_FILE_REPLACE_EXISTING | MOVE_FILE_WRITE_THROUGH) };
        if result == 0 {
                return Err(io::Error::last_os_error().into());
        }
        Ok(())
}

fn invalid_data<T>(message: String) -> Result<T> {
        Err(io::Error::new(io::ErrorKind::InvalidData, message).into())
}

trait CloneKey: Sized + Eq + Hash {
        fn clone_key(&self) -> Self;
}

impl CloneKey for CangjieEntry {
        fn clone_key(&self) -> Self {
                Self {
                        word: self.word.clone(),
                        cangjie5: self.cangjie5.clone(),
                        c5complex: self.c5complex,
                        c5code: self.c5code,
                        cangjie3: self.cangjie3.clone(),
                        c3complex: self.c3complex,
                        c3code: self.c3code,
                }
        }
}

impl CloneKey for QuickEntry {
        fn clone_key(&self) -> Self {
                Self {
                        word: self.word.clone(),
                        quick5: self.quick5.clone(),
                        q5complex: self.q5complex,
                        q5code: self.q5code,
                        quick3: self.quick3.clone(),
                        q3complex: self.q3complex,
                        q3code: self.q3code,
                }
        }
}

struct TemporaryFile {
        path: PathBuf,
        keep: std::cell::Cell<bool>,
}

impl TemporaryFile {
        fn new(path: PathBuf) -> Self {
                Self { path, keep: std::cell::Cell::new(false) }
        }

        fn keep(&self) {
                self.keep.set(true);
        }
}

impl Drop for TemporaryFile {
        fn drop(&mut self) {
                if !self.keep.get() {
                        let _ = fs::remove_file(&self.path);
                }
        }
}

#[cfg(test)]
mod tests {
        use super::*;

        #[test]
        fn serial_codes_match_swift_encoding() {
                assert_eq!(serial_code("abc"), 202122);
                assert_eq!(serial_code("a-b C!"), 2021);
                assert_eq!(serial_code("activitykit"), -6749006824106374921);
        }

        #[test]
        fn decimal_codes_wrap_at_64_bits() {
                assert_eq!(decimal_code([2, 3, 4]), 234);
                assert_eq!(decimal_code([]), 0);
        }

        #[test]
        fn quick_codes_keep_short_codes_and_abbreviate_long_ones() {
                assert_eq!(quick_code("a"), "a");
                assert_eq!(quick_code("ab"), "ab");
                assert_eq!(quick_code("abcde"), "ae");
        }
}
