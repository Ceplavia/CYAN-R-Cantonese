// RCantoneseService — the TSF text input processor object.
// Port of Jyutping.cpp (Activate/Deactivate + sinks) and related plumbing.
#![allow(dead_code)]

use std::sync::Mutex;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::UI::TextServices::*;

use crate::candidate::CandidateListPresenter;
use crate::globals;
use crate::keytable::CandidateMode;
use crate::processor::Processor;

pub struct ServiceState {
        pub thread_mgr: Option<ITfThreadMgr>,
        pub client_id: u32,
        pub activate_flags: u32,

        pub thread_mgr_event_sink_cookie: u32,
        pub text_edit_sink_context: Option<ITfContext>,
        pub text_edit_sink_cookie: u32,
        pub lang_profile_notify_sink_cookie: u32,
        pub thread_focus_sink_cookie: u32,

        pub composition: Option<ITfComposition>,
        pub context: Option<ITfContext>,

        pub ga_display_attribute_input: u32,
        pub ga_display_attribute_converted: u32,

        pub candidate_mode: CandidateMode,
        pub candidate_presenter: Option<ComObject<CandidateListPresenter>>,
        pub options_mode: bool,
        pub options_standalone_presenter: bool,

        pub doc_mgr_last_focused: Option<ITfDocumentMgr>,

        /// Owned IUnknown of this service object — used by edit sessions and
        /// sinks that need to reach back into the service state.
        pub self_iunknown: Option<IUnknown>,

        pub processor: Option<std::sync::Arc<Mutex<Processor>>>,
        pub composition_reading_code: String,

        /// Active after "/" opens the punctuation list — accumulates the
        /// rime-style "/x" symbol/character query (empty = the slash menu).
        pub symbol_query: Option<String>,
}

impl ServiceState {
        fn new() -> Self {
                Self {
                        thread_mgr: None,
                        client_id: 0,
                        activate_flags: 0,
                        thread_mgr_event_sink_cookie: TF_INVALID_COOKIE,
                        text_edit_sink_context: None,
                        text_edit_sink_cookie: TF_INVALID_COOKIE,
                        lang_profile_notify_sink_cookie: TF_INVALID_COOKIE,
                        thread_focus_sink_cookie: TF_INVALID_COOKIE,
                        composition: None,
                        context: None,
                        ga_display_attribute_input: u32::MAX,
                        ga_display_attribute_converted: u32::MAX,
                        candidate_mode: CandidateMode::None,
                        candidate_presenter: None,
                        options_mode: false,
                        options_standalone_presenter: false,
                        doc_mgr_last_focused: None,
                        self_iunknown: None,
                        processor: None,
                        composition_reading_code: String::new(),
                        symbol_query: None,
                }
        }

        pub fn is_composing(&self) -> bool {
                self.composition.is_some()
        }

        pub fn thread_mgr(&self) -> Option<&ITfThreadMgr> {
                self.thread_mgr.as_ref()
        }
}

#[implement(
        ITfTextInputProcessor,
        ITfTextInputProcessorEx,
        ITfThreadMgrEventSink,
        ITfTextEditSink,
        ITfKeyEventSink,
        ITfCompositionSink,
        ITfDisplayAttributeProvider,
        ITfActiveLanguageProfileNotifySink,
        ITfThreadFocusSink,
        ITfFunctionProvider,
        ITfFunction,
        ITfFnGetPreferredTouchKeyboardLayout
)]
pub struct RCantoneseService {
        pub(crate) state: Mutex<ServiceState>,
}

impl RCantoneseService {
        pub fn new() -> Self {
                globals::dll_add_ref();
                Self {
                        state: Mutex::new(ServiceState::new()),
                }
        }
}

impl RCantoneseService_Impl {
        /// Owned IUnknown reference of the owning COM object, for advising sinks.
        pub(crate) fn as_iunknown(&self) -> IUnknown {
                self.to_object().into_interface()
        }

        pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, ServiceState> {
                self.state.lock().unwrap_or_else(|e| e.into_inner())
        }

