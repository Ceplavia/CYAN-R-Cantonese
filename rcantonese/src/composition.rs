// Composition lifecycle — port of Composition.cpp / StartComposition.cpp /
// EndComposition.cpp / DisplayAttribute.cpp.
#![allow(dead_code)]

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::TextServices::*;

use crate::globals;
use crate::keytable::CandidateMode;
use crate::service::{RCantoneseService, ServiceState};

// ---------------------------------------------------------------------
// Generic edit session — port of CEditSessionBase.
// ---------------------------------------------------------------------

// TSF edit sessions are always executed on the caller's thread, so the
// closure does not need to be Send/Sync (Processor holds COM pointers).
pub type EditSessionFn = Box<dyn Fn(&mut ServiceState, u32, &ITfContext) -> Result<()>>;

#[implement(ITfEditSession)]
pub struct EditSession {
        pub service: IUnknown,
        pub context: ITfContext,
        pub action: EditSessionFn,
}

impl EditSession {
        pub fn new(service: &IUnknown, context: &ITfContext, action: EditSessionFn) -> Self {
                Self {
                        service: service.clone(),
                        context: context.clone(),
                        action,
                }
        }

        fn with_state(&self, f: impl FnOnce(&mut ServiceState)) {
                if let Ok(object) = ComObject::<RCantoneseService>::cast_from(&self.service) {
                        let service = object.get();
                        let mut state = service.state.lock().unwrap_or_else(|e| e.into_inner());
                        f(&mut state);
                }
        }
}

impl ITfEditSession_Impl for EditSession_Impl {
        fn DoEditSession(&self, ec: u32) -> Result<()> {
                crate::globals::guarded("DoEditSession", || {
                        if let Ok(object) = ComObject::<RCantoneseService>::cast_from(&self.service) {
                                let service = object.get();
                                // Bail on re-entrant sessions rather than deadlocking.
                                let mut state = match service.state.try_lock() {
                                        Ok(guard) => guard,
                                        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
                                        Err(std::sync::TryLockError::WouldBlock) => {
                                                return Err(Error::from_hresult(E_FAIL));
                                        }
                                };
                                return (self.action)(&mut state, ec, &self.context);
                        }
                        Err(Error::from_hresult(E_FAIL))
                })
        }
}

/// Port of CEditSessionBase::Request / RequestAsync — returns the inner edit-session hr.
pub fn request_edit_session(
        service: &IUnknown,
        context: &ITfContext,
        client_id: u32,
        flags: TF_CONTEXT_EDIT_CONTEXT_FLAGS,
        action: EditSessionFn,
) -> Result<()> {
        let session: ITfEditSession = EditSession::new(service, context, action).into();
        unsafe {
                match context.RequestEditSession(client_id, &session, flags) {
                        Ok(hr) if hr == TF_S_ASYNC => Ok(()),
                        Ok(hr) => hr.ok(),
                        Err(e) => Err(e),
                }
        }
}

// ---------------------------------------------------------------------
// Range helpers
// ---------------------------------------------------------------------

/// True if `range` is covered by `covering` — port of IsRangeCovered.
pub fn is_range_covered(ec: u32, range: &ITfRange, covering: &ITfRange) -> bool {
        unsafe {
                let Ok(start_cmp) = range.CompareStart(ec, covering, TF_ANCHOR_START) else {
                        return false;
                };
                if start_cmp < 0 {
                        return false;
                }
                let Ok(end_cmp) = range.CompareEnd(ec, covering, TF_ANCHOR_END) else {
                        return false;
                };
                end_cmp <= 0
        }
}

// ---------------------------------------------------------------------
// Start / end composition
// ---------------------------------------------------------------------

/// Port of _StartComposition. `sink` is the service's IUnknown (ITfCompositionSink).
pub fn start_composition(state: &mut ServiceState, ec: u32, context: &ITfContext, sink: &IUnknown) -> Result<()> {
        if state.composition.is_some() {
                return Ok(());
        }
        if state.context.is_some() {
                globals::log_error("StartComposition failed: saved context exists without a composition");
                return Err(Error::from_hresult(E_UNEXPECTED));
        }

        let insert_at_selection: ITfInsertAtSelection = context.cast()?;
        let range_insert = unsafe { insert_at_selection.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]) }?;

        let context_composition: ITfContextComposition = context.cast()?;
        let composition_sink: ITfCompositionSink = sink.cast()?;
        let composition = unsafe { context_composition.StartComposition(ec, &range_insert, &composition_sink) }?;

        // set selection to the composition range
        let selection = TF_SELECTION {
                range: std::mem::ManuallyDrop::new(Some(range_insert.clone())),
                style: TF_SELECTIONSTYLE {
                        ase: TF_AE_NONE,
                        fInterimChar: BOOL(0),
                },
        };
        let set_result = unsafe { context.SetSelection(ec, std::slice::from_ref(&selection)) };
        std::mem::forget(selection);

        match set_result {
                Ok(()) => {
                        state.context = Some(context.clone());
                        state.composition = Some(composition);
                        Ok(())
                }
                Err(e) => {
                        globals::log_error("StartComposition failed: SetSelection");
                        let _ = unsafe { composition.EndComposition(ec) };
                        Err(e)
                }
        }
}

