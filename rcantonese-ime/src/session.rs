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
use crate::{candview, config, punctuation, tsfbridge, ui};

const VK_BACK: u32 = 0x08;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
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
        /// Divider row (options menu) — never numbered or selectable.
        pub separator: bool,
}

impl candview::RowLike for CandItem {
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

#[derive(Clone, Copy, PartialEq)]
enum Mode {
        /// Normal jyutping input — input_keys drive the engine.
        Jyutping,
        /// Punctuation/symbol list — candidates are symbols, committed
        /// verbatim (no memory learning).
        Punct,
        /// Ctrl+` options list — candidate window shows settings rows.
        Options,
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
        /// Shift currently held — tracked by vk event order, not the
        /// keystate array (apps may pass states missing the shift bit).
        shift_down: bool,
        /// Any non-Shift key event arrived while Shift was held —
        /// suppresses the clean-tap open-state toggle on Shift release.
        shift_tainted: bool,
        composing: bool,
        /// ImmSetOpenStatus must run outside the session lock (imm32
        /// synchronously notifies the app window AND our UI window, whose
        /// handler re-enters with_session). Shift-tap stashes the desired
        /// state here; the outer process_key applies it after unlocking.
        pending_open: Option<bool>,
        /// The host draws its own IME UI (game reading hCandInfo /
        /// NI_SETCANDIDATE_* traffic) — keep feeding notifications but
        /// never paint our window on top.
        app_managed_ui: bool,
        /// Set at composition start; consumed by the post-deliver
        /// position_window(true) pass in process_key. The candidate
        /// anchor is resolved ONCE per composition/focus change —
        /// re-querying on every keystroke made the window flash-jump
        /// (coarse fallback first, real caret after the app reacted).
        needs_anchor: bool,
        /// The character before the caret is an ASCII digit — powers the
        /// shared number-input rule ("3." stays "."). Tracked from our own
        /// commits and from pass-through keys (see note_passed_key).
        prev_digit: bool,
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

/// Non-blocking accessors — get_or_init on the UI thread stalls the host
/// while the background warm-up is still opening sqlite/segmenter (or, in
/// a busy session, waiting on memory.sqlite3's cross-process lock). Key
/// handling takes whatever is already initialized and degrades to empty
/// candidates instead of freezing the app.
fn engine() -> Option<&'static CoreImeEngine> {
        ENGINE.get().and_then(|o| o.as_ref())
}

fn memory() -> Option<&'static Mutex<InputMemory>> {
        MEMORY.get()
}

