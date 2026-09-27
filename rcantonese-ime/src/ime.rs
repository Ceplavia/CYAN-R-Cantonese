// IMM32 IME exports — the .ime contract. IMM32 resolves these by name via
// GetProcAddress when the app's input profile selects our keyboard layout.

use std::ffi::c_void;

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::core::BOOL;

use crate::{globals, session, ui};

fn guarded<T: Default>(name: &str, default: T, f: impl FnOnce() -> T) -> T {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
                Ok(v) => v,
                Err(_) => {
                        globals::log_error(&format!("ime: {name} panicked"));
                        default
                }
        }
}

static IS_WINLOGON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn is_winlogon() -> bool {
        IS_WINLOGON.load(std::sync::atomic::Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "system" fn ImeInquire(info: *mut IMEINFO, uiclass: *mut u16, sysflags: u32) -> BOOL {
        guarded("ImeInquire", BOOL(0), || {
                if info.is_null() || uiclass.is_null() {
                        return BOOL(0);
                }
                if sysflags & IME_SYSINFO_WINLOGON != 0 {
                        IS_WINLOGON.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                unsafe {
                        let info = &mut *info;
                        info.dwPrivateDataSize = 0;
                        // SPECIAL_UI — we draw our own candidate window; apps
                        // must not render IME UI (correct inside games).
                        info.fdwProperty = IME_PROP_UNICODE | IME_PROP_SPECIAL_UI;
                        info.fdwConversionCaps = (IME_CMODE_FULLSHAPE | IME_CMODE_NATIVE).0;
                        info.fdwSentenceCaps = 0;
                        info.fdwUICaps = UI_CAP_2700;
                        info.fdwSCSCaps = SCS_CAP_COMPSTR;
                        info.fdwSelectCaps = SELECT_CAP_CONVERSION;
                        // UI window class — imm32 creates a window of this
                        // class per context and puts hIMC in extra slot 0.
                        let name = ui::ui_class_name();
                        std::ptr::copy_nonoverlapping(name.as_ptr(), uiclass, name.len());
                }
                BOOL(1)
        })
}

#[unsafe(no_mangle)]
extern "system" fn ImeConfigure(_hkl: HKL, _hwnd: HWND, _mode: u32, _data: *mut c_void) -> BOOL {
        BOOL(1)
}

#[unsafe(no_mangle)]
extern "system" fn ImeConversionList(
        _himc: HIMC,
        _src: windows::core::PCWSTR,
        _candlist: *mut CANDIDATELIST,
        _buflen: u32,
        _flag: u32,
) -> u32 {
        0
}

#[unsafe(no_mangle)]
extern "system" fn ImeDestroy(_force: u32) -> BOOL {
        BOOL(1)
}

#[unsafe(no_mangle)]
extern "system" fn ImeEscape(_himc: HIMC, _subfunc: u32, _data: *mut c_void) -> LRESULT {
        LRESULT(0)
}

#[unsafe(no_mangle)]
extern "system" fn ImeSelect(himc: HIMC, select: BOOL) -> BOOL {
        globals::log_error(&format!("ime: ImeSelect select={} himc={:x}", select.as_bool(), himc.0 as usize));
        guarded("ImeSelect", BOOL(0), || {
                session::on_select(himc, select.as_bool());
                BOOL(1)
        })
}

#[unsafe(no_mangle)]
extern "system" fn ImeSetActiveContext(himc: HIMC, focus: BOOL) -> BOOL {
        guarded("ImeSetActiveContext", BOOL(0), || {
                session::on_focus(himc, focus.as_bool());
                BOOL(1)
        })
}

#[unsafe(no_mangle)]
extern "system" fn ImeProcessKey(himc: HIMC, vkey: u32, lkeydata: LPARAM, keystate: *const u8) -> BOOL {
        guarded("ImeProcessKey", BOOL(0), || {
                if himc.0.is_null() || keystate.is_null() || is_winlogon() {
                        return BOOL(0);
                }
                let state: &[u8; 256] = unsafe { &*(keystate as *const [u8; 256]) };
                BOOL(session::process_key(himc, vkey, lkeydata.0, state) as i32)
        })
}

#[unsafe(no_mangle)]
extern "system" fn ImeToAsciiEx(
        _vkey: u32,
        _scancode: u32,
        _keystate: *const u8,
        _transbuf: *mut u32,
        _fustate: u32,
        _himc: HIMC,
) -> u32 {
        // All work happens in ImeProcessKey + the IMC message buffer —
        // same as weasel.
        0
}

#[unsafe(no_mangle)]
extern "system" fn NotifyIME(himc: HIMC, action: u32, index: u32, value: u32) -> BOOL {
        guarded("NotifyIME", BOOL(0), || {
                session::on_notify(himc, action, index, value);
                BOOL(1)
        })
}

#[unsafe(no_mangle)]
extern "system" fn ImeRegisterWord(_read: windows::core::PCWSTR, _style: u32, _s: windows::core::PCWSTR) -> BOOL {
        BOOL(0)
}

#[unsafe(no_mangle)]
extern "system" fn ImeUnregisterWord(_read: windows::core::PCWSTR, _style: u32, _s: windows::core::PCWSTR) -> BOOL {
        BOOL(0)
}

#[unsafe(no_mangle)]
extern "system" fn ImeGetRegisterWordStyle(_item: u32, _stylebuf: *mut c_void) -> u32 {
        0
}

#[unsafe(no_mangle)]
extern "system" fn ImeEnumRegisterWord(
        _proc: *const c_void,
        _read: windows::core::PCWSTR,
        _style: u32,
        _s: windows::core::PCWSTR,
        _data: *mut c_void,
) -> u32 {
        0
}

#[unsafe(no_mangle)]
extern "system" fn ImeSetCompositionString(
        _himc: HIMC,
        _index: u32,
        _comp: *const c_void,
        _complen: u32,
        _read: *const c_void,
        _readlen: u32,
) -> BOOL {
        BOOL(0)
}
