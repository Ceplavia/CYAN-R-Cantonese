// Candidate list presenter — port of CandidateListUIPresenter.cpp.
// The candidate window is a plain GDI popup (Direct2D rendering is a later
// refinement); the paging/selection model follows CandidateWindow.cpp.
#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::globals;
use crate::keytable::KeystrokeFunction;
use crate::processor::CandidateItem;

pub const TF_CLUIE_DOCUMENTMGR: u32 = 0x0001;
pub const TF_CLUIE_COUNT: u32 = 0x0002;
pub const TF_CLUIE_SELECTION: u32 = 0x0004;
pub const TF_CLUIE_STRING: u32 = 0x0008;
pub const TF_CLUIE_PAGEINDEX: u32 = 0x0010;
pub const TF_CLUIE_CURRENTPAGE: u32 = 0x0020;

const CANDIDATE_CLASS: &[u16] = {
        const fn wide(s: &str) -> [u16; 32] {
                let bytes = s.as_bytes();
                let mut out = [0u16; 32];
                let mut i = 0;
                while i < bytes.len() && i < 31 {
                        out[i] = bytes[i] as u16;
                        i += 1;
                }
                out
        }
        &wide("RCantoneseCandidateWindowClass")
};

// ---------------------------------------------------------------------
// Candidate window state (selection + paging, port of CCandidateWindow)
// ---------------------------------------------------------------------

#[derive(Default)]
struct CandidateWindowState {
        items: Vec<CandidateItem>,
        selection: usize,
        page_start_indices: Vec<usize>,
        /// Uniform page size — mirrored for the shared candview calls.
        page_size: usize,
        text_color: u32,
        back_color: u32,
        select_color: u32,
        comment_color: u32,
        candidate_font_size: u32,
        number_font_size: u32,
        comment_font_size: u32,
}

impl crate::candview::RowLike for crate::processor::CandidateItem {
        fn row_text(&self) -> &str {
                &self.text
        }
        fn row_comment(&self) -> &str {
                &self.comment
        }
        fn row_separator(&self) -> bool {
                self.separator
        }
}

impl CandidateWindowState {
        fn view_style(&self) -> crate::candview::Style {
                crate::candview::Style {
                        candidate_font_size: self.candidate_font_size,
                        number_font_size: self.number_font_size,
                        comment_font_size: self.comment_font_size,
                        text_color: self.text_color,
                        back_color: self.back_color,
                        select_color: self.select_color,
                        comment_color: self.comment_color,
                }
        }
}

impl CandidateWindowState {
        fn count(&self) -> u32 {
                self.items.len() as u32
        }

        fn rebuild_pages(&mut self, page_size: usize) {
                self.page_size = page_size.max(1);
                self.page_start_indices.clear();
                let page_size = page_size.max(1);
                let mut start = 0;
                while start < self.items.len() {
                        self.page_start_indices.push(start);
                        start += page_size;
                }
                if self.page_start_indices.is_empty() {
                        self.page_start_indices.push(0);
                }
                if self.selection >= self.items.len() {
                        self.selection = self.items.len().saturating_sub(1);
                }
        }

        /// Port of _GetCurrentPage.
        fn current_page(&self) -> usize {
                let mut page = 0usize;
                for (i, &start) in self.page_start_indices.iter().enumerate() {
                        if start > self.selection {
                                break;
                        }
                        page = i;
                }
                page
        }

        fn page_bounds(&self, page: usize) -> Option<(usize, usize)> {
                if page >= self.page_start_indices.len() {
                        return None;
                }
                let start = *self.page_start_indices.get(page)?;
                if start >= self.items.len() && !self.items.is_empty() {
                        return None;
                }
                let end = if page + 1 < self.page_start_indices.len() {
                        self.page_start_indices[page + 1].saturating_sub(1)
                } else {
                        self.items.len().saturating_sub(1)
                };
                if self.items.is_empty() {
                        return Some((0, 0));
                }
                if end < start {
                        return None;
                }
                Some((start, end))
        }

        /// Port of _SetSelectionInPage. `pos` is an offset within the current
        /// page, so it has to stay inside that page — a host that pages by 10
        /// must not let key 0 land on the next page of 7.
        fn set_selection_in_page(&mut self, pos: usize) -> bool {
                let page = self.current_page();
                let Some((start, end)) = self.page_bounds(page) else { return false };
                let index = start + pos;
                if index > end || index >= self.items.len() {
                        return false;
                }
                self.selection = index;
                true
        }

        /// Port of _MoveSelection.
        fn move_selection(&mut self, offset: i32) -> bool {
                if offset == 0 {
                        return true;
                }
                if self.items.is_empty() {
                        return false;
                }
                let next = self.selection as i64 + offset as i64;
                if next < 0 || next >= self.items.len() as i64 {
                        return false;
                }
                self.selection = next as usize;
                true
        }

        /// Port of _SetSelection (-1 selects the last item).
        fn set_selection(&mut self, index: i32) -> bool {
                let index = if index == -1 {
                        if self.items.is_empty() {
                                return false;
                        }
                        self.items.len() as i32 - 1
                } else {
                        index
                };
                if index < 0 || index as usize >= self.items.len() {
                        return false;
                }
                self.selection = index as usize;
                true
        }

        /// Port of _MovePage.
        fn move_page(&mut self, offset: i32) -> bool {
                if offset == 0 {
                        return true;
                }
                if self.items.is_empty() {
                        return false;
                }
                let current = self.current_page() as i64;
                let target = current + offset as i64;
                if target < 0 || target >= self.page_start_indices.len() as i64 {
                        return false;
                }
                let Some((current_start, _)) = self.page_bounds(current as usize) else { return false };
                let Some((target_start, target_end)) = self.page_bounds(target as usize) else { return false };
                let selection_offset = self.selection.saturating_sub(current_start);
                self.selection = (target_start + selection_offset).min(target_end);
                true
        }
}

// ---------------------------------------------------------------------
// Candidate window — minimal GDI popup.
// ---------------------------------------------------------------------

struct CandidateWindow {
        hwnd: HWND,
        state: std::sync::Arc<Mutex<CandidateWindowState>>,
        page_size: usize,
}