        /// Re-entrancy guard: sink callbacks can be invoked while we already
        /// hold the state lock (OS calls inside DoEditSession pump messages).
        /// Bail out instead of deadlocking the thread.
        pub(crate) fn try_lock(&self) -> Option<std::sync::MutexGuard<'_, ServiceState>> {
                match self.state.try_lock() {
                        Ok(guard) => Some(guard),
                        Err(std::sync::TryLockError::Poisoned(e)) => Some(e.into_inner()),
                        Err(std::sync::TryLockError::WouldBlock) => None,
                }
        }
}

// ---------------------------------------------------------------------
// ITfTextInputProcessor / ITfTextInputProcessorEx
// ---------------------------------------------------------------------

impl ITfTextInputProcessor_Impl for RCantoneseService_Impl {
        fn Activate(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32) -> Result<()> {
                globals::guarded("Activate", || ITfTextInputProcessorEx_Impl::ActivateEx(self, ptim, tid, 0))
        }

        fn Deactivate(&self) -> Result<()> {
                globals::guarded("Deactivate", || self.deactivate_inner())
        }
}

impl RCantoneseService_Impl {
        fn deactivate_inner(&self) -> Result<()> {
                let Some(mut state) = self.try_lock() else {
                        globals::log("Deactivate skipped: state locked (re-entrant)");
                        return Ok(());
                };
                globals::log("Deactivate start");

                if let Some(context) = state.context.clone() {
                        crate::composition::end_composition(&mut state, &context);
                }

                if let Some(presenter) = state.candidate_presenter.take() {
                        presenter.end();
                }
                state.candidate_mode = CandidateMode::None;

                // unadvise sinks
                if let Some(thread_mgr) = state.thread_mgr.clone() {
                        let sink: IUnknown = self.as_iunknown();
                        let _ = sink;
                        if let Ok(source) = thread_mgr.cast::<ITfSource>() {
                                unsafe {
                                        let _ = source.UnadviseSink(state.thread_mgr_event_sink_cookie);
                                        let _ = source.UnadviseSink(state.lang_profile_notify_sink_cookie);
                                        let _ = source.UnadviseSink(state.thread_focus_sink_cookie);
                                }
                        }
                        state.thread_mgr_event_sink_cookie = TF_INVALID_COOKIE;

                        if let Ok(keystroke_mgr) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                                unsafe {
                                        let _ = keystroke_mgr.UnadviseKeyEventSink(state.client_id);
                                }
                        }

                        // Unpreserve keys (e.g. Shift) so other IMEs keep working after we deactivate.
                        if let Some(processor) = &state.processor {
                                let processor = processor.lock().unwrap_or_else(|e| e.into_inner());
                                processor.unpreserve_all(&thread_mgr);
                                // Remove the langbar item + compartment sink — port of
                                // ~CLangBarItemButton: _UnregisterCompartment + _RemoveItem.
                                processor.teardown_language_bar(&thread_mgr);
                                crate::tray::deactivate();
                        }

                        // clear compartments
                        let thread_unknown: IUnknown = thread_mgr.cast().unwrap();
                        let open_close = crate::compartment::Compartment::new(
                                &thread_unknown,
                                state.client_id,
                                GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
                        );
                        let _ = open_close.clear();
                        let char_form = crate::compartment::Compartment::new(
                                &thread_unknown,
                                state.client_id,
                                globals::GUID_COMPARTMENT_CHARACTER_FORM,
                        );
                        let _ = char_form.clear();
                        let punct_form = crate::compartment::Compartment::new(
                                &thread_unknown,
                                state.client_id,
                                globals::GUID_COMPARTMENT_PUNCTUATION_FORM,
                        );
                        let _ = punct_form.clear();
                }
                state.thread_mgr = None;
                state.client_id = 0;
                state.doc_mgr_last_focused = None;
                state.processor = None;
                globals::log("Deactivate success");
                Ok(())
        }
}

impl ITfTextInputProcessorEx_Impl for RCantoneseService_Impl {
        fn ActivateEx(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32, dwflags: u32) -> Result<()> {
                globals::guarded("ActivateEx", || self.activate_ex_inner(ptim, tid, dwflags))
        }
}

