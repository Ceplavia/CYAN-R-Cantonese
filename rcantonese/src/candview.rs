// Shared candidate-window view — both transports render through this so
// rows, fonts, colors, separators, preedit and numbering stay identical.
// The TSF presenter (candidate.rs) and the IMM32 popup (ui.rs) only differ
// in how the window is owned/positioned; the content is one code path.

use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::globals;
use crate::settings::{CharacterForm, ImeSettings, PunctuationForm};
use crate::strings::*;
use crate::variants::CharacterVariant;

/// One rendered row. Implement on each transport's item type — the
/// shared painter only needs these three facts.
pub trait RowLike {
        fn row_text(&self) -> &str;
        fn row_comment(&self) -> &str;
        /// Divider line — never numbered, never selectable (options menu).
        fn row_separator(&self) -> bool {
                self.row_text().is_empty()
        }
}

/// A plain owned row for callers without their own item type.
pub struct Row {
        pub text: String,
        pub comment: String,
        pub separator: bool,
}

impl RowLike for Row {
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

/// Resolved visual parameters — both transports feed this from the same
/// ImeSettings fields.
#[derive(Clone, Copy)]
pub struct Style {
        pub candidate_font_size: u32,
        pub number_font_size: u32,
        pub comment_font_size: u32,
        pub text_color: u32,
        pub back_color: u32,
        pub select_color: u32,
        pub comment_color: u32,
}

impl Style {
        pub fn from_settings(s: &ImeSettings) -> Self {
                Self {
                        candidate_font_size: s.candidate_font_size,
                        number_font_size: s.candidate_number_font_size,
                        comment_font_size: s.candidate_comment_font_size,
                        text_color: s.candidate_text_color,
                        back_color: s.candidate_back_color,
                        select_color: s.candidate_select_color,
                        comment_color: s.candidate_comment_color,
                }
        }
}

// -- geometry ---------------------------------------------------------------

pub fn row_height(style: &Style) -> i32 {
        style.candidate_font_size.max(style.comment_font_size).max(style.number_font_size) as i32 + 10
}

/// The page containing `selection`: uniform pages of `page_size` rows.
pub fn page_bounds(len: usize, selection: usize, page_size: usize) -> (usize, usize) {
        let page_size = page_size.max(1);
        let start = (selection / page_size) * page_size;
        let end = (start + page_size - 1).min(len.saturating_sub(1));
        (start, end)
}

/// Clamp a candidate-window rect inside the nearest monitor's work area.
/// When there's no room below the caret, flip the window above the caret
/// line like other IMEs do.
pub fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32, caret_top: i32) -> (i32, i32) {
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

// -- fonts / text ------------------------------------------------------------

pub fn make_font(size: u32) -> HFONT {
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

/// Measure `text` under `font` — caller manages the DC and restores fonts.
pub unsafe fn text_width(hdc: HDC, font: HFONT, text: &[u16]) -> i32 {
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

// -- measure -----------------------------------------------------------------

/// Height of the window for the current page — preedit row + page rows.
pub fn measure_height(len: usize, selection: usize, page_size: usize, style: &Style, preedit: &str) -> i32 {
        let (_, end) = page_bounds(len, selection, page_size);
        let start = (selection / page_size.max(1)) * page_size.max(1);
        let rows = if len == 0 { 0 } else { (end - start + 1) as i32 };
        let preedit_h = if preedit.is_empty() { 0 } else { row_height(style) };
        preedit_h + rows * row_height(style) + 8
}

/// Width on the window's own DC — its DPI context matches BeginPaint's,
/// which fixed Notepad measuring ~30% too narrow on a screen DC.
pub fn measure_width<R: RowLike>(hwnd: HWND, rows: &[R], selection: usize, page_size: usize, style: &Style, preedit: &str) -> i32 {
        if rows.is_empty() {
                return 120;
        }
        let (start, end) = page_bounds(rows.len(), selection, page_size);
        let end = end.min(rows.len().saturating_sub(1));
        unsafe {
                let hdc = GetDC(Some(hwnd));
                if hdc.is_invalid() {
                        return 200;
                }
                let num_font = make_font(style.number_font_size);
                let cand_font = make_font(style.candidate_font_size);
                let cmt_font = make_font(style.comment_font_size);
                let mut max_cx = 40i32;
                if !preedit.is_empty() {
                        let wide: Vec<u16> = preedit.encode_utf16().collect();
                        max_cx = max_cx.max(text_width(hdc, cand_font, &wide));
                }
                let mut shown = 0usize; // numbering counts only non-separator rows
                for index in start..=end {
                        let Some(item) = rows.get(index) else { break };
                        if item.row_separator() {
                                continue;
                        }
                        shown += 1;
                        let label: Vec<u16> = format!("{}.", shown).encode_utf16().collect();
                        let text: Vec<u16> = format!(" {}", item.row_text()).encode_utf16().collect();
                        let comment: Vec<u16> = format!("  {}", item.row_comment()).encode_utf16().collect();
                        let cx = text_width(hdc, num_font, &label)
                                + text_width(hdc, cand_font, &text)
                                + text_width(hdc, cmt_font, &comment);
                        max_cx = max_cx.max(cx);
                }
                let _ = DeleteObject(num_font.into());
                let _ = DeleteObject(cand_font.into());
                let _ = DeleteObject(cmt_font.into());
                let _ = ReleaseDC(Some(hwnd), hdc);
                (max_cx + 16).max(120)
        }
}

// -- paint -------------------------------------------------------------------

/// Paint the whole window: background, optional preedit line, then the
/// visible page of rows — separators render as a centered divider and are
/// skipped by the numbering, the selection row is highlighted. Grows the
/// window when the real rendered extent exceeds the measured width.
pub fn paint<R: RowLike>(
        hwnd: HWND,
        hdc: HDC,
        rows: &[R],
        selection: usize,
        page_size: usize,
        style: &Style,
        preedit: &str,
) {
        unsafe {
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let brush = CreateSolidBrush(COLORREF(style.back_color));
                FillRect(hdc, &client, brush);
                let _ = DeleteObject(brush.into());
                if rows.is_empty() && preedit.is_empty() {
                        return;
                }
                let rh = row_height(style);
                let (start, end) = page_bounds(rows.len(), selection, page_size);
                let end = end.min(rows.len().saturating_sub(1));
                let num_font = make_font(style.number_font_size);
                let cand_font = make_font(style.candidate_font_size);
                let cmt_font = make_font(style.comment_font_size);
                // Restore the DC's original font before deleting ours —
                // DeleteObject on a still-selected font fails silently.
                let mut orig_font = HGDIOBJ::default();
                let mut max_right = 0i32;
                let mut y_off = 4i32;
                if !preedit.is_empty() {
                        let mut wide: Vec<u16> = preedit.encode_utf16().collect();
                        SetTextColor(hdc, COLORREF(style.text_color));
                        let old = SelectObject(hdc, cand_font.into());
                        if orig_font.is_invalid() {
                                orig_font = old;
                        }
                        let mut rc = RECT { left: 8, top: y_off, right: 4096, bottom: y_off + rh };
                        SetBkMode(hdc, TRANSPARENT);
                        DrawTextW(hdc, &mut wide, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        max_right = max_right.max(8 + text_width(hdc, cand_font, &wide));
                        y_off += rh;
                }
                let mut shown = 0usize; // numbering counts only non-separator rows
                for (i, index) in (start..=end).enumerate() {
                        let Some(item) = rows.get(index) else { break };
                        let y = y_off + i as i32 * rh;
                        let mut rc = RECT { left: 0, top: y, right: 4096, bottom: y + rh };
                        if item.row_separator() {
                                let pen = CreatePen(PS_SOLID, 1, COLORREF(style.comment_color));
                                let old_pen = SelectObject(hdc, pen.into());
                                let mid = y + rh / 2;
                                let _ = MoveToEx(hdc, 8, mid, None);
                                let _ = LineTo(hdc, client.right - 8, mid);
                                SelectObject(hdc, old_pen);
                                let _ = DeleteObject(pen.into());
                                continue;
                        }
                        shown += 1;
                        if index == selection {
                                let brush = CreateSolidBrush(COLORREF(style.select_color));
                                FillRect(hdc, &rc, brush);
                                let _ = DeleteObject(brush.into());
                        }
                        SetBkMode(hdc, TRANSPARENT);
                        let mut x = 8;
                        let mut label: Vec<u16> = format!("{}.", shown).encode_utf16().collect();
                        SetTextColor(hdc, COLORREF(style.text_color));
                        if orig_font.is_invalid() {
                                orig_font = SelectObject(hdc, num_font.into());
                        } else {
                                SelectObject(hdc, num_font.into());
                        }
                        rc.left = x;
                        DrawTextW(hdc, &mut label, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        x += text_width(hdc, num_font, &label);
                        let mut wide: Vec<u16> = format!(" {}", item.row_text()).encode_utf16().collect();
                        SelectObject(hdc, cand_font.into());
                        rc.left = x;
                        DrawTextW(hdc, &mut wide, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                        x += text_width(hdc, cand_font, &wide);
                        if !item.row_comment().is_empty() {
                                let mut comment: Vec<u16> = format!("  {}", item.row_comment()).encode_utf16().collect();
                                SelectObject(hdc, cmt_font.into());
                                rc.left = x;
                                SetTextColor(hdc, COLORREF(style.comment_color));
                                DrawTextW(hdc, &mut comment, &mut rc, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOCLIP);
                                x += text_width(hdc, cmt_font, &comment);
                        }
                        max_right = max_right.max(x);
                }
                if !orig_font.is_invalid() {
                        SelectObject(hdc, orig_font);
                }
                let _ = DeleteObject(num_font.into());
                let _ = DeleteObject(cand_font.into());
                let _ = DeleteObject(cmt_font.into());
                // Self-correct the window width using the real rendered extent.
                let needed = max_right + 8;
                let _ = GetClientRect(hwnd, &mut client);
                if needed > client.right - client.left {
                        let mut wr = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut wr);
                        let (nx, _) = clamp_to_work_area(wr.left, wr.top, needed + 4, wr.bottom - wr.top, wr.top);
                        let _ = MoveWindow(hwnd, nx, wr.top, needed + 4, wr.bottom - wr.top, true);
                }
        }
}

// -- options menu model (Ctrl+`) ---------------------------------------------
// Same rows as upstream _BuildOptionsRows: 3 character variants, a
// separator, 2 character forms, a separator, 2 punctuation forms — the
// current choice carries a ✓ comment.

pub fn options_rows(settings: &ImeSettings) -> Vec<Row> {
        let tick = |on: bool| if on { "\u{2713}".to_string() } else { String::new() };
        let v = settings.character_variant;
        let f = settings.character_form;
        let p = settings.punctuation_form;
        let sep = || Row { text: String::new(), comment: String::new(), separator: true };
        vec![
                Row { text: text_or(IDS_OPTIONS_CHARACTER_VARIANT_HONG_KONG, "Traditional Chinese (Hong Kong)").to_string(), comment: tick(v == CharacterVariant::HongKong), separator: false },
                Row { text: text_or(IDS_OPTIONS_CHARACTER_VARIANT_TAIWAN, "Traditional Chinese (Taiwan)").to_string(), comment: tick(v == CharacterVariant::Taiwan), separator: false },
                Row { text: text_or(IDS_OPTIONS_CHARACTER_VARIANT_SIMPLIFIED, "Simplified Chinese").to_string(), comment: tick(v == CharacterVariant::Simplified), separator: false },
                sep(),
                Row { text: text_or(IDS_OPTIONS_CHARACTER_FORM_HALF_WIDTH, "Half-width Symbols").to_string(), comment: tick(f == CharacterForm::HalfWidth), separator: false },
                Row { text: text_or(IDS_OPTIONS_CHARACTER_FORM_FULL_WIDTH, "Full-width Symbols").to_string(), comment: tick(f == CharacterForm::FullWidth), separator: false },
                sep(),
                Row { text: text_or(IDS_OPTIONS_PUNCTUATION_FORM_CANTONESE, "Chinese Punctuation").to_string(), comment: tick(p == PunctuationForm::Cantonese), separator: false },
                Row { text: text_or(IDS_OPTIONS_PUNCTUATION_FORM_ENGLISH, "English Punctuation").to_string(), comment: tick(p == PunctuationForm::English), separator: false },
        ]
}

/// What a picked options row changes — each transport applies it its own
/// way (TSF goes through Processor::set_*, IMM32 edits settings + saves).
pub enum OptionsChoice {
        Variant(CharacterVariant),
        Form(CharacterForm),
        Punct(PunctuationForm),
}

/// Selectable-row position (0..6, separators don't count) → the setting.
pub fn options_choice(pos: usize) -> Option<OptionsChoice> {
        match pos {
                0 => Some(OptionsChoice::Variant(CharacterVariant::HongKong)),
                1 => Some(OptionsChoice::Variant(CharacterVariant::Taiwan)),
                2 => Some(OptionsChoice::Variant(CharacterVariant::Simplified)),
                3 => Some(OptionsChoice::Form(CharacterForm::HalfWidth)),
                4 => Some(OptionsChoice::Form(CharacterForm::FullWidth)),
                5 => Some(OptionsChoice::Punct(PunctuationForm::Cantonese)),
                6 => Some(OptionsChoice::Punct(PunctuationForm::English)),
                _ => None,
        }
}

/// Digit key → selectable position — mirrors candidate_index_from_code:
/// main-row and numpad digits with no modifiers, mapped through the
/// candidate-index range (0 comes after 9).
pub fn options_digit(code: u32, modifiers: u32, range: &[u32]) -> Option<usize> {
        if !globals::check_modifiers(modifiers, 0) {
                return None;
        }
        let digit = match code {
                0x30..=0x39 => code - 0x30,
                0x60..=0x69 => code - 0x60,
                _ => return None,
        };
        range.iter().position(|&k| k == digit)
}

/// The candidate-selection ordering — KeystrokeEngine::set_candidate_list_range.
pub fn candidate_index_range(page_size: u32) -> Vec<u32> {
        (1..=page_size.max(1)).map(|i| if i == 10 { 0 } else { i }).collect()
}