impl CandidateWindow {
        fn create(parent: HWND, page_size: usize, state: std::sync::Arc<Mutex<CandidateWindowState>>) -> Option<Self> {
                candidate_class_atom()?;
                {
                        // Initialize appearance from settings — the settings
                        // file may have been changed by the config center.
                        let settings = crate::settings::load_settings();
                        let mut state_guard = state.lock().unwrap_or_else(|e| e.into_inner());
                        state_guard.candidate_font_size = settings.candidate_font_size;
                        state_guard.number_font_size = settings.candidate_number_font_size;
                        state_guard.comment_font_size = settings.candidate_comment_font_size;
                        state_guard.text_color = settings.candidate_text_color;
                        state_guard.back_color = settings.candidate_back_color;
                        state_guard.select_color = settings.candidate_select_color;
                        state_guard.comment_color = settings.candidate_comment_color;
                }
                let hwnd = create_candidate_hwnd(parent)?;
                let window = Self { hwnd, state, page_size };
                // Give the wnd proc access to the shared state. One extra
                // strong ref is leaked intentionally — it is reclaimed
                // when the window is destroyed.
                let ptr = std::sync::Arc::into_raw(window.state.clone());
                unsafe {
                        // `as _`: SetWindowLongPtrW takes isize
                        // on x64 but i32 on x86.
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as _);
                }
                Some(window)
        }

        fn show(&self, show: bool) {
                unsafe {
                        let _ = ShowWindow(self.hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
                }
        }

        fn is_visible(&self) -> bool {
                unsafe { IsWindowVisible(self.hwnd).as_bool() }
        }

        /// Clamp a candidate-window rect inside the nearest monitor's work
        /// area — shared implementation lives in candview.
        fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, caret_top: i32) -> (i32, i32) {
                crate::candview::clamp_to_work_area(x, y, w, h, caret_top)
        }

        fn move_to(&self, rect: RECT) {
                let (width, height) = {
                        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                        (measure_width(&state, self.hwnd) as i32, measure_height(&state, self.page_size) as i32)
                };
                let (w, h) = (width.max(80), height.max(24));
                let (x, y) = Self::clamp_to_work_area(rect.left, rect.bottom + 2, w, h, rect.top);
                unsafe {
                        let _ = MoveWindow(self.hwnd, x, y, w, h, true);
                }
        }

        fn invalidate(&self) {
                unsafe {
                        let _ = InvalidateRect(Some(self.hwnd), None, true);
                }
        }
}

impl Drop for CandidateWindow {
        fn drop(&mut self) {
                unsafe {
                        let _ = DestroyWindow(self.hwnd);
                }
        }
}

// ---------------------------------------------------------------------
// High z-order bands. Start menu, SearchHost and older UWP frames
// (ApplicationFrameHost, WindowsApps such as Notes) live above
// ZBID_DESKTOP. A normal WS_EX_TOPMOST popup is created in the desktop
// band and is painted underneath those windows, so the candidate list
// is invisible while Space still commits. CreateWindowInBand (user32,
// same contract WindInput uses) puts the popup in the host's own band.
// A zero-height GetTextExt on these hosts is a custom canvas with no
// line box, not the CUAS/IMM bridge — hiding our window there leaves
// nothing on screen.
// ---------------------------------------------------------------------

type CreateWindowInBandFn = unsafe extern "system" fn(
        WINDOW_EX_STYLE,
        u16,
        PCWSTR,
        WINDOW_STYLE,
        i32,
        i32,
        i32,
        i32,
        HWND,
        HMENU,
        HINSTANCE,
        *const core::ffi::c_void,
        u32,
) -> HWND;

type GetWindowBandFn = unsafe extern "system" fn(HWND, *mut u32) -> BOOL;

fn band_apis() -> Option<(CreateWindowInBandFn, GetWindowBandFn)> {
        static APIS: OnceLock<Option<(CreateWindowInBandFn, GetWindowBandFn)>> = OnceLock::new();
        *APIS.get_or_init(|| unsafe {
                let user32 = GetModuleHandleW(w!("user32.dll")).ok()?;
                let create = GetProcAddress(user32, s!("CreateWindowInBand"))?;
                let get = GetProcAddress(user32, s!("GetWindowBand"))?;
                Some((std::mem::transmute(create), std::mem::transmute(get)))
        })
}

fn candidate_class_atom() -> Option<u16> {
        static ATOM: OnceLock<u16> = OnceLock::new();
        let atom = *ATOM.get_or_init(register_candidate_class);
        if atom == 0 { None } else { Some(atom) }
}

fn register_candidate_class() -> u16 {
        let instance = globals::dll_instance();
        if instance.0.is_null() {
                return 0;
        }
        let wc = WNDCLASSW {
                hInstance: instance,
                lpszClassName: PCWSTR(CANDIDATE_CLASS.as_ptr()),
                lpfnWndProc: Some(candidate_wnd_proc),
                style: CS_VREDRAW | CS_HREDRAW,
                hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
                ..Default::default()
        };
        let atom = unsafe { RegisterClassW(&wc) };
        if atom != 0 {
                return atom;
        }
        // The class can outlive a previous load of this DLL in the same
        // process. CreateWindowInBand needs the atom; recover it from a
        // throwaway window. 1410 = ERROR_CLASS_ALREADY_EXISTS.
        unsafe {
                if GetLastError().0 != 1410 {
                        return 0;
                }
                let Ok(hwnd) = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        PCWSTR(CANDIDATE_CLASS.as_ptr()),
                        w!(""),
                        WINDOW_STYLE(0),
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Some(instance),
                        None,
                ) else {
                        return 0;
                };
                if hwnd.is_invalid() {
                        return 0;
                }
                // GCW_ATOM (-32). The value sits in the low 16 bits.
                // The windows crate exports GetClassLongPtrW only on 64-bit.
                #[cfg(target_pointer_width = "64")]
                let atom = GetClassLongPtrW(hwnd, GET_CLASS_LONG_INDEX(-32)) as u16;
                #[cfg(target_pointer_width = "32")]
                let atom = GetClassLongW(hwnd, GET_CLASS_LONG_INDEX(-32)) as u16;
                let _ = DestroyWindow(hwnd);
                atom
        }
}

fn module_path_lower() -> &'static str {
        static PATH: OnceLock<String> = OnceLock::new();
        PATH.get_or_init(|| {
                let mut buf = [0u16; 520];
                let n = unsafe { windows::Win32::System::LibraryLoader::GetModuleFileNameW(None, &mut buf) } as usize;
                String::from_utf16_lossy(&buf[..n]).to_lowercase()
        })
        .as_str()
}

/// Band of `preferred` (the text view's window) or, failing that, the
/// foreground window — but only when that window belongs to this process.
/// Owned popups stay above their owner, which is what puts the candidate
/// list on top of the Start search panel. No window enumeration: that
/// broadcasts into other wndprocs and can re-enter TSF mid-edit.
fn host_band_and_owner(preferred: HWND) -> (u32, HWND) {
        let Some((_, get_band)) = band_apis() else {
                return (0, HWND::default());
        };
        unsafe {
                let our_pid = GetCurrentProcessId();
                let fg = GetForegroundWindow();
                for hwnd in [preferred, fg] {
                        if hwnd.is_invalid() {
                                continue;
                        }
                        let mut pid = 0u32;
                        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
                        if pid != our_pid {
                                continue;
                        }
                        let mut band = 0u32;
                        if get_band(hwnd, &mut band).as_bool() && band > 1 {
                                return (band, hwnd);
                        }
                }
                (0, HWND::default())
        }
}

