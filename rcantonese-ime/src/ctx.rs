// INPUTCONTEXT / IMCC helpers — mirrors WeaselIME's _Initialize /
// _AddIMEMessage / composition-buffer plumbing.

#![allow(non_camel_case_types)]

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::Ime::*;

use crate::globals;

pub const MAX_STRING: usize = 256;

/// hCompStr payload: COMPOSITIONSTRING header followed by the composition
/// and result string buffers (same layout as weasel's CompositionInfo).
#[repr(C)]
pub struct CompInfo {
        pub cs: COMPOSITIONSTRING,
        pub comp_str: [u16; MAX_STRING],
        pub result_str: [u16; MAX_STRING],
}

impl CompInfo {
        fn reset(&mut self) {
                *self = unsafe { std::mem::zeroed() };
                self.cs.dwSize = size_of::<CompInfo>() as u32;
                self.cs.dwCompStrOffset = std::mem::offset_of!(CompInfo, comp_str) as u32;
                self.cs.dwResultStrOffset = std::mem::offset_of!(CompInfo, result_str) as u32;
        }
}

pub fn lock_imc(himc: HIMC) -> Option<&'static mut INPUTCONTEXT> {
        let ptr = unsafe { ImmLockIMC(himc) };
        if ptr.is_null() { None } else { Some(unsafe { &mut *ptr }) }
}

pub fn unlock_imc(himc: HIMC) {
        let _ = unsafe { ImmUnlockIMC(himc) };
}

/// Allocate (or reset) the hCompStr IMCC for this context.
pub fn init_compstr(himc: HIMC) -> bool {
        let Some(imc) = lock_imc(himc) else { return false };
        let size = size_of::<CompInfo>() as u32;
        let himcc = if imc.hCompStr.0.is_null() {
                unsafe { ImmCreateIMCC(size) }
        } else {
                unsafe { ImmReSizeIMCC(imc.hCompStr, size) }
        };
        if himcc.0.is_null() {
                unlock_imc(himc);
                return false;
        }
        imc.hCompStr = himcc;
        unlock_imc(himc);
        write_comp(himc, &[], &[]);
        true
}

/// Write composition + result text into hCompStr.
pub fn write_comp(himc: HIMC, comp: &[u16], result: &[u16]) -> bool {
        let Some(imc) = lock_imc(himc) else { return false };
        if imc.hCompStr.0.is_null() {
                unlock_imc(himc);
                return false;
        }
        let himcc = imc.hCompStr;
        unlock_imc(himc);
        let info = unsafe { ImmLockIMCC(himcc) } as *mut CompInfo;
        if info.is_null() {
                return false;
        }
        let info = unsafe { &mut *info };
        // ImmCreateIMCC pre-fills dwSize = sizeof(COMPOSITIONSTRING) — our
        // buffers need offsets pointing past it; initialize on first use.
        if info.cs.dwCompStrOffset == 0 || info.cs.dwResultStrOffset == 0 {
                info.reset();
        }
        let comp_len = comp.len().min(MAX_STRING - 1);
        info.comp_str[..comp_len].copy_from_slice(&comp[..comp_len]);
        info.comp_str[comp_len] = 0;
        info.cs.dwCompStrLen = comp_len as u32;
        info.cs.dwCursorPos = comp_len as u32;
        let result_len = result.len().min(MAX_STRING - 1);
        info.result_str[..result_len].copy_from_slice(&result[..result_len]);
        info.result_str[result_len] = 0;
        info.cs.dwResultStrLen = result_len as u32;
        let _ = unsafe { ImmUnlockIMCC(himcc) };
        true
}

/// Write the current candidate list into `hCandInfo` as a standard
/// CANDIDATELIST — apps that draw their own IME UI (games like WoW/EVE)
/// read this block on IMN_OPENCANDIDATE/IMN_CHANGECANDIDATE.
pub fn write_candlist(himc: HIMC, items: &[String], selection: u32, page_size: u32) -> bool {
        let Some(imc) = lock_imc(himc) else { return false };
        // Header (6 dwords) + offset array + UTF-16 strings.
        let mut str_bytes = 0usize;
        let mut offsets = Vec::with_capacity(items.len());
        let mut wide_items = Vec::with_capacity(items.len());
        for item in items.iter() {
                offsets.push((6 * 4 + items.len() * 4 + str_bytes) as u32);
                let wide: Vec<u16> = item.encode_utf16().chain(std::iter::once(0)).collect();
                str_bytes += wide.len() * 2;
                wide_items.push(wide);
        }
        let total = 6 * 4 + items.len() * 4 + str_bytes;
        let himcc = if imc.hCandInfo.0.is_null() {
                unsafe { ImmCreateIMCC(total as u32) }
        } else {
                unsafe { ImmReSizeIMCC(imc.hCandInfo, total as u32) }
        };
        if himcc.0.is_null() {
                unlock_imc(himc);
                return false;
        }
        imc.hCandInfo = himcc;
        unlock_imc(himc);

        let info = unsafe { ImmLockIMCC(himcc) };
        if info.is_null() {
                return false;
        }
        let info = info as *mut u32;
        unsafe {
                *info = total as u32;
                *info.add(1) = IME_CAND_UNKNOWN;
                *info.add(2) = items.len() as u32;
                *info.add(3) = selection;
                *info.add(4) = 0;
                *info.add(5) = page_size.max(1);
                let off_ptr = info.add(6);
                for (i, off) in offsets.iter().enumerate() {
                        *off_ptr.add(i) = *off;
                }
                let mut cur = (info as *mut u8).add(6 * 4 + items.len() * 4);
                for wide in &wide_items {
                        std::ptr::copy_nonoverlapping(wide.as_ptr(), cur as *mut u16, wide.len());
                        cur = cur.add(wide.len() * 2);
                }
        }
        let _ = unsafe { ImmUnlockIMCC(himcc) };
        true
}

