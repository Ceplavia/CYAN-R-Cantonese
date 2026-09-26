// r-cantonese-ime — IMM32 (.ime) front-end for R-Cantonese.
//
// Loaded by IMM32-based applications (games, legacy apps) that cannot use
// the TSF text service. Shares the engine/dictionary/settings code with the
// TSF DLL via #[path] includes — no COM, no TSF types in here.

#![allow(non_snake_case)]

#[path = "../../rcantonese/src/globals.rs"]
mod globals;
#[path = "../../rcantonese/src/types.rs"]
mod types;
#[path = "../../rcantonese/src/phrases.rs"]
mod phrases;
#[path = "../../rcantonese/src/extra.rs"]
mod extra;
#[path = "../../rcantonese/src/strings.rs"]
mod strings;
#[path = "../../rcantonese/src/variants.rs"]
mod variants;
#[path = "../../rcantonese/src/db.rs"]
mod db;
#[path = "../../rcantonese/src/segmenter.rs"]
mod segmenter;
#[path = "../../rcantonese/src/converter.rs"]
mod converter;
#[path = "../../rcantonese/src/engine.rs"]
mod engine;
#[path = "../../rcantonese/src/memory.rs"]
mod memory;
#[path = "../../rcantonese/src/settings.rs"]
mod settings;
#[path = "../../rcantonese/src/config.rs"]
mod config;
#[path = "../../rcantonese/src/punctuation.rs"]
mod punctuation;
#[path = "../../rcantonese/src/pinyin.rs"]
mod pinyin;
#[path = "../../rcantonese/src/shapes.rs"]
mod shapes;
#[path = "../../rcantonese/src/keytable.rs"]
mod keytable;

mod ctx;
mod session;
mod ui;
mod ime;

use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows::core::BOOL;

#[unsafe(no_mangle)]
extern "system" fn DllMain(hinstance: HINSTANCE, reason: u32, _reserved: *const core::ffi::c_void) -> BOOL {
        match reason {
                DLL_PROCESS_ATTACH => {
                        globals::DLL_INSTANCE.store(hinstance.0 as isize, std::sync::atomic::Ordering::Relaxed);
                        // .ime only loads into processes that actually
                        // selected our keyboard layout — one line per such
                        // process is worth the field diagnostics.
                        globals::log_error(&format!("ime: attached to pid={}", std::process::id()));
                        let _ = ui::register_classes();
                }
                DLL_PROCESS_DETACH => {
                        ui::shutdown();
                }
                _ => {}
        }
        BOOL(1)
}

// -- test hooks (rlib consumers only) ----------------------------------------

#[doc(hidden)]
pub fn __init_for_test() {
        globals::DLL_INSTANCE.store(
                unsafe {
                        windows::Win32::System::LibraryLoader::GetModuleHandleW(windows::core::w!("r_cantonese_ime.dll"))
                }
                .map(|h| h.0 as isize)
                .unwrap_or(0),
                std::sync::atomic::Ordering::Relaxed,
        );
        let _ = ui::register_classes();
}

#[doc(hidden)]
pub fn dll_instance_for_test() -> windows::Win32::Foundation::HINSTANCE {
        globals::dll_instance()
}

#[doc(hidden)]
pub fn test_ime_select(himc: windows::Win32::UI::Input::Ime::HIMC, select: bool) -> bool {
        session::on_select(himc, select);
        true
}

#[doc(hidden)]
pub fn test_open_status(himc: windows::Win32::UI::Input::Ime::HIMC) -> bool {
        unsafe { windows::Win32::UI::Input::Ime::ImmGetOpenStatus(himc).as_bool() }
}

#[doc(hidden)]
pub fn test_process_key(himc: windows::Win32::UI::Input::Ime::HIMC, vk: u32, keystate: &[u8; 256]) -> bool {
        session::process_key(himc, vk, 0, keystate)
}
