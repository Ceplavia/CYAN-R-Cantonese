// Key event sink + key handler dispatch — port of KeyEventSink.cpp,
// KeyHandler.cpp, KeyHandlerEditSession.cpp and KeyStateCategory.cpp.
#![allow(dead_code)]

use std::sync::Mutex;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::TextServices::*;

use crate::candidate::CandidateListPresenter;
use crate::compartment::Compartment;
use crate::composition;
use crate::globals;
use crate::keytable::*;
use crate::processor::{CandidateItem, Processor};
use crate::service::ServiceState;
use crate::types::VirtualInputKey;

// ---------------------------------------------------------------------
// Sink-level helpers (no edit session required)
// ---------------------------------------------------------------------

fn thread_compartment(state: &ServiceState, guid: GUID) -> Compartment {
        let unknown: Option<IUnknown> = state.thread_mgr.as_ref().and_then(|tm| tm.cast().ok());
        Compartment::new(&unknown.unwrap(), state.client_id, guid)
}

fn is_keyboard_disabled(state: &ServiceState) -> bool {
        thread_compartment(state, GUID_COMPARTMENT_KEYBOARD_DISABLED)
                .get_bool()
                .unwrap_or(false)
}

fn is_keyboard_open(state: &ServiceState) -> bool {
        thread_compartment(state, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)
                .get_bool()
                .unwrap_or(true)
}

/// Whether Caps Lock is toggled on — the IME treats it as English mode:
/// keys pass through untouched and the mode indicator shows "A".
fn caps_lock_on() -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
        unsafe { GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 }
}

/// Previous caps-lock state — drives the mode-indicator update on transition.
static CAPS_WAS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Port of _ConvertVirtualKey — map the key event to a character.
fn convert_virtual_key(wparam: usize, lparam: isize) -> (u32, u16) {
        let code = wparam as u32;
        if code == crate::keytable::VK_PACKET {
                let scancode = ((lparam as u32) & 0x00FF0000) >> 16;
                let mut buf = [0u16; 4];
                let keyboard = [0u8; 256];
                unsafe {
                        let count = ToUnicode(code, scancode, Some(&keyboard), &mut buf, 0);
                        if count > 0 {
                                return (code, buf[0]);
                        }
                        let count = ToUnicode(code, scancode, Some(&keyboard), &mut buf, 4);
                        if count > 0 {
                                return (code, buf[0]);
                        }
                }
                (code, 0)
        } else {
                (code, code as u16)
        }
}

