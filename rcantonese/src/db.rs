// SQLite access layer over the system winsqlite3.dll — mirrors ImeDatabase.
#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;
use std::ptr;

use crate::globals;
use crate::variants::CharacterStandard;

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READONLY: c_int = 0x00000001;
const SQLITE_OPEN_READWRITE: c_int = 0x00000002;
const SQLITE_OPEN_CREATE: c_int = 0x00000004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x00010000;
const SQLITE_TRANSIENT: isize = -1; // (sqlite3_destructor_type)-1

#[repr(C)]
struct Sqlite3 {
        _private: [u8; 0],
}

#[repr(C)]
struct Sqlite3Statement {
        _private: [u8; 0],
}

#[link(name = "winsqlite3")]
unsafe extern "C" {
        fn sqlite3_open_v2(filename: *const c_char, database: *mut *mut Sqlite3, flags: c_int, vfs: *const c_char) -> c_int;
        fn sqlite3_close_v2(database: *mut Sqlite3) -> c_int;
        fn sqlite3_errmsg(database: *mut Sqlite3) -> *const c_char;
        fn sqlite3_prepare16_v2(database: *mut Sqlite3, sql: *const u16, byte_count: c_int, statement: *mut *mut Sqlite3Statement, tail: *mut *const u16) -> c_int;
        fn sqlite3_step(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_finalize(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_reset(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_clear_bindings(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_bind_int64(statement: *mut Sqlite3Statement, index: c_int, value: i64) -> c_int;
        fn sqlite3_bind_text16(statement: *mut Sqlite3Statement, index: c_int, value: *const u16, byte_count: c_int, destructor: isize) -> c_int;
        fn sqlite3_column_int64(statement: *mut Sqlite3Statement, column: c_int) -> i64;
        fn sqlite3_column_int(statement: *mut Sqlite3Statement, column: c_int) -> c_int;
        fn sqlite3_column_text16(statement: *mut Sqlite3Statement, column: c_int) -> *const u16;
        fn sqlite3_column_bytes16(statement: *mut Sqlite3Statement, column: c_int) -> c_int;
        fn sqlite3_busy_timeout(database: *mut Sqlite3, ms: c_int) -> c_int;
        fn sqlite3_exec(
                database: *mut Sqlite3,
                sql: *const c_char,
                callback: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
                context: *mut c_void,
                error_message: *mut *mut c_char,
        ) -> c_int;
}

fn error_message(database: *mut Sqlite3) -> String {
        if database.is_null() {
                return "unknown SQLite error".to_owned();
        }
        let message = unsafe { sqlite3_errmsg(database) };
        if message.is_null() {
                return "unknown SQLite error".to_owned();
        }
        unsafe { CStr::from_ptr(message) }.to_string_lossy().into_owned()
}

pub struct Statement {
        database: *mut Sqlite3,
        raw: *mut Sqlite3Statement,
}

impl Statement {
        pub fn is_valid(&self) -> bool {
                !self.raw.is_null()
        }
        pub fn bind_i64(&self, index: c_int, value: i64) {
                unsafe {
                        sqlite3_bind_int64(self.raw, index, value);
                }
        }
        pub fn bind_text(&self, index: c_int, value: &str) {
                let wide: Vec<u16> = value.encode_utf16().collect();
                let byte_count = (wide.len() * 2) as c_int;
                unsafe {
                        sqlite3_bind_text16(self.raw, index, wide.as_ptr(), byte_count, SQLITE_TRANSIENT);
                }
        }
        pub fn reset(&self) {
                unsafe {
                        sqlite3_reset(self.raw);
                        sqlite3_clear_bindings(self.raw);
                }
        }
        /// Step once; returns true if a row is available.
        pub fn step(&self) -> bool {
                unsafe { sqlite3_step(self.raw) == SQLITE_ROW }
        }
        pub fn column_i64(&self, column: c_int) -> i64 {
                unsafe { sqlite3_column_int64(self.raw, column) }
        }
        pub fn column_int(&self, column: c_int) -> c_int {
                unsafe { sqlite3_column_int(self.raw, column) }
        }
        pub fn column_text(&self, column: c_int) -> String {
                unsafe {
                        let text = sqlite3_column_text16(self.raw, column);
                        if text.is_null() {
                                return String::new();
                        }
                        let byte_count = sqlite3_column_bytes16(self.raw, column);
                        let len = (byte_count as usize) / 2;
                        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
                }
        }
        pub fn error_message(&self) -> String {
                error_message(self.database)
        }
}

impl Drop for Statement {
        fn drop(&mut self) {
                unsafe {
                        sqlite3_finalize(self.raw);
                }
        }
}

// ---------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------

#[derive(Debug)]
pub struct LexiconRow {
        pub row_id: i64,
        pub word: String,
        pub romanization: String,
}

#[derive(Debug)]
pub struct SyllableRow {
        pub alias_code: i64,
        pub origin_code: i64,
        pub alias: String,
        pub origin: String,
}

#[derive(Debug)]
pub struct ShapeRow {
        pub row_id: i64,
        pub word: String,
        pub complex: i64,
}

#[derive(Debug)]
pub struct StructureRow {
        pub word: String,
        pub romanization: String,
}

#[derive(Debug)]
pub struct SymbolRow {
        pub row_id: i64,
        pub category: i32,
        pub unicode_version: i32,
        pub code_point: String,
        pub cantonese: String,
        pub romanization: String,
}

#[derive(Debug)]
pub struct PinyinSyllableRow {
        pub code: i64,
        pub syllable: String,
}

#[derive(Debug)]
pub struct MemoryRow {
        pub word: String,
        pub romanization: String,
        pub frequency: i64,
        pub latest: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CangjieVariant {
        Cangjie5,
        Cangjie3,
        Quick5,
        Quick3,
}

// ---------------------------------------------------------------------
// ImeDatabase
// ---------------------------------------------------------------------

pub struct ImeDatabase {
        raw: *mut Sqlite3,
}

impl ImeDatabase {
        pub fn open(path: &Path) -> Option<Self> {
                let path_text = path.to_str()?;
                let c_path = CString::new(path_text).ok()?;
                let mut raw: *mut Sqlite3 = ptr::null_mut();
                let result = unsafe { sqlite3_open_v2(c_path.as_ptr(), &mut raw, SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX, ptr::null()) };
                if result != SQLITE_OK {
                        globals::log_error(&format!("ImeDatabase open failed ({result}): {}", error_message(raw)));
                        if !raw.is_null() {
                                unsafe {
                                        sqlite3_close_v2(raw);
                                }
                        }
                        return None;
                }
                Some(Self { raw })
        }

        pub fn open_default() -> Option<Self> {
                Self::open(&globals::default_database_path())
        }

        /// Open a read-write user database (for input memory).
        pub fn open_readwrite(path: &Path) -> Option<Self> {
                let path_text = path.to_str()?;
                let c_path = CString::new(path_text).ok()?;
                let mut raw: *mut Sqlite3 = ptr::null_mut();
                let result = unsafe {
                        sqlite3_open_v2(c_path.as_ptr(), &mut raw, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX, ptr::null())
                };
                if result != SQLITE_OK {
                        if !raw.is_null() {
                                unsafe {
                                        sqlite3_close_v2(raw);
                                }
                        }
                        return None;
                }
                unsafe {
                        sqlite3_busy_timeout(raw, 250);
                }
                Some(Self { raw })
        }

        pub fn execute(&self, sql: &str) -> bool {
                let Ok(command) = CString::new(sql) else { return false };
                unsafe { sqlite3_exec(self.raw, command.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) == SQLITE_OK }
        }

        pub fn prepare(&self, sql: &str) -> Option<Statement> {
                if self.raw.is_null() {
                        return None;
                }
                // NUL-terminated — byte_count -1 makes sqlite read to NUL.
                let wide: Vec<u16> = sql.encode_utf16().chain(Some(0)).collect();
                let mut raw: *mut Sqlite3Statement = ptr::null_mut();
                let result = unsafe { sqlite3_prepare16_v2(self.raw, wide.as_ptr(), -1, &mut raw, ptr::null_mut()) };
                if result != SQLITE_OK || raw.is_null() {
                        globals::log_error(&format!("ImeDatabase prepare failed ({result}): {}", error_message(self.raw)));
                        return None;
                }
                Some(Statement { database: self.raw, raw })
        }

        fn read_lexicon_rows(&self, statement: &Statement) -> Vec<LexiconRow> {
                let mut rows = Vec::new();
                while statement.step() {
                        rows.push(LexiconRow {
                                row_id: statement.column_i64(0),
                                word: statement.column_text(1),
                                romanization: statement.column_text(2),
                        });
                }
                rows
        }

        fn read_shape_rows(&self, statement: &Statement) -> Vec<ShapeRow> {
                let mut rows = Vec::new();
                while statement.step() {
                        rows.push(ShapeRow {
                                row_id: statement.column_i64(0),
                                word: statement.column_text(1),
                                complex: statement.column_i64(2),
                        });
                }
                rows
        }

        fn read_symbol_rows(&self, statement: &Statement) -> Vec<SymbolRow> {
                let mut rows = Vec::new();
                while statement.step() {
                        rows.push(SymbolRow {
                                row_id: statement.column_i64(0),
                                category: statement.column_int(1),
                                unicode_version: statement.column_int(2),
                                code_point: statement.column_text(3),
                                cantonese: statement.column_text(4),
                                romanization: statement.column_text(5),
                        });
                }
                rows
        }

        fn normalize_limit(limit: i32) -> i64 {
                i64::from(if limit <= 0 { -1 } else { limit })
        }

        // -- lexicon ----------------------------------------------------

        pub fn query_lexicon_by_anchors(&self, anchors: i64, char_count: usize, limit: i32) -> Vec<LexiconRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, word, romanization FROM lexicon_core WHERE anchors = ? AND char_count = ? ORDER BY rowid LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, anchors);
                statement.bind_i64(2, char_count as i64);
                statement.bind_i64(3, Self::normalize_limit(limit));
                self.read_lexicon_rows(&statement)
        }

        pub fn query_lexicon_by_spell(&self, spell: i64, complexity: i64, limit: i32) -> Vec<LexiconRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, word, romanization FROM lexicon_core WHERE spell = ? AND complexity = ? ORDER BY rowid LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, spell);
                statement.bind_i64(2, complexity);
                statement.bind_i64(3, Self::normalize_limit(limit));
                self.read_lexicon_rows(&statement)
        }

        pub fn query_syllables(&self) -> Vec<SyllableRow> {
                let Some(statement) = self.prepare("SELECT alias_code, origin_code, alias, origin FROM syllable_core_table;") else {
                        return Vec::new();
                };
                let mut rows = Vec::new();
                while statement.step() {
                        rows.push(SyllableRow {
                                alias_code: statement.column_i64(0),
                                origin_code: statement.column_i64(1),
                                alias: statement.column_text(2),
                                origin: statement.column_text(3),
                        });
                }
                rows
        }

        // -- pinyin -----------------------------------------------------

        pub fn query_pinyin_by_spell(&self, spell: i64, complexity: i64, limit: i32) -> Vec<LexiconRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, word, romanization FROM pinyin_lexicon WHERE spell = ? AND complexity = ? ORDER BY rowid LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, spell);
                statement.bind_i64(2, complexity);
                statement.bind_i64(3, Self::normalize_limit(limit));
                self.read_lexicon_rows(&statement)
        }

        pub fn query_pinyin_by_anchors(&self, anchors: i64, char_count: usize, limit: i32) -> Vec<LexiconRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, word, romanization FROM pinyin_lexicon WHERE anchors = ? AND char_count = ? ORDER BY rowid LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, anchors);
                statement.bind_i64(2, char_count as i64);
                statement.bind_i64(3, Self::normalize_limit(limit));
                self.read_lexicon_rows(&statement)
        }

        pub fn query_pinyin_syllables(&self) -> Vec<PinyinSyllableRow> {
                let Some(statement) = self.prepare("SELECT code, syllable FROM syllable_pinyin_table ORDER BY code;") else {
                        return Vec::new();
                };
                let mut rows = Vec::new();
                while statement.step() {
                        rows.push(PinyinSyllableRow {
                                code: statement.column_i64(0),
                                syllable: statement.column_text(1),
                        });
                }
                rows
        }

        // -- cangjie / quick ----------------------------------------------

        fn cangjie_columns(variant: CangjieVariant) -> Option<(&'static str, &'static str)> {
                const C5_EXACT: &str = "SELECT rowid, word, c5complex FROM cangjie_table WHERE c5code = ? ORDER BY rowid;";
                const C5_PREFIX: &str =
                        "SELECT rowid, word, c5complex FROM cangjie_table WHERE cangjie5 GLOB ? ORDER BY c5complex ASC, rowid ASC LIMIT ?;";
                const C3_EXACT: &str = "SELECT rowid, word, c3complex FROM cangjie_table WHERE c3code = ? ORDER BY rowid;";
                const C3_PREFIX: &str =
                        "SELECT rowid, word, c3complex FROM cangjie_table WHERE cangjie3 GLOB ? ORDER BY c3complex ASC, rowid ASC LIMIT ?;";
                const Q5_EXACT: &str = "SELECT rowid, word, q5complex FROM quick_table WHERE q5code = ? ORDER BY rowid;";
                const Q5_PREFIX: &str =
                        "SELECT rowid, word, q5complex FROM quick_table WHERE quick5 GLOB ? ORDER BY q5complex ASC, rowid ASC LIMIT ?;";
                const Q3_EXACT: &str = "SELECT rowid, word, q3complex FROM quick_table WHERE q3code = ? ORDER BY rowid;";
                const Q3_PREFIX: &str =
                        "SELECT rowid, word, q3complex FROM quick_table WHERE quick3 GLOB ? ORDER BY q3complex ASC, rowid ASC LIMIT ?;";
                match variant {
                        CangjieVariant::Cangjie5 => Some((C5_EXACT, C5_PREFIX)),
                        CangjieVariant::Cangjie3 => Some((C3_EXACT, C3_PREFIX)),
                        CangjieVariant::Quick5 => Some((Q5_EXACT, Q5_PREFIX)),
                        CangjieVariant::Quick3 => Some((Q3_EXACT, Q3_PREFIX)),
                }
        }

        pub fn query_cangjie_by_exact_code(&self, variant: CangjieVariant, code: i64) -> Vec<ShapeRow> {
                if matches!(variant, CangjieVariant::Quick5 | CangjieVariant::Quick3) {
                        return self.query_quick_by_exact_code(variant, code);
                }
                let Some((exact_sql, _)) = Self::cangjie_columns(variant) else { return Vec::new() };
                let Some(statement) = self.prepare(exact_sql) else { return Vec::new() };
                statement.bind_i64(1, code);
                self.read_shape_rows(&statement)
        }

        pub fn query_cangjie_by_prefix(&self, variant: CangjieVariant, prefix: &str, limit: i32) -> Vec<ShapeRow> {
                if matches!(variant, CangjieVariant::Quick5 | CangjieVariant::Quick3) {
                        return self.query_quick_by_prefix(variant, prefix, limit);
                }
                let Some((_, prefix_sql)) = Self::cangjie_columns(variant) else { return Vec::new() };
                let Some(statement) = self.prepare(prefix_sql) else { return Vec::new() };
                statement.bind_text(1, &format!("{prefix}*"));
                statement.bind_i64(2, Self::normalize_limit(limit));
                self.read_shape_rows(&statement)
        }

        pub fn query_quick_by_exact_code(&self, variant: CangjieVariant, code: i64) -> Vec<ShapeRow> {
                let sql = match variant {
                        CangjieVariant::Quick5 => "SELECT rowid, word, q5complex FROM quick_table WHERE q5code = ? ORDER BY rowid;",
                        CangjieVariant::Quick3 => "SELECT rowid, word, q3complex FROM quick_table WHERE q3code = ? ORDER BY rowid;",
                        _ => return Vec::new(),
                };
                let Some(statement) = self.prepare(sql) else { return Vec::new() };
                statement.bind_i64(1, code);
                self.read_shape_rows(&statement)
        }

        pub fn query_quick_by_prefix(&self, variant: CangjieVariant, prefix: &str, limit: i32) -> Vec<ShapeRow> {
                let sql = match variant {
                        CangjieVariant::Quick5 => {
                                "SELECT rowid, word, q5complex FROM quick_table WHERE quick5 GLOB ? ORDER BY q5complex ASC, rowid ASC LIMIT ?;"
                        }
                        CangjieVariant::Quick3 => {
                                "SELECT rowid, word, q3complex FROM quick_table WHERE quick3 GLOB ? ORDER BY q3complex ASC, rowid ASC LIMIT ?;"
                        }
                        _ => return Vec::new(),
                };
                let Some(statement) = self.prepare(sql) else { return Vec::new() };
                statement.bind_text(1, &format!("{prefix}*"));
                statement.bind_i64(2, Self::normalize_limit(limit));
                self.read_shape_rows(&statement)
        }

        // -- stroke -----------------------------------------------------

        pub fn query_stroke_by_code(&self, code: i64, complex: i64) -> Vec<ShapeRow> {
                let Some(statement) =
                        self.prepare("SELECT rowid, word, complex FROM stroke_table WHERE code = ? AND complex = ? ORDER BY rowid;")
                else {
                        return Vec::new();
                };
                statement.bind_i64(1, code);
                statement.bind_i64(2, complex);
                self.read_shape_rows(&statement)
        }

        pub fn query_stroke_by_pattern(&self, pattern: &str, is_like: bool, limit: i32) -> Vec<ShapeRow> {
                let sql = if is_like {
                        "SELECT rowid, word, complex FROM stroke_table WHERE stroke LIKE ? ORDER BY complex ASC, rowid ASC LIMIT ?;"
                } else {
                        "SELECT rowid, word, complex FROM stroke_table WHERE stroke GLOB ? ORDER BY complex ASC, rowid ASC LIMIT ?;"
                };
                let Some(statement) = self.prepare(sql) else { return Vec::new() };
                statement.bind_text(1, pattern);
                statement.bind_i64(2, Self::normalize_limit(limit));
                self.read_shape_rows(&statement)
        }

        // -- structure ---------------------------------------------------

        pub fn query_structure_by_spell(&self, spell: i64, complexity: i64, limit: i32) -> Vec<StructureRow> {
                let Some(statement) = self.prepare(
                        "SELECT word, romanization FROM structure_table WHERE spell = ? AND complexity = ? ORDER BY rowid LIMIT ?;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, spell);
                statement.bind_i64(2, complexity);
                statement.bind_i64(3, Self::normalize_limit(limit));
                let mut rows = Vec::new();
                while statement.step() {
                        rows.push(StructureRow {
                                word: statement.column_text(0),
                                romanization: statement.column_text(1),
                        });
                }
                rows
        }

        // -- plain text / symbols / emoji ---------------------------------

        pub fn query_plain_texts_by_spell(&self, spell: i64, letter_count: usize) -> Vec<String> {
                let Some(statement) =
                        self.prepare("SELECT word FROM plain_text_table WHERE spell = ? AND letter_count = ? ORDER BY rowid;")
                else {
                        return Vec::new();
                };
                statement.bind_i64(1, spell);
                statement.bind_i64(2, letter_count as i64);
                let mut words = Vec::new();
                while statement.step() {
                        words.push(statement.column_text(0));
                }
                words
        }

        pub fn query_symbols_by_spell(&self, spell: i64, complexity: i64) -> Vec<SymbolRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, category, unicode_version, code_point, cantonese, romanization FROM symbol_table WHERE spell = ? AND complexity = ? ORDER BY rowid;",
                ) else {
                        return Vec::new();
                };
                statement.bind_i64(1, spell);
                statement.bind_i64(2, complexity);
                self.read_symbol_rows(&statement)
        }

        pub fn query_emoji_sequence(&self) -> Vec<SymbolRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, category, unicode_version, code_point, cantonese, romanization FROM symbol_table WHERE category > 0 AND category < 9 ORDER BY rowid;",
                ) else {
                        return Vec::new();
                };
                self.read_symbol_rows(&statement)
        }

        pub fn query_default_frequent_emojis(&self) -> Vec<SymbolRow> {
                let Some(statement) = self.prepare(
                        "SELECT rowid, category, unicode_version, code_point, cantonese, romanization FROM symbol_table WHERE category = 0 ORDER BY rowid;",
                ) else {
                        return Vec::new();
                };
                self.read_symbol_rows(&statement)
        }

        pub fn query_emoji_skin_target(&self, source: &str) -> Option<String> {
                let statement = self.prepare("SELECT target FROM emoji_skin_map WHERE source = ? LIMIT 1;")?;
                statement.bind_text(1, source);
                statement.step().then(|| statement.column_text(0))
        }

        pub fn lookup_romanizations_for_word(&self, word: &str) -> Vec<String> {
                let Some(statement) = self.prepare("SELECT romanization FROM lexicon_core WHERE word = ? ORDER BY rowid;") else {
                        return Vec::new();
                };
                statement.bind_text(1, word);
                let mut result = Vec::new();
                while statement.step() {
                        result.push(statement.column_text(0));
                }
                result
        }

        pub fn query_variant_target(&self, standard: CharacterStandard, source: u32) -> Option<u32> {
                let table = standard.variant_table_name();
                if table.is_empty() {
                        return None;
                }
                let sql = format!("SELECT target FROM {table} WHERE source = ? LIMIT 1;");
                let statement = self.prepare(&sql)?;
                statement.bind_i64(1, i64::from(source));
                if statement.step() {
                        let target = statement.column_i64(0);
                        if (0..=0x10FFFF).contains(&target) {
                                return Some(target as u32);
                        }
                }
                None
        }
}

impl Drop for ImeDatabase {
        fn drop(&mut self) {
                if !self.raw.is_null() {
                        unsafe {
                                sqlite3_close_v2(self.raw);
                        }
                        self.raw = ptr::null_mut();
                }
        }
}

unsafe impl Send for ImeDatabase {}
unsafe impl Sync for ImeDatabase {}
