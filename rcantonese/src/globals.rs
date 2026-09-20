// Global constants: CLSIDs, GUIDs, registry names, resource IDs.
#![allow(non_upper_case_globals)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use windows::core::{Error, GUID, Result};
use windows::Win32::Foundation::{HINSTANCE, MAX_PATH};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

pub const TEXTSERVICE_MODEL: &str = "Apartment";
// MAKELANGID(LANG_CHINESE, SUBLANG_CHINESE_HONGKONG)
pub const TEXTSERVICE_LANGID: u16 = 0x0c04;
pub const TEXTSERVICE_ICON_INDEX: u32 = 12; // -IDIS_IME
pub const TEXTSERVICE_SQLITE_DATA: &str = "ime.sqlite3";

pub const IDI_INPUT_MODE_CANTONESE: u32 = 21;
pub const IDI_INPUT_MODE_ABC: u32 = 23;

pub const USER_DEFAULT_SCREEN_DPI: f32 = 96.0;
pub const CANDIDATE_ROW_VERTICAL_SPACING: f32 = 6.0;
pub const CANDIDATE_ROW_PADDING_LEFT: f32 = 10.0;
pub const CANDIDATE_ROW_PADDING_RIGHT: f32 = 1.0;
pub const CANDIDATE_NUMBER_SPACING: f32 = 12.0;
pub const CANDIDATE_COMMENT_SPACING: f32 = 12.0;
pub const CANDIDATE_WINDOW_TEXT_EXTENT_GAP: i32 = 8;

pub const CANDIDATE_FONT_NAMES: &[&str] = &[
        "Inter",
        "Segoe UI",
        "Shanggu Sans",
        "Sarasa Gothic CL",
        "Plangothic P1",
        "Plangothic P2",
        "Microsoft YaHei UI",
        "Microsoft JhengHei UI",
        "MiSans L3",
        "Segoe UI Emoji",
        "Segoe UI Symbol",
];
pub const NUMBER_LABEL_FONT_NAMES: &[&str] = &["Doima", "Monaspace Neon", "JetBrains Mono", "Consolas"];
pub const COMMENT_FONT_NAMES: &[&str] = &[
        "Doima",
        "Monaspace Neon",
        "JetBrains Mono",
        "Inter",
        "Segoe UI",
        "Shanggu Sans",
        "Microsoft YaHei UI",
];

// ---------------------------------------------------------------------
// GUIDs
// ---------------------------------------------------------------------

// {D2291A80-84D8-4641-9AB2-BDD1472C846B}
pub const CLSID_RCANTONESE: GUID = GUID::from_values(0xd2291a80, 0x84d8, 0x4641, [0x9a, 0xb2, 0xbd, 0xd1, 0x47, 0x2c, 0x84, 0x6b]);

// {83955C0E-2C09-47a5-BCF3-F2B98E11EE8B}
pub const GUID_PROFILE: GUID = GUID::from_values(0x83955c0e, 0x2c09, 0x47a5, [0xbc, 0xf3, 0xf2, 0xb9, 0x8e, 0x11, 0xee, 0x8b]);

// {4B62B54B-F828-43B5-9095-A96DF9CBDF38}
pub const GUID_PRESERVEDKEY_INPUT_MODE: GUID =
        GUID::from_values(0x4b62b54b, 0xf828, 0x43b5, [0x90, 0x95, 0xa9, 0x6d, 0xf9, 0xcb, 0xdf, 0x38]);
// {5A08D6C4-4563-4E46-8DDB-65E75C4E73A3}
pub const GUID_PRESERVEDKEY_CHARACTER_FORM: GUID =
        GUID::from_values(0x5a08d6c4, 0x4563, 0x4e46, [0x8d, 0xdb, 0x65, 0xe7, 0x5c, 0x4e, 0x73, 0xa3]);
// {175F062E-B961-4AED-A3DF-59F78A02862D}
pub const GUID_PRESERVEDKEY_PUNCTUATION_FORM: GUID =
        GUID::from_values(0x175f062e, 0xb961, 0x4aed, [0xa3, 0xdf, 0x59, 0xf7, 0x8a, 0x2, 0x86, 0x2d]);
// {0475268D-82DA-4BBB-9038-E9FEFB4ED077}
pub const GUID_PRESERVEDKEY_VARIANT_TRADITIONAL: GUID =
        GUID::from_values(0x0475268d, 0x82da, 0x4bbb, [0x90, 0x38, 0xe9, 0xfe, 0xfb, 0x4e, 0xd0, 0x77]);
// {F3B1F904-E07C-4E79-8BA3-039E80013160}
pub const GUID_PRESERVEDKEY_VARIANT_HONGKONG: GUID =
        GUID::from_values(0xf3b1f904, 0xe07c, 0x4e79, [0x8b, 0xa3, 0x03, 0x9e, 0x80, 0x01, 0x31, 0x60]);
