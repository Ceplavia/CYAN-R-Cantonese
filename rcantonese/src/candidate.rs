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
                                        // `as _`: SetWindowLongPtrW takes isize
                                        // on x64 but i32 on x86.
                                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as _);
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

// Rendering goes through the shared candview module — the IMM32 popup
// paints the same rows, fonts and colors.

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
        /// `pbShow` from BeginUIElement is advisory only — we draw our own
        /// window regardless: apps that report true (Chromium) rely on the
        /// framework's candidate UI which never actually renders for our
        /// element, so honoring it produced no window at all (0.9.3).
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
                        globals::log(&format!("BeginUIElement ok: show={} id={id}", show.as_bool()));
                        presenter.ui_element_id.store(id, Ordering::Relaxed);
                        if !show.as_bool() {
                                presenter.updated_flags.store(TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING | TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE, Ordering::Relaxed);
                        }
                }
                presenter.is_show_mode.store(true, Ordering::Relaxed);
                match CandidateWindow::create(HWND::default(), presenter.page_size, presenter.window_state.clone()) {
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
                        DEFERRED_UI_OPS.with(|q| q.borrow_mut().push((unk, DEFER_END)));
                        crate::tray::post_deferred_ui();
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
                                if rect.top == rect.bottom || rect.left == rect.right {
                                        self.bridge_detected.store(true, Ordering::Relaxed);
                                        return true;
                                }
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
                                if rect.top == rect.bottom || rect.left == rect.right {
                                        bridge_detected.store(true, Ordering::Relaxed);
                                        return Ok(());
                                }
                                let (width, height) = {
                                        let state = window_state.lock().unwrap_or_else(|e| e.into_inner());
                                        (measure_width(&state, hwnd) as i32, measure_height(&state, page_size) as i32)
                                };
                                let (w, h) = (width.max(80), height.max(24));
                                let (nx, ny) = CandidateWindow::clamp_to_work_area(rect.left, rect.bottom + 2, w, h, rect.top);
                                unsafe {
                                        let _ = MoveWindow(hwnd, nx, ny, w, h, true);
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
                if let Some(unk) = unk {
                        DEFERRED_UI_OPS.with(|q| q.borrow_mut().push((unk, op)));
                        crate::tray::post_deferred_ui();
                }
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