/// Hosts whose text view has no Win32 line box (Start, Search, old UWP).
/// A zero-height GetTextExt here must not be treated as the CUAS bridge.
fn geometryless_host() -> bool {
        static CACHED: AtomicU8 = AtomicU8::new(0);
        match CACHED.load(Ordering::Relaxed) {
                1 => return false,
                2 => return true,
                _ => {}
        }
        let path = module_path_lower();
        let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
        // Start/Search live under SystemApps. Older UWP (Notes and the
        // other Windows 8-style hosts) run in ApplicationFrameHost, or as
        // a packaged exe whose top-level window is already above the
        // desktop band. Do not treat every WindowsApps process this way —
        // Photos and Terminal already show a normal popup.
        let yes = path.contains("\\systemapps\\")
                || file == "applicationframehost.exe"
                || file == "microsoft.notes.exe"
                || host_band_and_owner(HWND::default()).0 > 1;
        CACHED.store(if yes { 2 } else { 1 }, Ordering::Relaxed);
        yes
}

fn create_candidate_hwnd(parent: HWND) -> Option<HWND> {
        let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
        let style = WS_POPUP | WS_BORDER;
        if geometryless_host() {
                if let Some(hwnd) = create_in_host_band(ex, style, parent) {
                        return Some(hwnd);
                }
                static FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);
                if !FALLBACK_LOGGED.swap(true, Ordering::Relaxed) {
                        globals::log_error("candidate band window unavailable; using desktop popup");
                }
        }
        let hwnd = unsafe {
                CreateWindowExW(
                        ex,
                        PCWSTR(CANDIDATE_CLASS.as_ptr()),
                        w!(""),
                        style,
                        0,
                        0,
                        10,
                        10,
                        None,
                        None,
                        Some(globals::dll_instance()),
                        None,
                )
        }
        .ok()?;
        if hwnd.is_invalid() { None } else { Some(hwnd) }
}

fn create_in_host_band(ex: WINDOW_EX_STYLE, style: WINDOW_STYLE, preferred: HWND) -> Option<HWND> {
        let (create, _) = band_apis()?;
        let atom = candidate_class_atom()?;
        let (band, owner) = host_band_and_owner(preferred);
        if band <= 1 {
                static LOGGED: AtomicBool = AtomicBool::new(false);
                if !LOGGED.swap(true, Ordering::Relaxed) {
                        globals::log_error(&format!("candidate host band={band}; desktop popup"));
                }
                return None;
        }
        unsafe {
                let hwnd = create(ex, atom, w!(""), style, 0, 0, 10, 10, owner, HMENU::default(), globals::dll_instance(), std::ptr::null(), band);
                if !hwnd.is_invalid() {
                        static LOGGED: AtomicBool = AtomicBool::new(false);
                        if !LOGGED.swap(true, Ordering::Relaxed) {
                                globals::log_error(&format!(
                                        "candidate window in band {band} owner={:#x}",
                                        owner.0 as usize
                                ));
                        }
                        return Some(hwnd);
                }
                let err = GetLastError();
                // Same band, no owner — an owner in another band makes the
                // call fail, and a band-0 window sits under the shell again.
                let hwnd = create(
                        ex,
                        atom,
                        w!(""),
                        style,
                        0,
                        0,
                        10,
                        10,
                        HWND::default(),
                        HMENU::default(),
                        globals::dll_instance(),
                        std::ptr::null(),
                        band,
                );
                static LOGGED: AtomicBool = AtomicBool::new(false);
                if !LOGGED.swap(true, Ordering::Relaxed) {
                        globals::log_error(&format!(
                                "CreateWindowInBand band={band} owner failed err={} unowned={}",
                                err.0,
                                !hwnd.is_invalid()
                        ));
                }
                if hwnd.is_invalid() { None } else { Some(hwnd) }
        }
}

/// Where to anchor the list when GetTextExt has no line box. A non-zero
/// point is still a caret; a fully empty rect falls back to the Win32 caret,
/// then to the foreground window (under a small Notes-style window, just
/// below the Start search field otherwise).
fn anchor_for_degenerate(rect: RECT) -> RECT {
        if rect.left != 0 || rect.top != 0 {
                let mut placed = rect;
                if placed.bottom == placed.top {
                        placed.bottom = placed.top.saturating_add(1);
                }
                return placed;
        }
        unsafe {
                let mut gui = GUITHREADINFO::default();
                gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
                if GetGUIThreadInfo(GetCurrentThreadId(), &mut gui).is_ok() && !gui.hwndCaret.is_invalid() {
                        let c = gui.rcCaret;
                        if c.left != 0 || c.top != 0 || c.bottom != 0 {
                                let mut pt = POINT { x: c.left, y: c.bottom.max(c.top) };
                                if ClientToScreen(gui.hwndCaret, &mut pt).as_bool() {
                                        return RECT { left: pt.x, top: pt.y, right: pt.x, bottom: pt.y.saturating_add(1) };
                                }
                        }
                }
                let fg = GetForegroundWindow();
                let mut wr = RECT::default();
                if !fg.is_invalid() && GetWindowRect(fg, &mut wr).is_ok() && wr.right > wr.left {
                        let dpi = (GetDpiForWindow(fg) as i32).max(96);
                        let height = wr.bottom - wr.top;
                        // Compact UWP windows (Notes): hang the list under the frame.
                        // Large shell surfaces (Start): the query field sits near the top.
                        if height < (500 * dpi) / 96 {
                                return RECT { left: wr.left + (8 * dpi) / 96, top: wr.bottom, right: wr.left, bottom: wr.bottom.saturating_add(1) };
                        }
                        let y = wr.top + (64 * dpi) / 96;
                        let x = wr.left + (24 * dpi) / 96;
                        return RECT { left: x, top: y, right: x, bottom: y.saturating_add(1) };
                }
        }
        RECT { left: 80, top: 80, right: 80, bottom: 81 }
}

fn move_candidate_to(hwnd: HWND, window_state: &std::sync::Arc<Mutex<CandidateWindowState>>, page_size: usize, rect: RECT) {
        let (width, height) = {
                let state = window_state.lock().unwrap_or_else(|e| e.into_inner());
                (measure_width(&state, hwnd) as i32, measure_height(&state, page_size) as i32)
        };
        let (w, h) = (width.max(80), height.max(24));
        let (nx, ny) = CandidateWindow::clamp_to_work_area(rect.left, rect.bottom + 2, w, h, rect.top);
        unsafe {
                let _ = MoveWindow(hwnd, nx, ny, w, h, true);
        }
}