/// Port of _TestKeyDown — returns Some(state, code, wch) when the key will be eaten.
pub fn test_key(
        state: &mut ServiceState,
        _pic: &ITfContext,
        wparam: usize,
        lparam: isize,
        is_key_down: bool,
) -> Option<(KeystrokeState, u32, u16)> {
        globals::update_modifiers(wparam, lparam);
        let (code, wch) = convert_virtual_key(wparam, lparam);
        // Caps Lock = English mode: refresh the mode icon on transitions,
        // commit any live composition, and let keys pass through untouched.
        let caps = caps_lock_on();
        if CAPS_WAS_ON.swap(caps, std::sync::atomic::Ordering::Relaxed) != caps {
                crate::tray::update_mode(is_keyboard_open(state) && !caps);
        }
        if caps {
                if is_key_down {
                        globals::log(&format!("caps on: pass code={code:#04x} composing={}", state.is_composing()));
                }
                if is_key_down && (state.is_composing() || state.candidate_presenter.is_some()) {
                        return Some((
                                KeystrokeState {
                                        category: KeystrokeCategory::Composing,
                                        function: KeystrokeFunction::FinalizeTextStore,
                                },
                                code,
                                wch,
                        ));
                }
                return None;
        }
        if code == 0 || code == u32::MAX {
                return None;
        }
        if is_keyboard_disabled(state) {
                return None;
        }
        if !is_key_down {
                return None; // key-up only matters for preserved keys
        }
        // Never eat keys while a Windows key is held — TF_MOD has no WIN bit,
        // so query the OS directly or Win+D/Win+E/etc. get swallowed as
        // plain letters.
        unsafe {
                let lwin = GetAsyncKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_LWIN.0 as i32);
                let rwin = GetAsyncKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_RWIN.0 as i32);
                if ((lwin | rwin) as u16 & 0x8000) != 0 {
                        return None;
                }
        }
        // Options menu — port of IsOptionsShortcut / _optionsMode. The hotkey
        // list comes from settings.toml (`options_menu_keys`, default Ctrl+`);
        // it works in ABC mode too so the mode can be switched back.
        let modifiers = globals::modifiers_value();
        let options_hotkey = state
                .processor
                .as_ref()
                .map(|arc| {
                        arc.lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .settings
                                .options_menu_keys
                                .iter()
                                .any(|&(vk, m)| vk == code && globals::check_modifiers(modifiers, m))
                })
                .unwrap_or(false);
        if code == 0xC0 || options_hotkey {
                globals::log(&format!(
                        "options check: code={code:#04x} mods={modifiers:#06x} hotkey={options_hotkey} keys={:?}",
                        state.processor.as_ref().map(|p| p.lock().unwrap_or_else(|e| e.into_inner()).settings.options_menu_keys.clone())
                ));
        }
        if options_hotkey {
                return Some((
                        KeystrokeState {
                                category: KeystrokeCategory::Candidate,
                                function: KeystrokeFunction::ToggleOptions,
                        },
                        code,
                        wch,
                ));
        }
        if state.candidate_mode == CandidateMode::Options {
                // While the menu is open every key is eaten: digits select,
                // arrows move, anything else closes it.
                let function = match code {
                        0x30..=0x39 | 0x60..=0x69 if globals::check_modifiers(modifiers, 0) => KeystrokeFunction::SelectByNumber,
                        crate::keytable::VK_UP => KeystrokeFunction::MoveUp,
                        crate::keytable::VK_DOWN => KeystrokeFunction::MoveDown,
                        _ => KeystrokeFunction::Cancel,
                };
                return Some((KeystrokeState { category: KeystrokeCategory::Candidate, function }, code, wch));
        }
        if !is_keyboard_open(state) {
                // ABC mode — let everything through
                state.composition_reading_code.clear();
                return None;
        }
        let Some(processor) = state.processor.clone() else { return None };
        let processor = processor.lock().unwrap_or_else(|e| e.into_inner());

        // Punctuation fallback — port of _IsKeyEaten. A key that is both a
        // punctuation key and an input key (apostrophe) stays an input key.
        let modifiers = globals::modifiers_value();
        let is_no_modifier = globals::check_modifiers(modifiers, 0);
        let is_shifting = globals::check_modifiers(modifiers, globals::TF_MOD_SHIFT);
        // Unshifted "`" is a composition starter (pinyin prefix / symbol
        // list via the bare-"`" fallback) — keep it off the punctuation
        // path. Shifted "`" still opens the ~ symbol list.
        let punctuation_key = if is_shifting || (is_no_modifier && code != crate::keytable::VK_OEM_3) {
                crate::punctuation::PunctuationKey::for_virtual_key(code)
        } else {
                None
        };
        let should_handle_punctuation = punctuation_key.map(|k| k.should_handle(is_shifting)).unwrap_or(false);
        let is_unshifted_apostrophe = should_handle_punctuation
                && !is_shifting
                && code == windows::Win32::UI::Input::KeyboardAndMouse::VK_OEM_7.0 as u32;

        // "/x" symbol query — while the slash punctuation list is in query
        // mode, letters/digits extend the query instead of selecting.
        if state.candidate_mode == CandidateMode::Punctuation && is_no_modifier {
                if let Some(query) = &state.symbol_query {
                        let is_letter = (0x41u32..=0x5a).contains(&code);
                        // A digit on the bare slash menu starts the numeral
                        // query (/2 → variants); after a letter/digit query is
                        // open, digits select from the list.
                        let is_query_digit = (0x30u32..=0x39).contains(&code) && query.is_empty();
                        if code == crate::keytable::VK_BACK {
                                // Backspace on the bare "/" cancels the whole
                                // composition so the placeholder can be deleted.
                                let function = if query.is_empty() {
                                        KeystrokeFunction::Cancel
                                } else {
                                        KeystrokeFunction::SymbolQuery
                                };
                                return Some((
                                        KeystrokeState {
                                                category: KeystrokeCategory::Candidate,
                                                function,
                                        },
                                        code,
                                        wch,
                                ));
                        }
                        if is_letter || is_query_digit {
                                return Some((
                                        KeystrokeState {
                                                category: KeystrokeCategory::Candidate,
                                                function: KeystrokeFunction::SymbolQuery,
                                        },
                                        code,
                                        wch,
                                ));
                        }
                }
        }

        let mut key_state = KeystrokeState::NONE;
        let needed = processor.keys.is_virtual_key_need(
                code,
                wch,
                state.is_composing(),
                state.candidate_mode,
                processor.current_reverse_lookup_method(),
                &mut key_state,
        );
        if needed {
                let is_syllable_separator = is_unshifted_apostrophe && key_state.function == KeystrokeFunction::Input;
                if !should_handle_punctuation || is_syllable_separator {
                        globals::log(&format!("test_key: eat code={code:#04x} wch={wch:#06x} cat={:?} func={:?}", key_state.category, key_state.function));
                        return Some((key_state, code, wch));
                }
                if !is_shifting
                        && matches!(key_state.function, KeystrokeFunction::MovePageUp | KeystrokeFunction::MovePageDown)
                {
                        return Some((key_state, code, wch));
                }
        }
        if should_handle_punctuation {
                return Some((
                        KeystrokeState {
                                category: KeystrokeCategory::Composing,
                                function: KeystrokeFunction::PunctuationKey,
                        },
                        code,
                        wch,
                ));
        }
        // Full-width character form conversion (only outside candidate lists).
        if state.candidate_mode == CandidateMode::None
                && (0x20u16..=0x7e).contains(&wch)
                && thread_compartment(state, globals::GUID_COMPARTMENT_CHARACTER_FORM)
                        .get_bool()
                        .unwrap_or(false)
        {
                return Some((
                        KeystrokeState {
                                category: KeystrokeCategory::Composing,
                                function: KeystrokeFunction::CharacterForm,
                        },
                        code,
                        wch,
                ));
        }
        None
}

/// Port of _InvokeKeyHandler — run the handler inside an edit session.
pub fn invoke_key_handler(
        service: &IUnknown,
        client_id: u32,
        processor: std::sync::Arc<Mutex<Processor>>,
        context: &ITfContext,
        code: u32,
        wch: u16,
        key_state: KeystrokeState,
) -> Result<()> {
        let is_shifting = globals::modifiers_value() & globals::TF_MOD_ALLSHIFT != 0;
        // For non-input keys the table lookup fails; handlers only inspect the
        // key when the dispatch function is Input, so a sentinel is fine.
        let input_key = VirtualInputKey::for_key_code(code).unwrap_or(VirtualInputKey {
                character: '\0',
                text: "",
                code: -1,
                key_code: code,
        });
        composition::request_edit_session(
                service,
                context,
                client_id,
                TF_ES_ASYNCDONTCARE | TF_ES_READWRITE,
                Box::new(move |state, ec, context| {
                        key_state_handler(
                                state,
                                processor.clone(),
                                ec,
                                context,
                                code,
                                wch,
                                is_shifting,
                                input_key,
                                key_state,
                        )
                }),
        )
}

