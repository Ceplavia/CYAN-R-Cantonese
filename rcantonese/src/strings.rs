// Localized UI strings — replaces the Resources\Strings.*.rc2 string tables.
// The user's default UI language picks the table (mirroring LoadStringW);
// unknown ids fall back to English. All zh-* locales share one table, matching
// the upstream resources which use identical strings for every Chinese locale.
#![allow(dead_code)]

use windows::Win32::Globalization::GetUserDefaultUILanguage;

// String ids — same values as resource.h.
pub const IDIS_IME: u32 = 12;
pub const IDS_TEXTSERVICE_DESC: u32 = 20;
pub const IDS_DESC_INPUT_MODE_TOGGLE: u32 = 30;
pub const IDS_DESC_CHARACTER_FORM_TOGGLE: u32 = 31;
pub const IDS_DESC_PUNCTUATION_FORM_TOGGLE: u32 = 32;
pub const IDS_DESC_CHARACTER_VARIANT_TRADITIONAL: u32 = 33;
pub const IDS_DESC_CHARACTER_VARIANT_HONG_KONG: u32 = 34;
pub const IDS_DESC_CHARACTER_VARIANT_TAIWAN: u32 = 35;
pub const IDS_DESC_CHARACTER_VARIANT_SIMPLIFIED: u32 = 36;
pub const IDS_LANGBAR_INPUT_METHOD_MODE: u32 = 37;
pub const IDS_MENU_CANDIDATE_FONT_SIZE: u32 = 40;
pub const IDS_MENU_CANDIDATE_NUMBER_FONT_SIZE: u32 = 41;
pub const IDS_MENU_CANDIDATE_COMMENT_FONT_SIZE: u32 = 42;
pub const IDS_MENU_CANDIDATE_PAGE_SIZE: u32 = 43;
pub const IDS_MENU_PUNCTUATION_FORM: u32 = 44;
pub const IDS_MENU_PUNCTUATION_FORM_CANTONESE: u32 = 45;
pub const IDS_MENU_PUNCTUATION_FORM_ENGLISH: u32 = 46;
pub const IDS_MENU_CHARACTER_FORM: u32 = 47;
pub const IDS_MENU_CHARACTER_FORM_HALF_WIDTH: u32 = 48;
pub const IDS_MENU_CHARACTER_FORM_FULL_WIDTH: u32 = 49;
pub const IDS_MENU_CHARACTER_VARIANT: u32 = 50;
pub const IDS_MENU_CHARACTER_VARIANT_TRADITIONAL: u32 = 51;
pub const IDS_MENU_CHARACTER_VARIANT_HONG_KONG: u32 = 52;
pub const IDS_MENU_CHARACTER_VARIANT_TAIWAN: u32 = 53;
pub const IDS_MENU_CHARACTER_VARIANT_SIMPLIFIED: u32 = 54;
pub const IDS_MENU_MORE_SETTINGS: u32 = 55;
pub const IDS_LANGBAR_INPUT_MODE_TOOLTIP: u32 = 56;
pub const IDS_OPTIONS_CHARACTER_VARIANT_TRADITIONAL: u32 = 57;
pub const IDS_OPTIONS_CHARACTER_VARIANT_HONG_KONG: u32 = 58;
pub const IDS_OPTIONS_CHARACTER_VARIANT_TAIWAN: u32 = 59;
pub const IDS_OPTIONS_CHARACTER_VARIANT_SIMPLIFIED: u32 = 60;
pub const IDS_OPTIONS_CHARACTER_FORM_HALF_WIDTH: u32 = 61;
pub const IDS_OPTIONS_CHARACTER_FORM_FULL_WIDTH: u32 = 62;
pub const IDS_OPTIONS_PUNCTUATION_FORM_CANTONESE: u32 = 63;
pub const IDS_OPTIONS_PUNCTUATION_FORM_ENGLISH: u32 = 64;
pub const IDS_OPTIONS_INPUT_MODE_CANTONESE: u32 = 65;
pub const IDS_OPTIONS_INPUT_MODE_ABC: u32 = 66;
pub const IDS_DESC_CHARACTER_FORM_HALF_WIDTH: u32 = 67;
pub const IDS_DESC_CHARACTER_FORM_FULL_WIDTH: u32 = 68;
pub const IDS_DESC_PUNCTUATION_FORM_CANTONESE: u32 = 69;
pub const IDS_DESC_PUNCTUATION_FORM_ENGLISH: u32 = 70;
pub const IDS_DESC_INPUT_MODE_CANTONESE: u32 = 71;
pub const IDS_DESC_INPUT_MODE_ABC: u32 = 72;