impl RCantoneseService_Impl {
        fn activate_ex_inner(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32, dwflags: u32) -> Result<()> {
                let mut path = [0u16; 260];
                let len = unsafe { GetModuleFileNameW(None, &mut path) };
                let name = String::from_utf16_lossy(&path[..len as usize]);
                globals::log(&format!("ActivateEx start proc={}", name.rsplit('\\').next().unwrap_or(&name)));
                let thread_mgr = ptim.as_ref().ok_or(E_INVALIDARG)?;
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());

                state.thread_mgr = Some(thread_mgr.clone());
                state.client_id = tid;
                state.activate_flags = dwflags;
                state.self_iunknown = Some(self.as_iunknown());

                // ITfThreadMgrEventSink
                let self_sink: IUnknown = self.as_iunknown();
                if let Ok(source) = thread_mgr.cast::<ITfSource>() {
                        match unsafe { source.AdviseSink(&ITfThreadMgrEventSink::IID, &self_sink) } {
                                Ok(cookie) => state.thread_mgr_event_sink_cookie = cookie,
                                Err(_) => {
                                        drop(state);
                                        let _ = ITfTextInputProcessor_Impl::Deactivate(self);
                                        return Err(Error::from_hresult(HRESULT(0x80004005u32 as i32)));
                                }
                        }
                }

                // text edit sink on focused document
                unsafe {
                        if let Ok(doc_mgr) = thread_mgr.GetFocus() {
                                if let Ok(context) = doc_mgr.GetTop() {
                                        if let Ok(source) = context.cast::<ITfSource>() {
                                                if let Ok(cookie) = source.AdviseSink(&ITfTextEditSink::IID, &self_sink) {
                                                        state.text_edit_sink_context = Some(context);
                                                        state.text_edit_sink_cookie = cookie;
                                                }
                                        }
                                }
                        }
                }

                // key event sink
                if let Ok(keystroke_mgr) = thread_mgr.cast::<ITfKeystrokeMgr>() {
                        let sink: ITfKeyEventSink = self_sink.cast().unwrap();
                        if unsafe { keystroke_mgr.AdviseKeyEventSink(tid, &sink, true) }.is_err() {
                                drop(state);
                                let _ = ITfTextInputProcessor_Impl::Deactivate(self);
                                return Err(Error::from_hresult(HRESULT(0x80004005u32 as i32)));
                        }
                }

                // active language profile notify sink + thread focus sink
                if let Ok(source) = thread_mgr.cast::<ITfSource>() {
                        if let Ok(cookie) = unsafe { source.AdviseSink(&ITfActiveLanguageProfileNotifySink::IID, &self_sink) } {
                                state.lang_profile_notify_sink_cookie = cookie;
                        }
                        if let Ok(cookie) = unsafe { source.AdviseSink(&ITfThreadFocusSink::IID, &self_sink) } {
                                state.thread_focus_sink_cookie = cookie;
                        }
                }

                // display attribute guid atoms — registered via ITfCategoryMgr
                if let Ok(mgr) = thread_mgr.cast::<ITfCategoryMgr>() {
                        unsafe {
                                if let Ok(atom) = mgr.RegisterGUID(&globals::GUID_DISPLAY_ATTRIBUTE_INPUT) {
                                        state.ga_display_attribute_input = atom;
                                }
                                if let Ok(atom) = mgr.RegisterGUID(&globals::GUID_DISPLAY_ATTRIBUTE_CONVERTED) {
                                        state.ga_display_attribute_converted = atom;
                                }
                        }
                }

                // engine
                if let Some(processor) = Processor::new(&thread_mgr, tid) {
                        let processor = std::sync::Arc::new(Mutex::new(processor));
                        if let Ok(p) = processor.lock() {
                                if let Some(item) = &p.lang_bar {
                                        *item.settings_handler.lock().unwrap_or_else(|e| e.into_inner()) =
                                                Some(std::sync::Arc::downgrade(&processor));
                                }
                        }
                        crate::tray::activate(&std::sync::Arc::downgrade(&processor));
                        state.processor = Some(processor);
                }

