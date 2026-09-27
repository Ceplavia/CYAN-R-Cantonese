// In-process IMM32 host test — drives the real imm32 message flow:
// ImmCreateContext + ImmAssociateContext + our exports + a real message
// pump. Validates that keystrokes turn into WM_IME_* messages carrying the
// right result string, without needing admin or a real app.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static RESULT_TEXT: AtomicUsize = AtomicUsize::new(0);
static mut RESULT_BUF: [u16; 64] = [0; 64];
static SEEN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
                        WM_IME_COMPOSITION => {
                                SEEN.fetch_add(1, Ordering::Relaxed);
                                if lparam.0 as u32 & GCS_RESULTSTR.0 != 0 {
                                        let himc = ImmGetContext(hwnd);
                                        let buf = std::ptr::addr_of_mut!(RESULT_BUF);
                                        let n = ImmGetCompositionStringW(
                                                himc,
                                                GCS_RESULTSTR,
                                                Some((*buf).as_mut_ptr() as *mut _),
                                                (((*buf).len()) * 2) as u32,
                                        );
                                        if n > 0 {
                                                RESULT_TEXT.store(n as usize, Ordering::Relaxed);
                                        }
                                        let _ = ImmReleaseContext(hwnd, himc);
                                }
                                LRESULT(0)
                        }
                        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
        }
}

fn pump() {
        unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                }
        }
}

fn keystate() -> [u8; 256] {
        [0u8; 256]
}

#[test]
fn jyutping_commit_flow() {
        r_cantonese_ime::__init_for_test();

        // Host window + message-routed input context.
        let wc = WNDCLASSW {
                lpszClassName: w!("immtest_wnd"),
                lpfnWndProc: Some(wnd_proc),
                hInstance: r_cantonese_ime::dll_instance_for_test(),
                ..Default::default()
        };
        unsafe {
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("immtest_wnd"),
                        w!("t"),
                        WINDOW_STYLE(0),
                        0,
                        0,
                        100,
                        100,
                        None,
                        None,
                        None,
                        None,
                )
                .unwrap();
                let himc = ImmCreateContext();
                assert!(!himc.0.is_null());
                let _ = ImmAssociateContext(hwnd, himc);
                {
                        let imc = ImmLockIMC(himc);
                        assert!(!imc.is_null());
                        // ImmAssociateContext doesn't fill INPUTCONTEXT.hWnd
                        // on modern Windows — the system sets it for real
                        // contexts. Patch it for the synthetic test.
                        (*imc).hWnd = hwnd;
                        let _ = ImmUnlockIMC(himc);
                }

                // Select the IME — equivalent to the user picking our layout.
                assert!(r_cantonese_ime::test_ime_select(himc, true));
                assert!(r_cantonese_ime::test_open_status(himc), "context should be open after select");

                // Type "nei" then space → commit the first candidate.
                for vk in [0x4Eu32, 0x45, 0x49] {
                        assert!(r_cantonese_ime::test_process_key(himc, vk, &keystate()), "vk {vk:#x} not consumed");
                }
                pump();
                assert!(SEEN.load(Ordering::Relaxed) >= 1, "no WM_IME_COMPOSITION seen");

                // Space commits the top candidate.
                assert!(r_cantonese_ime::test_process_key(himc, 0x20, &keystate()));
                pump();
                let n = RESULT_TEXT.load(Ordering::Relaxed);
                assert!(n > 0, "no result string delivered");
                {
                        let buf = unsafe { &*std::ptr::addr_of!(RESULT_BUF) };
                        let raw_bytes: Vec<u8> = buf.iter().flat_map(|c| c.to_le_bytes()).collect();
                        println!("n={n} raw={:02x?}", &raw_bytes[..(n as usize).min(16)]);
                }
                let buf = unsafe { &*std::ptr::addr_of!(RESULT_BUF) };
                let text = String::from_utf16_lossy(&buf[..(n as usize / 2)]);
                println!("committed: {text}");
                assert!(!text.is_empty() && text != "nei");

                // ESC while idle → not consumed.
                assert!(!r_cantonese_ime::test_process_key(himc, 0x1B, &keystate()));
                // Idle letter starts a fresh composition.
                assert!(r_cantonese_ime::test_process_key(himc, 0x4E, &keystate()));
                assert!(r_cantonese_ime::test_process_key(himc, 0x1B, &keystate()), "esc should cancel");
                let _ = ImmDestroyContext(himc);
                let _ = DestroyWindow(hwnd);
        }
}

#[test]
fn ime_inquire_fills_info() {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::BOOL;
        use windows::core::PCSTR;
        let path = "C:\\Windows\\System32\\r-cantonese.ime".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let h = unsafe { LoadLibraryW(windows::core::PCWSTR(path.as_ptr())) }.unwrap();
        let inq: unsafe extern "system" fn(*mut IMEINFO, *mut u16, u32) -> BOOL = unsafe {
                std::mem::transmute(GetProcAddress(h, PCSTR(b"ImeInquire\0".as_ptr())).unwrap())
        };
        let mut info = IMEINFO::default();
        let mut cls = [0u16; 64];
        let ok = unsafe { inq(&mut info, cls.as_mut_ptr(), 0) };
        assert!(ok.as_bool(), "ImeInquire returned false");
        println!("prop={:#x} conv={:#x} ui={:#x} scs={:#x} sel={:#x} priv={} class={:?}",
                info.fdwProperty, info.fdwConversionCaps, info.fdwUICaps, info.fdwSCSCaps,
                info.fdwSelectCaps, info.dwPrivateDataSize,
                String::from_utf16_lossy(&cls[..cls.iter().position(|&c| c==0).unwrap_or(64)]));
}
