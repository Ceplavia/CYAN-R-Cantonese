// Per-input-context session: owns the jyutping key buffer, candidate list
// and composition state for one HIMC. Mirrors the processor.rs pipeline but
// drives IMM32 messages instead of TSF edit sessions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::core::PCWSTR;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::ctx;
use crate::engine::CoreImeEngine;
use crate::globals;
use crate::memory::InputMemory;
use crate::settings::{CharacterForm, ImeSettings, PunctuationForm};
use crate::types::{join_lexicons, text_from_keys, Candidate, ReverseLookupMethod, VirtualInputKey};

// Composite GCS_COMP (SDK macro) — all composition-related flags at once.
const GCS_COMP: u32 = GCS_COMPREADSTR.0 | GCS_COMPSTR.0 | GCS_COMPATTR.0 | GCS_COMPREADATTR.0 | GCS_COMPCLAUSE.0 | GCS_COMPREADCLAUSE.0 | GCS_CURSORPOS.0 | GCS_DELTASTART.0;
use crate::variants::standard_for_variant;
use crate::{config, punctuation, ui};

const VK_BACK: u32 = 0x08;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;

/// A displayed candidate (text + optional comment).
#[derive(Clone)]
pub struct CandItem {
        pub text: String,
        pub comment: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
        /// Normal jyutping input — input_keys drive the engine.
        Jyutping,
        /// Punctuation/symbol list — candidates are symbols, committed
        /// verbatim (no memory learning).
        Punct,
}

pub struct Session {
        himc: usize,
        input_keys: Vec<VirtualInputKey>,
        candidates: Vec<Candidate>,
        items: Vec<CandItem>,
        selection: usize,
        mode: Mode,
        /// "/x" rime-style symbol query — Some when slash list is queryable.
        slash_query: Option<String>,
        punct_shift: bool,
        /// Set once a real key arrived while Shift was held — prevents the
        /// Shift-keyup from toggling the keyboard open state.
        shift_used: bool,
        composing: bool,
        settings: ImeSettings,
}

static SESSIONS: OnceLock<Mutex<HashMap<usize, Session>>> = OnceLock::new();
static ENGINE: OnceLock<Option<CoreImeEngine>> = OnceLock::new();
static MEMORY: OnceLock<Mutex<InputMemory>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<usize, Session>> {
        SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// ime.sqlite3 lives in the install dir — resolve it via the TSF dll's
/// InProcServer32 path (the .ime itself sits in System32/SysWOW64).
fn database_path() -> Option<PathBuf> {
        use windows::Win32::System::Registry::*;
        use windows::core::w;
        let mut value = [0u16; 512];
        let mut len = (value.len() * 2) as u32;
        let status = unsafe {
                RegGetValueW(
                        HKEY_LOCAL_MACHINE,
                        w!("SOFTWARE\\Classes\\CLSID\\{D2291A80-84D8-4641-9AB2-BDD1472C846B}\\InProcServer32"),
                        PCWSTR::null(),
                        RRF_RT_REG_SZ,
                        None,
                        Some(value.as_mut_ptr() as *mut _),
                        Some(&mut len),
                )
        };
        let dir = if status == ERROR_SUCCESS {
                let s = String::from_utf16_lossy(&value[..(len as usize / 2).saturating_sub(1)]);
                PathBuf::from(s).parent().map(|p| p.to_path_buf())
        } else {
                None
        }
        .unwrap_or_else(|| PathBuf::from("C:\\Program Files\\R-Cantonese"));
        let db = dir.join(globals::TEXTSERVICE_SQLITE_DATA);
        if db.exists() { Some(db) } else { None }
}

fn engine() -> Option<&'static CoreImeEngine> {
        ENGINE
                .get_or_init(|| database_path().and_then(|p| CoreImeEngine::prepare_path(&p)))
                .as_ref()
}

fn memory() -> &'static Mutex<InputMemory> {
        MEMORY.get_or_init(|| {
                let mut m = InputMemory::new();
                m.prepare();
                Mutex::new(m)
        })
}

fn reverse_lookup_method(keys: &[VirtualInputKey]) -> ReverseLookupMethod {
        keys.first().map(|k| ReverseLookupMethod::from_first_key(*k)).unwrap_or(ReverseLookupMethod::None)
}