const EN_US: &[(u32, &str)] = &[
        (IDIS_IME, "R-Cantonese Input Method"),
        (IDS_TEXTSERVICE_DESC, "R-Cantonese"),
        (IDS_DESC_INPUT_MODE_TOGGLE, "Input Mode (Cantonese/ABC, Shift)"),
        (IDS_DESC_CHARACTER_FORM_TOGGLE, "Character Form (Half-width/Full-width, Shift+Space)"),
        (IDS_DESC_PUNCTUATION_FORM_TOGGLE, "Punctuation Form (Cantonese/English, Ctrl+.)"),
        (IDS_DESC_CHARACTER_VARIANT_TRADITIONAL, "Traditional Characters (Ctrl+Shift+1)"),
        (IDS_DESC_CHARACTER_VARIANT_HONG_KONG, "Traditional Characters, HK (Ctrl+Shift+2)"),
        (IDS_DESC_CHARACTER_VARIANT_TAIWAN, "Traditional Characters, TW (Ctrl+Shift+3)"),
        (IDS_DESC_CHARACTER_VARIANT_SIMPLIFIED, "Simplified Characters (Ctrl+Shift+4)"),
        (IDS_DESC_CHARACTER_FORM_HALF_WIDTH, "Half-width Characters (Ctrl+Shift+5)"),
        (IDS_DESC_CHARACTER_FORM_FULL_WIDTH, "Full-width Characters (Ctrl+Shift+6)"),
        (IDS_DESC_PUNCTUATION_FORM_CANTONESE, "Cantonese Punctuation (Ctrl+Shift+7)"),
        (IDS_DESC_PUNCTUATION_FORM_ENGLISH, "English Punctuation (Ctrl+Shift+8)"),
        (IDS_DESC_INPUT_MODE_CANTONESE, "Cantonese Mode (Ctrl+Shift+9)"),
        (IDS_DESC_INPUT_MODE_ABC, "ABC Mode (Ctrl+Shift+0)"),
        (IDS_LANGBAR_INPUT_METHOD_MODE, "Input Mode"),
        (IDS_MENU_CANDIDATE_FONT_SIZE, "Candidate Font Size"),
        (IDS_MENU_CANDIDATE_NUMBER_FONT_SIZE, "Number Font Size"),
        (IDS_MENU_CANDIDATE_COMMENT_FONT_SIZE, "Comment Font Size"),
        (IDS_MENU_CANDIDATE_PAGE_SIZE, "Candidates per Page"),
        (IDS_MENU_PUNCTUATION_FORM, "Punctuation Form"),
        (IDS_MENU_PUNCTUATION_FORM_CANTONESE, "Cantonese"),
        (IDS_MENU_PUNCTUATION_FORM_ENGLISH, "English"),
        (IDS_MENU_CHARACTER_FORM, "Half-width / Full-width"),
        (IDS_MENU_CHARACTER_FORM_HALF_WIDTH, "Half-width"),
        (IDS_MENU_CHARACTER_FORM_FULL_WIDTH, "Full-width"),
        (IDS_MENU_CHARACTER_VARIANT, "Candidate Characters"),
        (IDS_MENU_CHARACTER_VARIANT_TRADITIONAL, "Traditional"),
        (IDS_MENU_CHARACTER_VARIANT_HONG_KONG, "Traditional, HK"),
        (IDS_MENU_CHARACTER_VARIANT_TAIWAN, "Traditional, TW"),
        (IDS_MENU_CHARACTER_VARIANT_SIMPLIFIED, "Simplified"),
        (IDS_MENU_MORE_SETTINGS, "Settings Center…"),
        (IDS_LANGBAR_INPUT_MODE_TOOLTIP, "Left-click to switch modes\nRight-click for more options"),
        (IDS_OPTIONS_CHARACTER_VARIANT_TRADITIONAL, "Traditional"),
        (IDS_OPTIONS_CHARACTER_VARIANT_HONG_KONG, "Traditional Chinese (Hong Kong)"),
        (IDS_OPTIONS_CHARACTER_VARIANT_TAIWAN, "Traditional Chinese (Taiwan)"),
        (IDS_OPTIONS_CHARACTER_VARIANT_SIMPLIFIED, "Simplified Chinese"),
        (IDS_OPTIONS_CHARACTER_FORM_HALF_WIDTH, "Half-width Symbols"),
        (IDS_OPTIONS_CHARACTER_FORM_FULL_WIDTH, "Full-width Symbols"),
        (IDS_OPTIONS_PUNCTUATION_FORM_CANTONESE, "Chinese Punctuation"),
        (IDS_OPTIONS_PUNCTUATION_FORM_ENGLISH, "English Punctuation"),
        (IDS_OPTIONS_INPUT_MODE_CANTONESE, "Cantonese Mode"),
        (IDS_OPTIONS_INPUT_MODE_ABC, "ABC Mode"),
];

