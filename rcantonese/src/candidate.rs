// Candidate list presenter — port of CandidateListUIPresenter.cpp.
// The candidate window is a plain GDI popup (Direct2D rendering is a later
// refinement); the paging/selection model follows CandidateWindow.cpp.
#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
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
        text_color: u32,
        back_color: u32,
        select_color: u32,
        comment_color: u32,
        candidate_font_size: u32,
        number_font_size: u32,
        comment_font_size: u32,
}

impl CandidateWindowState {
        fn count(&self) -> u32 {
                self.items.len() as u32
        }

        fn rebuild_pages(&mut self, page_size: usize) {
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

        /// Port of _SetSelectionInPage.
        fn set_selection_in_page(&mut self, pos: usize) -> bool {
                if pos >= self.items.len() {
                        return false;
                }
                let page = self.current_page();
                let Some((start, _)) = self.page_bounds(page) else { return false };
                let index = start + pos;
                if index >= self.items.len() {
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
        fn register_class() -> Option<u16> {
                let instance = globals::dll_instance();
                if instance.0.is_null() {
                        return None;
                }
                let mut class = WNDCLASSW::default();
                unsafe {
                        if GetClassInfoW(Some(instance), PCWSTR(CANDIDATE_CLASS.as_ptr()), &mut class).is_ok() {
                                return Some(0);
                        }
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
                if atom == 0 { None } else { Some(atom) }
        }

        fn create(parent: HWND, page_size: usize, state: std::sync::Arc<Mutex<CandidateWindowState>>) -> Option<Self> {
                Self::register_class()?;
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
                                Some(parent),
                                None,
                                Some(globals::dll_instance()),
                                None,
                        )
                };
                match hwnd {
                        Ok(hwnd) if !hwnd.is_invalid() => {
                                let window = Self { hwnd, state, page_size };
                                // Give the wnd proc access to the shared state. One extra
                                // strong ref is leaked intentionally — it is reclaimed
                                // when the window is destroyed.
                                let ptr = std::sync::Arc::into_raw(window.state.clone());
                                unsafe {
                                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
                                }
                                Some(window)
                        }
                        _ => None,
                }
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
        /// area. When there's no room below the caret, flip the window above
        /// the caret line like other IMEs do.
        fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, caret_top: i32) -> (i32, i32) {
                unsafe {
                        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
                        let mut info = MONITORINFO {
                                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                                ..Default::default()
                        };
                        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                                return (x, y);
                        }
                        let work = info.rcWork;
                        let nx = x.clamp(work.left, (work.right - w).max(work.left));
                        let mut ny = if y + h > work.bottom { caret_top - h - 2 } else { y };
                        if ny < work.top {
                                ny = work.top;
                        }
                        if ny + h > work.bottom {
                                ny = (work.bottom - h).max(work.top);
                        }
                        (nx, ny)
                }
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

fn row_height(state: &CandidateWindowState) -> i32 {
        state.candidate_font_size.max(state.comment_font_size).max(state.number_font_size) as i32 + 10
}

fn measure_height(state: &CandidateWindowState, page_size: usize) -> u32 {
        let page = state.current_page();
        let rows = state
                .page_bounds(page)
                .map(|(s, e)| e - s + 1)
                .unwrap_or(0)
                .min(page_size.max(1));
        (rows.max(1) as i32 * row_height(state) + 8) as u32
}

fn measure_width(state: &CandidateWindowState, hwnd: HWND) -> u32 {
        let page = state.current_page();
        let Some((start, end)) = state.page_bounds(page) else { return 120 };
        let end = end.min(state.items.len().saturating_sub(1));
        if state.items.is_empty() || start > end {
                return 120;
        }
        // Measure on the candidate window's own DC — its DPI context matches
        // the BeginPaint DC used by paint_candidates. GetDC(None) (screen DC)
        // can resolve the font at a different DPI in some host processes,
        // which made Notepad measure ~30% too narrow and clip comments.
        unsafe {
                let hdc = GetDC(Some(hwnd));
                if hdc.is_invalid() {
                        return 200;
                }
                let font = CreateFontW(
                        -(state.candidate_font_size as i32),
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
                );
                let old_font = SelectObject(hdc, font.into());
                let mut max_cx = 40i32;
                for (index, item) in state.items[start..=end].iter().enumerate() {
                        let line = if item.comment.is_empty() {
                                format!("{}. {}", index + 1 - start, item.text)
                        } else {
                                format!("{}. {}  {}", index + 1 - start, item.text, item.comment)
                        };
                        let wide: Vec<u16> = line.encode_utf16().collect();
                        let mut size = SIZE::default();
                        if GetTextExtentPoint32W(hdc, &wide, &mut size).as_bool() {
                                max_cx = max_cx.max(size.cx);
                        }
                }
                SelectObject(hdc, old_font);
                let _ = DeleteObject(font.into());
                let _ = ReleaseDC(Some(hwnd), hdc);
                let width = (max_cx + 16) as u32;
                globals::log(&format!("measure_width: items={} max_cx={max_cx} w={width}", state.items.len()));
                width
        }
}

unsafe extern "system" fn candidate_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
                match msg {
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
        unsafe {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<CandidateWindowState>;
                if ptr.is_null() {
                        return;
                }
                let state = &*ptr;
                let state = state.lock().unwrap_or_else(|e| e.into_inner());
                // Paint the background — the class brush is system COLOR_WINDOW;
                // the configured back color overrides it here.
                {
                        let mut client = RECT::default();
                        let _ = GetClientRect(hwnd, &mut client);
                        let brush = CreateSolidBrush(COLORREF(state.back_color));
                        FillRect(hdc, &client, brush);
                        let _ = DeleteObject(brush.into());
                }
                let rh = row_height(&state);
                let page = state.current_page();
                let Some((start, end)) = state.page_bounds(page) else { return };
                let font = CreateFontW(
                        -(state.candidate_font_size as i32),
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
                        DEFAULT_PITCH.0 as u32,
                        w!("Microsoft JhengHei"),
                );
                let old_font = SelectObject(hdc, font.into());
                // Track the true rendered right edge on the paint DC — its DPI
                // context is authoritative; if the window is too narrow we
                // resize below instead of trusting an off-DC measurement.
                let mut max_right = 0i32;
                let mut shown = 0usize; // numbering counts only non-separator rows
                for (i, index) in (start..=end).enumerate() {
                        let Some(item) = state.items.get(index) else { break };
                        let y = 4 + i as i32 * rh;
                        let mut rc = RECT { left: 0, top: y, right: 4096, bottom: y + rh };
                        if item.separator {
                                // Divider line across the row, vertically centered.
                                let pen = CreatePen(PS_SOLID, 1, COLORREF(state.comment_color));
                                let old_pen = SelectObject(hdc, pen.into());
                                let mid = y + rh / 2;
                                let mut client = RECT::default();
                                let _ = GetClientRect(hwnd, &mut client);
                                let _ = MoveToEx(hdc, 8, mid, None);
                                let _ = LineTo(hdc, client.right - 8, mid);
                                SelectObject(hdc, old_pen);
                                let _ = DeleteObject(pen.into());
                                continue;
                        }
                        shown += 1;
                        if index == state.selection {
                                let brush = CreateSolidBrush(COLORREF(state.select_color));
                                FillRect(hdc, &rc, brush);
                                let _ = DeleteObject(brush.into());
                        }
                        let label = format!("{}. {}", shown, item.text);
                        let mut wide: Vec<u16> = label.encode_utf16().collect();
                        SetBkMode(hdc, TRANSPARENT);
                        SetTextColor(hdc, COLORREF(state.text_color));
                        rc.left = 8;
                        DrawTextW(hdc, &mut wide, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        let mut size = SIZE::default();
                        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
                        let mut right = 8 + size.cx;
                        if !item.comment.is_empty() {
                                let mut comment: Vec<u16> = format!(" {}", item.comment).encode_utf16().collect();
                                rc.left = 8 + size.cx + 6;
                                SetTextColor(hdc, COLORREF(state.comment_color));
                                DrawTextW(hdc, &mut comment, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                                let mut csize = SIZE::default();
                                let _ = GetTextExtentPoint32W(hdc, &comment, &mut csize);
                                right = rc.left + csize.cx;
                        }
                        max_right = max_right.max(right);
                }
                SelectObject(hdc, old_font);
                let _ = DeleteObject(font.into());
                // Self-correct the window width using the real rendered extent.
                let needed = max_right + 8;
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                if needed > client.right - client.left {
                        let mut wr = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut wr);
                        let (nx, _) = CandidateWindow::clamp_to_work_area(
                                wr.left,
                                wr.top,
                                needed + 4,
                                wr.bottom - wr.top,
                                wr.top,
                        );
                        let _ = MoveWindow(hwnd, nx, wr.top, needed + 4, wr.bottom - wr.top, true);
                }
        }
}

// ---------------------------------------------------------------------
// CandidateListPresenter — port of CCandidateListUIPresenter.
// ---------------------------------------------------------------------

#[implement(ITfUIElement, ITfCandidateListUIElement, ITfCandidateListUIElementBehavior, ITfIntegratableCandidateListUIElement, ITfTextLayoutSink)]
pub struct CandidateListPresenter {
        service: IUnknown,
        page_size: usize,
        hide_window: bool,

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
        layout_sink_cookie: AtomicU32,
        edit_cookie: AtomicU32,
}

impl CandidateListPresenter {
        pub fn new(service: &IUnknown, page_size: usize, hide_window: bool) -> Self {
                globals::dll_add_ref();
                Self {
                        service: service.clone(),
                        page_size: page_size.max(1),
                        hide_window,
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
                        layout_sink_cookie: AtomicU32::new(TF_INVALID_COOKIE),
                        edit_cookie: AtomicU32::new(u32::MAX),
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
                if let Some(ui_mgr) = presenter.ui_element_mgr.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                        let element: ITfUIElement = this.to_interface();
                        let mut show = BOOL(0);
                        let mut id = 0u32;
                        match unsafe { ui_mgr.BeginUIElement(&element, &mut show, &mut id) } {
                                Ok(()) => {}
                                Err(e) => {
                                        globals::log_error(&format!("BeginUIElement failed: {e:?}"));
                                        presenter.is_show_mode.store(true, Ordering::Relaxed);
                                        return Err(e);
                                }
                        }
                        globals::log(&format!("BeginUIElement ok: show={} id={id}", show.as_bool()));
                        presenter.is_show_mode.store(show.as_bool(), Ordering::Relaxed);
                        presenter.ui_element_id.store(id, Ordering::Relaxed);
                        if !show.as_bool() {
                                presenter.updated_flags.store(TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING | TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                        }
                } else {
                        // Could not register the element — still show our own window.
                        presenter.is_show_mode.store(true, Ordering::Relaxed);
                }

                // Create the window (positioned under the composition range).
                match CandidateWindow::create(HWND::default(), presenter.page_size, presenter.window_state.clone()) {
                        Some(window) => {
                                globals::log("candidate window created");
                                *presenter.window.lock().unwrap_or_else(|e| e.into_inner()) = Some(window);
                        }
                        None => globals::log_error("candidate window create FAILED"),
                }
                let _ = ec;
                presenter.refresh_window();
                Ok(())
        }

        /// Port of _EndCandidateList.
        pub fn end(&self) {
                self.end_with_context(false, None);
        }

        pub fn end_with_context(&self, _force: bool, _context: Option<&ITfContext>) {
                self.edit_cookie.store(u32::MAX, Ordering::Relaxed);
                if let Some(context) = self.context.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        if let Ok(source) = context.cast::<ITfSource>() {
                                unsafe {
                                        let _ = source.UnadviseSink(self.layout_sink_cookie.swap(TF_INVALID_COOKIE, Ordering::Relaxed));
                                }
                        }
                }
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

        pub fn set_text(&self, items: Vec<CandidateItem>) {
                {
                        let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        state.items = items;
                        state.selection = 0;
                        state.rebuild_pages(self.page_size);
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
                                let (width, height) = {
                                        let state = window_state.lock().unwrap_or_else(|e| e.into_inner());
                                        (measure_width(&state, hwnd) as i32, measure_height(&state, page_size) as i32)
                                };
                                let (w, h) = (width.max(80), height.max(24));
                                let (nx, ny) = CandidateWindow::clamp_to_work_area(rect.left, rect.bottom + 2, w, h, rect.top);
                                unsafe {
                                        let _ = MoveWindow(hwnd, nx, ny, w, h, true);
                                }
                                return true;
                        }
                }
                let is_show = self.is_show_mode.load(Ordering::Relaxed);
                let _ = crate::composition::request_edit_session(
                        &self.service,
                        &context,
                        client_id,
                        TF_ES_ASYNCDONTCARE | TF_ES_READ,
                        Box::new(move |_state, ec, _ctx| {
                                let mut rect = RECT::default();
                                let mut clipped = BOOL(0);
                                unsafe { view.GetTextExt(ec, &range, &mut rect, &mut clipped)? };
                                let (width, height) = {
                                        let state = window_state.lock().unwrap_or_else(|e| e.into_inner());
                                        (measure_width(&state, hwnd) as i32, measure_height(&state, page_size) as i32)
                                };
                                let (w, h) = (width.max(80), height.max(24));
                                let (nx, ny) = CandidateWindow::clamp_to_work_area(rect.left, rect.bottom + 2, w, h, rect.top);
                                unsafe {
                                        let _ = MoveWindow(hwnd, nx, ny, w, h, true);
                                        if is_show {
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
                        if self.is_show_mode.load(Ordering::Relaxed) && !self.hide_window {
                                let visible = window.is_visible();
                                drop(guard);
                                if self.move_window_to_text_ext() || visible {
                                        let guard = self.window.lock().unwrap_or_else(|e| e.into_inner());
                                        if let Some(window) = guard.as_ref() {
                                                window.show(true);
                                                window.invalidate();
                                        }
                                }
                                return;
                        }
                        window.invalidate();
                }
        }

        fn update_ui_element(&self) {
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
                        self.show(bshow.as_bool());
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
                        if pindex.is_null() {
                                return Err(Error::from_hresult(E_INVALIDARG));
                        }
                        let mut state = self.window_state.lock().unwrap_or_else(|e| e.into_inner());
                        state.page_start_indices.clear();
                        unsafe {
                                for i in 0..upagecnt as usize {
                                        state.page_start_indices.push(*pindex.add(i) as usize);
                                }
                        }
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
