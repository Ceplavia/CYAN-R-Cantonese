use std::error::Error;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io;
use std::path::Path;
use std::ptr;

const SQLITE_OK: c_int = 0;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READWRITE: c_int = 0x00000002;
const SQLITE_OPEN_CREATE: c_int = 0x00000004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x00010000;

#[repr(C)]
struct Sqlite3 {
        _private: [u8; 0],
}

#[repr(C)]
struct Sqlite3Statement {
        _private: [u8; 0],
}

#[cfg_attr(target_os = "windows", link(name = "winsqlite3"))]
#[cfg_attr(not(target_os = "windows"), link(name = "sqlite3"))]
unsafe extern "C" {
        fn sqlite3_open_v2(filename: *const c_char, database: *mut *mut Sqlite3, flags: c_int, virtual_file_system: *const c_char) -> c_int;
        fn sqlite3_close_v2(database: *mut Sqlite3) -> c_int;
        fn sqlite3_errmsg(database: *mut Sqlite3) -> *const c_char;
        fn sqlite3_exec(
                database: *mut Sqlite3,
                sql: *const c_char,
                callback: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
                context: *mut c_void,
                error_message: *mut *mut c_char,
        ) -> c_int;
        fn sqlite3_prepare_v2(database: *mut Sqlite3, sql: *const c_char, byte_count: c_int, statement: *mut *mut Sqlite3Statement, tail: *mut *const c_char) -> c_int;
        fn sqlite3_step(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_finalize(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_reset(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_clear_bindings(statement: *mut Sqlite3Statement) -> c_int;
        fn sqlite3_bind_int64(statement: *mut Sqlite3Statement, index: c_int, value: i64) -> c_int;
        fn sqlite3_bind_text(
                statement: *mut Sqlite3Statement,
                index: c_int,
                value: *const c_char,
                byte_count: c_int,
                destructor: Option<unsafe extern "C" fn(*mut c_void)>,
        ) -> c_int;
}

pub struct Database {
        raw: *mut Sqlite3,
}

impl Database {
        pub fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
                let path_text = path.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "database path is not valid UTF-8"))?;
                let path_string = CString::new(path_text)?;
                let mut raw = ptr::null_mut();
                let result = unsafe { sqlite3_open_v2(path_string.as_ptr(), &mut raw, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX, ptr::null()) };
                if result != SQLITE_OK {
                        let message = error_message(raw);
                        if !raw.is_null() {
                                unsafe {
                                        sqlite3_close_v2(raw);
                                }
                        }
                        return Err(io::Error::other(format!("failed to open {}: {message}", path.display())).into());
                }
                Ok(Self { raw })
        }

        pub fn execute(&self, sql: &str) -> Result<(), Box<dyn Error>> {
                let command = CString::new(sql)?;
                let result = unsafe { sqlite3_exec(self.raw, command.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) };
                self.check(result, sql)
        }

        pub fn prepare(&self, sql: &str) -> Result<Statement, Box<dyn Error>> {
                let command = CString::new(sql)?;
                let byte_count = c_int::try_from(sql.len())?;
                let mut raw = ptr::null_mut();
                let result = unsafe { sqlite3_prepare_v2(self.raw, command.as_ptr(), byte_count, &mut raw, ptr::null_mut()) };
                self.check(result, sql)?;
                Ok(Statement { database: self.raw, raw })
        }

        fn check(&self, result: c_int, context: &str) -> Result<(), Box<dyn Error>> {
                if result == SQLITE_OK {
                        return Ok(());
                }
                Err(io::Error::other(format!("{}: {context}", error_message(self.raw))).into())
        }
}

impl Drop for Database {
        fn drop(&mut self) {
                unsafe {
                        sqlite3_close_v2(self.raw);
                }
        }
}

pub struct Statement {
        database: *mut Sqlite3,
        raw: *mut Sqlite3Statement,
}

impl Statement {
        pub fn bind_i64(&mut self, index: c_int, value: i64) -> Result<(), Box<dyn Error>> {
                let result = unsafe { sqlite3_bind_int64(self.raw, index, value) };
                self.check(result, "failed to bind integer")
        }

        pub fn bind_text(&mut self, index: c_int, value: &str) -> Result<(), Box<dyn Error>> {
                let byte_count = c_int::try_from(value.len())?;
                let result = unsafe { sqlite3_bind_text(self.raw, index, value.as_ptr().cast(), byte_count, None) };
                self.check(result, "failed to bind text")
        }

        pub fn insert(&mut self) -> Result<(), Box<dyn Error>> {
                let result = unsafe { sqlite3_step(self.raw) };
                if result != SQLITE_DONE {
                        return Err(io::Error::other(error_message(self.database)).into());
                }
                let reset_result = unsafe { sqlite3_reset(self.raw) };
                self.check(reset_result, "failed to reset statement")?;
                let clear_result = unsafe { sqlite3_clear_bindings(self.raw) };
                self.check(clear_result, "failed to clear statement bindings")
        }

        fn check(&self, result: c_int, context: &str) -> Result<(), Box<dyn Error>> {
                if result == SQLITE_OK {
                        return Ok(());
                }
                Err(io::Error::other(format!("{}: {context}", error_message(self.database))).into())
        }
}

impl Drop for Statement {
        fn drop(&mut self) {
                unsafe {
                        sqlite3_finalize(self.raw);
                }
        }
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