impl Session {
        fn new(himc: HIMC) -> Self {
                Self {
                        himc: himc.0 as usize,
                        input_keys: Vec::new(),
                        candidates: Vec::new(),
                        items: Vec::new(),
                        selection: 0,
                        mode: Mode::Jyutping,
                        slash_query: None,
                        punct_shift: false,
                        shift_used: false,
                        composing: false,
                        settings: config::load(),
                }
        }

        fn imc(&self) -> HIMC {
                HIMC(self.himc as *mut _)
        }

        fn page_size(&self) -> usize {
                self.settings.candidate_page_size.max(1) as usize
        }

        fn is_open(&self) -> bool {
                unsafe { ImmGetOpenStatus(self.imc()).as_bool() }
        }

        // -- composition plumbing ------------------------------------------

        fn start_composition(&mut self) {
                self.composing = true;
                ctx::push_message(self.imc(), WM_IME_STARTCOMPOSITION, 0, 0);
                self.position_window();
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_CHANGECANDIDATE as usize, 0);
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_OPENCANDIDATE as usize, 0);
        }

        fn end_composition(&mut self) {
                self.composing = false;
                ctx::write_comp(self.imc(), &[], &[]);
                ctx::push_message(self.imc(), WM_IME_ENDCOMPOSITION, 0, 0);
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_CLOSECANDIDATE as usize, 0);
                ui::hide_candidates();
        }

        /// Commit `text` into the target app and clear the session.
        fn commit(&mut self, text: &str) {
                let wide: Vec<u16> = text.encode_utf16().collect();
                let had_comp = self.composing;
                ctx::write_comp(self.imc(), &[], &wide);
                if had_comp {
                        ctx::push_message(
                                self.imc(),
                                WM_IME_COMPOSITION,
                                0,
                                (GCS_COMP | GCS_RESULTSTR.0) as isize,
                        );
                        self.end_composition();
                } else {
                        // No open composition — still deliver via RESULTSTR so
                        // the app picks the text up (punctuation commit path).
                        ctx::push_message(self.imc(), WM_IME_STARTCOMPOSITION, 0, 0);
                        ctx::push_message(
                                self.imc(),
                                WM_IME_COMPOSITION,
                                0,
                                (GCS_COMP | GCS_RESULTSTR.0) as isize,
                        );
                        self.end_composition();
                }
                self.clear();
        }

        fn clear(&mut self) {
                self.input_keys.clear();
                self.candidates.clear();
                self.items.clear();
                self.selection = 0;
                self.slash_query = None;
                self.mode = Mode::Jyutping;
        }

        /// Refresh the composition string + candidate window to match state.
        fn sync(&mut self) {
                // compstr = raw input text — apps that render it show the
                // jyutping letters inline; our own window shows candidates.
                let raw = text_from_keys(&self.input_keys);
                let wide: Vec<u16> = raw.encode_utf16().collect();
                ctx::write_comp(self.imc(), &wide, &[]);
                ctx::push_message(
                        self.imc(),
                        WM_IME_COMPOSITION,
                        0,
                        (GCS_COMPSTR.0 | GCS_CURSORPOS.0) as isize,
                );
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_CHANGECANDIDATE as usize, 0);
                // Feed hCandInfo for apps that render their own IME UI
                // (games read CANDIDATELIST on IMN_* and draw in-engine).
                let texts: Vec<String> = self.items.iter().map(|c| c.text.clone()).collect();
                let _ = ctx::write_candlist(self.imc(), &texts, self.selection as u32, self.settings.candidate_page_size);
                if self.items.is_empty() {
                        ui::hide_candidates();
                } else {
                        self.position_window();
                        ui::show_candidates(&self.items, self.selection, &self.settings);
                }
        }

        fn position_window(&self) {
                let Some(imc) = ctx::lock_imc(self.imc()) else { return };
                // Weasel's OnIMEFocus: default comp form at the caret once.
                if imc.fdwInit & INIT_COMPFORM == 0 {
                        imc.cfCompForm.dwStyle = CFS_DEFAULT;
                        unsafe {
                                let mut p = windows::Win32::Foundation::POINT::default();
                                let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
                                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(imc.hWnd, &mut p);
                                imc.cfCompForm.ptCurrentPos = p;
                        }
                        imc.fdwInit |= INIT_COMPFORM;
                }
                let hwnd = imc.hWnd;
                let style = imc.cfCompForm.dwStyle;
                let pt = imc.cfCompForm.ptCurrentPos;
                let cand_pt = imc.cfCandForm[0].ptCurrentPos;
                let font_h = unsafe { imc.lfFont.W.lfHeight };
                ctx::unlock_imc(self.imc());
                let mut screen = match style {
                        CFS_POINT | CFS_FORCE_POSITION | CFS_RECT => pt,
                        CFS_CANDIDATEPOS => cand_pt,
                        _ => {
                                // CFS_DEFAULT / unknown — use the caret.
                                let mut p = windows::Win32::Foundation::POINT::default();
                                unsafe {
                                        let _ = windows::Win32::UI::WindowsAndMessaging::GetCaretPos(&mut p);
                                }
                                p
                        }
                };
                unsafe {
                        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut screen);
                }
                ui::set_position(screen.x, screen.y + font_h.abs() + 2);
        }

        // -- candidate pipeline --------------------------------------------

        fn recompute(&mut self) {
                if self.mode == Mode::Punct {
                        self.recompute_punct();
                        return;
                }
                self.candidates = self.compute_suggestions();
                self.items = self
                        .candidates
                        .iter()
                        .map(|c| CandItem {
                                text: c.text.clone(),
                                comment: c.comment.clone().unwrap_or_default(),
                        })
                        .collect();
                self.selection = 0;
                globals::log(&format!(
                        "ime: input={} items={} first={}",
                        text_from_keys(&self.input_keys),
                        self.items.len(),
                        self.items.first().map(|c| c.text.as_str()).unwrap_or("")
                ));
        }

        fn compute_suggestions(&mut self) -> Vec<Candidate> {
                let Some(engine) = engine() else { return Vec::new() };
                let standard = standard_for_variant(self.settings.character_variant);
                let method = reverse_lookup_method(&self.input_keys);
                if method == ReverseLookupMethod::None {
                        let memory_suggestions = {
                                let seg = engine.segment(&self.input_keys);
                                memory().lock().map(|m| m.suggest(&self.input_keys, &seg, &engine.segmenter)).unwrap_or_default()
                        };
                        let seg = engine.segment(&self.input_keys);
                        let queried = engine.suggest(&self.input_keys, &seg, true);
                        let texts = if queried.is_empty() {
                                engine.search_plain_texts(&self.input_keys)
                        } else {
                                Vec::new()
                        };
                        let symbols = engine.search_symbols(&self.input_keys, &seg);
                        crate::converter::dispatch(
                                &engine.database,
                                &memory_suggestions,
                                &[],
                                &texts,
                                &symbols,
                                &queried,
                                crate::types::RomanizationForm::Full,
                                standard,
                        )
                } else {
                        let mut suggestions = crate::converter::transform(
                                &engine.database,
                                &engine.search_plain_texts(&self.input_keys),
                                crate::types::RomanizationForm::Full,
                                standard,
                        );
                        let query_keys = if self.input_keys.len() > 1 {
                                self.input_keys[1..].to_vec()
                        } else {
                                Vec::new()
                        };
                        if !query_keys.is_empty() {
                                suggestions.extend(crate::converter::transform(
                                        &engine.database,
                                        &engine.reverse_lookup(method, &query_keys),
                                        crate::types::RomanizationForm::Full,
                                        standard,
                                ));
                        }
                        suggestions
                }
        }

        fn recompute_punct(&mut self) {
                let items = match &self.slash_query {
                        Some(q) if !q.is_empty() => {
                                if q.chars().all(|c| c.is_ascii_digit()) {
                                        punctuation::numeral_variants(q.chars().last().unwrap_or('0'))
                                } else {
                                        punctuation::letter_variants(q.chars().last().unwrap_or('a'))
                                }
                                .iter()
                                .map(|s| CandItem {
                                        text: s.text.to_string(),
                                        comment: match (s.comment, s.secondary_comment) {
                                                (Some(a), Some(b)) => format!("{} {}", a, b),
                                                (Some(a), None) => a.to_string(),
                                                _ => String::new(),
                                        },
                                })
                                .collect()
                        }
                        _ => {
                                let key = punctuation::slash_key();
                                key.symbols(self.punct_shift)
                                        .iter()
                                        .map(|s| CandItem {
                                                text: s.text.to_string(),
                                                comment: match (s.comment, s.secondary_comment) {
                                                        (Some(a), Some(b)) => format!("{} {}", a, b),
                                                        (Some(a), None) => a.to_string(),
                                                        _ => String::new(),
                                                },
                                        })
                                        .collect()
                        }
                };
                self.candidates.clear();
                self.items = items;
                self.selection = self.selection.min(self.items.len().saturating_sub(1));
        }

        fn open_punct_list(&mut self, key: &'static punctuation::PunctuationKey, is_shifting: bool) -> bool {
                let symbols = key.symbols(is_shifting);
                if symbols.is_empty() {
                        return false;
                }
                self.clear();
                self.mode = Mode::Punct;
                self.punct_shift = is_shifting;
                self.slash_query = if key.key_code == VK_OEM_2.0 as u32 && !is_shifting {
                        Some(String::new())
                } else {
                        None
                };
                self.items = symbols
                        .iter()
                        .map(|s| CandItem {
                                text: s.text.to_string(),
                                comment: match (s.comment, s.secondary_comment) {
                                        (Some(a), Some(b)) => format!("{} {}", a, b),
                                        (Some(a), None) => a.to_string(),
                                        _ => String::new(),
                                },
                        })
                        .collect();
                self.selection = 0;
                if !self.composing {
                        self.start_composition();
                }
                self.sync();
                true
        }

        /// Commit the selected candidate (memory learning for jyutping).
        fn commit_selection(&mut self) {
                let index = self.selection;
                if self.mode == Mode::Punct {
                        let text = self.items.get(index).map(|c| c.text.clone());
                        if let Some(text) = text {
                                self.commit(&text);
                        }
                        return;
                }
                let Some(cand) = self.candidates.get(index).cloned() else {
                        // No candidates — commit raw input.
                        let raw = text_from_keys(&self.input_keys);
                        if !raw.is_empty() {
                                self.commit(&raw);
                        }
                        return;
                };
                if cand.lexicon.is_cantonese() {
                        if let Some(joined) = join_lexicons(&[cand.lexicon.clone()]) {
                                if let Ok(mut m) = memory().lock() {
                                        m.handle(&joined);
                                }
                        }
                }
                self.commit(&cand.text);
        }

        fn commit_raw(&mut self) {
                let raw = text_from_keys(&self.input_keys);
                self.commit(&raw);
        }

        // -- key dispatch ----------------------------------------------------

        /// Called from ImeProcessKey. `keystate` is the 256-byte keyboard
        /// state array. Returns true when the IME consumes the key.
        pub fn process_key(&mut self, vk: u32, lparam: isize, keystate: &[u8; 256]) -> bool {
                let shift = keystate[VK_SHIFT as usize] & 0x80 != 0;
                let ctrl = keystate[VK_CONTROL as usize] & 0x80 != 0;
                let alt = keystate[VK_MENU as usize] & 0x80 != 0;
                let keyup = (lparam & (1 << 31)) != 0;

                if vk == VK_SHIFT {
                        // Clean Shift tap toggles the keyboard open state
                        // (中/英) — same as the TSF path's Shift handling.
                        if keyup {
                                if !self.shift_used {
                                        let open = self.is_open();
                                        unsafe {
                                                let _ = ImmSetOpenStatus(self.imc(), !open);
                                        }
                                        ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_SETOPENSTATUS as usize, 0);
                                        if open && self.composing {
                                                self.cancel();
                                        }
                                }
                                self.shift_used = false;
                        }
                        return false;
                }
                if shift && vk != VK_SHIFT {
                        self.shift_used = true;
                }
                if ctrl || alt {
                        return false;
                }
                if !self.is_open() {
                        return false;
                }
                if keyup {
                        return false;
                }

                match self.mode {
                        Mode::Punct => return self.process_punct_key(vk, shift),
                        Mode::Jyutping => {}
                }

                // Letters / apostrophe / grave — input keys.
                if let Some(key) = VirtualInputKey::for_key_code(vk) {
                        if key.is_number() {
                                return self.process_digit(key.digit() as usize, shift);
                        }
                        return self.process_input_key(key);
                }

                match vk {
                        VK_SPACE => {
                                if shift {
                                        // Shift+Space = literal space.
                                        if self.composing {
                                                let raw = text_from_keys(&self.input_keys);
                                                self.commit(&raw);
                                        }
                                        let full = self.settings.character_form == CharacterForm::FullWidth;
                                        self.commit(if full { "\u{3000}" } else { " " });
                                        true
                                } else if self.composing {
                                        self.commit_selection();
                                        true
                                } else {
                                        false
                                }
                        }
                        VK_RETURN => {
                                if self.composing {
                                        self.commit_raw();
                                        true
                                } else {
                                        false
                                }
                        }
                        VK_ESCAPE => {
                                if self.composing {
                                        self.cancel();
                                        true
                                } else {
                                        false
                                }
                        }
                        VK_BACK => {
                                if self.composing {
                                        self.backspace();
                                        true
                                } else {
                                        false
                                }
                        }
                        VK_LEFT | VK_UP => self.move_selection(-1),
                        VK_RIGHT | VK_DOWN => self.move_selection(1),
                        VK_PRIOR => self.move_page(-1),
                        VK_NEXT => self.move_page(1),
                        _ => self.process_punctuation(vk, shift),
                }
        }

        fn process_digit(&mut self, digit: usize, shift: bool) -> bool {
                if self.composing && !self.items.is_empty() {
                        // 1-9 select, 0 = tenth item like upstream.
                        let index = if digit == 0 { 9 } else { digit - 1 };
                        if index < self.items.len() {
                                self.selection = index;
                                self.commit_selection();
                        }
                        return true;
                }
                if shift {
                        // Shift+digit — symbol candidates (e.g. ⇧2 → @/＠).
                        if let Some(pk) = punctuation::PunctuationKey::for_virtual_key(0x30 + digit as u32) {
                                if pk.should_handle(true) {
                                        return self.open_punct_list(pk, true);
                                }
                        }
                        return true;
                }
                false
        }

        fn process_input_key(&mut self, key: VirtualInputKey) -> bool {
                if !self.composing {
                        self.start_composition();
                }
                self.input_keys.push(key);
                self.recompute();
                self.sync();
                true
        }

        fn process_punctuation(&mut self, vk: u32, shift: bool) -> bool {
                let Some(key) = punctuation::PunctuationKey::for_virtual_key(vk) else {
                        return false;
                };
                if !key.should_handle(shift) {
                        return false;
                }
                // In-progress composition — commit the selected/raw text
                // first, then the punctuation (matches handle_punctuation_key).
                if self.composing {
                        if self.items.is_empty() {
                                self.commit_raw();
                        } else {
                                self.commit_selection();
                        }
                }
                let cantonese = self.settings.punctuation_form == PunctuationForm::Cantonese;
                let output = if cantonese {
                        key.instant_symbol(shift)
                } else {
                        Some(key.text(shift))
                };
                match output {
                        Some(text) => {
                                self.commit(text);
                                true
                        }
                        None => self.open_punct_list(key, shift),
                }
        }

        fn process_punct_key(&mut self, vk: u32, _shift: bool) -> bool {
                // Symbol candidate list is open.
                if let Some(key) = VirtualInputKey::for_key_code(vk) {
                        if key.is_number() {
                                let digit = key.digit() as usize;
                                if let Some(q) = &mut self.slash_query {
                                        if q.is_empty() {
                                                q.push(char::from_digit(digit as u32, 10).unwrap_or('0'));
                                                self.recompute();
                                                self.sync();
                                                return true;
                                        }
                                }
                                let index = if digit == 0 { 9 } else { digit - 1 };
                                if index < self.items.len() {
                                        self.selection = index;
                                        self.commit_selection();
                                }
                                return true;
                        }
                        // Letter — extend slash query if active, else treat as
                        // new jyutping input (commit selection first).
                        if self.slash_query.is_some() {
                                if let Some(q) = &mut self.slash_query {
                                        q.push(key.character);
                                }
                                self.recompute();
                                self.sync();
                                return true;
                        }
                        self.commit_selection();
                        self.mode = Mode::Jyutping;
                        return self.process_input_key(key);
                }
                match vk {
                        VK_SPACE | VK_RETURN => {
                                if self.items.is_empty() {
                                        self.cancel();
                                } else {
                                        self.commit_selection();
                                }
                                true
                        }
                        VK_ESCAPE => {
                                self.cancel();
                                true
                        }
                        VK_BACK => {
                                if let Some(q) = &mut self.slash_query {
                                        if q.pop().is_none() {
                                                self.cancel();
                                        } else {
                                                self.recompute();
                                                self.sync();
                                        }
                                } else {
                                        self.cancel();
                                }
                                true
                        }
                        VK_LEFT | VK_UP => self.move_selection(-1),
                        VK_RIGHT | VK_DOWN => self.move_selection(1),
                        VK_PRIOR => self.move_page(-1),
                        VK_NEXT => self.move_page(1),
                        _ => true,
                }
        }

        fn backspace(&mut self) {
                self.input_keys.pop();
                if self.input_keys.is_empty() {
                        self.cancel();
                        return;
                }
                self.recompute();
                self.sync();
        }

        fn move_selection(&mut self, offset: i32) -> bool {
                if !self.composing || self.items.is_empty() {
                        return false;
                }
                let next = self.selection as i64 + offset as i64;
                if next < 0 || next >= self.items.len() as i64 {
                        return true; // eat — keep the key out of the app
                }
                self.selection = next as usize;
                self.sync();
                true
        }

        fn move_page(&mut self, offset: i32) -> bool {
                if !self.composing || self.items.is_empty() {
                        return false;
                }
                let page_size = self.page_size();
                let next = self.selection as i64 + offset as i64 * page_size as i64;
                let clamped = next.clamp(0, self.items.len() as i64 - 1);
                self.selection = clamped as usize;
                self.sync();
                true
        }

        fn cancel(&mut self) {
                self.clear();
                if self.composing {
                        self.end_composition();
                }
        }
}