// {C59C2038-FA4A-463D-8775-3303EA96FE3A}
pub const GUID_PRESERVEDKEY_VARIANT_TAIWAN: GUID =
        GUID::from_values(0xc59c2038, 0xfa4a, 0x463d, [0x87, 0x75, 0x33, 0x03, 0xea, 0x96, 0xfe, 0x3a]);
// {33E79976-A5DA-478F-88E1-C2C5937E00D3}
pub const GUID_PRESERVEDKEY_VARIANT_SIMPLIFIED: GUID =
        GUID::from_values(0x33e79976, 0xa5da, 0x478f, [0x88, 0xe1, 0xc2, 0xc5, 0x93, 0x7e, 0x00, 0xd3]);
// {AAF51363-3468-4686-804C-316856046EEE}
pub const GUID_PRESERVEDKEY_CHARFORM_HALF: GUID =
        GUID::from_values(0xaaf51363, 0x3468, 0x4686, [0x80, 0x4c, 0x31, 0x68, 0x56, 0x04, 0x6e, 0xee]);
// {3A660A21-6946-4747-92C9-79A721337B4D}
pub const GUID_PRESERVEDKEY_CHARFORM_FULL: GUID =
        GUID::from_values(0x3a660a21, 0x6946, 0x4747, [0x92, 0xc9, 0x79, 0xa7, 0x21, 0x33, 0x7b, 0x4d]);
// {96E92228-88F9-4CEF-B584-7AD03D46DD3F}
pub const GUID_PRESERVEDKEY_PUNCT_CANTONESE: GUID =
        GUID::from_values(0x96e92228, 0x88f9, 0x4cef, [0xb5, 0x84, 0x7a, 0xd0, 0x3d, 0x46, 0xdd, 0x3f]);
// {EF8C1B1D-2136-4A9A-9EBD-C7CC44BF0F5D}
pub const GUID_PRESERVEDKEY_PUNCT_ENGLISH: GUID =
        GUID::from_values(0xef8c1b1d, 0x2136, 0x4a9a, [0x9e, 0xbd, 0xc7, 0xcc, 0x44, 0xbf, 0x0f, 0x5d]);
// {E0242C08-897B-4C04-B08A-CCF175FD0B1D}
pub const GUID_PRESERVEDKEY_MODE_CANTONESE: GUID =
        GUID::from_values(0xe0242c08, 0x897b, 0x4c04, [0xb0, 0x8a, 0xcc, 0xf1, 0x75, 0xfd, 0x0b, 0x1d]);
// {F9E34CCD-309E-4B43-8A86-288D825E5616}
pub const GUID_PRESERVEDKEY_MODE_ABC: GUID =
        GUID::from_values(0xf9e34ccd, 0x309e, 0x4b43, [0x8a, 0x86, 0x28, 0x8d, 0x82, 0x5e, 0x56, 0x16]);

// {101011C5-CF72-4F0C-A515-153019593F10}
pub const GUID_COMPARTMENT_CHARACTER_FORM: GUID =
        GUID::from_values(0x101011c5, 0xcf72, 0x4f0c, [0xa5, 0x15, 0x15, 0x30, 0x19, 0x59, 0x3f, 0x10]);
// {DD321BCC-A7F8-4561-9B61-9B3508C9BA97}
pub const GUID_COMPARTMENT_PUNCTUATION_FORM: GUID =
        GUID::from_values(0xdd321bcc, 0xa7f8, 0x4561, [0x9b, 0x61, 0x9b, 0x35, 0x8, 0xc9, 0xba, 0x97]);

// {4C802E2C-8140-4436-A5E5-F7C544EBC9CD}
pub const GUID_DISPLAY_ATTRIBUTE_INPUT: GUID =
        GUID::from_values(0x4c802e2c, 0x8140, 0x4436, [0xa5, 0xe5, 0xf7, 0xc5, 0x44, 0xeb, 0xc9, 0xcd]);
// {9A1CC683-F2A7-4701-9C6E-2DA69A5CD474}
pub const GUID_DISPLAY_ATTRIBUTE_CONVERTED: GUID =
        GUID::from_values(0x9a1cc683, 0xf2a7, 0x4701, [0x9c, 0x6e, 0x2d, 0xa6, 0x9a, 0x5c, 0xd4, 0x74]);

// {84B0749F-8DE7-4732-907A-3BCB150A01A8}
pub const GUID_CANDIDATE_UI_ELEMENT: GUID = GUID::from_values(0x84b0749f, 0x8de7, 0x4732, [0x90, 0x7a, 0x3b, 0xcb, 0x15, 0xa, 0x1, 0xa8]);

// ---------------------------------------------------------------------
// Module-wide state
// ---------------------------------------------------------------------

