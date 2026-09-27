// Candidate window for the IMM32 path — standalone topmost GDI popup.
// The IME advertises IME_PROP_SPECIAL_UI so apps never draw IME UI; we own
// the whole experience. Rendering mirrors candidate.rs (3 fonts, settings
// colors) but positions itself from the INPUTCONTEXT caret form.

use std::sync::Mutex;
use std::sync::OnceLock;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{w, PCWSTR};

use crate::candview;
use crate::globals;
use crate::session::CandItem;
use crate::settings::ImeSettings;

const CANDIDATE_CLASS: &[u16] = &[82, 67, 97, 110, 116, 111, 110, 101, 115, 101, 73, 77, 69, 67, 97, 110, 100, 0]; // "RCantoneseIMECand"
const UI_CLASS: &[u16] = &[82, 67, 97, 110, 116, 111, 110, 101, 115, 101, 73, 77, 69, 85, 73, 0]; // "RCantoneseIMEUI"

pub fn ui_class_name() -> &'static [u16] {
        UI_CLASS
}

// ---------------------------------------------------------------------------
// Shared window state — one window per process is enough (IME messages all
// arrive on the app's UI thread).
// ---------------------------------------------------------------------------

pub struct CandState {
        pub items: Vec<CandItem>,
        /// Raw input letters being composed ("neihou") — the preedit line
        /// drawn above the candidates. IMM32 apps relying on SPECIAL_UI
        /// never render GCS_COMPSTR, so the composition text lives here.
        pub input_text: String,
        pub selection: usize,
        pub page_size: usize,
        pub candidate_font_size: u32,
        pub number_font_size: u32,
        pub comment_font_size: u32,
        pub text_color: u32,
        pub back_color: u32,
        pub select_color: u32,
        pub comment_color: u32,
}

impl Default for CandState {
        fn default() -> Self {
                Self {
                        items: Vec::new(),
                        input_text: String::new(),
                        selection: 0,
                        page_size: 7,
                        candidate_font_size: crate::settings::DEFAULT_CANDIDATE_FONT_SIZE,
                        number_font_size: crate::settings::DEFAULT_CANDIDATE_NUMBER_FONT_SIZE,
                        comment_font_size: crate::settings::DEFAULT_CANDIDATE_COMMENT_FONT_SIZE,
                        text_color: 0,
                        back_color: 0xFFFFFF,
                        select_color: 0xF8CDA8,
                        comment_color: 0x808080,
                }
        }
}

struct Window {
        hwnd: isize,
        pos: (i32, i32),
}

impl Window {
        fn hwnd(&self) -> HWND {
                HWND(self.hwnd as *mut _)
        }
}

static STATE: OnceLock<std::sync::Arc<Mutex<CandState>>> = OnceLock::new();
static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

fn state() -> &'static std::sync::Arc<Mutex<CandState>> {
        STATE.get_or_init(|| std::sync::Arc::new(Mutex::new(CandState::default())))
}

pub fn register_classes() -> windows::core::Result<()> {
        let instance = globals::dll_instance();
        unsafe {
                // IME UI window class — imm32 creates one per IMC and stuffs
                // the hIMC into window-extra slot 0.
                let wc = WNDCLASSEXW {
                        cbSize: size_of::<WNDCLASSEXW>() as u32,
                        style: CS_IME,
                        lpfnWndProc: Some(ui_wnd_proc),
                        cbWndExtra: 2 * size_of::<isize>() as i32,
                        hInstance: instance,
                        lpszClassName: PCWSTR(UI_CLASS.as_ptr()),
                        ..Default::default()
                };
                if RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                        return Err(windows::core::Error::from(GetLastError()));
                }

                let cand = WNDCLASSW {
                        hInstance: instance,
                        lpszClassName: PCWSTR(CANDIDATE_CLASS.as_ptr()),
                        lpfnWndProc: Some(cand_wnd_proc),
                        style: CS_VREDRAW | CS_HREDRAW,
                        hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                        ..Default::default()
                };
                let _ = RegisterClassW(&cand);
        }
        Ok(())
}

pub fn shutdown() {
        if let Ok(mut guard) = WINDOW.lock() {
                if let Some(w) = guard.take() {
                        unsafe {
                                let _ = DestroyWindow(w.hwnd());
                        }
                }
        }
}

fn ensure_window() -> Option<HWND> {
        let mut guard = WINDOW.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = &*guard {
                unsafe {
                        if IsWindow(Some(w.hwnd())).as_bool() {
                                return Some(w.hwnd());
                        }
                }
        }
        let hwnd = unsafe {
                CreateWindowExW(
                        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                        PCWSTR(CANDIDATE_CLASS.as_ptr()),
                        w!(""),
                        WS_POPUP | WS_BORDER,
                        0,
                        0,
                        10,
                        10,
                        None,
                        None,
                        Some(globals::dll_instance()),
                        None,
                )
        };
        match hwnd {
                Ok(hwnd) if !hwnd.is_invalid() => {
                        let ptr = std::sync::Arc::into_raw(state().clone());
                        unsafe {
                                SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as _);
                        }
                        *guard = Some(Window { hwnd: hwnd.0 as isize, pos: (0, 0) });
                        Some(hwnd)
                }
                _ => None,
        }
}

// -- public API used by session.rs ------------------------------------------