/// Port of CKeyStateCategory::KeyStateHandler — dispatch by category/function.
fn key_state_handler(
        state: &mut ServiceState,
        processor: std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
        code: u32,
        wch: u16,
        is_shifting: bool,
        input_key: VirtualInputKey,
        key_state: KeystrokeState,
) -> Result<()> {
        use KeystrokeCategory as C;
        use KeystrokeFunction as F;
        match (key_state.category, key_state.function) {
                (_, F::ToggleOptions) => handle_options_toggle(state, ec, context),
                (C::Candidate, F::SelectByNumber) if state.candidate_mode == CandidateMode::Options => {
                        handle_options_select(state, ec, context, code)
                }
                (C::Candidate, F::Cancel) if state.candidate_mode == CandidateMode::Options => handle_options_close(state, ec, context),
                (C::Composing, F::Input) => handle_composition_input(state, processor, ec, context, input_key, is_shifting),
                (C::Composing, F::FinalizeTextstoreAndInput) => {
                        handle_composition_finalize(state, ec, context, false)?;
                        handle_composition_input(state, processor, ec, context, input_key, is_shifting)
                }
                (C::Composing, F::FinalizeTextStore) => {
                        if code == crate::keytable::VK_RETURN || code == crate::keytable::VK_SPACE {
                                handle_composition_finalize_raw(state, ec, context)
                        } else {
                                handle_composition_finalize(state, ec, context, false)
                        }
                }
                (C::Composing, F::FinalizeCandidateListAndInput) => {
                        handle_composition_finalize(state, ec, context, true)?;
                        handle_composition_input(state, processor, ec, context, input_key, is_shifting)
                }
                (C::Composing, F::FinalizeCandidateList) => handle_composition_finalize(state, ec, context, true),
                (C::Composing, F::Convert) => handle_composition_convert(state, ec, context),
                (C::Composing, F::Cancel) => handle_cancel(state, ec, context),
                (C::Composing, F::Backspace) => handle_composition_backspace(state, ec, context),
                (C::Composing, f) if is_arrow_function(f) => handle_composition_arrow_key(state, ec, context, f),
                (C::Composing, F::CharacterForm) => handle_composition_character_form(state, ec, context, wch),
                (C::Composing, F::PunctuationKey) => handle_punctuation_key(state, ec, context, code, is_shifting),

                (C::Candidate, F::FinalizeCandidateList) => handle_candidate_finalize(state, processor, ec, context),
                (C::Candidate, F::FinalizeCandidateListAndInput) => {
                        handle_candidate_finalize(state, processor.clone(), ec, context)?;
                        handle_composition_input(state, processor, ec, context, input_key, is_shifting)
                }
                (C::Candidate, F::Convert) => handle_candidate_worker(state, ec, context),
                (C::Candidate, F::Cancel) => handle_cancel(state, ec, context),
                (C::Candidate, f) if is_arrow_function(f) => {
                        if let Some(p) = &state.candidate_presenter {
                                p.advise_ui_changed_by_arrow_key(f);
                        }
                        Ok(())
                }
                (C::Candidate, F::SelectByNumber) => handle_candidate_select_by_number(state, processor, ec, context, code),
                (C::Candidate, F::SymbolQuery) => handle_symbol_query(state, ec, context, code),
                (C::Candidate, F::ForgetCandidate) => handle_candidate_forget(state, processor, ec, context),

                (C::Phrase, F::FinalizeCandidateList) => handle_phrase_finalize(state, ec, context),
                (C::Phrase, F::Cancel) => handle_cancel(state, ec, context),
                (C::Phrase, f) if is_arrow_function(f) => {
                        if let Some(p) = &state.candidate_presenter {
                                p.advise_ui_changed_by_arrow_key(f);
                        }
                        Ok(())
                }
                (C::Phrase, F::SelectByNumber) => handle_phrase_select_by_number(state, processor, ec, context, code),
                _ => Err(Error::from_hresult(E_INVALIDARG)),
        }
}

fn is_arrow_function(f: KeystrokeFunction) -> bool {
        use KeystrokeFunction::*;
        matches!(
                f,
                MoveLeft | MoveRight | MoveUp | MoveDown | MovePageUp | MovePageDown | MovePageTop | MovePageBottom
        )
}

// ---------------------------------------------------------------------
// Composition handlers — port of KeyHandler.cpp
// ---------------------------------------------------------------------

fn handle_composition_input(
        state: &mut ServiceState,
        processor: std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
        input_key: VirtualInputKey,
        is_shifting: bool,
) -> Result<()> {
        if state.candidate_presenter.is_some() && state.candidate_mode != CandidateMode::Incremental {
                handle_composition_finalize(state, ec, context, false)?;
        }

        let mut started = false;
        if !state.is_composing() {
                let Some(sink) = state.self_iunknown.clone() else { return Err(Error::from_hresult(E_UNEXPECTED)) };
                if let Err(e) = composition::start_composition(state, ec, context, &sink) {
                        processor.lock().unwrap_or_else(|p| p.into_inner()).clear_input_keys();
                        return Err(e);
                }
                started = true;
        }
        if state.composition.is_none() {
                processor.lock().unwrap_or_else(|p| p.into_inner()).clear_input_keys();
                return Err(Error::from_hresult(E_UNEXPECTED));
        }

        // is the insertion point covered by the composition?
        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
                if context
                        .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
                        .is_err()
                        || fetched != 1
                {
                        if started {
                                processor.lock().unwrap_or_else(|p| p.into_inner()).clear_input_keys();
                                composition::terminate_composition(state, ec, context, false);
                        }
                        return Err(Error::from_hresult(E_FAIL));
                }
        }
        let comp_range = unsafe { state.composition.as_ref().ok_or(E_UNEXPECTED)?.GetRange() }?;
        let sel_range = unsafe { std::mem::ManuallyDrop::take(&mut selection[0].range) }.ok_or(E_POINTER)?;
        let covered = composition::is_range_covered(ec, &sel_range, &comp_range);
        if !covered {
                if started {
                        processor.lock().unwrap_or_else(|p| p.into_inner()).clear_input_keys();
                        composition::terminate_composition(state, ec, context, false);
                }
                return Ok(());
        }

        processor
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .add_input_key(input_key, is_shifting);
        handle_composition_input_worker(state, processor, ec, context)
}