pub static DLL_INSTANCE: AtomicIsize = AtomicIsize::new(0);
pub static DLL_REF_COUNT: AtomicIsize = AtomicIsize::new(-1);

pub fn dll_instance() -> HINSTANCE {
        HINSTANCE(DLL_INSTANCE.load(Ordering::Relaxed) as *mut _)
}

pub fn dll_add_ref() {
        DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn dll_release() {
        DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
}

pub fn dll_ref_count() -> isize {
        DLL_REF_COUNT.load(Ordering::SeqCst)
}

/// Full path of this DLL.
pub fn module_path() -> Option<std::path::PathBuf> {
        let mut buffer = vec![0u16; MAX_PATH as usize];
        let length = unsafe { GetModuleFileNameW(Some(dll_instance().into()), &mut buffer) };
        if length == 0 || length as usize >= buffer.len() {
                return None;
        }
        buffer.truncate(length as usize);
        Some(std::path::PathBuf::from(String::from_utf16_lossy(&buffer)))
}

/// Default path of ime.sqlite3: same directory as the DLL.
pub fn default_database_path() -> std::path::PathBuf {
        match module_path() {
                Some(path) => path.with_file_name(TEXTSERVICE_SQLITE_DATA),
                None => std::path::PathBuf::from(TEXTSERVICE_SQLITE_DATA),
        }
}

/// File log path — %LOCALAPPDATA%\RCantonese\Logs\RCantonese.log, beside
/// settings.toml and memory.sqlite3 so failures are easy to find.
pub fn log_file_path() -> Option<std::path::PathBuf> {
        static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        PATH.get_or_init(|| {
                let base = std::env::var_os("LOCALAPPDATA")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(std::env::temp_dir);
                let dir = base.join("RCantonese").join("Logs");
                if std::fs::create_dir_all(&dir).is_err() {
                        return None;
                }
                Some(dir.join("RCantonese.log"))
        })
        .clone()
}

/// Debug/trace logging — compiled out of release builds (the IME is
/// injected into every text-input process, so chatter must not touch
/// disk in production).
pub fn log(message: &str) {
        let _ = message;
        #[cfg(test)]
        eprintln!("{message}");
        #[cfg(debug_assertions)]
        write_log_line(message);
}

/// Error logging — kept in release builds; failures must be diagnosable
/// without a debugger.
pub fn log_error(message: &str) {
        #[cfg(test)]
        eprintln!("ERROR: {message}");
        write_log_line(&format!("ERROR: {message}"));
}

fn write_log_line(message: &str) {
        if let Some(path) = log_file_path() {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                        let _ = writeln!(file, "{message}");
                }
        }
        unsafe {
                let wide: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
                windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(windows::core::PCWSTR(wide.as_ptr()));
        }
}

// ---------------------------------------------------------------------
// Keyboard modifier tracking — port of Globals.cpp UpdateModifiers.
// ---------------------------------------------------------------------

pub const TF_MOD_ALT: u32 = 0x0001;
pub const TF_MOD_CONTROL: u32 = 0x0002;
pub const TF_MOD_SHIFT: u32 = 0x0004;
pub const TF_MOD_RALT: u32 = 0x0008;
pub const TF_MOD_RCONTROL: u32 = 0x0010;
pub const TF_MOD_RSHIFT: u32 = 0x0020;
pub const TF_MOD_LALT: u32 = 0x0040;
pub const TF_MOD_LCONTROL: u32 = 0x0080;
pub const TF_MOD_LSHIFT: u32 = 0x0100;
pub const TF_MOD_ON_KEYUP: u32 = 0x0200;
pub const TF_MOD_IGNORE_ALL_MODIFIER: u32 = 0x0400;

pub const TF_MOD_ALLALT: u32 = TF_MOD_RALT | TF_MOD_LALT | TF_MOD_ALT;
pub const TF_MOD_ALLCONTROL: u32 = TF_MOD_RCONTROL | TF_MOD_LCONTROL | TF_MOD_CONTROL;
pub const TF_MOD_ALLSHIFT: u32 = TF_MOD_RSHIFT | TF_MOD_LSHIFT | TF_MOD_SHIFT;

pub const TF_MOD_ON_KEYUP_SHIFT_ONLY: u32 = 0x00010000 | TF_MOD_ON_KEYUP;

pub static MODIFIERS_VALUE: AtomicU32 = AtomicU32::new(0);
pub static IS_SHIFT_KEY_DOWN_ONLY: AtomicBool = AtomicBool::new(false);
pub static IS_CONTROL_KEY_DOWN_ONLY: AtomicBool = AtomicBool::new(false);
pub static IS_ALT_KEY_DOWN_ONLY: AtomicBool = AtomicBool::new(false);