                globals::log("ActivateEx success");
                Ok(())
        }
}

// ---------------------------------------------------------------------
// ITfThreadMgrEventSink
// ---------------------------------------------------------------------

impl ITfThreadMgrEventSink_Impl for RCantoneseService_Impl {
        fn OnInitDocumentMgr(&self, _pdim: Ref<'_, ITfDocumentMgr>) -> Result<()> {
                Ok(())
        }
        fn OnUninitDocumentMgr(&self, _pdim: Ref<'_, ITfDocumentMgr>) -> Result<()> {
                Ok(())
        }

        fn OnSetFocus(&self, pdimfocus: Ref<'_, ITfDocumentMgr>, pdimprevfocus: Ref<'_, ITfDocumentMgr>) -> Result<()> {
                globals::guarded("OnSetFocus", || {
                        let Some(mut state) = self.try_lock() else { return Ok(()) };
                        let self_sink: IUnknown = self.as_iunknown();

                        // unadvise the old text edit sink
                        if let Some(old) = state.text_edit_sink_context.take() {
                                if let Ok(source) = old.cast::<ITfSource>() {
                                        unsafe {
                                                let _ = source.UnadviseSink(state.text_edit_sink_cookie);
                                        }
                                }
                                state.text_edit_sink_cookie = TF_INVALID_COOKIE;
                        }

                        let focus = pdimfocus.cloned();
                        if let Some(doc_mgr) = focus.clone() {
                                unsafe {
                                        if let Ok(context) = doc_mgr.GetTop() {
                                                if let Ok(source) = context.cast::<ITfSource>() {
                                                        if let Ok(cookie) = source.AdviseSink(&ITfTextEditSink::IID, &self_sink) {
                                                                state.text_edit_sink_context = Some(context);
                                                                state.text_edit_sink_cookie = cookie;
                                                        }
                                                }
                                        }
                                }
                        }

                        // Update the language bar to reflect the open/close compartment.
                        if let Some(processor) = state.processor.as_ref() {
                                let _ = pdimprevfocus;
                                processor
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .update_language_bar(state.thread_mgr.as_ref(), state.client_id, focus.is_some());
                        }
                        state.doc_mgr_last_focused = focus;
                        Ok(())
                })
        }

        fn OnPushContext(&self, _pic: Ref<'_, ITfContext>) -> Result<()> {
                Ok(())
        }
        fn OnPopContext(&self, _pic: Ref<'_, ITfContext>) -> Result<()> {
                Ok(())
        }
}

// ---------------------------------------------------------------------
// ITfTextEditSink — port of TextEditSink.cpp
// ---------------------------------------------------------------------

impl ITfTextEditSink_Impl for RCantoneseService_Impl {
        fn OnEndEdit(&self, pic: Ref<'_, ITfContext>, ecreadonly: u32, _peditrecord: Ref<'_, ITfEditRecord>) -> Result<()> {
                globals::guarded("OnEndEdit", || {
                        let Some(mut state) = self.try_lock() else { return Ok(()) };
                        let Some(context) = pic.as_ref() else { return Ok(()) };
                        let Some(composition) = state.composition.clone() else { return Ok(()) };
                        let Some(range) = (unsafe { composition.GetRange().ok() }) else {
                                return Ok(());
                        };
                        // selection after edit
                        let mut selection = [TF_SELECTION::default()];
                        let mut fetched = 0u32;
                        unsafe {
                                if context.GetSelection(ecreadonly, TF_DEFAULT_SELECTION, &mut selection, &mut fetched).is_err() || fetched != 1 {
                                        return Ok(());
                                }
                        }
                        // If the selection moved out of the composing range, terminate the composition.
                        let Some(sel_range) = selection[0].range.as_ref() else { return Ok(()) };
                        if !crate::composition::is_range_covered(ecreadonly, sel_range, &range) {
                                crate::composition::terminate_composition(&mut state, ecreadonly, &context, false);
                        }
                        Ok(())
                })
        }
}