/// Port of _HandleCompositionInputWorker.
fn handle_composition_input_worker(
        state: &mut ServiceState,
        processor: std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
) -> Result<()> {
        let (reading, candidates) = {
                let mut p = processor.lock().unwrap_or_else(|e| e.into_inner());
                let reading = p.reading_string().unwrap_or_default();
                let candidates = p.candidate_list();
                (reading, candidates)
        };
        globals::log(&format!("input_worker: reading={reading:?} candidates={}", candidates.len()));

        let reading_wide: Vec<u16> = reading.encode_utf16().collect();
        composition::add_composing_and_char(state, ec, context, &reading_wide)?;

        if !candidates.is_empty() {
                create_and_start_candidate(state, &processor, ec, context)?;
                if let Some(presenter) = &state.candidate_presenter {
                        presenter.clear_list();
                        presenter.set_text(candidates);
                }
        } else if let Some(presenter) = state.candidate_presenter.take() {
                presenter.end();
                state.candidate_mode = CandidateMode::None;
                state.symbol_query = None;
        }
        Ok(())
}

/// Port of _CreateAndStartCandidate.
fn create_and_start_candidate(
        state: &mut ServiceState,
        processor: &std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
) -> Result<()> {
        if (state.candidate_mode == CandidateMode::Phrase || state.candidate_mode == CandidateMode::None)
                && state.candidate_presenter.is_some()
        {
                if let Some(presenter) = state.candidate_presenter.take() {
                        presenter.end();
                }
                state.candidate_mode = CandidateMode::None;
        }

        if state.candidate_presenter.is_none() {
                let service = state.self_iunknown.clone().ok_or(E_UNEXPECTED)?;
                let page_size = processor
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .keys
                        .candidate_list_index_range()
                        .len()
                        .max(1);
                let presenter: ComObject<CandidateListPresenter> =
                        CandidateListPresenter::new(&service, page_size, false).into();
                state.candidate_mode = CandidateMode::Incremental;

                let doc_mgr = unsafe { context.GetDocumentMgr()? };
                let range = unsafe { state.composition.as_ref().ok_or(E_UNEXPECTED)?.GetRange()? };
                let Some(thread_mgr) = state.thread_mgr.clone() else {
                        return Err(Error::from_hresult(E_UNEXPECTED));
                };
                if let Err(e) = CandidateListPresenter::start(&presenter, state.client_id, &thread_mgr, &doc_mgr, context, ec, &range) {
                        globals::log_error(&format!("candidate start failed: {e:?}"));
                        presenter.end();
                        state.candidate_mode = CandidateMode::None;
                        state.symbol_query = None;
                        return Err(e);
                }
                globals::log("candidate presenter started");
                state.candidate_presenter = Some(presenter);
        }
        Ok(())
}

/// Port of _HandleComplete.
fn handle_complete(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        composition::delete_candidate_list(state, false, Some(context));
        composition::terminate_composition(state, ec, context, false);
        Ok(())
}

/// Port of _HandleCancel.
fn handle_cancel(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        // remove dummy text in the composition range
        if let Some(composition) = &state.composition {
                if let Ok(range) = unsafe { composition.GetRange() } {
                        unsafe {
                                let _ = range.SetText(ec, 0, &[]);
                        }
                }
        }
        composition::delete_candidate_list(state, false, Some(context));
        composition::terminate_composition(state, ec, context, false);
        Ok(())
}

/// Port of _HandleCompositionFinalize.
fn handle_composition_finalize(state: &mut ServiceState, ec: u32, context: &ITfContext, is_candidate_list: bool) -> Result<()> {
        if is_candidate_list {
                if let Some(presenter) = &state.candidate_presenter {
                        let candidate = presenter.selected_candidate_string();
                        let index = presenter.selected_candidate_index();
                        if let Some(text) = candidate {
                                if !text.is_empty() {
                                        let wide: Vec<u16> = text.encode_utf16().collect();
                                        composition::add_composing_and_char(state, ec, context, &wide)?;
                                        if state.candidate_mode != CandidateMode::Punctuation {
                                                if let (Some(index), Some(processor)) = (index, state.processor.clone()) {
                                                        processor
                                                                .lock()
                                                                .unwrap_or_else(|p| p.into_inner())
                                                                .commit_selected_candidate_for_memory(index);
                                                }
                                        }
                                }
                        }
                }
        } else if state.is_composing() {
                let mut selection = [TF_SELECTION::default()];
                let mut fetched = 0u32;
                unsafe {
                        if context
                                .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
                                .is_err()
                                || fetched != 1
                        {
                                return Err(Error::from_hresult(S_FALSE));
                        }
                }
                if let Some(composition_obj) = state.composition.clone() {
                        if let Ok(comp_range) = unsafe { composition_obj.GetRange() } {
                                let sel_range = unsafe { std::mem::ManuallyDrop::take(&mut selection[0].range) }.ok_or(E_POINTER)?;
                                if composition::is_range_covered(ec, &sel_range, &comp_range) {
                                        composition::end_composition(state, context);
                                }
                        }
                }
        }
        handle_cancel(state, ec, context)
}

/// Port of _HandleCompositionFinalizeRaw.
pub(crate) fn handle_composition_finalize_raw(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        let Some(processor) = state.processor.clone() else {
                return Err(Error::from_hresult(S_FALSE));
        };
        let raw = processor.lock().unwrap_or_else(|p| p.into_inner()).raw_input_text();
        if raw.is_empty() {
                return handle_cancel(state, ec, context);
        }
        let wide: Vec<u16> = raw.encode_utf16().collect();
        if state.is_composing() {
                composition::add_composing_and_char(state, ec, context, &wide)?;
        } else {
                composition::add_char_and_finalize(ec, context, &wide)?;
        }
        handle_complete(state, ec, context)
}

