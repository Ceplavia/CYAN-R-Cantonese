// Punctuation key table — direct port of PunctuationKey.cpp.
#![allow(dead_code)]

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// A punctuation candidate: (text, comment, secondaryComment).
#[derive(Clone, Copy)]
pub struct PunctuationSymbol {
        pub text: &'static str,
        pub comment: Option<&'static str>,
        pub secondary_comment: Option<&'static str>,
}

pub struct PunctuationKey {
        pub key_code: u32,
        pub key_text: &'static str,
        pub shifting_key_text: &'static str,
        pub instant_symbol: Option<&'static str>,
        pub instant_shifting_symbol: Option<&'static str>,
        pub symbols: &'static [PunctuationSymbol],
        pub shifting_symbols: &'static [PunctuationSymbol],
}

const fn s(text: &'static str, comment: Option<&'static str>, secondary: Option<&'static str>) -> PunctuationSymbol {
        PunctuationSymbol {
                text,
                comment,
                secondary_comment: secondary,
        }
}

const COMMA_SYMBOLS: &[PunctuationSymbol] = &[s("，", None, None)];
const COMMA_SHIFTING: &[PunctuationSymbol] = &[
        s("《", None, None),
        s("〈", None, None),
        s("«", None, None),
        s("‹", None, None),
        s("⟨", None, None),
        s("˂", None, None),
        s("˱", None, None),
];

const PERIOD_SYMBOLS: &[PunctuationSymbol] = &[s("。", None, None)];
const PERIOD_SHIFTING: &[PunctuationSymbol] = &[
        s("》", None, None),
        s("〉", None, None),
        s("»", None, None),
        s("›", None, None),
        s("⟩", None, None),
        s("˃", None, None),
        s("˲", None, None),
];

const SLASH_SYMBOLS: &[PunctuationSymbol] = &[
        s("、", None, None),
        s("､", Some("半寬頓號"), None),
        s("/", Some("半寬"), None),
        s("／", Some("全寬"), None),
        s("÷", None, None),
        s("≠", None, None),
        s("　", Some("全寬空格"), None),
];
const SLASH_SHIFTING: &[PunctuationSymbol] = &[s("？", None, None)];

const SEMICOLON_SYMBOLS: &[PunctuationSymbol] = &[s("；", None, None)];
const SEMICOLON_SHIFTING: &[PunctuationSymbol] = &[s("：", None, None)];

const QUOTE_SYMBOLS: &[PunctuationSymbol] = &[
        s("‘", Some("左單引號"), None),
        s("’", Some("右單引號"), None),
        s("'", Some("半寬"), None),
        s("＇", Some("全寬"), None),
];
const QUOTE_SHIFTING: &[PunctuationSymbol] = &[
        s("“", Some("左雙引號"), None),
        s("”", Some("右雙引號"), None),
        s("\"", Some("半寬"), None),
        s("＂", Some("全寬"), None),
];

const BRACKET_LEFT_SYMBOLS: &[PunctuationSymbol] = &[
        s("「", None, None),
        s("【", None, None),
        s("〔", None, None),
        s("［", Some("全寬"), None),
        s("〚", None, None),
        s("〘", None, None),
];
const BRACKET_LEFT_SHIFTING: &[PunctuationSymbol] = &[
        s("『", None, None),
        s("〖", None, None),
        s("｛", Some("全寬"), None),
];

const BRACKET_RIGHT_SYMBOLS: &[PunctuationSymbol] = &[
        s("」", None, None),
        s("】", None, None),
        s("〕", None, None),
        s("］", Some("全寬"), None),
        s("〛", None, None),
        s("〙", None, None),
];
const BRACKET_RIGHT_SHIFTING: &[PunctuationSymbol] = &[
        s("』", None, None),
        s("〗", None, None),
        s("｝", Some("全寬"), None),
];

const BACKSLASH_SYMBOLS: &[PunctuationSymbol] = &[
        s("、", None, None),
        s("\\", Some("半寬"), None),
        s("＼", Some("全寬"), None),
];
const BACKSLASH_SHIFTING: &[PunctuationSymbol] = &[
        s("·", Some("間隔號"), None),
        s("・", Some("中點"), None),
        s("|", Some("半寬"), None),
        s("｜", Some("全寬"), None),
        s("§", None, None),
        s("¦", None, None),
        s("‖", None, None),
        s("︴", None, None),
];

