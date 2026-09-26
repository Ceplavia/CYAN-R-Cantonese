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

pub fn set_position(x: i32, y: i32) {
        if let Ok(mut guard) = WINDOW.lock() {
                if let Some(w) = &mut *guard {
                        w.pos = (x, y);
                }
        }
}

pub fn show_candidates(items: &[CandItem], selection: usize, settings: &ImeSettings) {
        let Some(hwnd) = ensure_window() else { return };
        {
                let mut st = state().lock().unwrap_or_else(|e| e.into_inner());
                st.items = items.to_vec();
                st.selection = selection;
                st.page_size = settings.candidate_page_size.max(1) as usize;
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
                let pos = WINDOW.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|w| w.pos).unwrap_or((0, 0));
                clamp_to_work_area(pos.0, pos.1, w, h, pos.1 - h - 2)
        };
        unsafe {
                let _ = MoveWindow(hwnd, x, y, w, h, true);
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
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

// -- geometry ----------------------------------------------------------------

fn row_height(st: &CandState) -> i32 {
        st.candidate_font_size.max(st.comment_font_size).max(st.number_font_size) as i32 + 10
}

fn page_bounds(st: &CandState) -> (usize, usize) {
        let page_size = st.page_size.max(1);
        let page = st.selection / page_size;
        let start = page * page_size;
        let end = (start + page_size - 1).min(st.items.len().saturating_sub(1));
        (start, end)
}

fn measure(hwnd: HWND) -> (i32, i32) {
        let st = state().lock().unwrap_or_else(|e| e.into_inner());
        if st.items.is_empty() {
                return (120, row_height(&st) + 8);
        }
        let (start, end) = page_bounds(&st);
        let mut max_cx = 40i32;
        unsafe {
                let hdc = GetDC(Some(hwnd));
                if hdc.is_invalid() {
                        return (200, 24);
                }
                let num_font = make_font(st.number_font_size);
                let cand_font = make_font(st.candidate_font_size);
                let cmt_font = make_font(st.comment_font_size);
                let mut shown = 0usize;
                for index in start..=end {
                        let Some(item) = st.items.get(index) else { break };
                        shown += 1;
                        let label: Vec<u16> = format!("{}.", shown).encode_utf16().collect();
                        let text: Vec<u16> = format!(" {}", item.text).encode_utf16().collect();
                        let comment: Vec<u16> = format!("  {}", item.comment).encode_utf16().collect();
                        let cx = text_width(hdc, num_font, &label)
                                + text_width(hdc, cand_font, &text)
                                + text_width(hdc, cmt_font, &comment);
                        max_cx = max_cx.max(cx);
                }
                let _ = DeleteObject(num_font.into());
                let _ = DeleteObject(cand_font.into());
                let _ = DeleteObject(cmt_font.into());
                let _ = ReleaseDC(Some(hwnd), hdc);
        }
        let rows = (end - start + 1) as i32;
        (max_cx + 16, rows * row_height(&st) + 8)
}

fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, caret_top: i32) -> (i32, i32) {
        unsafe {
                let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                };
                if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                        return (x, y);
                }
                let work = info.rcWork;
                let nx = x.clamp(work.left, (work.right - w).max(work.left));
                let mut ny = if y + h > work.bottom { caret_top.max(work.top) } else { y };
                if ny + h > work.bottom {
                        ny = (work.bottom - h).max(work.top);
                }
                (nx, ny)
        }
}

// -- painting ----------------------------------------------------------------

fn make_font(size: u32) -> HFONT {
        unsafe {
                CreateFontW(
                        -(size as i32),
                        0,
                        0,
                        0,
                        FW_NORMAL.0 as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET,
                        OUT_DEFAULT_PRECIS,
                        CLIP_DEFAULT_PRECIS,
                        CLEARTYPE_QUALITY,
                        DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
                        w!("Microsoft JhengHei"),
                )
        }
}

unsafe fn text_width(hdc: HDC, font: HFONT, text: &[u16]) -> i32 {
        unsafe {
                if text.is_empty() {
                        return 0;
                }
                let old = SelectObject(hdc, font.into());
                let mut size = SIZE::default();
                let _ = GetTextExtentPoint32W(hdc, text, &mut size);
                SelectObject(hdc, old);
                size.cx
        }
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
        unsafe {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<CandState>;
                if ptr.is_null() {
                        return;
                }
                let st = (*ptr).lock().unwrap_or_else(|e| e.into_inner());
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let brush = CreateSolidBrush(COLORREF(st.back_color));
                FillRect(hdc, &client, brush);
                let _ = DeleteObject(brush.into());
                if st.items.is_empty() {
                        return;
                }
                let rh = row_height(&st);
                let (start, end) = page_bounds(&st);
                let num_font = make_font(st.number_font_size);
                let cand_font = make_font(st.candidate_font_size);
                let cmt_font = make_font(st.comment_font_size);
                let mut orig_font = HGDIOBJ::default();
                let mut shown = 0usize;
                for (i, index) in (start..=end).enumerate() {
                        let Some(item) = st.items.get(index) else { break };
                        let y = 4 + i as i32 * rh;
                        let mut rc = RECT { left: 0, top: y, right: 4096, bottom: y + rh };
                        shown += 1;
                        if index == st.selection {
                                let sel = CreateSolidBrush(COLORREF(st.select_color));
                                FillRect(hdc, &rc, sel);
                                let _ = DeleteObject(sel.into());
                        }
                        SetBkMode(hdc, TRANSPARENT);
                        let mut x = 8;
                        let mut label: Vec<u16> = format!("{}.", shown).encode_utf16().collect();
                        SetTextColor(hdc, COLORREF(st.text_color));
                        let old = SelectObject(hdc, num_font.into());
                        if orig_font.is_invalid() {
                                orig_font = old;
                        }
                        rc.left = x;
                        DrawTextW(hdc, &mut label, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        x += text_width(hdc, num_font, &label);
                        let mut wide: Vec<u16> = format!(" {}", item.text).encode_utf16().collect();
                        SelectObject(hdc, cand_font.into());
                        rc.left = x;
                        DrawTextW(hdc, &mut wide, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        x += text_width(hdc, cand_font, &wide);
                        if !item.comment.is_empty() {
                                let mut comment: Vec<u16> = format!("  {}", item.comment).encode_utf16().collect();
                                SelectObject(hdc, cmt_font.into());
                                rc.left = x;
                                SetTextColor(hdc, COLORREF(st.comment_color));
                                DrawTextW(hdc, &mut comment, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        }
                }
                if !orig_font.is_invalid() {
                        SelectObject(hdc, orig_font);
                }
                let _ = DeleteObject(num_font.into());
                let _ = DeleteObject(cand_font.into());
                let _ = DeleteObject(cmt_font.into());
        }
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