pub fn warm_engine() {
        let _ = ENGINE.get_or_init(|| database_path().and_then(|p| CoreImeEngine::prepare_path(&p)));
        let _ = MEMORY.get_or_init(|| {
                let mut m = InputMemory::new();
                m.prepare();
                Mutex::new(m)
        });
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
                        shift_down: false,
                        shift_tainted: false,
                        composing: false,
                        pending_open: None,
                        app_managed_ui: false,
                        needs_anchor: false,
                        prev_digit: false,
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
                // The anchor is NOT resolved here — the app only creates its
                // caret/updates forms while handling the messages queued
                // below, and the TSF bridge can't run under the session
                // lock. The post-deliver pass in process_key resolves it.
                self.needs_anchor = true;
                ctx::push_message(self.imc(), WM_IME_STARTCOMPOSITION, 0, 0);
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_CHANGECANDIDATE as usize, 0);
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_OPENCANDIDATE as usize, 0);
        }

        fn end_composition(&mut self) {
                self.composing = false;
                // Do NOT clear hCompStr here — TRANSMSGs are delivered after
                // the session lock drops, and the app reads GCS_RESULTSTR while
                // handling WM_IME_COMPOSITION. Wiping now makes every commit
                // arrive empty (Chromium inserted nothing). The next
                // composition overwrites the buffer anyway.
                ctx::push_message(self.imc(), WM_IME_ENDCOMPOSITION, 0, 0);
                ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_CLOSECANDIDATE as usize, 0);
                ui::hide_candidates();
        }

        /// Commit `text` into the target app and clear the session.
        fn commit(&mut self, text: &str) {
                let t0 = std::time::Instant::now();
                self.prev_digit = text.chars().last().is_some_and(|c| c.is_ascii_digit());
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
                globals::log(&format!(
                        "ime: commit '{}' in {:?} pid={}",
                        text,
                        t0.elapsed(),
                        std::process::id()
                ));
        }

        fn clear(&mut self) {
                self.input_keys.clear();
                self.candidates.clear();
                self.items.clear();
                self.selection = 0;
                self.slash_query = None;
                self.mode = Mode::Jyutping;
        }

        /// Letters shown on the candidate window's preedit row — the raw
        /// jyutping input, or "/query" while the slash symbol list is active.
        fn preedit_text(&self) -> String {
                if self.mode == Mode::Punct {
                        self.slash_query
                                .as_ref()
                                .map(|q| format!("/{q}"))
                                .unwrap_or_default()
                } else {
                        text_from_keys(&self.input_keys)
                }
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
                // Even with no candidates the preedit row must stay visible —
                // SPECIAL_UI apps never render GCS_COMPSTR, so our window is
                // the only place the typed letters (and reverse-lookup
                // prefixes like x/v/q/`) appear.
                let preedit = self.preedit_text();
                if self.app_managed_ui || (self.items.is_empty() && preedit.is_empty()) {
                        ui::hide_candidates();
                } else if !self.needs_anchor {
                        // Anchor already resolved — the window stays put for
                        // the rest of this composition. While needs_anchor is
                        // pending the first paint is deferred to the
                        // post-deliver pass so it opens directly at the real
                        // caret instead of flashing at a stale position.
                        let ps = self.page_size();
                        ui::show_candidates(&self.items, self.selection, &self.settings, &preedit, ps);
                }
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
                                separator: false,
                        })
                        .collect();
                self.selection = 0;
                globals::log(&format!(
                        "ime: input={} items={} first={} pid={}",
                        text_from_keys(&self.input_keys),
                        self.items.len(),
                        self.items.first().map(|c| c.text.as_str()).unwrap_or(""),
                        std::process::id()
                ));
        }

        fn compute_suggestions(&mut self) -> Vec<Candidate> {
                let Some(engine) = engine() else { return Vec::new() };
                let standard = standard_for_variant(self.settings.character_variant);
                let method = reverse_lookup_method(&self.input_keys);
                if method == ReverseLookupMethod::None {
                        let memory_suggestions = {
                                let seg = engine.segment(&self.input_keys);
                                memory()
                                        .and_then(|m| m.lock().ok().map(|g| g.suggest(&self.input_keys, &seg, &engine.segmenter)))
                                        .unwrap_or_default()
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
                                        separator: false,
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
                                                separator: false,
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
                                separator: false,
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
                if self.mode == Mode::Options {
                        self.apply_options_row(index);
                        return;
                }
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
                                if let Some(mut m) = memory().and_then(|m| m.lock().ok()) {
                                        m.handle(&joined);
                                }
                        }
                }
                // Incremental finalize (upstream's _HandleIncrementalCandidateFinalize):
                // a candidate that consumes only part of the input commits its
                // text, and the tail keys keep composing — "faandous" → pick
                // 反倒 (faandou) leaves 's' running for the next candidate.
                let consumed = cand.lexicon.input_count.min(self.input_keys.len());
                let mut tail: Vec<VirtualInputKey> = self.input_keys[consumed..].to_vec();
                while tail.first().is_some_and(|k| k.is_apostrophe()) {
                        tail.remove(0);
                }
                // Reverse-lookup candidates consume keys after the leading '
                // — upstream re-anchors the tail with it so the lookup
                // method persists into the next segment.
                if consumed > 0
                        && reverse_lookup_method(&self.input_keys) != ReverseLookupMethod::None
                        && !tail.is_empty()
                {
                        tail.insert(0, self.input_keys[0]);
                }
                if consumed > 0 && !tail.is_empty() {
                        self.commit(&cand.text);
                        self.input_keys = tail;
                        self.start_composition();
                        self.recompute();
                        self.sync();
                        return;
                }
                self.commit(&cand.text);
        }

        fn commit_index(&mut self, index: usize) {
                if index < self.items.len() {
                        self.selection = index;
                }
                self.commit_selection();
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

                // Caps Lock = English mode, same as the TSF path: a live
                // composition is finalized as raw text and every key passes
                // through untouched. The event's own keystate snapshot carries
                // the toggle bit — same view the app has (and testable).
                if keystate[VK_CAPITAL.0 as usize] & 1 != 0 {
                        if !keyup && self.composing {
                                self.commit_raw();
                        }
                        return false;
                }

                let is_shift_vk = vk == VK_SHIFT || vk == VK_LSHIFT || vk == VK_RSHIFT;
                if is_shift_vk {
                        // Clean Shift tap toggles the keyboard open state
                        // (中/英) — same as the TSF path's Shift handling.
                        if keyup {
                                if self.shift_down && !self.shift_tainted {
                                        // ImmSetOpenStatus SendMessages
                                        // IMN_SETOPENSTATUS to the app window
                                        // AND our IME UI window — both
                                        // re-enter with_session → defer until
                                        // the lock is released.
                                        let open = self.is_open();
                                        self.pending_open = Some(!open);
                                        ctx::push_message(self.imc(), WM_IME_NOTIFY, IMN_SETOPENSTATUS as usize, 0);
                                        if open && self.composing {
                                                self.cancel();
                                        }
                                }
                                self.shift_down = false;
                                self.shift_tainted = false;
                        } else if !self.shift_down {
                                self.shift_down = true;
                                self.shift_tainted = false;
                        }
                        return false;
                }
                if self.shift_down {
                        // Any other key event while Shift is held —
                        // consumed or not — makes the release a non-tap.
                        self.shift_tainted = true;
                }
                // Modifier flags in TF_MOD_* terms — globals::update_modifiers
                // only runs on the TSF path; here we derive them from the
                // keystate array so check_modifiers sees the same values.
                let mut modifiers = 0u32;
                if shift {
                        modifiers |= globals::TF_MOD_SHIFT;
                }
                if ctrl {
                        modifiers |= globals::TF_MOD_CONTROL;
                }
                if alt {
                        modifiers |= globals::TF_MOD_ALT;
                }
                // Options-menu hotkey — settings.options_menu_keys
                // (default Ctrl+`), same trigger as the TSF path's
                // IsOptionsShortcut. Reachable in ABC mode too.
                if !keyup && self.is_options_hotkey(vk, modifiers) {
                        self.toggle_options();
                        return true;
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
                        Mode::Options => return self.process_options_key(vk, modifiers),
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

        /// A key we declined gets typed/handled by the app itself — keep
        /// `prev_digit` in sync: digits set it, keys that produce other text
        /// or move the caret clear it, pure modifiers leave it alone.
        fn note_passed_key(&mut self, vk: u32, shift: bool, ctrl: bool, alt: bool) {
                const MODIFIER_VKS: &[u32] = &[
                        VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, 0xA2, 0xA3, // L/RCONTROL
                        VK_MENU, 0xA4, 0xA5,                                    // L/RMENU
                        0x5B, 0x5C,                                             // L/RWIN
                        VK_CAPITAL.0 as u32, 0x90, 0x91,                        // caps/num/scroll lock
                ];
                if (0x30..=0x39).contains(&vk) || (0x60..=0x69).contains(&vk) {
                        self.prev_digit = !shift && !ctrl && !alt;
                } else if !MODIFIER_VKS.contains(&vk) {
                        self.prev_digit = false;
                }
        }

        fn process_digit(&mut self, digit: usize, shift: bool) -> bool {
                if shift {
                        // Shift+digit is a punctuation key, not a candidate
                        // selection — same precedence as the TSF path
                        // (should_handle_punctuation beats SelectByNumber).
                        // Map numpad vks onto the top-row digit so the same
                        // punctuation table applies.
                        return self.process_punctuation(0x30 + digit as u32, true);
                }
                if self.composing && !self.items.is_empty() {
                        // Page-relative select — upstream's
                        // _SetSelectionInPage: the digit addresses the row
                        // within the CURRENT page, via the same
                        // candidate-index ordering as the TSF path.
                        let range = candview::candidate_index_range(self.settings.candidate_page_size);
                        if let Some(pos) = range.iter().position(|&d| d as usize == digit) {
                                let (start, _) = candview::page_bounds(self.items.len(), self.selection, self.page_size());
                                let index = start + pos;
                                if index < self.items.len() {
                                        self.selection = index;
                                        self.commit_selection();
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
                // Shared decision (instant vs list vs digit-period) lives in
                // punctuation::decide — identical rule as the TSF path.
                let cantonese = self.settings.punctuation_form == PunctuationForm::Cantonese;
                let action = punctuation::decide(vk, shift, cantonese, self.prev_digit);
                if matches!(action, punctuation::PunctAction::Pass) {
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
                match action {
                        punctuation::PunctAction::Commit(text) => {
                                self.commit(text);
                                true
                        }
                        punctuation::PunctAction::OpenList(key) => self.open_punct_list(key, shift),
                        punctuation::PunctAction::Pass => false,
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

        // -- options list (Ctrl+`) ------------------------------------------
        // Same rows as the TSF options presenter: 3 character variants, a
        // separator, 2 character forms, a separator, 2 punctuation forms.

        fn toggle_options(&mut self) {
                if self.mode == Mode::Options {
                        self.close_options();
                        return;
                }
                self.mode = Mode::Options;
                self.items = self.options_items();
                self.selection = 0;
                // Same one-shot anchoring as compositions — defer the first
                // paint to post-deliver resolve_anchor (allow_tsf=true) so
                // the menu opens at the real caret, not a stale/form pos.
                self.needs_anchor = true;
        }

        fn close_options(&mut self) {
                self.mode = Mode::Jyutping;
                if self.composing && !self.input_keys.is_empty() {
                        self.recompute();
                        self.sync();
                } else {
                        // Clear the rows — NI_OPENCANDIDATE re-shows whatever
                        // `items` still holds, so leaving the menu rows here
                        // resurrects the options list.
                        self.items.clear();
                        self.selection = 0;
                        ui::hide_candidates();
                }
        }

        /// Options rows — the shared candview model (same as the TSF
        /// path's _BuildOptionsRows port).
        fn options_items(&self) -> Vec<CandItem> {
                candview::options_rows(&self.settings)
                        .into_iter()
                        .map(|r| CandItem { text: r.text, comment: r.comment, separator: r.separator })
                        .collect()
        }

        /// Selectable position (separators don't count) → setting, then
        /// close — same order as the TSF path's _ApplyOptionsSelection.
        fn apply_options_pos(&mut self, pos: usize) {
                match candview::options_choice(pos) {
                        Some(candview::OptionsChoice::Variant(v)) => self.settings.character_variant = v,
                        Some(candview::OptionsChoice::Form(f)) => self.settings.character_form = f,
                        Some(candview::OptionsChoice::Punct(p)) => self.settings.punctuation_form = p,
                        None => {}
                }
                let _ = config::save(&self.settings);
                self.close_options();
        }

        /// Flat row index (what NI_SELECTCANDIDATESTR reports) → apply.
        fn apply_options_row(&mut self, index: usize) {
                if index >= self.items.len() || self.items[index].separator {
                        return;
                }
                let pos = self.items[..index].iter().filter(|i| !i.separator).count();
                self.apply_options_pos(pos);
        }

        fn move_options(&mut self, offset: i32) -> bool {
                let len = self.items.len() as i32;
                let mut i = self.selection as i32;
                loop {
                        let next = i + offset;
                        if next < 0 || next >= len {
                                break;
                        }
                        i = next;
                        if !self.items[i as usize].separator {
                                break;
                        }
                }
                self.selection = i as usize;
                let preedit = self.preedit_text();
                let ps = self.items.len();
                ui::show_candidates(&self.items, self.selection, &self.settings, &preedit, ps);
                true
        }

        /// Ctrl+`-style trigger — honors settings.options_menu_keys, the
        /// same list the TSF path's IsOptionsShortcut reads.
        fn is_options_hotkey(&self, vk: u32, modifiers: u32) -> bool {
                self.settings
                        .options_menu_keys
                        .iter()
                        .any(|&(k, m)| k == vk && globals::check_modifiers(modifiers, m))
        }

        /// Options-mode keys — same contract as the TSF path: unmodified
        /// digits pick a row (apply + close), Up/Down move the highlight,
        /// every other key just closes the menu.
        fn process_options_key(&mut self, vk: u32, modifiers: u32) -> bool {
                if vk == VK_UP {
                        return self.move_options(-1);
                }
                if vk == VK_DOWN {
                        return self.move_options(1);
                }
                let range = candview::candidate_index_range(self.settings.candidate_page_size);
                if let Some(pos) = candview::options_digit(vk, modifiers, &range) {
                        self.apply_options_pos(pos);
                        return true;
                }
                // A digit key that doesn't map to a position is still eaten
                // (upstream's SelectByNumber dispatch does the same).
                if matches!(vk, 0x30..=0x39 | 0x60..=0x69) {
                        return true;
                }
                self.close_options();
                true
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

/// Last-resort anchor: inside the app's client area (screen coords). Apps
/// with no caret, no forms and no TSF document (self-drawn game fields) still
/// get a window-anchored box — the mouse cursor is never used.
fn window_anchor(hwnd: windows::Win32::Foundation::HWND) -> Option<windows::Win32::Foundation::POINT> {
        unsafe {
                let mut rc = windows::Win32::Foundation::RECT::default();
                if windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc).is_err()
                        || rc.right <= rc.left
                {
                        return None;
                }
                let mut p = windows::Win32::Foundation::POINT {
                        x: rc.left + (rc.right - rc.left) / 4,
                        y: rc.top + (rc.bottom - rc.top) / 3,
                };
                if !windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p).as_bool() {
                        return None;
                }
                Some(p)
        }
}

/// Last candidate-window position per app window — the fallback for apps
/// that create their caret only after WM_IME_STARTCOMPOSITION is delivered
/// (first key of a composition sees no caret at all). Stored window-relative
/// so dragging the host window keeps the anchor attached to the field.
static LAST_POS: Mutex<Option<(usize, i32, i32, i32, i32)>> = Mutex::new(None);

fn client_origin(hwnd: windows::Win32::Foundation::HWND) -> Option<(i32, i32)> {
        let mut p = windows::Win32::Foundation::POINT::default();
        unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p) }
                .as_bool()
                .then_some((p.x, p.y))
}

fn last_pos(hwnd: windows::Win32::Foundation::HWND) -> Option<(windows::Win32::Foundation::POINT, &'static str)> {
        let (h, ox, oy, x, y) = LAST_POS.lock().ok().and_then(|g| *g)?;
        if h != hwnd.0 as usize {
                return None;
        }
        let (nox, noy) = client_origin(hwnd)?;
        Some((
                windows::Win32::Foundation::POINT { x: x + (nox - ox), y: y + (noy - oy) },
                "last",
        ))
}

/// Bottom-left of the FOCUSED control — Qt apps (QQ, Telegram) expose no
/// caret and no TSF doc, but the edit box is the focus window, so its
/// bottom edge is near the caret.
fn focus_rect_position(hwnd: windows::Win32::Foundation::HWND) -> Option<windows::Win32::Foundation::POINT> {
        let tid = unsafe { GetWindowThreadProcessId(hwnd, None) };
        let mut gui = GUITHREADINFO {
                cbSize: size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
        };
        if unsafe { GetGUIThreadInfo(tid, &mut gui) }.is_err() {
                return None;
        }
        let focus = gui.hwndFocus;
        if focus.0.is_null() {
                return None;
        }
        let mut rc = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(focus, &mut rc) }.is_err() || rc.bottom <= rc.top || rc.right <= rc.left {
                return None;
        }
        Some(windows::Win32::Foundation::POINT { x: rc.left, y: rc.bottom })
}

fn remember_pos(hwnd: windows::Win32::Foundation::HWND, x: i32, y: i32) {
        let Some((ox, oy)) = client_origin(hwnd) else { return };
        if let Ok(mut g) = LAST_POS.lock() {
                *g = Some((hwnd.0 as usize, ox, oy, x, y));
        }
}

/// Anchor the candidate window to the text field. Priority:
///   1. cfCandForm[0]/cfCompForm — the app told us where IME UI belongs.
///   2. The owning thread's real caret (GetGUIThreadInfo) — cheap and
///      safe; Qt/Chromium create a caret when their field is focused.
///   3. TSF caret bridge — only when no caret exists AND `allow_tsf`
///      (post-lock calls only — RequestEditSession re-enters the app text
///      store synchronously and must never run under SESSIONS).
///   4. Last position used for the same window — Qt/Chromium only create
///      their caret after handling WM_IME_STARTCOMPOSITION, so the first
///      key of a composition can see no caret at all.
///   5. Inside the app's client area — never the mouse cursor.
fn position_window(himc: HIMC, allow_tsf: bool) {
        let Some(imc) = ctx::lock_imc(himc) else { return };
        let hwnd = imc.hWnd;
        let cand_form = imc.cfCandForm[0];
        let comp_style = imc.cfCompForm.dwStyle;
        let comp_pt = imc.cfCompForm.ptCurrentPos;
        let font_h = unsafe { imc.lfFont.W.lfHeight };
        ctx::unlock_imc(himc);
        if hwnd.0.is_null() {
                return;
        }
        let client = match cand_form.dwStyle {
                CFS_POINT | CFS_RECT | CFS_FORCE_POSITION | CFS_CANDIDATEPOS | CFS_EXCLUDE => {
                        Some(cand_form.ptCurrentPos)
                }
                _ => match comp_style {
                        CFS_CANDIDATEPOS => Some(cand_form.ptCurrentPos),
                        CFS_POINT | CFS_RECT | CFS_FORCE_POSITION | CFS_EXCLUDE => Some(comp_pt),
                        _ => None,
                },
        };
        // Fallback chain (imperative so the tsfbridge query — which must run
        // outside the session lock — runs at most once and can be ranked):
        //   form > caret > tsf(selection|docend) > focus > tsf(view) > last > anchor.
        // Selection extents and the doc-end anchor are real caret positions
        // (GetTop gives the FOCUSED context — the input doc, not the
        // read-only chat history); only the view rect is a coarse guess.
        let mut screen = client
                .and_then(|mut p| {
                        unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p) }
                                .as_bool()
                                .then_some(p)
                })
                .map(|p| (p, "form"));
        if screen.is_none() {
                screen = caret_position(hwnd).map(|p| (p, "caret"));
        }
        let mut tsf_low: Option<(windows::Win32::Foundation::POINT, &'static str)> = None;
        if screen.is_none() && allow_tsf {
                match tsfbridge::caret_rect() {
                        Some((rc, tsfbridge::RectKind::View)) => {
                                tsf_low = Some((
                                        windows::Win32::Foundation::POINT { x: rc.left, y: rc.bottom },
                                        "tsf-low",
                                ));
                        }
                        Some((rc, _)) => {
                                screen = Some((
                                        windows::Win32::Foundation::POINT { x: rc.left, y: rc.bottom },
                                        "tsf",
                                ));
                        }
                        None => {}
                }
        }
        if screen.is_none() {
                screen = focus_rect_position(hwnd).map(|p| (p, "focus"));
        }
        if screen.is_none() {
                screen = tsf_low;
        }
        if screen.is_none() {
                screen = last_pos(hwnd);
        }
        if screen.is_none() {
                screen = window_anchor(hwnd).map(|p| (p, "anchor"));
        }
        let Some((s, src)) = screen else { return };
        globals::log(&format!("ime: pos ({},{}) via {} pid={}", s.x, s.y, src, std::process::id()));
        remember_pos(hwnd, s.x, s.y);
        ui::set_position(s.x, s.y + font_h.abs() + 2);
}

/// Caret of the IMC window's owning thread in screen coords (our own
/// GetCaretPos only sees carets WE created). Chromium creates a real 1x1
/// system caret for Chinese IMEs when the renderer reports bounds.
/// None when no caret exists.
fn caret_position(hwnd: windows::Win32::Foundation::HWND) -> Option<windows::Win32::Foundation::POINT> {
        use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
        unsafe {
                let tid = windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, None);
                let mut info = GUITHREADINFO {
                        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                        ..Default::default()
                };
                if !GetGUIThreadInfo(tid, &mut info).is_ok()
                        || info.rcCaret.right <= info.rcCaret.left
                {
                        globals::log(&format!(
                                "ime: caret miss tid={tid} hwndCaret={:?} rcCaret=({},{},{},{}) hwndFocus={:?} pid={}",
                                info.hwndCaret, info.rcCaret.left, info.rcCaret.top,
                                info.rcCaret.right, info.rcCaret.bottom, info.hwndFocus,
                                std::process::id()
                        ));
                        return None;
                }
                let mut p = windows::Win32::Foundation::POINT {
                        x: info.rcCaret.left,
                        y: info.rcCaret.bottom,
                };
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(info.hwndCaret, &mut p);
                Some(p)
        }
}

fn with_session<R>(himc: HIMC, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
        let mut map = sessions().lock().unwrap_or_else(|e| e.into_inner());
        let session = map.entry(himc.0 as usize).or_insert_with(|| Session::new(himc));
        Some(f(session))
}

pub fn on_select(himc: HIMC, selected: bool) {
        if selected {
                ctx::set_open_state(himc, true);
                ctx::init_compstr(himc);
                with_session(himc, |s| {
                        s.settings = config::load();
                });
                // Engine warms lazily on first real key — doing sqlite +
                // segmenter init inside ImeSelect stalls the host UI
                // thread (and, when the IME is the session default, every
                // process pays it at once).
                std::thread::spawn(warm_engine);
        } else {
                with_session(himc, |s| s.cancel());
                ctx::deliver(himc);
                if let Ok(mut map) = sessions().lock() {
                        map.remove(&(himc.0 as usize));
                }
                ctx::set_open_state(himc, false);
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

/// NotifyIME — apps adjust caret/composition position, page the candidate
/// list, and toggle open state here. NI_* candidate-driving actions mean
/// the host renders its own IME UI — we flip `app_managed_ui` so our
/// window never paints on top.
pub fn on_notify(himc: HIMC, action: u32, index: u32, value: u32) {
        globals::log(&format!("ime: notify action={action:#x} index={index} value={value}"));
        match action {
                a if a == IMN_SETCOMPOSITIONWINDOW || a == IMN_SETCANDIDATEPOS => {
                        with_session(himc, |s| position_window(s.imc(), false));
                }
                a if a == IMN_SETOPENSTATUS => {
                        // App toggled fOpen (e.g. game closed its chat box) —
                        // mirror it into fdwConversion and drop any pending
                        // composition the weasel way.
                        if let Some(imc) = ctx::lock_imc(himc) {
                                let open = imc.fOpen.as_bool();
                                imc.fdwConversion = if open {
                                        (IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE | IME_CMODE_SYMBOL).0
                                } else {
                                        0
                                };
                                ctx::unlock_imc(himc);
                                if !open {
                                        with_session(himc, |s| s.cancel());
                                }
                        }
                }
                a if a == NI_SELECTCANDIDATESTR.0 => {
                        // App picked a candidate by index from its own UI.
                        with_session(himc, |s| {
                                s.app_managed_ui = true;
                                ui::hide_candidates();
                                s.commit_index(index as usize);
                        });
                }
                a if a == NI_COMPOSITIONSTR.0 => {
                        // Lifecycle control, NOT "app renders candidates" —
                        // Qt sends CPS_CANCEL after every composition end.
                        // Treating this as app-managed UI left the flag set
                        // forever and our window never showed again.
                        with_session(himc, |s| match index {
                                x if x == CPS_CANCEL.0 => s.cancel(),
                                x if x == CPS_COMPLETE.0 && s.composing => s.commit_selection(),
                                _ => {}
                        });
                }
                a if a == NI_SETCANDIDATE_PAGESTART.0 => {
                        // App asked to page to `index` — apply, but keep
                        // drawing our own UI (Qt sets page size/start even
                        // though it doesn't render candidates itself).
                        with_session(himc, |s| {
                                if !s.items.is_empty() {
                                        let page_size = s.page_size();
                                        let page = (index as usize / page_size.max(1)) * page_size.max(1);
                                        s.selection = page.min(s.items.len().saturating_sub(1));
                                        s.sync();
                                }
                        });
                }
                a if a == NI_OPENCANDIDATE.0 => {
                        // App requests the candidate window open — show ours.
                        with_session(himc, |s| {
                                if !s.items.is_empty() && !s.app_managed_ui && !s.needs_anchor {
                                        position_window(s.imc(), false);
                                        let preedit = s.preedit_text();
                                        let ps = s.page_size();
                                        ui::show_candidates(&s.items, s.selection, &s.settings, &preedit, ps);
                                }
                        });
                }
                a if a == NI_CLOSECANDIDATE.0 => {
                        ui::hide_candidates();
                }
                _ => {}
        }
        // Deliver whatever the handlers queued — SendMessage must not run
        // while a with_session lock is held (app wndprocs re-enter here).
        ctx::deliver(himc);
        // Handlers can also open a composition (e.g. NI_SELECTCANDIDATESTR
        // partial-commit tails) — resolve their anchor post-deliver too.
        resolve_anchor(himc);
}

/// Post-deliver anchor resolution — Qt/Chromium create their caret only
/// while handling the WM_IME_STARTCOMPOSITION we just sent, so the real
/// position only exists now. Runs ONCE per composition (needs_anchor is
/// consumed) — the candidate window then stays put for the composition's
/// lifetime. Outside the session lock — tsfbridge's RequestEditSession
/// must never run under SESSIONS (host text store re-enters our exports).
fn resolve_anchor(himc: HIMC) {
        let need_anchor = with_session(himc, |s| s.needs_anchor).unwrap_or(false);
        if !need_anchor {
                return;
        }
        position_window(himc, true);
        with_session(himc, |s| {
                s.needs_anchor = false;
                // First paint was deferred (composition sync, options menu)
                // — show now that the anchor is real.
                let preedit = s.preedit_text();
                if !s.app_managed_ui && (!s.items.is_empty() || !preedit.is_empty()) {
                        let ps = if s.mode == Mode::Options { s.items.len() } else { s.page_size() };
                        ui::show_candidates(&s.items, s.selection, &s.settings, &preedit, ps);
                }
        });
}

pub fn process_key(himc: HIMC, vk: u32, lparam: isize, keystate: &[u8; 256]) -> bool {
        let (accepted, pending_open) = with_session(himc, |s| {
                let a = s.process_key(vk, lparam, keystate);
                (a, s.pending_open.take())
        })
        .unwrap_or((false, None));
        // Apply open-state toggles outside the lock — ImmSetOpenStatus sends
        // WM_IME_NOTIFY synchronously to both the app window and our IME UI
        // window, and both paths re-enter with_session.
        if let Some(open) = pending_open {
                ctx::set_open_state(himc, open);
        }
        // Flush queued messages only after the session lock is released —
        // SendMessage re-entrancy deadlocks it otherwise (Chromium/Electron).
        ctx::deliver(himc);
        // Qt/Chromium create their caret only while handling the
        // WM_IME_STARTCOMPOSITION we just delivered — resolve the anchor
        // once, now that the app has reacted.
        resolve_anchor(himc);
        if !accepted && (lparam & (1 << 31)) == 0 {
                // The app itself will handle this keydown — track digits for
                // the number-context "." rule and drop the flag on keys that
                // produce other text or move the caret.
                let (shift, ctrl, alt) = (
                        keystate[VK_SHIFT as usize] & 0x80 != 0,
                        keystate[VK_CONTROL as usize] & 0x80 != 0,
                        keystate[VK_MENU as usize] & 0x80 != 0,
                );
                with_session(himc, |s| s.note_passed_key(vk, shift, ctrl, alt));
        }
        if accepted {
                globals::log(&format!("ime: consumed vk={vk:#x} pid={}", std::process::id()));
        }
        accepted
}

pub fn on_focus(himc: HIMC, focus: bool) {
        if focus {
                // Give the composition window a sane position immediately —
                // some apps never send IMN_SETCOMPOSITIONWINDOW. Runs outside
                // the session lock — the TSF bridge may query the app.
                position_window(himc, true);
        }
}