const GRAVE_SYMBOLS: &[PunctuationSymbol] = &[
        s("`", Some("重音符"), None),
        s("‵", None, None),
        s("‶", None, None),
        s("‷", None, None),
        s("′", Some("撇號"), None),
        s("″", Some("雙撇"), None),
        s("‴", None, None),
        s("⁗", None, None),
];
const GRAVE_SHIFTING: &[PunctuationSymbol] = &[
        s("~", Some("半寬"), None),
        s("～", Some("全寬"), None),
        s("˜", None, None),
        s("˷", None, None),
        s("ⸯ", None, None),
        s("≈", None, None),
        s("≋", None, None),
        s("≃", None, None),
        s("≅", None, None),
        s("≇", None, None),
        s("∽", None, None),
        s("⋍", None, None),
        s("≌", None, None),
        s("﹏", None, None),
        s("﹋", None, None),
        s("﹌", None, None),
        s("︴", None, None),
];

const MINUS_SYMBOLS: &[PunctuationSymbol] = &[s("-", None, None)];
const MINUS_SHIFTING: &[PunctuationSymbol] = &[s("——", None, None)];

const EQUAL_SYMBOLS: &[PunctuationSymbol] = &[s("=", None, None), s("々", None, None), s("〃", None, None)];
const EQUAL_SHIFTING: &[PunctuationSymbol] = &[s("+", None, None)];

const NUMBER0_SYMBOLS: &[PunctuationSymbol] = &[s("0", None, None)];
const NUMBER0_SHIFTING: &[PunctuationSymbol] = &[s("）", None, None)];
const NUMBER1_SYMBOLS: &[PunctuationSymbol] = &[s("1", None, None)];
const NUMBER1_SHIFTING: &[PunctuationSymbol] = &[s("！", None, None)];
const NUMBER2_SYMBOLS: &[PunctuationSymbol] = &[s("2", None, None)];
const NUMBER2_SHIFTING: &[PunctuationSymbol] = &[s("@", Some("半寬"), None), s("©", None, None), s("®", None, None), s("℗", None, None)];
const NUMBER3_SYMBOLS: &[PunctuationSymbol] = &[s("3", None, None)];
const NUMBER3_SHIFTING: &[PunctuationSymbol] = &[s("#", Some("半寬"), None), s("№", None, None)];
const NUMBER4_SYMBOLS: &[PunctuationSymbol] = &[s("4", None, None)];
const NUMBER4_SHIFTING: &[PunctuationSymbol] = &[
        s("￥", None, None),
        s("$", Some("半寬"), None),
        s("€", None, None),
        s("£", None, None),
        s("¥", None, None),
        s("¢", None, None),
        s("¤", None, None),
        s("₩", None, None),
];
const NUMBER5_SYMBOLS: &[PunctuationSymbol] = &[s("5", None, None)];
const NUMBER5_SHIFTING: &[PunctuationSymbol] = &[
        s("%", Some("半寬"), None),
        s("％", Some("全寬"), None),
        s("°", None, None),
        s("℃", None, None),
        s("‰", None, None),
        s("‱", None, None),
        s("℉", None, None),
        s("℅", None, None),
        s("℆", None, None),
        s("℀", None, None),
        s("℁", None, None),
        s("⅍", None, None),
        s("∞", None, None),
];
const NUMBER6_SYMBOLS: &[PunctuationSymbol] = &[s("6", None, None)];
const NUMBER6_SHIFTING: &[PunctuationSymbol] = &[s("……", None, None), s("…", None, None), s("^", Some("半寬"), None), s("＾", Some("全寬"), None)];
const NUMBER7_SYMBOLS: &[PunctuationSymbol] = &[s("7", None, None)];
const NUMBER7_SHIFTING: &[PunctuationSymbol] = &[s("＆", Some("全寬"), None), s("§", None, None), s("¦", None, None), s("&", Some("半寬"), None)];
const NUMBER8_SYMBOLS: &[PunctuationSymbol] = &[s("8", None, None)];
const NUMBER8_SHIFTING: &[PunctuationSymbol] = &[
        s("*", Some("半寬"), None),
        s("＊", Some("全寬"), None),
        s("·", Some("間隔號"), None),
        s("・", Some("中點"), None),
        s("×", Some("乘號"), None),
        s("※", Some("參攷號"), None),
        s("❂", None, None),
        s("⁂", None, None),
        s("☮", None, None),
        s("☯", None, None),
        s("☣", None, None),
];
const NUMBER9_SYMBOLS: &[PunctuationSymbol] = &[s("9", None, None)];
const NUMBER9_SHIFTING: &[PunctuationSymbol] = &[s("（", None, None)];