/// `None` means a real CUAS/IMM bridge (hide our window). `Some` is a rect
/// the candidate window should be moved to.
fn resolve_text_ext(rect: RECT) -> Option<RECT> {
        if rect.top != rect.bottom {
                return Some(rect);
        }
        if !geometryless_host() {
                return None;
        }
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if !LOGGED.swap(true, Ordering::Relaxed) {
                let anchor = anchor_for_degenerate(rect);
                globals::log_error(&format!(
                        "geometryless host text-ext=({},{}..{},{}) anchor=({},{})",
                        rect.left, rect.top, rect.right, rect.bottom, anchor.left, anchor.top
                ));
                return Some(anchor);
        }
        Some(anchor_for_degenerate(rect))
}

// Rendering goes through the shared candview module.

fn measure_height(state: &CandidateWindowState, _page_size: usize) -> u32 {
        crate::candview::measure_height(
                state.items.len(),
                state.selection,
                state.page_size,
                &state.view_style(),
                "",
        ) as u32
}

fn measure_width(state: &CandidateWindowState, hwnd: HWND) -> u32 {
        let width = crate::candview::measure_width(
                hwnd,
                &state.items,
                state.selection,
                state.page_size,
                &state.view_style(),
                "",
        ) as u32;
        globals::log(&format!("measure_width: items={} w={width}", state.items.len()));
        width
}

unsafe extern "system" fn candidate_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
                        // WS_EX_NOACTIVATE is not enough: a click can still pull
                        // keyboard focus off the host edit (Start / UWP then
                        // kills the composition). Refuse activation.
                        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
                        WM_PAINT => {
                                let mut ps = PAINTSTRUCT::default();
                                let hdc = BeginPaint(hwnd, &mut ps);
                                globals::guarded_value("WM_PAINT", (), || paint_candidates(hwnd, hdc));
                                let _ = EndPaint(hwnd, &ps);
                                LRESULT(0)
                        }
                        WM_NCDESTROY => {
                                // Reclaim the leaked Arc that create() stored in GWLP_USERDATA.
                                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<CandidateWindowState>;
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

fn paint_candidates(hwnd: HWND, hdc: HDC) {
        // The window state lives in the presenter; this is reached through
        // GWLP_USERDATA pointing at the presenter-shared state if set.
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<CandidateWindowState> };
        if ptr.is_null() {
                return;
        }
        let state = unsafe { &*ptr }.lock().unwrap_or_else(|e| e.into_inner());
        crate::candview::paint(hwnd, hdc, &state.items, state.selection, state.page_size, &state.view_style(), "");
}

// ---------------------------------------------------------------------
// CandidateListPresenter — port of CCandidateListUIPresenter.
// ---------------------------------------------------------------------

#[implement(ITfUIElement, ITfCandidateListUIElement, ITfCandidateListUIElementBehavior, ITfIntegratableCandidateListUIElement, ITfTextLayoutSink)]
pub struct CandidateListPresenter {
        service: IUnknown,
        page_size: usize,
        hide_window: bool,
        /// Set when GetTextExt reports a degenerate rect — the signature of a
        /// cicero-unaware (CUAS/IMM bridge) host that can't position UI. The
        /// bridge already hands the app a standard CANDIDATELIST (games draw
        /// it in-engine; classic apps get the system IME window), so our own
        /// window is suppressed there.
        bridge_detected: std::sync::Arc<AtomicBool>,

        window_state: std::sync::Arc<Mutex<CandidateWindowState>>,
        window: Mutex<Option<CandidateWindow>>,
        document_mgr: Mutex<Option<ITfDocumentMgr>>,
        ui_element_mgr: Mutex<Option<ITfUIElementMgr>>,
        context: Mutex<Option<ITfContext>>,
        range: Mutex<Option<ITfRange>>,

        ui_element_id: AtomicU32,
        client_id: AtomicU32,
        updated_flags: AtomicU32,
        is_show_mode: AtomicBool,
        /// Page width taken from the host's SetPageIndex (0 = it never
        /// called, so paging and digit keys stay on the IME setting).
        host_page_size: AtomicU32,
        layout_sink_cookie: AtomicU32,
        edit_cookie: AtomicU32,
        /// Self IUnknown so &self methods can enqueue deferred UI ops with a
        /// strong ref — see DEFERRED_UI_OPS. Set by start(), cleared by end().
        self_unk: Mutex<Option<IUnknown>>,
}

impl CandidateListPresenter {
        pub fn new(service: &IUnknown, page_size: usize, hide_window: bool) -> Self {
                globals::dll_add_ref();
                Self {
                        service: service.clone(),
                        page_size: page_size.max(1),
                        hide_window,
                        bridge_detected: std::sync::Arc::new(AtomicBool::new(false)),
                        window_state: std::sync::Arc::new(Mutex::new(CandidateWindowState::default())),
                        window: Mutex::new(None),
                        document_mgr: Mutex::new(None),
                        ui_element_mgr: Mutex::new(None),
                        context: Mutex::new(None),
                        range: Mutex::new(None),
                        ui_element_id: AtomicU32::new(u32::MAX),
                        client_id: AtomicU32::new(0),
                        updated_flags: AtomicU32::new(0),
                        is_show_mode: AtomicBool::new(true),
                        host_page_size: AtomicU32::new(0),
                        layout_sink_cookie: AtomicU32::new(TF_INVALID_COOKIE),
                        edit_cookie: AtomicU32::new(u32::MAX),
                        self_unk: Mutex::new(None),
                }
        }