// ---------------------------------------------------------------------------
// Session registry — keyed by HIMC (like weasel's HIMCMap).
// ---------------------------------------------------------------------------

fn with_session<R>(himc: HIMC, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
        let mut map = sessions().lock().unwrap_or_else(|e| e.into_inner());
        let session = map.entry(himc.0 as usize).or_insert_with(|| Session::new(himc));
        Some(f(session))
}

pub fn on_select(himc: HIMC, selected: bool) {
        if selected {
                unsafe {
                        let _ = ImmSetOpenStatus(himc, true);
                }
                ctx::init_compstr(himc);
                with_session(himc, |s| {
                        s.settings = config::load();
                });
                // Engine warms lazily on first real key — doing sqlite +
                // segmenter init inside ImeSelect stalls the host UI
                // thread (and, when the IME is the session default, every
                // process pays it at once).
                std::thread::spawn(|| {
                        let _ = engine();
                        let _ = memory();
                });
        } else {
                with_session(himc, |s| s.cancel());
                if let Ok(mut map) = sessions().lock() {
                        map.remove(&(himc.0 as usize));
                }
                unsafe {
                        let _ = ImmSetOpenStatus(himc, false);
                }
                if let Some(imc) = ctx::lock_imc(himc) {
                        if !imc.hCompStr.0.is_null() {
                                let h = imc.hCompStr;
                                imc.hCompStr = HIMCC::default();
                                ctx::unlock_imc(himc);
                                let _ = unsafe { ImmDestroyIMCC(h) };
                        } else {
                                ctx::unlock_imc(himc);
                        }
                }
        }
}