// Numeral variants for the rime-style "/" + digit query (e.g. /2 → 二 貳 ² Ⅱ).
const NUMERAL_0: &[PunctuationSymbol] = &[
        s("0", Some("半寬"), None),
        s("０", Some("全寬"), None),
        s("零", Some("中文"), None),
        s("〇", Some("中文"), None),
        s("⁰", Some("上標"), None),
        s("₀", Some("下標"), None),
];
const NUMERAL_1: &[PunctuationSymbol] = &[
        s("1", Some("半寬"), None),
        s("１", Some("全寬"), None),
        s("一", Some("中文"), None),
        s("壹", Some("大寫"), None),
        s("¹", Some("上標"), None),
        s("₁", Some("下標"), None),
        s("Ⅰ", Some("羅馬數字"), None),
];
const NUMERAL_2: &[PunctuationSymbol] = &[
        s("2", Some("半寬"), None),
        s("２", Some("全寬"), None),
        s("二", Some("中文"), None),
        s("貳", Some("大寫"), None),
        s("²", Some("上標"), None),
        s("₂", Some("下標"), None),
        s("Ⅱ", Some("羅馬數字"), None),
];
const NUMERAL_3: &[PunctuationSymbol] = &[
        s("3", Some("半寬"), None),
        s("３", Some("全寬"), None),
        s("三", Some("中文"), None),
        s("叁", Some("大寫"), None),
        s("³", Some("上標"), None),
        s("₃", Some("下標"), None),
        s("Ⅲ", Some("羅馬數字"), None),
];
const NUMERAL_4: &[PunctuationSymbol] = &[
        s("4", Some("半寬"), None),
        s("４", Some("全寬"), None),
        s("四", Some("中文"), None),
        s("肆", Some("大寫"), None),
        s("⁴", Some("上標"), None),
        s("₄", Some("下標"), None),
        s("Ⅳ", Some("羅馬數字"), None),
];
const NUMERAL_5: &[PunctuationSymbol] = &[
        s("5", Some("半寬"), None),
        s("５", Some("全寬"), None),
        s("五", Some("中文"), None),
        s("伍", Some("大寫"), None),
        s("⁵", Some("上標"), None),
        s("₅", Some("下標"), None),
        s("Ⅴ", Some("羅馬數字"), None),
];
const NUMERAL_6: &[PunctuationSymbol] = &[
        s("6", Some("半寬"), None),
        s("６", Some("全寬"), None),
        s("六", Some("中文"), None),
        s("陸", Some("大寫"), None),
        s("⁶", Some("上標"), None),
        s("₆", Some("下標"), None),
        s("Ⅵ", Some("羅馬數字"), None),
];
const NUMERAL_7: &[PunctuationSymbol] = &[
        s("7", Some("半寬"), None),
        s("７", Some("全寬"), None),
        s("七", Some("中文"), None),
        s("柒", Some("大寫"), None),
        s("⁷", Some("上標"), None),
        s("₇", Some("下標"), None),
        s("Ⅶ", Some("羅馬數字"), None),
];
const NUMERAL_8: &[PunctuationSymbol] = &[
        s("8", Some("半寬"), None),
        s("８", Some("全寬"), None),
        s("八", Some("中文"), None),
        s("捌", Some("大寫"), None),
        s("⁸", Some("上標"), None),
        s("₈", Some("下標"), None),
        s("Ⅷ", Some("羅馬數字"), None),
];
const NUMERAL_9: &[PunctuationSymbol] = &[
        s("9", Some("半寬"), None),
        s("９", Some("全寬"), None),
        s("九", Some("中文"), None),
        s("玖", Some("大寫"), None),
        s("⁹", Some("上標"), None),
        s("₉", Some("下標"), None),
        s("Ⅸ", Some("羅馬數字"), None),
];

/// The "/" punctuation key (exposed for the "/x" symbol query).
pub fn slash_key() -> &'static PunctuationKey {
        &SLASH
}

/// The "`" punctuation key — its symbol list is shown when "`" alone is
/// typed (before it becomes a pinyin reverse-lookup prefix).
pub fn grave_key() -> &'static PunctuationKey {
        &GRAVE
}

