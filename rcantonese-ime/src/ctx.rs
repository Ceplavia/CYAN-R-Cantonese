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