/// Port of _HandleCompositionConvert.
fn handle_composition_convert(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        let Some(processor) = state.processor.clone() else { return Ok(()) };
        let candidates = processor.lock().unwrap_or_else(|p| p.into_inner()).candidate_list();
        if candidates.is_empty() {
                return Ok(());
        }
        if let Some(presenter) = state.candidate_presenter.take() {
                presenter.end();
                state.candidate_mode = CandidateMode::None;
                state.symbol_query = None;
        }
        let service = state.self_iunknown.clone().ok_or(E_UNEXPECTED)?;
        let page_size = processor
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .keys
                .candidate_list_index_range()
                .len()
                .max(1);
        let presenter: ComObject<CandidateListPresenter> =
                CandidateListPresenter::new(&service, page_size, false).into();
        state.candidate_mode = CandidateMode::Original;
        let doc_mgr = unsafe { context.GetDocumentMgr()? };
        let Some(composition_obj) = state.composition.clone() else {
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        let range = unsafe { composition_obj.GetRange()? };
        let Some(thread_mgr) = state.thread_mgr.clone() else {
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        CandidateListPresenter::start(&presenter, state.client_id, &thread_mgr, &doc_mgr, context, ec, &range)?;
        presenter.set_text(candidates);
        state.candidate_presenter = Some(presenter);
        Ok(())
}

/// Port of _HandleCompositionBackspace.
fn handle_composition_backspace(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        if !state.is_composing() {
                return Ok(());
        }
        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
                if context
                        .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
                        .is_err()
                        || fetched != 1
                {
                        return Err(Error::from_hresult(S_FALSE));
                }
        }
        if let Some(composition_obj) = state.composition.clone() {
                if let Ok(comp_range) = unsafe { composition_obj.GetRange() } {
                        let sel_range = unsafe { std::mem::ManuallyDrop::take(&mut selection[0].range) }.ok_or(E_POINTER)?;
                        if !composition::is_range_covered(ec, &sel_range, &comp_range) {
                                return Ok(());
                        }
                }
        }
        let Some(processor) = state.processor.clone() else { return Ok(()) };
        let count = processor.lock().unwrap_or_else(|p| p.into_inner()).input_key_count();
        if count > 0 {
                processor
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove_input_key(count - 1);
                if processor.lock().unwrap_or_else(|p| p.into_inner()).input_key_count() > 0 {
                        handle_composition_input_worker(state, processor, ec, context)?;
                } else {
                        handle_cancel(state, ec, context)?;
                }
        }
        Ok(())
}

/// Port of _HandleCompositionArrowKey.
fn handle_composition_arrow_key(state: &mut ServiceState, ec: u32, context: &ITfContext, function: KeystrokeFunction) -> Result<()> {
        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
                if context
                        .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
                        .is_err()
                        || fetched != 1
                {
                        return Ok(());
                }
        }
        if let Some(presenter) = &state.candidate_presenter {
                presenter.advise_ui_changed_by_arrow_key(function);
        }
        unsafe {
                let _ = context.SetSelection(ec, &selection);
        }
        Ok(())
}

// ---------------------------------------------------------------------
// Punctuation — port of _HandlePunctuationKey / _FinalizeBeforePunctuation /
// _StartPunctuationCandidateList
// ---------------------------------------------------------------------

fn handle_punctuation_key(state: &mut ServiceState, ec: u32, context: &ITfContext, code: u32, is_shifting: bool) -> Result<()> {
        let Some(key) = crate::punctuation::PunctuationKey::for_virtual_key(code) else {
                return Err(Error::from_hresult(E_INVALIDARG));
        };
        if !key.should_handle(is_shifting) {
                return Err(Error::from_hresult(E_INVALIDARG));
        }
        finalize_before_punctuation(state, ec, context)?;

        let is_cantonese = thread_compartment(state, globals::GUID_COMPARTMENT_PUNCTUATION_FORM)
                .get_bool()
                .unwrap_or(true);
        let output = if is_cantonese {
                key.instant_symbol(is_shifting)
        } else {
                Some(key.text(is_shifting))
        };
        if let Some(output) = output {
                let wide: Vec<u16> = output.encode_utf16().collect();
                return composition::add_char_and_finalize(ec, context, &wide);
        }
        start_punctuation_candidate_list(state, ec, context, key, is_shifting)
}

fn finalize_before_punctuation(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        if state.candidate_presenter.is_some() && state.candidate_mode != CandidateMode::None {
                if let Some(presenter) = &state.candidate_presenter {
                        let text = presenter.selected_candidate_string();
                        let index = presenter.selected_candidate_index();
                        if let Some(text) = text.filter(|t| !t.is_empty()) {
                                let wide: Vec<u16> = text.encode_utf16().collect();
                                composition::add_composing_and_char(state, ec, context, &wide)?;
                                if state.candidate_mode != CandidateMode::Punctuation {
                                        if let (Some(index), Some(processor)) = (index, state.processor.clone()) {
                                                processor
                                                        .lock()
                                                        .unwrap_or_else(|p| p.into_inner())
                                                        .commit_selected_candidate_for_memory(index);
                                        }
                                }
                        }
                }
                return handle_complete(state, ec, context);
        }
        if state.is_composing() {
                return handle_composition_finalize_raw(state, ec, context);
        }
        Ok(())
}