// Latin letter variants for the rime-style "/" + letter query (e.g. /a →
// à á ǎ â ā ä …). Tone-marked vowels first (陰平 陽平 上聲 去聲), then
// diacritic forms from European alphabets.
const LETTER_A: &[PunctuationSymbol] = &[
        s("ā", Some("陰平"), None), s("á", Some("陽平"), None), s("ǎ", Some("上聲"), None), s("à", Some("去聲"), None),
        s("â", None, None), s("ä", None, None), s("å", None, None), s("æ", None, None),
        s("Ā", Some("大寫"), None), s("Á", Some("大寫"), None), s("Ǎ", Some("大寫"), None), s("À", Some("大寫"), None),
        s("Â", Some("大寫"), None), s("Ä", Some("大寫"), None), s("Å", Some("大寫"), None), s("Æ", Some("大寫"), None),
];
const LETTER_E: &[PunctuationSymbol] = &[
        s("ē", Some("陰平"), None), s("é", Some("陽平"), None), s("ě", Some("上聲"), None), s("è", Some("去聲"), None),
        s("ê", None, None), s("ë", None, None),
        s("Ē", Some("大寫"), None), s("É", Some("大寫"), None), s("Ě", Some("大寫"), None), s("È", Some("大寫"), None),
        s("Ê", Some("大寫"), None), s("Ë", Some("大寫"), None),
];
const LETTER_I: &[PunctuationSymbol] = &[
        s("ī", Some("陰平"), None), s("í", Some("陽平"), None), s("ǐ", Some("上聲"), None), s("ì", Some("去聲"), None),
        s("î", None, None), s("ï", None, None),
        s("Ī", Some("大寫"), None), s("Í", Some("大寫"), None), s("Ǐ", Some("大寫"), None), s("Ì", Some("大寫"), None),
        s("Î", Some("大寫"), None), s("Ï", Some("大寫"), None),
];
const LETTER_O: &[PunctuationSymbol] = &[
        s("ō", Some("陰平"), None), s("ó", Some("陽平"), None), s("ǒ", Some("上聲"), None), s("ò", Some("去聲"), None),
        s("ô", None, None), s("ö", None, None), s("ø", None, None), s("œ", None, None),
        s("Ō", Some("大寫"), None), s("Ó", Some("大寫"), None), s("Ǒ", Some("大寫"), None), s("Ò", Some("大寫"), None),
        s("Ô", Some("大寫"), None), s("Ö", Some("大寫"), None), s("Ø", Some("大寫"), None), s("Œ", Some("大寫"), None),
];
const LETTER_U: &[PunctuationSymbol] = &[
        s("ū", Some("陰平"), None), s("ú", Some("陽平"), None), s("ǔ", Some("上聲"), None), s("ù", Some("去聲"), None),
        s("û", None, None), s("ü", Some("德語"), None),
        s("Ū", Some("大寫"), None), s("Ú", Some("大寫"), None), s("Ǔ", Some("大寫"), None), s("Ù", Some("大寫"), None),
        s("Û", Some("大寫"), None), s("Ü", Some("大寫"), None),
];
const LETTER_V: &[PunctuationSymbol] = &[
        s("ǖ", Some("ü 陰平"), None), s("ǘ", Some("ü 陽平"), None), s("ǚ", Some("ü 上聲"), None), s("ǜ", Some("ü 去聲"), None),
        s("ü", None, None), s("Ü", Some("大寫"), None),
];
const LETTER_C: &[PunctuationSymbol] = &[s("ç", None, None), s("č", None, None), s("ć", None, None), s("Ç", Some("大寫"), None), s("Č", Some("大寫"), None), s("Ć", Some("大寫"), None)];
const LETTER_N: &[PunctuationSymbol] = &[s("ń", None, None), s("ň", None, None), s("ǹ", None, None), s("ñ", None, None), s("Ń", Some("大寫"), None), s("Ň", Some("大寫"), None), s("Ǹ", Some("大寫"), None), s("Ñ", Some("大寫"), None)];
const LETTER_S: &[PunctuationSymbol] = &[s("ś", None, None), s("š", None, None), s("ß", None, None), s("Ś", Some("大寫"), None), s("Š", Some("大寫"), None)];
const LETTER_Y: &[PunctuationSymbol] = &[s("ý", None, None), s("ŷ", None, None), s("ÿ", None, None), s("Ý", Some("大寫"), None), s("Ŷ", Some("大寫"), None), s("Ÿ", Some("大寫"), None)];
const LETTER_Z: &[PunctuationSymbol] = &[s("ź", None, None), s("ž", None, None), s("Ź", Some("大寫"), None), s("Ž", Some("大寫"), None)];

