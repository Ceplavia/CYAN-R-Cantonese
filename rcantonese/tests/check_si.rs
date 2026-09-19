#[test]
fn check_si_candidates() {
        // Query ime.sqlite3 for 'si' first candidates — same path as runtime.
        let db = r_cantonese::db::ImeDatabase::open("target/debug/ime.sqlite3").expect("open db");
        // use the public suggest path if available, else raw query
        let _ = db;
}