/// Port of _TerminateComposition — must be called inside an edit session.
pub fn terminate_composition(state: &mut ServiceState, ec: u32, context: &ITfContext, _from_deactivate: bool) {
        let Some(composition) = state.composition.take() else {
                return;
        };
        clear_composition_display_attributes(state, ec, context, &composition);
        unsafe {
                if composition.EndComposition(ec).is_err() {
                        // if EndComposition fails, drop the candidate list
                        delete_candidate_list(state, true, Some(context));
                }
        }
        state.context = None;
}

/// Port of _EndComposition — ends the composition through an edit session.
pub fn end_composition(state: &mut ServiceState, context: &ITfContext) {
        let Some(service) = state.self_iunknown.clone() else { return };
        let client_id = state.client_id;
        let _ = request_edit_session(
                &service,
                context,
                client_id,
                TF_ES_ASYNCDONTCARE | TF_ES_READWRITE,
                Box::new(|state, ec, context| {
                        terminate_composition(state, ec, context, true);
                        Ok(())
                }),
        );
}

/// Port of ITfCompositionSink::OnCompositionTerminated body.
pub fn on_composition_terminated(
        state: &mut ServiceState,
        ecwrite: u32,
        context: &ITfContext,
        composition: Option<&ITfComposition>,
) {
        // Clear dummy composition
        if let Some(comp) = composition {
                if let Ok(range) = unsafe { comp.GetRange() } {
                        unsafe {
                                let _ = range.SetText(ecwrite, 0, &[]);
                        }
                }
        }
        let is_current = match (composition, &state.composition) {
                (Some(a), Some(b)) => a.as_raw() == b.as_raw(),
                _ => false,
        };
        if !is_current {
                return;
        }
        state.composition = None;
        state.context = None;
        delete_candidate_list(state, false, Some(context));
}

// ---------------------------------------------------------------------
// Display attributes + language property
// ---------------------------------------------------------------------

pub fn set_composition_display_attributes(state: &ServiceState, ec: u32, context: &ITfContext, atom: u32) -> bool {
        let Some(composition) = &state.composition else { return false };
        let Ok(range) = (unsafe { composition.GetRange() }) else { return false };
        let Ok(property) = (unsafe { context.GetProperty(&GUID_PROP_ATTRIBUTE) }) else { return false };
        let mut var = VARIANT::default();
        var.Anonymous.Anonymous = std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: atom as i32 },
        });
        unsafe { property.SetValue(ec, &range, &var).is_ok() }
}

pub fn clear_composition_display_attributes(state: &ServiceState, ec: u32, context: &ITfContext, composition: &ITfComposition) {
        let _ = state;
        let Ok(range) = (unsafe { composition.GetRange() }) else { return };
        if let Ok(property) = unsafe { context.GetProperty(&GUID_PROP_ATTRIBUTE) } {
                unsafe {
                        let _ = property.Clear(ec, &range);
                }
        }
}

/// Port of _SetCompositionLanguage.
pub fn set_composition_language(state: &ServiceState, ec: u32, context: &ITfContext) {
        let Some(composition) = &state.composition else { return };
        let Ok(range) = (unsafe { composition.GetRange() }) else { return };
        let Ok(property) = (unsafe { context.GetProperty(&GUID_PROP_LANGID) }) else { return };
        let mut var = VARIANT::default();
        var.Anonymous.Anonymous = std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: globals::TEXTSERVICE_LANGID as i32 },
        });
        unsafe {
                let _ = property.SetValue(ec, &range, &var);
        }
}

// ---------------------------------------------------------------------
// Text insertion helpers — port of _AddComposingAndChar etc.
// ---------------------------------------------------------------------

/// Port of _InsertAtSelection — query-only insert range.
pub fn insert_at_selection(ec: u32, context: &ITfContext, text: &[u16]) -> Result<ITfRange> {
        let insert_at_selection: ITfInsertAtSelection = context.cast()?;
        unsafe { insert_at_selection.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, text) }
}