/// Accented/diacritic letter variants for "/" + letter (rime-style).
pub fn letter_variants(letter: char) -> &'static [PunctuationSymbol] {
        match letter {
                'a' => LETTER_A,
                'e' => LETTER_E,
                'i' => LETTER_I,
                'o' => LETTER_O,
                'u' => LETTER_U,
                'v' => LETTER_V,
                'c' => LETTER_C,
                'n' => LETTER_N,
                's' => LETTER_S,
                'y' => LETTER_Y,
                'z' => LETTER_Z,
                _ => &[],
        }
}

/// Numeral-variant candidates for "/" + digit.
pub fn numeral_variants(digit: char) -> &'static [PunctuationSymbol] {
        match digit {
                '0' => NUMERAL_0,
                '1' => NUMERAL_1,
                '2' => NUMERAL_2,
                '3' => NUMERAL_3,
                '4' => NUMERAL_4,
                '5' => NUMERAL_5,
                '6' => NUMERAL_6,
                '7' => NUMERAL_7,
                '8' => NUMERAL_8,
                '9' => NUMERAL_9,
                _ => &[],
        }
}

const COMMA: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_COMMA.0 as u32,
        key_text: ",",
        shifting_key_text: "<",
        instant_symbol: Some("，"),
        instant_shifting_symbol: None,
        symbols: COMMA_SYMBOLS,
        shifting_symbols: COMMA_SHIFTING,
};
const PERIOD: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_PERIOD.0 as u32,
        key_text: ".",
        shifting_key_text: ">",
        instant_symbol: Some("。"),
        instant_shifting_symbol: None,
        symbols: PERIOD_SYMBOLS,
        shifting_symbols: PERIOD_SHIFTING,
};
const SLASH: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_2.0 as u32,
        key_text: "/",
        shifting_key_text: "?",
        instant_symbol: None,
        instant_shifting_symbol: Some("？"),
        symbols: SLASH_SYMBOLS,
        shifting_symbols: SLASH_SHIFTING,
};
const SEMICOLON: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_1.0 as u32,
        key_text: ";",
        shifting_key_text: ":",
        instant_symbol: Some("；"),
        instant_shifting_symbol: Some("："),
        symbols: SEMICOLON_SYMBOLS,
        shifting_symbols: SEMICOLON_SHIFTING,
};
const QUOTE: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_7.0 as u32,
        key_text: "'",
        shifting_key_text: "\"",
        instant_symbol: None,
        instant_shifting_symbol: None,
        symbols: QUOTE_SYMBOLS,
        shifting_symbols: QUOTE_SHIFTING,
};
const BRACKET_LEFT: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_4.0 as u32,
        key_text: "[",
        shifting_key_text: "{",
        instant_symbol: None,
        instant_shifting_symbol: None,
        symbols: BRACKET_LEFT_SYMBOLS,
        shifting_symbols: BRACKET_LEFT_SHIFTING,
};
const BRACKET_RIGHT: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_6.0 as u32,
        key_text: "]",
        shifting_key_text: "}",
        instant_symbol: None,
        instant_shifting_symbol: None,
        symbols: BRACKET_RIGHT_SYMBOLS,
        shifting_symbols: BRACKET_RIGHT_SHIFTING,
};
const BACKSLASH: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_5.0 as u32,
        key_text: "\\",
        shifting_key_text: "|",
        instant_symbol: None,
        instant_shifting_symbol: None,
        symbols: BACKSLASH_SYMBOLS,
        shifting_symbols: BACKSLASH_SHIFTING,
};
const GRAVE: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_3.0 as u32,
        key_text: "`",
        shifting_key_text: "~",
        instant_symbol: None,
        instant_shifting_symbol: None,
        symbols: GRAVE_SYMBOLS,
        shifting_symbols: GRAVE_SHIFTING,
};
const MINUS: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_MINUS.0 as u32,
        key_text: "-",
        shifting_key_text: "_",
        instant_symbol: Some("-"),
        instant_shifting_symbol: Some("——"),
        symbols: MINUS_SYMBOLS,
        shifting_symbols: MINUS_SHIFTING,
};
const EQUAL: PunctuationKey = PunctuationKey {
        key_code: VK_OEM_PLUS.0 as u32,
        key_text: "=",
        shifting_key_text: "+",
        instant_symbol: None,
        instant_shifting_symbol: Some("+"),
        symbols: EQUAL_SYMBOLS,
        shifting_symbols: EQUAL_SHIFTING,
};