// ---------------------------------------------------------------------
// ITfKeyEventSink — port of KeyEventSink.cpp
// ---------------------------------------------------------------------

impl ITfKeyEventSink_Impl for RCantoneseService_Impl {
        fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
                Ok(())
        }

        fn OnTestKeyDown(&self, pic: Ref<'_, ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
                globals::guarded("OnTestKeyDown", || {
                        let Some(mut state) = self.try_lock() else { return Ok(BOOL(0)) };
                        let Some(context) = pic.as_ref() else { return Ok(BOOL(0)) };
                        match crate::keys::test_key(&mut state, context, wparam.0, lparam.0, true) {
                                Some(_) => Ok(BOOL(1)),
                                None => Ok(BOOL(0)),
                        }
                })
        }

        fn OnTestKeyUp(&self, _pic: Ref<'_, ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
                globals::guarded("OnTestKeyUp", || {
                        globals::update_modifiers(wparam.0, lparam.0);
                        Ok(BOOL(0))
                })
        }

        fn OnKeyDown(&self, pic: Ref<'_, ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
                globals::guarded("OnKeyDown", || {
                        let Some(mut state) = self.try_lock() else { return Ok(BOOL(0)) };
                        let Some(context) = pic.as_ref() else { return Ok(BOOL(0)) };
                        let context = context.clone();
                        let Some((key_state, code, wch)) = crate::keys::test_key(&mut state, &context, wparam.0, lparam.0, true) else {
                                return Ok(BOOL(0));
                        };
                        let Some(service) = state.self_iunknown.clone() else { return Ok(BOOL(0)) };
                        let Some(processor) = state.processor.clone() else { return Ok(BOOL(0)) };
                        let client_id = state.client_id;
                        drop(state);
                        // The edit session may run synchronously; do not hold the lock.
                        match crate::keys::invoke_key_handler(&service, client_id, processor, &context, code, wch, key_state) {
                                Ok(()) => Ok(BOOL(1)),
                                Err(_) => Ok(BOOL(0)),
                        }
                })
        }

        fn OnKeyUp(&self, _pic: Ref<'_, ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
                globals::guarded("OnKeyUp", || {
                        globals::update_modifiers(wparam.0, lparam.0);
                        Ok(BOOL(0))
                })
        }

        fn OnPreservedKey(&self, _pic: Ref<'_, ITfContext>, rguid: *const GUID) -> Result<BOOL> {
                globals::guarded("OnPreservedKey", || {
                        if rguid.is_null() {
                                return Ok(BOOL(0));
                        }
                        let guid = unsafe { *rguid };
                        let Some(state) = self.try_lock() else { return Ok(BOOL(0)) };
                        let (Some(processor), Some(thread_mgr)) = (state.processor.clone(), state.thread_mgr.clone()) else {
                                return Ok(BOOL(0));
                        };
                        drop(state);
                        let eaten = processor
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .on_preserved_key(&guid, &thread_mgr);
                        Ok(BOOL(eaten as i32))
                })
        }
}

// ---------------------------------------------------------------------
// ITfCompositionSink
// ---------------------------------------------------------------------

impl ITfCompositionSink_Impl for RCantoneseService_Impl {
        fn OnCompositionTerminated(&self, ecwrite: u32, pcomposition: Ref<'_, ITfComposition>) -> Result<()> {
                globals::guarded("OnCompositionTerminated", || {
                        let Some(mut state) = self.try_lock() else { return Ok(()) };
                        if let Some(context) = state.context.clone() {
                                crate::composition::on_composition_terminated(&mut state, ecwrite, &context, pcomposition.as_ref());
                        }
                        Ok(())
                })
        }
}

// ---------------------------------------------------------------------
// ITfActiveLanguageProfileNotifySink
// ---------------------------------------------------------------------

