// TSF caret bridge — asks the thread's TSF manager for the focused
// document's selection rectangle in screen coordinates.
//
// Why this exists: Chromium/Electron hosts run the IMM32 path for our IME
// (the hklSubstitute layout is an .ime), but they never create a system
// caret nor call ImmSetCandidateWindow unless the renderer reports caret
// bounds — canvas-driven editors don't. Their TSF text store DOES know the
// field position though, and our TIP shares the process, so the focused
// document is reachable through TF_ThreadMgr on this very thread. In
// pure-IMM32 apps (WoW) GetFocus simply fails and we fall back.

use std::sync::{Arc, Mutex};

use windows::core::implement;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::TextServices::*;

/// Confidence of the rect we managed to get — a real selection extent is
/// the caret itself; the doc-end anchor and the view rect are only
/// approximations and can point at the wrong document in multi-doc apps
/// (Devin's chat history vs. its input box).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RectKind {
        Selection,
        DocEnd,
        View,
}

#[implement(ITfEditSession)]
struct CaretQuery {
        context: ITfContext,
        out: Arc<Mutex<Option<(RECT, RectKind)>>>,
}

impl ITfEditSession_Impl for CaretQuery_Impl {
        fn DoEditSession(&self, ec: u32) -> windows::core::Result<()> {
                let view = unsafe { self.context.GetActiveView() }?;
                let mut sel = [TF_SELECTION::default()];
                let mut fetched = 0u32;
                unsafe { self.context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut fetched) }?;
                if fetched > 0 {
                        if let Some(range) = sel[0].range.as_ref() {
                                let mut rc = RECT::default();
                                let mut clipped = windows::core::BOOL::default();
                                if unsafe { view.GetTextExt(ec, range, &mut rc, &mut clipped) }.is_ok()
                                        && rc.right > rc.left
                                        && rc.bottom > rc.top
                                {
                                        *self.out.lock().unwrap_or_else(|e| e.into_inner()) =
                                                Some((rc, RectKind::Selection));
                                        return Ok(());
                                }
                        }
                }
                // No selection object (Chromium's store is dormant while it
                // drives IMM32) — the context's END anchor is a collapsed
                // range at the insertion point, so GetTextExt on it gives
                // the caret rect that follows the text as it grows.
                if let Ok(range) = unsafe { self.context.GetEnd(ec) } {
                        let mut rc = RECT::default();
                        let mut clipped = windows::core::BOOL::default();
                        if unsafe { view.GetTextExt(ec, &range, &mut rc, &mut clipped) }.is_ok()
                                && rc.bottom > rc.top
                        {
                                crate::globals::log(&format!(
                                        "ime: tsfbridge docext ({},{})-({},{})",
                                        rc.left, rc.top, rc.right, rc.bottom
                                ));
                                *self.out.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Some((rc, RectKind::DocEnd));
                                return Ok(());
                        }
                }
                // Last resort — the view's screen rect; its bottom-left is
                // still inside/near the editing area.
                match unsafe { view.GetScreenExt() } {
                        Ok(rc) if rc.right > rc.left && rc.bottom > rc.top => {
                                crate::globals::log(&format!(
                                        "ime: tsfbridge screenext ({},{})-({},{})",
                                        rc.left, rc.top, rc.right, rc.bottom
                                ));
                                *self.out.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Some((rc, RectKind::View));
                        }
                        _ => crate::globals::log("ime: tsfbridge no rect"),
                }
                Ok(())
        }
}

/// Screen rect of the focused text field's selection/caret plus a
/// confidence tag, or None when the foreground document isn't reachable
/// through TSF.
pub fn caret_rect() -> Option<(RECT, RectKind)> {
        crate::globals::log("ime: tsfbridge enter");
        let tm: ITfThreadMgr = unsafe {
                CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)
        }
        .ok()?;
        crate::globals::log("ime: tsfbridge tm ok");
        let dm = unsafe { tm.GetFocus() }.ok()?;
        crate::globals::log("ime: tsfbridge focus ok");
        let context = unsafe { dm.GetTop() }.ok()?;
        crate::globals::log("ime: tsfbridge ctx ok");
        let out: Arc<Mutex<Option<(RECT, RectKind)>>> = Arc::new(Mutex::new(None));
        let sink: ITfEditSession = CaretQuery {
                context: context.clone(),
                out: out.clone(),
        }
        .into();
        // Synchronous read-only session — runs DoEditSession on this thread.
        let _hr: windows::core::HRESULT = unsafe {
                context.RequestEditSession(GetCurrentThreadId(), &sink, TF_ES_SYNC | TF_ES_READ)
        }
        .ok()?;
        crate::globals::log("ime: tsfbridge session done");
        out.lock().unwrap_or_else(|e| e.into_inner()).take()
}