        /// Port of _StartCandidateList.
        /// `ui_element_mgr` comes from the THREAD manager (ITfThreadMgr) —
        /// ITfDocumentMgr does not implement it.
        pub fn start(
                this: &ComObject<CandidateListPresenter>,
                client_id: u32,
                thread_mgr: &ITfThreadMgr,
                doc_mgr: &ITfDocumentMgr,
                context: &ITfContext,
                ec: u32,
                range: &ITfRange,
        ) -> Result<()> {
                let presenter = this.get();
                presenter.client_id.store(client_id, Ordering::Relaxed);
                presenter.edit_cookie.store(ec, Ordering::Relaxed);
                *presenter.document_mgr.lock().unwrap_or_else(|e| e.into_inner()) = Some(doc_mgr.clone());
                *presenter.context.lock().unwrap_or_else(|e| e.into_inner()) = Some(context.clone());
                *presenter.range.lock().unwrap_or_else(|e| e.into_inner()) = Some(range.clone());

                // Advise the layout sink so the window can follow the text.
                if let Ok(source) = context.cast::<ITfSource>() {
                        let sink: ITfTextLayoutSink = this.to_interface();
                        let unknown: IUnknown = sink.cast()?;
                        if let Ok(cookie) = unsafe { source.AdviseSink(&ITfTextLayoutSink::IID, &unknown) } {
                                presenter.layout_sink_cookie.store(cookie, Ordering::Relaxed);
                        }
                }

                // Register the UI element with the thread manager's element mgr.
                // Store the mgr before BeginUIElement so end() can always
                // EndUIElement a partially-registered element.
                let ui_mgr = thread_mgr.cast::<ITfUIElementMgr>();
                globals::log(&format!("presenter::start ui_mgr={}", ui_mgr.is_ok()));
                *presenter.ui_element_mgr.lock().unwrap_or_else(|e| e.into_inner()) = ui_mgr.ok();

                // BeginUIElement/UpdateUIElement/EndUIElement synchronously
                // broadcast to the app's ITfUIElementSink — and apps like
                // Chromium probe the document while handling the notification,
                // which deadlocks because we're inside an edit session holding
                // the doc lock. Defer ALL sink-touching calls out of the
                // session via the refresh window (same mechanism the langbar
                // compartment sink uses for the identical hazard).
                match this.cast::<IUnknown>() {
                        Ok(unk) => {
                                *presenter.self_unk.lock().unwrap_or_else(|e| e.into_inner()) = Some(unk);
                                presenter.defer_ui_op(DEFER_BEGIN);
                        }
                        Err(e) => {
                                globals::log_error(&format!("presenter::start: self_unk cast failed: {e:?}"));
                                presenter.is_show_mode.store(true, Ordering::Relaxed);
                        }
                }
                let _ = ec;
                Ok(())
        }

        /// Deferred tail of start() — runs on the UI thread via the refresh
        /// window, after the edit session released the document lock.
        /// `pbShow` from BeginUIElement is advisory — apps that report true
        /// (Chromium) rely on the framework's candidate UI which never
        /// actually renders for our element, so honoring it produced no
        /// window at all (0.9.3). SystemApps (Start, SearchHost) and older
        /// UWP hosts report the opposite (`pbShow` false, "we will draw")
        /// and also don't draw. Always create our own window; those hosts
        /// get it in their z-order band so it isn't covered.
        pub fn deferred_begin(this: &ComObject<CandidateListPresenter>) {
                let presenter = this.get();
                if let Some(ui_mgr) = presenter.ui_element_mgr.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                        let element: ITfUIElement = this.to_interface();
                        let mut show = BOOL(0);
                        let mut id = 0u32;
                        match unsafe { ui_mgr.BeginUIElement(&element, &mut show, &mut id) } {
                                Ok(()) => {}
                                Err(e) => {
                                        globals::log_error(&format!("BeginUIElement failed: {e:?}"));
                                        presenter.is_show_mode.store(true, Ordering::Relaxed);
                                        return;
                                }
                        }
                        static LOGGED: AtomicBool = AtomicBool::new(false);
                        if !LOGGED.swap(true, Ordering::Relaxed) {
                                globals::log_error(&format!("BeginUIElement ok: show={} id={id} systemapp={}", show.as_bool(), is_system_apps_process()));
                        }
                        presenter.ui_element_id.store(id, Ordering::Relaxed);
                        // pbShow=FALSE means a sink claimed the element. That
                        // claim is not honored: Chromium never paints, and
                        // Start / old UWP claim it and paint nothing either.
                        // Keep the flag update so a sink that does read the
                        // list still sees a dirty element, and always draw.
                        if !show.as_bool() {
                                presenter.updated_flags.store(TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING | TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                        }
                }
                presenter.is_show_mode.store(true, Ordering::Relaxed);
                let owner = {
                        let context = presenter.context.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        context
                                .as_ref()
                                .and_then(|context| unsafe {
                                        let view = context.GetActiveView().ok()?;
                                        view.GetWnd().ok()
                                })
                                .filter(|hwnd| !hwnd.is_invalid())
                                .unwrap_or_default()
                };
                match CandidateWindow::create(owner, presenter.page_size, presenter.window_state.clone()) {
                        Some(window) => {
                                globals::log("candidate window created");
                                *presenter.window.lock().unwrap_or_else(|e| e.into_inner()) = Some(window);
                        }
                        None => globals::log_error("candidate window create FAILED"),
                }
                presenter.refresh_window();
        }

        /// Port of _EndCandidateList.
        pub fn end(&self) {
                self.end_with_context(false, None);
        }