fn start_punctuation_candidate_list(
        state: &mut ServiceState,
        ec: u32,
        context: &ITfContext,
        key: &'static crate::punctuation::PunctuationKey,
        is_shifting: bool,
) -> Result<()> {
        let symbols = key.symbols(is_shifting);
        if symbols.is_empty() {
                return Err(Error::from_hresult(S_FALSE));
        }
        let sink = state.self_iunknown.clone().ok_or(E_UNEXPECTED)?;
        composition::start_composition(state, ec, context, &sink)?;

        let placeholder: Vec<u16> = key.text(is_shifting).encode_utf16().collect();
        if let Err(e) = composition::add_composing_and_char(state, ec, context, &placeholder) {
                let _ = handle_cancel(state, ec, context);
                return Err(e);
        }

        let page_size = state
                .processor
                .as_ref()
                .map(|p| p.lock().unwrap_or_else(|x| x.into_inner()).keys.candidate_list_index_range().len().max(1))
                .unwrap_or(7);
        let presenter: ComObject<CandidateListPresenter> =
                CandidateListPresenter::new(&sink, page_size, false).into();
        state.candidate_mode = CandidateMode::Punctuation;

        let doc_mgr = unsafe { context.GetDocumentMgr()? };
        let range = unsafe { state.composition.as_ref().ok_or(E_UNEXPECTED)?.GetRange()? };
        let Some(thread_mgr) = state.thread_mgr.clone() else {
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        if let Err(e) = CandidateListPresenter::start(&presenter, state.client_id, &thread_mgr, &doc_mgr, context, ec, &range) {
                let _ = handle_cancel(state, ec, context);
                return Err(e);
        }

        let items: Vec<CandidateItem> = symbols
                .iter()
                .map(|symbol| CandidateItem {
                        text: symbol.text.to_string(),
                        comment: match (symbol.comment, symbol.secondary_comment) {
                                (Some(a), Some(b)) => format!("{} {}", a, b),
                                (Some(a), None) => a.to_string(),
                                _ => String::new(),
                        },
                        input_count: 0,
                        separator: false,
                })
                .collect();
        presenter.set_text(items);
        state.candidate_presenter = Some(presenter);
        // "/" enables the rime-style query: /2 → numeral variants,
        // /nei → single characters by jyutping.
        let is_slash = key.key_code == windows::Win32::UI::Input::KeyboardAndMouse::VK_OEM_2.0 as u32;
        state.symbol_query = if is_slash && !is_shifting { Some(String::new()) } else { None };
        Ok(())
}

/// "/x" query editing — port extension inspired by rime-cantonese.
/// Letters extend a jyutping lookup, the first digit picks numeral variants,
/// Backspace pops. Rebuilds the punctuation candidate list in place.
fn handle_symbol_query(
        state: &mut ServiceState,
        ec: u32,
        context: &ITfContext,
        code: u32,
) -> Result<()> {
        {
                let Some(query) = &mut state.symbol_query else { return Ok(()) };
                if code == crate::keytable::VK_BACK {
                        query.pop();
                } else if let Some(ch) = char::from_u32(code) {
                        query.push(ch.to_ascii_lowercase());
                }
        }
        let query = state.symbol_query.clone().unwrap_or_default();

        let items: Vec<CandidateItem> = if query.is_empty() {
                crate::punctuation::slash_key().symbols(false)
                        .iter()
                        .map(|symbol| CandidateItem {
                                text: symbol.text.to_string(),
                                comment: match (symbol.comment, symbol.secondary_comment) {
                                        (Some(a), Some(b)) => format!("{} {}", a, b),
                                        (Some(a), None) => a.to_string(),
                                        _ => String::new(),
                                },
                                input_count: 0,
                        separator: false,
                        })
                        .collect()
        } else if query.chars().all(|c| c.is_ascii_digit()) {
                crate::punctuation::numeral_variants(query.chars().last().unwrap_or('0'))
                        .iter()
                        .map(|symbol| CandidateItem {
                                text: symbol.text.to_string(),
                                comment: match (symbol.comment, symbol.secondary_comment) {
                                        (Some(a), Some(b)) => format!("{} {}", a, b),
                                        (Some(a), None) => a.to_string(),
                                        _ => String::new(),
                                },
                                input_count: 0,
                        separator: false,
                        })
                        .collect()
        } else {
                // Letter query — accented/diacritic variants (rime-cantonese
                // style: /a → ā á ǎ à â ä …). Extra letters are ignored.
                let letter = query.chars().next().unwrap_or_default();
                crate::punctuation::letter_variants(letter)
                        .iter()
                        .map(|symbol| CandidateItem {
                                text: symbol.text.to_string(),
                                comment: match (symbol.comment, symbol.secondary_comment) {
                                        (Some(a), Some(b)) => format!("{} {}", a, b),
                                        (Some(a), None) => a.to_string(),
                                        _ => String::new(),
                                },
                                input_count: 0,
                        separator: false,
                        })
                        .collect()
        };

        // Update the composition text to "/" + query.
        let reading = format!("/{query}");
        let wide: Vec<u16> = reading.encode_utf16().collect();
        composition::add_composing_and_char(state, ec, context, &wide)?;

        if let Some(presenter) = &state.candidate_presenter {
                presenter.clear_list();
                presenter.set_text(items);
        }
        Ok(())
}

/// Port of _HandleCompositionCharacterForm — commit the full-width character.
fn handle_composition_character_form(state: &mut ServiceState, ec: u32, context: &ITfContext, wch: u16) -> Result<()> {
        let full = if wch == 0x20 { 0x3000u16 } else { wch + 0xFEE0 };
        composition::add_char_and_finalize(ec, context, &[full])?;
        handle_cancel(state, ec, context)
}

// ---------------------------------------------------------------------
// Options menu — port of _optionsMode / ToggleOptionsMode / _BuildOptionsRows.
// A candidate-window menu toggled by Ctrl+`: digits select, Esc closes.
// ---------------------------------------------------------------------

/// Port of _BuildOptionsRows — the ten rows, `✓` on the active choice.
fn options_rows(processor: &std::sync::Arc<Mutex<Processor>>) -> Vec<CandidateItem> {
        use crate::settings::*;
        use crate::variants::CharacterVariant;
        use crate::strings::*;
        let p = processor.lock().unwrap_or_else(|e| e.into_inner());
        let variant = p.current_character_variant();
        let form = p.current_character_form();
        let punct = p.current_punctuation_form();
        let mode = p.current_input_method_mode();
        let _ = mode; // input-mode rows removed — Shift toggles it directly
        let selected = [
                variant == CharacterVariant::HongKong,
                variant == CharacterVariant::Taiwan,
                variant == CharacterVariant::Simplified,
                form == CharacterForm::HalfWidth,
                form == CharacterForm::FullWidth,
                punct == PunctuationForm::Cantonese,
                punct == PunctuationForm::English,
        ];
        let ids = [
                IDS_OPTIONS_CHARACTER_VARIANT_HONG_KONG,
                IDS_OPTIONS_CHARACTER_VARIANT_TAIWAN,
                IDS_OPTIONS_CHARACTER_VARIANT_SIMPLIFIED,
                IDS_OPTIONS_CHARACTER_FORM_HALF_WIDTH,
                IDS_OPTIONS_CHARACTER_FORM_FULL_WIDTH,
                IDS_OPTIONS_PUNCTUATION_FORM_CANTONESE,
                IDS_OPTIONS_PUNCTUATION_FORM_ENGLISH,
        ];
        let fallbacks = [
                "Traditional Chinese (Hong Kong)",
                "Traditional Chinese (Taiwan)",
                "Simplified Chinese",
                "Half-width Symbols",
                "Full-width Symbols",
                "Chinese Punctuation",
                "English Punctuation",
        ];
        let sep = || CandidateItem { text: String::new(), comment: String::new(), input_count: 0, separator: true };
        let mut items: Vec<CandidateItem> = (0..3)
                .map(|i| CandidateItem {
                        text: text(ids[i]).unwrap_or(fallbacks[i]).to_string(),
                        comment: if selected[i] { "\u{2713}".to_string() } else { String::new() },
                        input_count: 0,
                        separator: false,
                })
                .collect();
        items.push(sep());
        for i in 3..5 {
                items.push(CandidateItem {
                        text: text(ids[i]).unwrap_or(fallbacks[i]).to_string(),
                        comment: if selected[i] { "\u{2713}".to_string() } else { String::new() },
                        input_count: 0,
                        separator: false,
                });
        }
        items.push(sep());
        for i in 5..7 {
                items.push(CandidateItem {
                        text: text(ids[i]).unwrap_or(fallbacks[i]).to_string(),
                        comment: if selected[i] { "\u{2713}".to_string() } else { String::new() },
                        input_count: 0,
                        separator: false,
                });
        }
        items
}

/// Port of ToggleOptionsMode.
fn handle_options_toggle(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        if state.candidate_mode == CandidateMode::Options {
                return handle_options_close(state, ec, context);
        }
        let Some(processor) = state.processor.clone() else {
                globals::log("options toggle: no processor");
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        let items = options_rows(&processor);
        if let Some(presenter) = &state.candidate_presenter {
                // A composition's candidate list is showing — swap in the
                // options rows; closing restores the candidates.
                presenter.clear_list();
                presenter.set_text(items);
                state.candidate_mode = CandidateMode::Options;
                globals::log("options toggle: swapped into existing presenter");
                return Ok(());
        }
        // Standalone presenter anchored at the caret — port of _StartOptions.
        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0u32;
        unsafe {
                if let Err(e) = context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched) {
                        globals::log_error(&format!("options toggle: GetSelection failed {e:?}"));
                        return Err(e);
                }
        }
        if fetched != 1 {
                globals::log("options toggle: GetSelection fetched!=1");
                return Err(Error::from_hresult(E_FAIL));
        }
        let range = unsafe { std::mem::ManuallyDrop::take(&mut selection[0].range) }.ok_or(E_POINTER)?;
        let sink = state.self_iunknown.clone().ok_or(E_UNEXPECTED)?;
        let doc_mgr = unsafe { context.GetDocumentMgr()? };
        let Some(thread_mgr) = state.thread_mgr.clone() else {
                globals::log("options toggle: no thread_mgr");
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        let presenter: ComObject<CandidateListPresenter> = CandidateListPresenter::new(&sink, 10, false).into();
        if let Err(e) = CandidateListPresenter::start(&presenter, state.client_id, &thread_mgr, &doc_mgr, context, ec, &range) {
                globals::log_error(&format!("options toggle: presenter start failed {e:?}"));
                presenter.end();
                return Err(e);
        }
        presenter.set_text(items);
        state.candidate_presenter = Some(presenter);
        state.candidate_mode = CandidateMode::Options;
        globals::log("options toggle: standalone presenter shown");
        Ok(())
}

/// Close the menu; when a composition is live, rebuild its candidate list.
fn handle_options_close(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        if let Some(presenter) = state.candidate_presenter.take() {
                presenter.end();
        }
        state.candidate_mode = CandidateMode::None;
        if state.is_composing() {
                if let Some(processor) = state.processor.clone() {
                        return handle_composition_input_worker(state, processor, ec, context);
                }
        }
        Ok(())
}

/// Port of _ApplyOptionsSelection — row index → setting, then close.
fn handle_options_select(state: &mut ServiceState, ec: u32, context: &ITfContext, code: u32) -> Result<()> {
        let Some(processor) = state.processor.clone() else {
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        let Some(pos) = candidate_index_from_code(&processor, code) else {
                return Err(Error::from_hresult(S_FALSE));
        };
        let Some(thread_mgr) = state.thread_mgr.clone() else {
                return Err(Error::from_hresult(E_UNEXPECTED));
        };
        {
                use crate::settings::*;
                use crate::variants::CharacterVariant;
                let mut p = processor.lock().unwrap_or_else(|e| e.into_inner());
                match pos {
                        0 => {
                                p.set_character_variant(CharacterVariant::HongKong);
                        }
                        1 => {
                                p.set_character_variant(CharacterVariant::Taiwan);
                        }
                        2 => {
                                p.set_character_variant(CharacterVariant::Simplified);
                        }
                        3 => p.set_character_form(CharacterForm::HalfWidth, &thread_mgr),
                        4 => p.set_character_form(CharacterForm::FullWidth, &thread_mgr),
                        5 => p.set_punctuation_form(PunctuationForm::Cantonese, &thread_mgr),
                        6 => p.set_punctuation_form(PunctuationForm::English, &thread_mgr),
                        _ => {}
                }
        }
        handle_options_close(state, ec, context)
}

// ---------------------------------------------------------------------
// Candidate handlers — port of CandidateListUIPresenter.cpp key handlers
// ---------------------------------------------------------------------

fn handle_candidate_finalize(state: &mut ServiceState, processor: std::sync::Arc<Mutex<Processor>>, ec: u32, context: &ITfContext) -> Result<()> {
        if let Some(presenter) = &state.candidate_presenter {
                let text = presenter.selected_candidate_string();
                let input_count = presenter.selected_input_count();
                let index = presenter.selected_candidate_index();
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                        // incremental finalize: if the candidate consumed only part
                        // of the input, continue composing with the tail keys.
                        if state.candidate_mode == CandidateMode::Incremental && input_count > 0 {
                                let tail = processor
                                        .lock()
                                        .unwrap_or_else(|p| p.into_inner())
                                        .candidate_tail_input_events(input_count);
                                if !tail.is_empty() {
                                        return incremental_finalize(state, processor, ec, context, text, index, tail);
                                }
                        }
                        let wide: Vec<u16> = text.encode_utf16().collect();
                        composition::add_composing_and_char(state, ec, context, &wide)?;
                        if state.candidate_mode != CandidateMode::Punctuation {
                                if let Some(index) = index {
                                        processor
                                                .lock()
                                                .unwrap_or_else(|p| p.into_inner())
                                                .commit_selected_candidate_for_memory(index);
                                }
                        }
                }
        }
        handle_complete(state, ec, context)
}

/// Port of _HandleIncrementalCandidateFinalize.
fn incremental_finalize(
        state: &mut ServiceState,
        processor: std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
        text: String,
        index: Option<u32>,
        tail_events: Vec<crate::types::BasicInputEvent>,
) -> Result<()> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        composition::add_composing_and_char(state, ec, context, &wide)?;
        if let Some(index) = index {
                processor
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .append_selected_candidate_for_memory(index);
        }
        if let Some(presenter) = state.candidate_presenter.take() {
                presenter.end();
        }
        state.candidate_mode = CandidateMode::None;
        state.symbol_query = None;
        composition::terminate_composition(state, ec, context, false);

        processor
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .set_input_events(tail_events);
        let sink = state.self_iunknown.clone().ok_or(E_UNEXPECTED)?;
        if let Err(e) = composition::start_composition(state, ec, context, &sink) {
                processor.lock().unwrap_or_else(|p| p.into_inner()).clear_input_keys();
                return Err(e);
        }
        handle_composition_input_worker(state, processor, ec, context)
}

/// Port of _HandleCandidateConvert → _HandleCandidateWorker.
fn handle_candidate_worker(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        let Some(presenter) = &state.candidate_presenter else {
                if state.candidate_mode == CandidateMode::Incremental {
                        return handle_composition_finalize_raw(state, ec, context);
                }
                return Ok(());
        };
        let Some(text) = presenter.selected_candidate_string() else {
                if state.candidate_mode == CandidateMode::Incremental {
                        return handle_composition_finalize_raw(state, ec, context);
                }
                return Err(Error::from_hresult(S_FALSE));
        };
        let _ = text;
        // Upstream would build a phrase candidate list here; it is always empty,
        // so fall through to finalize.
        handle_candidate_finalize(state, state.processor.clone().ok_or(E_FAIL)?, ec, context)
}

/// Port of _HandleCandidateSelectByNumber.
fn handle_candidate_select_by_number(
        state: &mut ServiceState,
        processor: std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
        code: u32,
) -> Result<()> {
        let Some(pos) = candidate_index_from_code(&processor, code) else {
                return Err(Error::from_hresult(S_FALSE));
        };
        if let Some(presenter) = &state.candidate_presenter {
                if presenter.set_selection_in_page(pos) {
                        return handle_candidate_worker(state, ec, context);
                }
        }
        Err(Error::from_hresult(S_FALSE))
}

/// Port of _HandleCandidateForget.
fn handle_candidate_forget(state: &mut ServiceState, processor: std::sync::Arc<Mutex<Processor>>, ec: u32, context: &ITfContext) -> Result<()> {
        let Some(presenter) = &state.candidate_presenter else {
                return Err(Error::from_hresult(S_FALSE));
        };
        let Some(index) = presenter.selected_candidate_index() else {
                return Err(Error::from_hresult(S_FALSE));
        };
        let forgot = processor
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .forget_candidate_from_memory(index);
        if !forgot {
                return Err(Error::from_hresult(S_FALSE));
        }
        handle_composition_input_worker(state, processor, ec, context)
}

/// Port of _HandlePhraseFinalize.
fn handle_phrase_finalize(state: &mut ServiceState, ec: u32, context: &ITfContext) -> Result<()> {
        if let Some(presenter) = &state.candidate_presenter {
                if let Some(text) = presenter.selected_candidate_string().filter(|t| !t.is_empty()) {
                        let wide: Vec<u16> = text.encode_utf16().collect();
                        composition::add_char_and_finalize(ec, context, &wide)?;
                }
        }
        handle_complete(state, ec, context)
}

/// Port of _HandlePhraseSelectByNumber.
fn handle_phrase_select_by_number(
        state: &mut ServiceState,
        processor: std::sync::Arc<Mutex<Processor>>,
        ec: u32,
        context: &ITfContext,
        code: u32,
) -> Result<()> {
        let Some(pos) = candidate_index_from_code(&processor, code) else {
                return Err(Error::from_hresult(S_FALSE));
        };
        if let Some(presenter) = &state.candidate_presenter {
                if presenter.set_selection_in_page(pos) {
                        return handle_phrase_finalize(state, ec, context);
                }
        }
        Err(Error::from_hresult(S_FALSE))
}

/// Port of CCandidateRange::GetIndex — map a key code to a page position.
fn candidate_index_from_code(processor: &std::sync::Arc<Mutex<Processor>>, code: u32) -> Option<usize> {
        let processor = processor.lock().unwrap_or_else(|p| p.into_inner());
        let range = processor.keys.candidate_list_index_range();
        let digit = match code {
                0x30..=0x39 => code - 0x30,
                0x60..=0x69 => code - 0x60,
                _ => return None,
        };
        range.iter().position(|&k| k == digit)
}