/// Last computed position, kept independent of the window's existence —
/// position_window runs before ensure_window on the first keystroke, so a
/// window-scoped pos used to be dropped and the popup opened at (0,0).
static POSITION: Mutex<(i32, i32)> = Mutex::new((0, 0));

pub fn set_position(x: i32, y: i32) {
        *POSITION.lock().unwrap_or_else(|e| e.into_inner()) = (x, y);
        if let Ok(mut guard) = WINDOW.lock() {
                if let Some(w) = &mut *guard {
                        w.pos = (x, y);
                        // Re-anchor a live popup — better position sources can
                        // resolve AFTER the first show (the post-deliver TSF
                        // query only runs once the app has reacted to our
                        // messages), so a visible window must actually move.
                        let hwnd = w.hwnd();
                        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
                                let mut rc = RECT::default();
                                if unsafe { GetWindowRect(hwnd, &mut rc) }.is_ok() {
                                        let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
                                        if ww > 0 && wh > 0 {
                                                let (nx, ny) = candview::clamp_to_work_area(x, y, ww, wh, y);
                                                unsafe {
                                                        let _ = SetWindowPos(
                                                                hwnd,
                                                                Some(HWND_TOPMOST),
                                                                nx,
                                                                ny,
                                                                0,
                                                                0,
                                                                SWP_NOACTIVATE | SWP_NOSIZE,
                                                        );
                                                }
                                        }
                                }
                        }
                }
        }
}

pub fn show_candidates(items: &[CandItem], selection: usize, settings: &ImeSettings, input_text: &str, page_size: usize) {
        let Some(hwnd) = ensure_window() else { return };
        {
                let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
                st.items = items.to_vec();
                st.input_text = input_text.to_string();
                st.selection = selection;
                st.page_size = page_size.max(1);
                st.candidate_font_size = settings.candidate_font_size;
                st.number_font_size = settings.candidate_number_font_size;
                st.comment_font_size = settings.candidate_comment_font_size;
                st.text_color = settings.candidate_text_color;
                st.back_color = settings.candidate_back_color;
                st.select_color = settings.candidate_select_color;
                st.comment_color = settings.candidate_comment_color;
        }
        let (w, h) = measure(hwnd);
        let (x, y) = {
                let pos = *POSITION.lock().unwrap_or_else(|e| e.into_inner());
                candview::clamp_to_work_area(pos.0, pos.1, w, h, pos.1)
        };
        unsafe {
                // HWND_TOPMOST on every show — the topmost band order can be
                // lost when another window steals focus mid-session (e.g.
                // the config window), which left the popup painting under
                // the app window.
                let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        x,
                        y,
                        w,
                        h,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                let _ = InvalidateRect(Some(hwnd), None, true);
        }
}

pub fn hide_candidates() {
        if let Ok(guard) = WINDOW.lock() {
                if let Some(w) = &*guard {
                        unsafe {
                                let _ = ShowWindow(w.hwnd(), SW_HIDE);
                        }
                }
        }
}

// -- geometry / painting — the actual GDI work lives in the shared --------
// candview module so this window is pixel-identical to the TSF presenter.

fn style_of(st: &CandState) -> candview::Style {
        candview::Style {
                candidate_font_size: st.candidate_font_size,
                number_font_size: st.number_font_size,
                comment_font_size: st.comment_font_size,
                text_color: st.text_color,
                back_color: st.back_color,
                select_color: st.select_color,
                comment_color: st.comment_color,
        }
}

fn measure(hwnd: HWND) -> (i32, i32) {
        let st = state().lock().unwrap_or_else(|e| e.into_inner());
        let style = style_of(&st);
        (
                candview::measure_width(hwnd, &st.items, st.selection, st.page_size, &style, &st.input_text),
                candview::measure_height(st.items.len(), st.selection, st.page_size, &style, &st.input_text),
        )
}

unsafe extern "system" fn cand_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
                        WM_PAINT => {
                                let mut ps = PAINTSTRUCT::default();
                                let hdc = BeginPaint(hwnd, &mut ps);
                                paint(hwnd, hdc);
                                let _ = EndPaint(hwnd, &ps);
                                LRESULT(0)
                        }
                        WM_NCDESTROY => {
                                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<CandState>;
                                if !ptr.is_null() {
                                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                                        drop(std::sync::Arc::from_raw(ptr));
                                }
                                DefWindowProcW(hwnd, msg, wparam, lparam)
                        }
                        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
        }
}

fn paint(hwnd: HWND, hdc: HDC) {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<CandState> };
        if ptr.is_null() {
                return;
        }
        let st = unsafe { &*ptr }.lock().unwrap_or_else(|e| e.into_inner());
        let style = style_of(&st);
        candview::paint(hwnd, hdc, &st.items, st.selection, st.page_size, &style, &st.input_text);
}

// -- IME UI window -----------------------------------------------------------
// imm32 creates this window per IMC and stores the hIMC in extra slot 0.

unsafe extern "system" fn ui_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                let himc_bits = GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(0));
                if himc_bits != 0 {
                        let himc = windows::Win32::UI::Input::Ime::HIMC(himc_bits as *mut _);
                        match msg {
                                WM_IME_SELECT => {
                                        crate::session::on_select(himc, wparam.0 != 0);
                                }
                                WM_IME_STARTCOMPOSITION | WM_IME_NOTIFY => {
                                        // Reposition our window when the app
                                        // moves the caret / sets positions.
                                        crate::session::on_focus(himc, true);
                                }
                                _ => {}
                        }
                        return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
        }
}