/// NotifyIME — apps adjust caret/composition position and open state here.
pub fn on_notify(himc: HIMC, action: u32) {
        if action == IMN_SETCOMPOSITIONWINDOW || action == IMN_SETCANDIDATEPOS {
                with_session(himc, |s| s.position_window());
        } else if action == IMN_SETOPENSTATUS {
                // App closed the IME (e.g. game closed its chat box) —
                // drop any pending composition the weasel way.
                if let Some(imc) = ctx::lock_imc(himc) {
                        let open = imc.fOpen;
                        ctx::unlock_imc(himc);
                        if !open.as_bool() {
                                with_session(himc, |s| s.cancel());
                        }
                }
        }
        // IMN_SETOPENSTATUS: app toggled fOpen — re-read in process_key.
}

pub fn process_key(himc: HIMC, vk: u32, lparam: isize, keystate: &[u8; 256]) -> bool {
        let accepted = with_session(himc, |s| s.process_key(vk, lparam, keystate)).unwrap_or(false);
        if accepted {
                globals::log(&format!("ime: consumed vk={vk:#x}"));
        }
        accepted
}

pub fn on_focus(himc: HIMC, focus: bool) {
        if focus {
                // Give the composition window a sane position immediately —
                // some apps never send IMN_SETCOMPOSITIONWINDOW.
                with_session(himc, |s| {
                        s.position_window();
                });
        }
}
