#[test]
fn check_memory_rows() {
        use windows::Win32::Storage::FileSystem::*;
        use std::ffi::c_void;
        #[link(name = "winsqlite3")]
        unsafe extern "system" {
                fn sqlite3_open_v2(filename: *const u8, db: *mut *mut c_void, flags: i32, vfs: *const u8) -> i32;
                fn sqlite3_prepare_v2(db: *mut c_void, sql: *const u8, nbyte: i32, stmt: *mut *mut c_void, tail: *mut *mut u8) -> i32;
                fn sqlite3_step(stmt: *mut c_void) -> i32;
                fn sqlite3_column_int(stmt: *mut c_void, col: i32) -> i32;
                fn sqlite3_column_text(stmt: *mut c_void, col: i32) -> *const u8;
                fn sqlite3_finalize(stmt: *mut c_void) -> i32;
                fn sqlite3_close(db: *mut c_void) -> i32;
        }
        let path = format!("{}/RCantonese/memory.sqlite3", std::env::var("LOCALAPPDATA").unwrap());
        let mut db: *mut c_void = std::ptr::null_mut();
        let cpath = std::ffi::CString::new(path.clone()).unwrap();
        unsafe {
                let rc = sqlite3_open_v2(cpath.as_ptr() as *const u8, &mut db, 2, std::ptr::null());
                assert_eq!(rc, 0, "open failed {rc}");
                let mut stmt: *mut c_void = std::ptr::null_mut();
                let sql = b"SELECT COUNT(*) FROM memory2608\0";
                let rc = sqlite3_prepare_v2(db, sql.as_ptr() as *const u8, sql.len() as i32, &mut stmt, std::ptr::null_mut());
                assert_eq!(rc, 0);
                let rc = sqlite3_step(stmt);
                assert_eq!(rc, 100);
                let count = sqlite3_column_int(stmt, 0);
                println!("memory2608 rows: {count}");
                sqlite3_finalize(stmt);
                let mut stmt: *mut c_void = std::ptr::null_mut();
                let sql = b"SELECT word, romanization, frequency, anchors, spell FROM memory2608 ORDER BY frequency DESC LIMIT 15\0";
                let rc = sqlite3_prepare_v2(db, sql.as_ptr() as *const u8, sql.len() as i32, &mut stmt, std::ptr::null_mut());
                assert_eq!(rc, 0);
                while sqlite3_step(stmt) == 100 {
                        let w = sqlite3_column_text(stmt, 0);
                        let r = sqlite3_column_text(stmt, 1);
                        let f = sqlite3_column_int(stmt, 2);
                        let a = sqlite3_column_int(stmt, 3);
                        let s = sqlite3_column_int(stmt, 4);
                        let wstr = std::ffi::CStr::from_ptr(w as *const _).to_string_lossy();
                        let rstr = std::ffi::CStr::from_ptr(r as *const _).to_string_lossy();
                        println!("  {wstr} [{rstr}] freq={f} anchors={a} spell={s}");
                }
                sqlite3_finalize(stmt);
                sqlite3_close(db);
        }
}
