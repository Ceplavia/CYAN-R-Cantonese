// INPUTCONTEXT / IMCC helpers — mirrors WeaselIME's _Initialize /
// _AddIMEMessage / composition-buffer plumbing.

#![allow(non_camel_case_types)]

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
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

/// Append a TRANSMSG to the context's message buffer and ask imm32 to
/// deliver it (ImmGenerateMessage posts the queued messages to hWnd).
pub fn push_message(himc: HIMC, message: u32, wparam: usize, lparam: isize) {
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
                let _ = ImmGenerateMessage(himc);
        }
}