// Shared by zh-HK, zh-TW, zh-CN, zh-SG and zh-MO, matching the upstream
// resources which ship identical strings for all Chinese locales.
const ZH: &[(u32, &str)] = &[
        (IDIS_IME, "R-Cantonese 輸入法"),
        (IDS_TEXTSERVICE_DESC, "R-Cantonese"),
        (IDS_DESC_INPUT_MODE_TOGGLE, "輸入模式 (中文/ABC, Shift)"),
        (IDS_DESC_CHARACTER_FORM_TOGGLE, "字符寬度 (半寬/全寬, Shift+Space)"),
        (IDS_DESC_PUNCTUATION_FORM_TOGGLE, "標點符號 (中文/英文, Ctrl+.)"),
        (IDS_DESC_CHARACTER_VARIANT_TRADITIONAL, "傳統漢字 (Ctrl+Shift+1)"),
        (IDS_DESC_CHARACTER_VARIANT_HONG_KONG, "傳統漢字・香港 (Ctrl+Shift+2)"),
        (IDS_DESC_CHARACTER_VARIANT_TAIWAN, "傳統漢字・臺灣 (Ctrl+Shift+3)"),
        (IDS_DESC_CHARACTER_VARIANT_SIMPLIFIED, "簡化字 (Ctrl+Shift+4)"),
        (IDS_DESC_CHARACTER_FORM_HALF_WIDTH, "半寬字符 (Ctrl+Shift+5)"),
        (IDS_DESC_CHARACTER_FORM_FULL_WIDTH, "全寬字符 (Ctrl+Shift+6)"),
        (IDS_DESC_PUNCTUATION_FORM_CANTONESE, "中文標點 (Ctrl+Shift+7)"),
        (IDS_DESC_PUNCTUATION_FORM_ENGLISH, "英文標點 (Ctrl+Shift+8)"),
        (IDS_DESC_INPUT_MODE_CANTONESE, "粵拼模式 (Ctrl+Shift+9)"),
        (IDS_DESC_INPUT_MODE_ABC, "ABC 模式 (Ctrl+Shift+0)"),
        (IDS_LANGBAR_INPUT_METHOD_MODE, "輸入模式"),
        (IDS_MENU_CANDIDATE_FONT_SIZE, "候選詞字號"),
        (IDS_MENU_CANDIDATE_NUMBER_FONT_SIZE, "候選詞編號字號"),
        (IDS_MENU_CANDIDATE_COMMENT_FONT_SIZE, "候選詞註釋字號"),
        (IDS_MENU_CANDIDATE_PAGE_SIZE, "每頁候選詞數目"),
        (IDS_MENU_PUNCTUATION_FORM, "標點符號"),
        (IDS_MENU_PUNCTUATION_FORM_CANTONESE, "中文"),
        (IDS_MENU_PUNCTUATION_FORM_ENGLISH, "英文"),
        (IDS_MENU_CHARACTER_FORM, "數字、字母字符寬度"),
        (IDS_MENU_CHARACTER_FORM_HALF_WIDTH, "半寬"),
        (IDS_MENU_CHARACTER_FORM_FULL_WIDTH, "全寬"),
        (IDS_MENU_CHARACTER_VARIANT, "候選詞字符集"),
        (IDS_MENU_CHARACTER_VARIANT_TRADITIONAL, "傳統漢字"),
        (IDS_MENU_CHARACTER_VARIANT_HONG_KONG, "傳統漢字・香港"),
        (IDS_MENU_CHARACTER_VARIANT_TAIWAN, "傳統漢字・臺灣"),
        (IDS_MENU_CHARACTER_VARIANT_SIMPLIFIED, "簡化字"),
        (IDS_MENU_MORE_SETTINGS, "配置中心…"),
        (IDS_LANGBAR_INPUT_MODE_TOOLTIP, "左鍵切換模式\n右鍵開啟選單"),
        (IDS_OPTIONS_CHARACTER_VARIANT_TRADITIONAL, "傳統漢字"),
        (IDS_OPTIONS_CHARACTER_VARIANT_HONG_KONG, "繁體中文（香港）"),
        (IDS_OPTIONS_CHARACTER_VARIANT_TAIWAN, "繁體中文（台灣）"),
        (IDS_OPTIONS_CHARACTER_VARIANT_SIMPLIFIED, "簡體中文"),
        (IDS_OPTIONS_CHARACTER_FORM_HALF_WIDTH, "半型符號"),
        (IDS_OPTIONS_CHARACTER_FORM_FULL_WIDTH, "全型符號"),
        (IDS_OPTIONS_PUNCTUATION_FORM_CANTONESE, "中文標點"),
        (IDS_OPTIONS_PUNCTUATION_FORM_ENGLISH, "英文標點"),
        (IDS_OPTIONS_INPUT_MODE_CANTONESE, "粵拼模式"),
        (IDS_OPTIONS_INPUT_MODE_ABC, "ABC 模式"),
];

const LANG_CHINESE: u16 = 0x04;

fn is_chinese_ui() -> bool {
        // The settings.toml override wins; "auto" follows the OS UI language.
        match crate::config::load().ui_language.as_str() {
                "zh" => return true,
                "en" => return false,
                _ => {}
        }
        unsafe { GetUserDefaultUILanguage() & 0x3ff == LANG_CHINESE }
}

/// Localized string for `id`, or `None` when the id is unknown.
pub fn text(id: u32) -> Option<&'static str> {
        let table = if is_chinese_ui() { ZH } else { EN_US };
        table.iter().chain(EN_US.iter()).find(|(key, _)| *key == id).map(|(_, value)| *value)
}

/// Localized string for `id`, falling back to `fallback` when unknown.
pub fn text_or(id: u32, fallback: &'static str) -> &'static str {
        text(id).unwrap_or(fallback)
}