        pub fn end_with_context(&self, _force: bool, _context: Option<&ITfContext>) {
                self.edit_cookie.store(u32::MAX, Ordering::Relaxed);
                // Take self_unk first — taking (not cloning) leaves None so a
                // later Drop can't requeue an END for a half-destroyed object.
                let unk = self.self_unk.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(context) = self.context.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        if let Ok(source) = context.cast::<ITfSource>() {
                                unsafe {
                                        let _ = source.UnadviseSink(self.layout_sink_cookie.swap(TF_INVALID_COOKIE, Ordering::Relaxed));
                                }
                        }
                }
                // Hide our window right away (pure GDI on our own hwnd) —
                // the EndUIElement broadcast itself goes out after the edit
                // session releases the doc lock.
                if let Some(window) = self.window.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        window.show(false);
                }
                if let Some(unk) = unk {
                        if inline_ui_host() {
                                if let Ok(obj) = ComObject::<CandidateListPresenter>::cast_from(&unk) {
                                        obj.get().deferred_end();
                                }
                        } else {
                                DEFERRED_UI_OPS.with(|q| q.borrow_mut().push((unk, DEFER_END)));
                                crate::tray::post_deferred_ui();
                        }
                }
        }

        /// Deferred tail of end() — EndUIElement notifies app sinks, same
        /// reentrancy hazard as BeginUIElement.
        fn deferred_end(&self) {
                let id = self.ui_element_id.swap(u32::MAX, Ordering::Relaxed);
                if id != u32::MAX {
                        if let Some(ui_mgr) = self.ui_element_mgr.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                                unsafe {
                                        let _ = ui_mgr.EndUIElement(id);
                                }
                        }
                }
                *self.ui_element_mgr.lock().unwrap_or_else(|e| e.into_inner()) = None;
                *self.document_mgr.lock().unwrap_or_else(|e| e.into_inner()) = None;
                *self.range.lock().unwrap_or_else(|e| e.into_inner()) = None;
                *self.window.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }

        // -- list/selection model -----------------------------------------

        /// Width the host asked for, or the IME setting when it has not.
        fn effective_page_size(&self) -> usize {
                let host = self.host_page_size.load(Ordering::Relaxed);
                if host > 0 { host as usize } else { self.page_size }
        }

        /// `Some` only after SetPageIndex told us a real page width.
        pub fn host_page_size(&self) -> Option<u32> {
                let host = self.host_page_size.load(Ordering::Relaxed);
                if host == 0 { None } else { Some(host) }
        }

        pub fn set_text(&self, items: Vec<CandidateItem>) {
                {
                        let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        state.items = items;
                        state.selection = 0;
                        // A host that already declared its page width (FFXIV's
                        // 10) keeps it across candidate refreshes. Otherwise
                        // the IME setting (default 7) stands.
                        state.rebuild_pages(self.effective_page_size());
                }
                self.updated_flags
                        .fetch_or(TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING | TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                self.update_ui_element();
                self.refresh_window();
        }

        pub fn clear_list(&self) {
                {
                        let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        state.items.clear();
                        state.selection = 0;
                        state.page_start_indices.clear();
                }
                self.updated_flags.fetch_or(TF_CLUIE_COUNT | TF_CLUIE_SELECTION, Ordering::Relaxed);
                self.update_ui_element();
        }

        pub fn get_selection(&self) -> u32 {
                self.window_state.lock().unwrap_or_else(|e| e.into_inner()).selection as u32
        }

        pub fn set_selection(&self, index: i32) -> bool {
                let changed = self.window_state.lock().unwrap_or_else(|e| e.into_inner()).set_selection(index);
                if changed {
                        self.updated_flags.fetch_or(TF_CLUIE_SELECTION | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                        self.update_ui_element();
                        self.refresh_window();
                }
                changed
        }

        pub fn move_selection(&self, offset: i32) -> bool {
                let changed = self.window_state.lock().unwrap_or_else(|e| e.into_inner()).move_selection(offset);
                if changed {
                        self.updated_flags.fetch_or(TF_CLUIE_SELECTION | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                        self.update_ui_element();
                        self.refresh_window();
                }
                changed
        }

        pub fn move_page(&self, offset: i32) -> bool {
                let changed = self.window_state.lock().unwrap_or_else(|e| e.into_inner()).move_page(offset);
                if changed {
                        self.updated_flags.fetch_or(TF_CLUIE_SELECTION | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                        self.update_ui_element();
                        self.refresh_window();
                }
                changed
        }

        pub fn set_selection_in_page(&self, pos: usize) -> bool {
                let changed = self.window_state.lock().unwrap_or_else(|e| e.into_inner()).set_selection_in_page(pos);
                if changed {
                        self.updated_flags.fetch_or(TF_CLUIE_SELECTION, Ordering::Relaxed);
                        self.update_ui_element();
                        self.refresh_window();
                }
                changed
        }

        pub fn selected_candidate_string(&self) -> Option<String> {
                let state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                state.items.get(state.selection).map(|i| i.text.clone())
        }

        pub fn selected_candidate_index(&self) -> Option<u32> {
                let state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                if state.items.is_empty() {
                        None
                } else {
                        Some(state.selection as u32)
                }
        }

        pub fn selected_input_count(&self) -> usize {
                let state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                state.items.get(state.selection).map(|i| i.input_count).unwrap_or(0)
        }

        /// Port of AdviseUIChangedByArrowKey.
        pub fn advise_ui_changed_by_arrow_key(&self, function: KeystrokeFunction) {
                match function {
                        KeystrokeFunction::MoveUp => {
                                self.move_selection(-1);
                        }
                        KeystrokeFunction::MoveDown => {
                                self.move_selection(1);
                        }
                        KeystrokeFunction::MovePageUp => {
                                self.move_page(-1);
                        }
                        KeystrokeFunction::MovePageDown => {
                                self.move_page(1);
                        }
                        KeystrokeFunction::MovePageTop => {
                                self.set_selection(0);
                        }
                        KeystrokeFunction::MovePageBottom => {
                                self.set_selection(-1);
                        }
                        _ => {}
                }
        }

        pub fn set_text_color(&self, text: u32, back: u32) {
                let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                state.text_color = text;
                state.back_color = back;
                self.refresh_window();
        }

        pub fn update_font_sizes(&self) {
                let settings = crate::settings::load_settings();
                let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                state.candidate_font_size = settings.candidate_font_size;
                state.number_font_size = settings.candidate_number_font_size;
                state.comment_font_size = settings.candidate_comment_font_size;
                state.text_color = settings.candidate_text_color;
                state.back_color = settings.candidate_back_color;
                state.select_color = settings.candidate_select_color;
                state.comment_color = settings.candidate_comment_color;
                drop(state);
                self.refresh_window();
        }

        /// Port of _MoveWindowToTextExt + CGetTextExtentEditSession.
        /// Callers almost always run inside an edit session, so the stored
        /// edit cookie is usually still valid — try GetTextExt directly first.
        /// When it fails (stale cookie, e.g. from OnLayoutChange after the
        /// session ended) queue an async read-only session that moves AND
        /// shows the window, so it never appears at 0,0.
        /// Returns true when the window was positioned synchronously.
        pub fn move_window_to_text_ext(&self) -> bool {
                let context = self.context.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let range = self.range.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let window_state = self.window_state.clone();
                let window = self.window.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|w| w.hwnd);
                let (Some(context), Some(range), Some(hwnd)) = (context, range, window) else { return false };
                let page_size = self.page_size;
                let client_id = self.client_id.load(Ordering::Relaxed);
                let view = match unsafe { context.GetActiveView() } {
                        Ok(v) => v,
                        Err(_) => return false,
                };
                let ec = self.edit_cookie.load(Ordering::Relaxed);
                if ec != u32::MAX {
                        let mut rect = RECT::default();
                        let mut clipped = BOOL(0);
                        if unsafe { view.GetTextExt(ec, &range, &mut rect, &mut clipped) }.is_ok() {
                                // Weasel's CUAS test: zero *height* means a
                                // cicero-unaware host. Width may legitimately
                                // be zero — a collapsed caret has left==right.
                                // Start / old UWP also report zero height, but
                                // they are TSF hosts with no line box; they
                                // are excluded in resolve_text_ext.
                                match resolve_text_ext(rect) {
                                        None => {
                                                self.bridge_detected.store(true, Ordering::Relaxed);
                                                static BRIDGE_LOGGED: AtomicBool = AtomicBool::new(false);
                                                if !BRIDGE_LOGGED.swap(true, Ordering::Relaxed) {
                                                        globals::log_error(&format!("bridge detected: zero-height text-ext rect=({},{}..{},{})", rect.left, rect.top, rect.right, rect.bottom));
                                                }
                                        }
                                        Some(anchor) => {
                                                // A real rect means the earlier degenerate
                                                // read was transient (fresh text field not
                                                // laid out yet — AutoCAD/Qt hit this on the
                                                // first composition per field).
                                                self.bridge_detected.store(false, Ordering::Relaxed);
                                                move_candidate_to(hwnd, &window_state, page_size, anchor);
                                        }
                                }
                                return true;
                        }
                }
                if geometryless_host() {
                        self.bridge_detected.store(false, Ordering::Relaxed);
                        move_candidate_to(hwnd, &window_state, page_size, anchor_for_degenerate(RECT::default()));
                        return true;
                }
                let is_show = self.is_show_mode.load(Ordering::Relaxed);
                let bridge_detected = self.bridge_detected.clone();
                let _ = crate::composition::request_edit_session(
                        &self.service,
                        &context,
                        client_id,
                        TF_ES_ASYNCDONTCARE | TF_ES_READ,
                        Box::new(move |_state, ec, _ctx| {
                                let mut rect = RECT::default();
                                let mut clipped = BOOL(0);
                                unsafe { view.GetTextExt(ec, &range, &mut rect, &mut clipped)? };
                                match resolve_text_ext(rect) {
                                        None => {
                                                bridge_detected.store(true, Ordering::Relaxed);
                                                return Ok(());
                                        }
                                        Some(anchor) => {
                                                bridge_detected.store(false, Ordering::Relaxed);
                                                move_candidate_to(hwnd, &window_state, page_size, anchor);
                                        }
                                }
                                unsafe {
                                        if is_show && !bridge_detected.load(Ordering::Relaxed) {
                                                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                                                let _ = InvalidateRect(Some(hwnd), None, true);
                                        }
                                }
                                Ok(())
                        }),
                );
                false
        }

        fn refresh_window(&self) {
                let guard = self.window.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(window) = guard.as_ref() {
                        if self.is_show_mode.load(Ordering::Relaxed) && !self.hide_window && !self.bridge_detected.load(Ordering::Relaxed) {
                                let visible = window.is_visible();
                                drop(guard);
                                let _ = visible;
                                // Attempt the synchronous position first —
                                // failure queues an async session that both
                                // moves AND shows the window. Show regardless:
                                // a briefly-misplaced window beats an
                                // invisible one on first activation.
                                let _ = self.move_window_to_text_ext();
                                let guard = self.window.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(window) = guard.as_ref() {
                                        window.show(!self.bridge_detected.load(Ordering::Relaxed));
                                        window.invalidate();
                                }
                                return;
                        }
                        window.invalidate();
                }
        }

        /// Queue a UI op for post-session delivery — see DEFERRED_UI_OPS.
        fn defer_ui_op(&self, op: u8) {
                let unk = self.self_unk.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let Some(unk) = unk else { return };
                // SystemApps and older UWP hosts (ApplicationFrameHost, the
                // Notes package) may run the edit session on a thread that
                // never pumps our message-only window — keys are handled but
                // BeginUIElement never runs, so no candidate window. Their
                // sinks are OS-authored and safe to notify inside a session.
                // Chromium is not in this set: its sink probes the document
                // and deadlocks if called while we hold the doc lock.
                if inline_ui_host() {
                        if let Ok(obj) = ComObject::<CandidateListPresenter>::cast_from(&unk) {
                                match op {
                                        DEFER_BEGIN => Self::deferred_begin(&obj),
                                        DEFER_UPDATE => obj.get().do_update_ui_element(),
                                        DEFER_END => obj.get().deferred_end(),
                                        _ => {}
                                }
                        }
                        return;
                }
                DEFERRED_UI_OPS.with(|q| q.borrow_mut().push((unk, op)));
                crate::tray::post_deferred_ui();
        }

        /// UpdateUIElement synchronously calls the app's UIElement sink —
        /// never inside an edit session (same deadlock class as
        /// BeginUIElement). All callers are key-path/session code, so defer.
        fn update_ui_element(&self) {
                self.defer_ui_op(DEFER_UPDATE);
        }

        fn do_update_ui_element(&self) {
                let id = self.ui_element_id.load(Ordering::Relaxed);
                if id == u32::MAX {
                        return;
                }
                if let Some(ui_mgr) = self.ui_element_mgr.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        unsafe {
                                let _ = ui_mgr.UpdateUIElement(id);
                        }
                }
        }

        pub fn show(&self, show_window: bool) {
                let visible = self
                        .window
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .map(|w| w.is_visible())
                        .unwrap_or(false);
                if show_window && !self.hide_window {
                        let positioned = self.move_window_to_text_ext();
                        if self.bridge_detected.load(Ordering::Relaxed) {
                                // Bridge host — candidates surface through the
                                // synthesized CANDIDATELIST; keep ours hidden.
                                if let Some(window) = self.window.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                                        window.show(false);
                                }
                                return;
                        }
                        if let Some(window) = self.window.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                                window.show(positioned || visible);
                        }
                        return;
                }
                if let Some(window) = self.window.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        window.show(false);
                }
        }

        pub fn is_shown(&self) -> bool {
                self.window
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .map(|w| w.is_visible())
                        .unwrap_or(false)
        }
}