fn key_down(vkey: u32) -> bool {
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(vkey as i32) as u16 & 0x8000 != 0 }
}

pub fn update_modifiers(wparam: usize, lparam: isize) {
        const VK_MENU: u32 = 0x12;
        const VK_CONTROL: u32 = 0x11;
        const VK_SHIFT: u32 = 0x10;

        let menu_down = key_down(VK_MENU);
        let ctrl_down = key_down(VK_CONTROL);
        let shift_down = key_down(VK_SHIFT);

        let mut modifiers = MODIFIERS_VALUE.load(Ordering::Relaxed);
        let mut shift_only = false;
        let mut ctrl_only = false;
        let mut alt_only = false;

        match wparam & 0xff {
                x if x == VK_MENU as usize => {
                        if menu_down {
                                if lparam & 0x01000000 != 0 {
                                        modifiers |= TF_MOD_RALT | TF_MOD_ALT;
                                } else {
                                        modifiers |= TF_MOD_LALT | TF_MOD_ALT;
                                }
                                if lparam & 0x40000000 == 0 {
                                        if !ctrl_down && !shift_down {
                                                alt_only = true;
                                        }
                                }
                        }
                }
                x if x == VK_CONTROL as usize => {
                        if ctrl_down {
                                if lparam & 0x01000000 != 0 {
                                        modifiers |= TF_MOD_RCONTROL | TF_MOD_CONTROL;
                                } else {
                                        modifiers |= TF_MOD_LCONTROL | TF_MOD_CONTROL;
                                }
                                if lparam & 0x40000000 == 0 {
                                        if !shift_down && !menu_down {
                                                ctrl_only = true;
                                        }
                                }
                        }
                }
                x if x == VK_SHIFT as usize => {
                        if shift_down {
                                if ((lparam >> 16) & 0x00ff) == 0x36 {
                                        modifiers |= TF_MOD_RSHIFT | TF_MOD_SHIFT;
                                } else {
                                        modifiers |= TF_MOD_LSHIFT | TF_MOD_SHIFT;
                                }
                                if lparam & 0x40000000 == 0 {
                                        if !menu_down && !ctrl_down {
                                                shift_only = true;
                                        }
                                }
                        }
                }
                _ => {}
        }

        if menu_down {
                modifiers |= TF_MOD_ALT;
        } else {
                modifiers &= !TF_MOD_ALLALT;
        }
        if ctrl_down {
                modifiers |= TF_MOD_CONTROL;
        } else {
                modifiers &= !TF_MOD_ALLCONTROL;
        }
        if shift_down {
                modifiers |= TF_MOD_SHIFT;
        } else {
                modifiers &= !TF_MOD_ALLSHIFT;
        }

        MODIFIERS_VALUE.store(modifiers, Ordering::Relaxed);
        IS_SHIFT_KEY_DOWN_ONLY.store(shift_only, Ordering::Relaxed);
        IS_CONTROL_KEY_DOWN_ONLY.store(ctrl_only, Ordering::Relaxed);
        IS_ALT_KEY_DOWN_ONLY.store(alt_only, Ordering::Relaxed);
}

pub fn modifiers_value() -> u32 {
        MODIFIERS_VALUE.load(Ordering::Relaxed)
}

/// Port of CheckModifiers.
pub fn check_modifiers(mod_current: u32, mod_expected: u32) -> bool {
        let expected = mod_expected & !TF_MOD_ON_KEYUP;
        if mod_expected & TF_MOD_IGNORE_ALL_MODIFIER != 0 {
                return true;
        }
        if mod_current == expected {
                return true;
        }
        if mod_current != 0 && expected == 0 {
                return false;
        }
        const GROUPS: [(u32, u32); 3] = [
                (TF_MOD_ALT, TF_MOD_ALLALT & !TF_MOD_ALT),
                (TF_MOD_SHIFT, TF_MOD_ALLSHIFT & !TF_MOD_SHIFT),
                (TF_MOD_CONTROL, TF_MOD_ALLCONTROL & !TF_MOD_CONTROL),
        ];
        for (flag, rl_bits) in GROUPS {
                if expected & flag != 0 {
                        if mod_current & flag == 0 {
                                return false;
                        }
                } else if (expected ^ mod_current) & rl_bits != 0 {
                        return false;
                }
        }
        true
}

/// Run a COM-facing callback body so a Rust panic can never unwind across the
/// ABI — that would abort the host process. Panics are logged and turned into
/// E_FAIL instead.
pub fn guarded<T>(name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
                log(&format!("panic caught in {name}"));
                Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL))
        })
}

/// Same as `guarded` for non-Result callbacks (e.g. window procs).
pub fn guarded_value<T>(name: &str, fallback: T, f: impl FnOnce() -> T) -> T {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
                log(&format!("panic caught in {name}"));
                fallback
        })
}
