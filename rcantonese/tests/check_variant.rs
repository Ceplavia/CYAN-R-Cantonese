// Query the deployed db directly through winsqlite3 to check variant tables.
use std::ffi::c_void;

type Sqlite3 = c_void;
type Stmt = c_void;
#[link(name = "winsqlite3")]
unsafe extern "system" {
    fn sqlite3_open_v2(f: *const u8, d: *mut *mut Sqlite3, flags: i32, vfs: *const u8) -> i32;
    fn sqlite3_prepare16_v2(db: *mut Sqlite3, sql: *const u16, n: i32, stmt: *mut *mut Stmt, tail: *mut *mut u16) -> i32;
    fn sqlite3_step(s: *mut Stmt) -> i32;
    fn sqlite3_column_int64(s: *mut Stmt, c: i32) -> i64;
    fn sqlite3_finalize(s: *mut Stmt) -> i32;
    fn sqlite3_close(d: *mut Sqlite3) -> i32;
}

fn count(db: *mut Sqlite3, sql: &str) -> i64 {
    let mut w: Vec<u16> = sql.encode_utf16().collect(); w.push(0);
    let mut stmt: *mut Stmt = std::ptr::null_mut();
    unsafe {
        assert_eq!(sqlite3_prepare16_v2(db, w.as_ptr(), -1, &mut stmt, std::ptr::null_mut()), 0);
        let v = if sqlite3_step(stmt) == 100 { sqlite3_column_int64(stmt, 0) } else { -1 };
        sqlite3_finalize(stmt); v
    }
}

#[test]
fn check_variant_tables() {
    let mut db: *mut Sqlite3 = std::ptr::null_mut();
    let path: Vec<u8> = b"../target/debug/ime.sqlite3\0".to_vec();
    assert_eq!(unsafe { sqlite3_open_v2(path.as_ptr(), &mut db, 0x1, std::ptr::null()) }, 0);
    for t in ["variant_sim", "variant_hk", "variant_tw", "variant_prc"] {
        println!("{t}: {} rows", count(db, &format!("SELECT COUNT(*) FROM {t}")));
    }
    println!("sim 9AD4 -> {}", count(db, "SELECT target FROM variant_sim WHERE source=39380"));
    unsafe { sqlite3_close(db) };
}
