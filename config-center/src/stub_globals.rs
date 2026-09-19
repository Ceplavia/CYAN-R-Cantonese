// Stub for rcantonese/src/globals.rs — only the items config.rs needs.
#![allow(dead_code)]

pub const TF_MOD_ALT: u32 = 0x0001;
pub const TF_MOD_CONTROL: u32 = 0x0002;
pub const TF_MOD_SHIFT: u32 = 0x0004;

pub fn log(msg: &str) {
        eprintln!("{msg}");
}

pub fn log_error(msg: &str) {
        eprintln!("ERROR: {msg}");
}