// ---------------------------------------------------------------------
// Deferred UI ops — BeginUIElement/UpdateUIElement/EndUIElement all
// synchronously notify the app's ITfUIElementSink. Chromium's sink probes
// the document while handling them, so calling them inside an edit session
// (where we hold the doc lock) deadlocks the host window. We queue the ops
// and the per-thread refresh window replays them once the session ends —
// the same deferral the langbar compartment sink already uses.
// ---------------------------------------------------------------------

/// True when the host process is a modern system shell under
/// %SystemRoot%\SystemApps (SearchHost, TextInputHost,
/// StartMenuExperienceHost). Their UI-element sinks are safe to notify
/// inside an edit session, and their thread may never pump the deferred
/// UI window — see defer_ui_op.
fn is_system_apps_process() -> bool {
        static IS: OnceLock<bool> = OnceLock::new();
        *IS.get_or_init(|| module_path_lower().contains("\\systemapps\\"))
}

fn inline_ui_host() -> bool {
        if is_system_apps_process() {
                return true;
        }
        let path = module_path_lower();
        let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
        file == "applicationframehost.exe" || file == "microsoft.notes.exe"
}

const DEFER_BEGIN: u8 = 0;
const DEFER_UPDATE: u8 = 1;
const DEFER_END: u8 = 2;