impl ITfActiveLanguageProfileNotifySink_Impl for RCantoneseService_Impl {
        fn OnActivated(&self, clsid: *const GUID, _guidprofile: *const GUID, factivated: BOOL) -> Result<()> {
                globals::guarded("OnActivated", || {
                        unsafe {
                                if *clsid != globals::CLSID_RCANTONESE {
                                        return Ok(());
                                }
                        }
                        let Some(state) = self.try_lock() else { return Ok(()) };
                        if let Some(processor) = state.processor.as_ref() {
                                let mut processor = processor.lock().unwrap_or_else(|e| e.into_inner());
                                if factivated.as_bool() {
                                        processor.on_activated(state.thread_mgr.as_ref(), state.client_id);
                                } else {
                                        processor.on_deactivated();
                                }
                        }
                        Ok(())
                })
        }
}

// ---------------------------------------------------------------------
// ITfThreadFocusSink
// ---------------------------------------------------------------------

impl ITfThreadFocusSink_Impl for RCantoneseService_Impl {
        fn OnSetThreadFocus(&self) -> Result<()> {
                globals::guarded("OnSetThreadFocus", || {
                        let Some(state) = self.try_lock() else { return Ok(()) };
                        crate::tray::thread_focus_gained();
                        if let Some(processor) = state.processor.as_ref() {
                                processor
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .on_thread_focus(state.thread_mgr.as_ref(), state.client_id);
                        }
                        Ok(())
                })
        }
        fn OnKillThreadFocus(&self) -> Result<()> {
                globals::guarded("OnKillThreadFocus", || {
                        crate::tray::thread_focus_lost();
                        Ok(())
                })
        }
}

// ---------------------------------------------------------------------
// ITfDisplayAttributeProvider
// ---------------------------------------------------------------------

impl ITfDisplayAttributeProvider_Impl for RCantoneseService_Impl {
        fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
                let enu: IEnumTfDisplayAttributeInfo = crate::display_attr::DisplayAttributeEnum::new().into();
                Ok(enu)
        }

        fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
                unsafe {
                        let guid = *guid;
                        if guid == globals::GUID_DISPLAY_ATTRIBUTE_INPUT {
                                let info: ITfDisplayAttributeInfo =
                                        crate::display_attr::DisplayAttributeInfo::new(globals::GUID_DISPLAY_ATTRIBUTE_INPUT, true).into();
                                return Ok(info);
                        }
                        if guid == globals::GUID_DISPLAY_ATTRIBUTE_CONVERTED {
                                let info: ITfDisplayAttributeInfo =
                                        crate::display_attr::DisplayAttributeInfo::new(globals::GUID_DISPLAY_ATTRIBUTE_CONVERTED, false).into();
                                return Ok(info);
                        }
                        Err(Error::from_hresult(E_INVALIDARG))
                }
        }
}

// ---------------------------------------------------------------------
// ITfFunctionProvider + ITfFunction + ITfFnGetPreferredTouchKeyboardLayout
// ---------------------------------------------------------------------

impl ITfFunctionProvider_Impl for RCantoneseService_Impl {
        fn GetType(&self) -> Result<GUID> {
                Ok(globals::CLSID_RCANTONESE)
        }
        fn GetDescription(&self) -> Result<BSTR> {
                Err(Error::from_hresult(E_NOTIMPL))
        }
        fn GetFunction(&self, _rguid: *const GUID, _riid: *const GUID) -> Result<IUnknown> {
                Err(Error::from_hresult(E_NOINTERFACE))
        }
}

impl ITfFunction_Impl for RCantoneseService_Impl {
        fn GetDisplayName(&self) -> Result<BSTR> {
                Ok(BSTR::from("R-Cantonese"))
        }
}

impl ITfFnGetPreferredTouchKeyboardLayout_Impl for RCantoneseService_Impl {
        fn GetLayout(&self, ptkblayouttype: *mut TKBLayoutType, _pwpreferredlayoutid: *const u16) -> Result<()> {
                if ptkblayouttype.is_null() {
                        return Err(Error::from_hresult(E_INVALIDARG));
                }
                unsafe {
                        *ptkblayouttype = TKBLayoutType(2); // TKBLT_OPTIMIZED
                }
                Ok(())
        }
}
