// Query the deployed db directly through winsqlite3 — 'si' candidates.
use std::ffi::c_void;

type Sqlite3 = c_void;
type Stmt = c_void;
#[link(name = "winsqlite3")]
unsafe extern "system" {
        fn sqlite3_open_v2(f: *const u8, d: *mut *mut Sqlite3, flags: i32, vfs: *const u8) -> i32;
        fn sqlite3_prepare_v2(db: *mut Sqlite3, sql: *const u8, n: i32, stmt: *mut *mut Stmt, tail: *mut *mut u8) -> i32;
        fn sqlite3_step(s: *mut Stmt) -> i32;
        fn sqlite3_column_text(s: *mut Stmt, c: i32) -> *const u8;
        fn sqlite3_finalize(s: *mut Stmt) -> i32;
        fn sqlite3_close(d: *mut Sqlite3) -> i32;
}

#[test]
fn check_si_candidates() {
        let mut db: *mut Sqlite3 = std::ptr::null_mut();
        let path: Vec<u8> = b"../target/debug/ime.sqlite3\0".to_vec();
        assert_eq!(unsafe { sqlite3_open_v2(path.as_ptr(), &mut db, 0x1, std::ptr::null()) }, 0);
        let sql = b"SELECT word FROM lexicon_core WHERE romanization='si' LIMIT 10\0";
        let mut stmt: *mut Stmt = std::ptr::null_mut();
        unsafe {
                assert_eq!(sqlite3_prepare_v2(db, sql.as_ptr() as *const u8, sql.len() as i32, &mut stmt, std::ptr::null_mut()), 0);
                let mut n = 0;
                while sqlite3_step(stmt) == 100 {
                        let w = sqlite3_column_text(stmt, 0);
                        println!("  {}", std::ffi::CStr::from_ptr(w as *const _).to_string_lossy());
                        n += 1;
                }
                println!("si candidates: {n}");
                sqlite3_finalize(stmt);
                sqlite3_close(db);
        }
}