/// Port of _FindComposingRange.
pub fn find_composing_range(ec: u32, context: &ITfContext, selection: &ITfRange) -> Option<ITfRange> {
        let property = unsafe { context.GetProperty(&GUID_PROP_COMPOSING).ok()? };
        let mut enum_ranges: Option<IEnumTfRanges> = None;
        unsafe {
                property.EnumRanges(ec, &mut enum_ranges, selection).ok()?;
        }
        let enum_ranges = enum_ranges?;
        loop {
                let mut found: [Option<ITfRange>; 1] = [None];
                let mut fetched = 0u32;
                unsafe {
                        if enum_ranges.Next(&mut found, &mut fetched).is_err() || fetched != 1 {
                                return None;
                        }
                }
                let Some(range) = found[0].take() else { return None };
                if let Ok(var) = unsafe { property.GetValue(ec, &range) } {
                        let val: i32 = unsafe {
                                if var.Anonymous.Anonymous.vt == VT_I4 {
                                        var.Anonymous.Anonymous.Anonymous.lVal
                                } else {
                                        0
                                }
                        };
                        if val != 0 {
                                return Some(range);
                        }
                }
        }
}

/// Port of _SetInputString.
pub fn set_input_string(state: &mut ServiceState, ec: u32, context: &ITfContext, range: Option<&ITfRange>, text: &[u16], exist_composing: bool) -> Result<()> {
        let mut insert_range: Option<ITfRange> = None;
        let target = if !exist_composing {
                insert_range = Some(insert_at_selection(ec, context, text)?);
                insert_range.as_ref()
        } else {
                range
        };
        if let Some(target) = target {
                unsafe {
                        target.SetText(ec, 0, text)?;
                }
        }
        set_composition_language(state, ec, context);
        set_composition_display_attributes(state, ec, context, state.ga_display_attribute_input);

        // move the selection just past the inserted text
        if let Some(target) = target {
                if let Ok(selection_range) = unsafe { target.Clone() } {
                        unsafe {
                                let _ = selection_range.Collapse(ec, TF_ANCHOR_END);
                                let selection = TF_SELECTION {
                                        range: std::mem::ManuallyDrop::new(Some(selection_range.clone())),
                                        style: TF_SELECTIONSTYLE {
                                                ase: TF_AE_NONE,
                                                fInterimChar: BOOL(0),
                                        },
                                };
                                let _ = context.SetSelection(ec, std::slice::from_ref(&selection));
                                std::mem::forget(selection);
                        }
                }
        }
        Ok(())
}

/// Port of _AddComposingAndChar.
pub fn add_composing_and_char(state: &mut ServiceState, ec: u32, context: &ITfContext, text: &[u16]) -> Result<()> {
        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
                if context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched).is_err() || fetched == 0 {
                        return Ok(());
                }
        }
        let ahead = unsafe { context.GetStart(ec)? };
        let sel_range = unsafe { std::mem::ManuallyDrop::take(&mut selection[0].range) }.ok_or(E_POINTER)?;
        unsafe {
                ahead.ShiftEndToRange(ec, &sel_range, TF_ANCHOR_START)?;
        }
        let composing_range = find_composing_range(ec, context, &ahead);
        set_input_string(state, ec, context, composing_range.as_ref(), text, composing_range.is_some())?;
        Ok(())
}

/// Port of _AddCharAndFinalize.
pub fn add_char_and_finalize(ec: u32, context: &ITfContext, text: &[u16]) -> Result<()> {
        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
                context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)?;
                if fetched != 1 {
                        return Err(Error::from_hresult(E_FAIL));
                }
        }
        let sel_range = unsafe { std::mem::ManuallyDrop::take(&mut selection[0].range) }.ok_or(E_POINTER)?;
        unsafe {
                sel_range.SetText(ec, 0, text)?;
                let _ = sel_range.Collapse(ec, TF_ANCHOR_END);
                let selection = TF_SELECTION {
                        range: std::mem::ManuallyDrop::new(Some(sel_range)),
                        style: TF_SELECTIONSTYLE {
                                ase: TF_AE_NONE,
                                fInterimChar: BOOL(0),
                        },
                };
                context.SetSelection(ec, std::slice::from_ref(&selection))?;
                std::mem::forget(selection);
        }
        Ok(())
}

/// Port of _DeleteCandidateList.
pub fn delete_candidate_list(state: &mut ServiceState, force: bool, context: Option<&ITfContext>) {
        // Upstream clears the keystroke buffer unconditionally — a finalize
        // with no candidate window (e.g. caps-lock gibberish) must still
        // reset the reading, otherwise the next letter appends to it.
        if let Some(processor) = &state.processor {
                if let Ok(mut p) = processor.lock() {
                        p.clear_input_keys();
                }
        }
        if let Some(presenter) = state.candidate_presenter.take() {
                presenter.end_with_context(force, context);
        }
        state.candidate_mode = CandidateMode::None;
        state.symbol_query = None;
        state.composition_reading_code.clear();
}