// ---------------------------------------------------------------------------
// Delivery reentrancy control.
//
// ImmGenerateMessage SendMessages WM_IME_* into the app; the app's handler
// (or imm32 itself, e.g. NI_CONTEXTUPDATED echoes) can synchronously call
// NotifyIME again. Two hazards without a guard:
//   1. Infinite recursion — a nested deliver re-triggers the same notify path
//      (Telegram/Qt did this to the point of stack overflow, 0xC000041D).
//   2. Mid-iteration realloc — push_message resizes hMsgBuf while imm32 is
//      walking it, leaving imm32 with a dangling IMCC pointer.
// While DELIVERING is set, push_message stages into PENDING and the outer
// deliver loop flushes it once the in-flight ImmGenerateMessage returns.
// ---------------------------------------------------------------------------
thread_local! {
        static DELIVERING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static PENDING: std::cell::RefCell<Vec<(usize, u32, usize, isize)>> =
                const { std::cell::RefCell::new(Vec::new()) };
}

/// Append a TRANSMSG to the context's message buffer. Deliberately does NOT
/// call ImmGenerateMessage here: delivery runs SendMessage into the app's
/// wndproc, and apps like Chromium re-enter our exports (NotifyIME etc.)
/// while handling WM_IME_* — which deadlocks the non-reentrant session
/// mutex. Callers must invoke deliver() after releasing it.
pub fn push_message(himc: HIMC, message: u32, wparam: usize, lparam: isize) {
        // try_with — these thread-locals can be touched during thread
        // teardown (a late deliver racing DLL_THREAD_DETACH); .with would
        // panic there, which inside a host process is fatal.
        if DELIVERING.try_with(|d| d.get()).unwrap_or(false) {
                let _ = PENDING.try_with(|p| {
                        p.borrow_mut().push((himc.0 as usize, message, wparam, lparam));
                });
                return;
        }
        push_message_now(himc, message, wparam, lparam);
}

fn push_message_now(himc: HIMC, message: u32, wparam: usize, lparam: isize) {
        let Some(imc) = lock_imc(himc) else { return };
        let count = imc.dwNumMsgBuf;
        let size = (count + 1) as usize * size_of::<TRANSMSG>();
        let himcc = if imc.hMsgBuf.0.is_null() {
                unsafe { ImmCreateIMCC(size as u32) }
        } else {
                unsafe { ImmReSizeIMCC(imc.hMsgBuf, size as u32) }
        };
        if himcc.0.is_null() {
                unlock_imc(himc);
                globals::log_error("ime: ImmReSizeIMCC failed");
                return;
        }
        imc.hMsgBuf = himcc;
        imc.dwNumMsgBuf = count + 1;
        unlock_imc(himc);
        let buf = unsafe { ImmLockIMCC(himcc) } as *mut TRANSMSG;
        if buf.is_null() {
                return;
        }
        unsafe {
                let slot = &mut *buf.add(count as usize);
                slot.message = message;
                slot.wParam = WPARAM(wparam);
                slot.lParam = LPARAM(lparam);
                let _ = ImmUnlockIMCC(himcc);
        }
}

fn queued_count(himc: HIMC) -> u32 {
        let Some(imc) = lock_imc(himc) else { return 0 };
        let n = imc.dwNumMsgBuf;
        unlock_imc(himc);
        n
}

/// Flush queued TRANSMSGs to the app's window. Must be called AFTER the
/// session mutex is released — SendMessage can re-enter our exports.
/// Nested deliveries are no-ops: the outer loop keeps ImmGenerateMessage-ing
/// until both hMsgBuf and the staged PENDING list drain (bounded).
pub fn deliver(himc: HIMC) {
        // TLS already destroyed → nothing sane to do but bail.
        if DELIVERING.try_with(|d| d.replace(true)).unwrap_or(true) {
                return;
        }
        let t0 = std::time::Instant::now();
        let mut rounds = 0u32;
        for _ in 0..64 {
                // Move anything staged during the previous delivery into the
                // IMC buffer — never resize hMsgBuf while imm32 is walking it.
                let staged: Vec<(usize, u32, usize, isize)> =
                        PENDING.try_with(|p| std::mem::take(&mut *p.borrow_mut()))
                                .unwrap_or_default();
                for (h, m, w, l) in staged {
                        push_message_now(HIMC(h as *mut _), m, w, l);
                }
                if queued_count(himc) == 0 {
                        break;
                }
                rounds += 1;
                unsafe {
                        let _ = ImmGenerateMessage(himc);
                }
        }
        let dt = t0.elapsed();
        if dt.as_millis() > 20 {
                crate::globals::log(&format!(
                        "ime: deliver took {dt:?} rounds={rounds} pid={}",
                        std::process::id()
                ));
        }
        let _ = DELIVERING.try_with(|d| d.set(false));
}

/// Open/close the context AND mirror the state into fdwConversion —
/// Chromium/Electron checks `fdwConversion & IME_CMODE_NATIVE` to decide
/// whether the IME is in CJK input mode; with it stuck at 0 (ALPHANUMERIC)
/// every keystroke is eaten but composition results are silently dropped.
pub fn set_open_state(himc: HIMC, open: bool) {
        unsafe {
                let _ = ImmSetOpenStatus(himc, open);
        }
        if let Some(imc) = lock_imc(himc) {
                imc.fdwConversion = if open {
                        (IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE | IME_CMODE_SYMBOL).0
                } else {
                        0
                };
                imc.fdwSentence = 0;
                unlock_imc(himc);
        }
}