thread_local! {
        static DEFERRED_UI_OPS: std::cell::RefCell<Vec<(IUnknown, u8)>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn run_deferred_ui_ops() {
        loop {
                // FIFO — BEGIN must precede the UPDATE/END ops queued behind it.
                let next = DEFERRED_UI_OPS.with(|q| {
                        let mut q = q.borrow_mut();
                        if q.is_empty() { None } else { Some(q.remove(0)) }
                });
                let Some((unk, op)) = next else { break };
                let Ok(obj) = ComObject::<CandidateListPresenter>::cast_from(&unk) else { continue };
                match op {
                        DEFER_BEGIN => CandidateListPresenter::deferred_begin(&obj),
                        DEFER_UPDATE => obj.get().do_update_ui_element(),
                        DEFER_END => obj.get().deferred_end(),
                        _ => {}
                }
        }
}

impl Drop for CandidateListPresenter {
        fn drop(&mut self) {
                self.end();
                globals::dll_release();
        }
}

impl ITfUIElement_Impl for CandidateListPresenter_Impl {
        fn GetDescription(&self) -> Result<BSTR> {
                Ok(BSTR::from("Cand"))
        }
        fn GetGUID(&self) -> Result<GUID> {
                Ok(globals::GUID_CANDIDATE_UI_ELEMENT)
        }
        fn Show(&self, bshow: BOOL) -> Result<()> {
                globals::guarded("CandUIElement::Show", || {
                        let want = bshow.as_bool();
                        // Shell and older UWP sinks call Show(FALSE) to say
                        // they will paint the element. They don't, and a
                        // later call is not followed by refresh_window, so
                        // keep the list visible while it still has rows.
                        if !want && geometryless_host() {
                                let has_items = !self
                                        .window_state
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .items
                                        .is_empty();
                                if has_items {
                                        self.show(true);
                                        return Ok(());
                                }
                        }
                        self.show(want);
                        Ok(())
                })
        }
        fn IsShown(&self) -> Result<BOOL> {
                globals::guarded("CandUIElement::IsShown", || Ok(BOOL(self.is_shown() as i32)))
        }
}

impl ITfCandidateListUIElement_Impl for CandidateListPresenter_Impl {
        fn GetUpdatedFlags(&self) -> Result<u32> {
                Ok(self.updated_flags.load(Ordering::Relaxed))
        }
        fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> {
                self.document_mgr
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone()
                        .ok_or_else(|| Error::from_hresult(E_UNEXPECTED))
        }
        fn GetCount(&self) -> Result<u32> {
                Ok(self.window_state.lock().unwrap_or_else(|e| e.into_inner()).count())
        }
        fn GetSelection(&self) -> Result<u32> {
                Ok(self.get_selection())
        }
        fn GetString(&self, uindex: u32) -> Result<BSTR> {
                globals::guarded("CandUIElement::GetString", || {
                        let state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        let Some(item) = state.items.get(uindex as usize) else {
                                return Err(Error::from_hresult(E_FAIL));
                        };
                        Ok(BSTR::from(item.text.as_str()))
                })
        }
        fn GetPageIndex(&self, pindex: *mut u32, usize_: u32, pupagecnt: *mut u32) -> Result<()> {
                globals::guarded("CandUIElement::GetPageIndex", || {
                        let state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        unsafe {
                                if !pupagecnt.is_null() {
                                        *pupagecnt = state.page_start_indices.len() as u32;
                                }
                                if !pindex.is_null() {
                                        let count = (usize_ as usize).min(state.page_start_indices.len());
                                        for i in 0..count {
                                                *pindex.add(i) = state.page_start_indices[i] as u32;
                                        }
                                }
                        }
                        Ok(())
                })
        }
        fn SetPageIndex(&self, pindex: *const u32, upagecnt: u32) -> Result<()> {
                globals::guarded("CandUIElement::SetPageIndex", || {
                        if pindex.is_null() || upagecnt == 0 {
                                return Err(Error::from_hresult(E_INVALIDARG));
                        }
                        let mut starts = Vec::with_capacity(upagecnt as usize);
                        unsafe {
                                for i in 0..upagecnt as usize {
                                        starts.push(*pindex.add(i) as usize);
                                }
                        }
                        // The host's page width is the gap between the first
                        // two page starts (FFXIV: 0, 10, 20 → 10). One page
                        // alone doesn't say how wide the page is, so the IME
                        // default stays in force.
                        let width = starts.get(1).and_then(|second| starts.first().map(|first| second.saturating_sub(*first))).filter(|w| *w > 0);
                        if let Some(width) = width {
                                self.host_page_size.store(width as u32, Ordering::Relaxed);
                        }
                        static LOGGED: AtomicBool = AtomicBool::new(false);
                        if !LOGGED.swap(true, Ordering::Relaxed) {
                                globals::log_error(&format!("SetPageIndex pages={upagecnt} width={}", width.unwrap_or(0)));
                        }
                        let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(width) = width {
                                state.page_size = width;
                        }
                        state.page_start_indices = starts;
                        Ok(())
                })
        }
        fn GetCurrentPage(&self) -> Result<u32> {
                globals::guarded("CandUIElement::GetCurrentPage", || {
                        Ok(self.window_state.lock().unwrap_or_else(|e| e.into_inner()).current_page() as u32)
                })
        }
}

impl ITfCandidateListUIElementBehavior_Impl for CandidateListPresenter_Impl {
        fn SetSelection(&self, nindex: u32) -> Result<()> {
                globals::guarded("CandBehavior::SetSelection", || {
                        self.set_selection(nindex as i32);
                        Ok(())
                })
        }
        fn Finalize(&self) -> Result<()> {
                // Commit the selected candidate via the service key handler path.
                Ok(())
        }
        fn Abort(&self) -> Result<()> {
                globals::guarded("CandBehavior::Abort", || {
                        self.show(false);
                        Ok(())
                })
        }
}

impl ITfIntegratableCandidateListUIElement_Impl for CandidateListPresenter_Impl {
        fn SetIntegrationStyle(&self, _guidintegrationstyle: &GUID) -> Result<()> {
                Ok(())
        }
        fn GetSelectionStyle(&self) -> Result<TfIntegratableCandidateListSelectionStyle> {
                Ok(TfIntegratableCandidateListSelectionStyle(0))
        }
        fn OnKeyDown(&self, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
                Ok(BOOL(0))
        }
        fn ShowCandidateNumbers(&self) -> Result<BOOL> {
                Ok(BOOL(0))
        }
        fn FinalizeExactCompositionString(&self) -> Result<()> {
                Ok(())
        }
}

impl ITfTextLayoutSink_Impl for CandidateListPresenter_Impl {
        fn OnLayoutChange(&self, _pic: Ref<'_, ITfContext>, lcode: TfLayoutCode, _pview: Ref<'_, ITfContextView>) -> Result<()> {
                globals::guarded("OnLayoutChange", || {
                        match lcode {
                                TF_LC_CHANGE => {
                                        self.move_window_to_text_ext();
                                }
                                TF_LC_DESTROY => {
                                        self.show(false);
                                }
                                _ => {}
                        }
                        Ok(())
                })
        }
}