macro_rules! number_key {
        ($digit:literal, $shifted:literal, $instant:expr, $shift_instant:expr, $symbols:expr, $shift_symbols:expr) => {
                PunctuationKey {
                        key_code: $digit as u32,
                        key_text: stringify!($digit),
                        shifting_key_text: $shifted,
                        instant_symbol: $instant,
                        instant_shifting_symbol: $shift_instant,
                        symbols: $symbols,
                        shifting_symbols: $shift_symbols,
                }
        };
}

static NUMBER0: PunctuationKey = number_key!('0', ")", Some("0"), Some("）"), NUMBER0_SYMBOLS, NUMBER0_SHIFTING);
static NUMBER1: PunctuationKey = number_key!('1', "!", Some("1"), Some("！"), NUMBER1_SYMBOLS, NUMBER1_SHIFTING);
static NUMBER2: PunctuationKey = number_key!('2', "@", Some("2"), None, NUMBER2_SYMBOLS, NUMBER2_SHIFTING);
static NUMBER3: PunctuationKey = number_key!('3', "#", Some("3"), None, NUMBER3_SYMBOLS, NUMBER3_SHIFTING);
static NUMBER4: PunctuationKey = number_key!('4', "$", Some("4"), None, NUMBER4_SYMBOLS, NUMBER4_SHIFTING);
static NUMBER5: PunctuationKey = number_key!('5', "%", Some("5"), None, NUMBER5_SYMBOLS, NUMBER5_SHIFTING);
static NUMBER6: PunctuationKey = number_key!('6', "^", Some("6"), None, NUMBER6_SYMBOLS, NUMBER6_SHIFTING);
static NUMBER7: PunctuationKey = number_key!('7', "&", Some("7"), None, NUMBER7_SYMBOLS, NUMBER7_SHIFTING);
static NUMBER8: PunctuationKey = number_key!('8', "*", Some("8"), None, NUMBER8_SYMBOLS, NUMBER8_SHIFTING);
static NUMBER9: PunctuationKey = number_key!('9', "(", Some("9"), Some("（"), NUMBER9_SYMBOLS, NUMBER9_SHIFTING);

impl PunctuationKey {
        pub fn is_number_key(&self) -> bool {
                (b'0' as u32..=b'9' as u32).contains(&self.key_code)
        }

        /// Only handle number keys when shifting; other keys always.
        pub fn should_handle(&self, is_shifting: bool) -> bool {
                !self.is_number_key() || is_shifting
        }

        pub fn text(&self, is_shifting: bool) -> &'static str {
                if is_shifting { self.shifting_key_text } else { self.key_text }
        }

        pub fn instant_symbol(&self, is_shifting: bool) -> Option<&'static str> {
                if is_shifting { self.instant_shifting_symbol } else { self.instant_symbol }
        }

        pub fn symbols(&self, is_shifting: bool) -> &'static [PunctuationSymbol] {
                if is_shifting { self.shifting_symbols } else { self.symbols }
        }

        pub fn for_virtual_key(key_code: u32) -> Option<&'static PunctuationKey> {
                Some(match key_code {
                        x if x == VK_OEM_COMMA.0 as u32 => &COMMA,
                        x if x == VK_OEM_PERIOD.0 as u32 => &PERIOD,
                        x if x == VK_OEM_2.0 as u32 => &SLASH,
                        x if x == VK_OEM_1.0 as u32 => &SEMICOLON,
                        x if x == VK_OEM_7.0 as u32 => &QUOTE,
                        x if x == VK_OEM_4.0 as u32 => &BRACKET_LEFT,
                        x if x == VK_OEM_6.0 as u32 => &BRACKET_RIGHT,
                        x if x == VK_OEM_5.0 as u32 => &BACKSLASH,
                        x if x == VK_OEM_3.0 as u32 => &GRAVE,
                        x if x == VK_OEM_MINUS.0 as u32 => &MINUS,
                        x if x == VK_OEM_PLUS.0 as u32 => &EQUAL,
                        x if x == b'0' as u32 => &NUMBER0,
                        x if x == b'1' as u32 => &NUMBER1,
                        x if x == b'2' as u32 => &NUMBER2,
                        x if x == b'3' as u32 => &NUMBER3,
                        x if x == b'4' as u32 => &NUMBER4,
                        x if x == b'5' as u32 => &NUMBER5,
                        x if x == b'6' as u32 => &NUMBER6,
                        x if x == b'7' as u32 => &NUMBER7,
                        x if x == b'8' as u32 => &NUMBER8,
                        x if x == b'9' as u32 => &NUMBER9,
                        _ => return None,
                })
        }
}
