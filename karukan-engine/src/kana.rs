use unicode_normalization::UnicodeNormalization;

/// Apply NFKC normalization to text.
///
/// This is needed for models whose tokenizer does NOT support full-width ASCII
/// characters in its vocabulary. Without NFKC normalization, characters like
/// `（`, `）`, `！`, `？` are incorrectly tokenized as EOS tokens, causing
/// generation to stop prematurely.
///
/// NFKC normalization converts:
/// - Full-width ASCII → Half-width: `（` → `(`, `！` → `!`, `？` → `?`
/// - Full-width digits → Half-width: `０` → `0`, `１` → `1`
/// - Compatibility characters → Canonical forms
///
/// Note: Hiragana, Katakana, and Kanji are NOT affected by NFKC normalization.
/// The special jinen tokens (U+EE00-U+EE02) in Private Use Area are also preserved.
pub fn normalize_nfkc(text: &str) -> String {
    text.nfkc().collect()
}

/// Convert hiragana to katakana
pub fn hiragana_to_katakana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            // Hiragana range (U+3041-U+3096) -> Katakana (U+30A1-U+30F6)
            '\u{3041}'..='\u{3096}' => std::char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Convert hiragana to half-width katakana
pub fn hiragana_to_halfwidth_katakana(text: &str) -> String {
    // First convert to full-width katakana, then to half-width
    let katakana = hiragana_to_katakana(text);
    katakana_to_halfwidth(&katakana)
}

/// Convert full-width katakana to half-width katakana
fn katakana_to_halfwidth(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        match c {
            'ァ' => result.push_str("\u{FF67}"),
            'ィ' => result.push_str("\u{FF68}"),
            'ゥ' => result.push_str("\u{FF69}"),
            'ェ' => result.push_str("\u{FF6A}"),
            'ォ' => result.push_str("\u{FF6B}"),
            'ッ' => result.push_str("\u{FF6F}"),
            'ャ' => result.push_str("\u{FF6C}"),
            'ュ' => result.push_str("\u{FF6D}"),
            'ョ' => result.push_str("\u{FF6E}"),
            'ア' => result.push_str("\u{FF71}"),
            'イ' => result.push_str("\u{FF72}"),
            'ウ' => result.push_str("\u{FF73}"),
            'エ' => result.push_str("\u{FF74}"),
            'オ' => result.push_str("\u{FF75}"),
            'カ' => result.push_str("\u{FF76}"),
            'キ' => result.push_str("\u{FF77}"),
            'ク' => result.push_str("\u{FF78}"),
            'ケ' => result.push_str("\u{FF79}"),
            'コ' => result.push_str("\u{FF7A}"),
            'サ' => result.push_str("\u{FF7B}"),
            'シ' => result.push_str("\u{FF7C}"),
            'ス' => result.push_str("\u{FF7D}"),
            'セ' => result.push_str("\u{FF7E}"),
            'ソ' => result.push_str("\u{FF7F}"),
            'タ' => result.push_str("\u{FF80}"),
            'チ' => result.push_str("\u{FF81}"),
            'ツ' => result.push_str("\u{FF82}"),
            'テ' => result.push_str("\u{FF83}"),
            'ト' => result.push_str("\u{FF84}"),
            'ナ' => result.push_str("\u{FF85}"),
            'ニ' => result.push_str("\u{FF86}"),
            'ヌ' => result.push_str("\u{FF87}"),
            'ネ' => result.push_str("\u{FF88}"),
            'ノ' => result.push_str("\u{FF89}"),
            'ハ' => result.push_str("\u{FF8A}"),
            'ヒ' => result.push_str("\u{FF8B}"),
            'フ' => result.push_str("\u{FF8C}"),
            'ヘ' => result.push_str("\u{FF8D}"),
            'ホ' => result.push_str("\u{FF8E}"),
            'マ' => result.push_str("\u{FF8F}"),
            'ミ' => result.push_str("\u{FF90}"),
            'ム' => result.push_str("\u{FF91}"),
            'メ' => result.push_str("\u{FF92}"),
            'モ' => result.push_str("\u{FF93}"),
            'ヤ' => result.push_str("\u{FF94}"),
            'ユ' => result.push_str("\u{FF95}"),
            'ヨ' => result.push_str("\u{FF96}"),
            'ラ' => result.push_str("\u{FF97}"),
            'リ' => result.push_str("\u{FF98}"),
            'ル' => result.push_str("\u{FF99}"),
            'レ' => result.push_str("\u{FF9A}"),
            'ロ' => result.push_str("\u{FF9B}"),
            'ワ' => result.push_str("\u{FF9C}"),
            'ヲ' => result.push_str("\u{FF66}"),
            'ン' => result.push_str("\u{FF9D}"),
            'ー' => result.push_str("\u{FF70}"),
            '。' => result.push_str("\u{FF61}"),
            '、' => result.push_str("\u{FF64}"),
            '・' => result.push_str("\u{FF65}"),
            // Dakuten (voiced) variants: base + combining dakuten
            'ガ' => result.push_str("\u{FF76}\u{FF9E}"),
            'ギ' => result.push_str("\u{FF77}\u{FF9E}"),
            'グ' => result.push_str("\u{FF78}\u{FF9E}"),
            'ゲ' => result.push_str("\u{FF79}\u{FF9E}"),
            'ゴ' => result.push_str("\u{FF7A}\u{FF9E}"),
            'ザ' => result.push_str("\u{FF7B}\u{FF9E}"),
            'ジ' => result.push_str("\u{FF7C}\u{FF9E}"),
            'ズ' => result.push_str("\u{FF7D}\u{FF9E}"),
            'ゼ' => result.push_str("\u{FF7E}\u{FF9E}"),
            'ゾ' => result.push_str("\u{FF7F}\u{FF9E}"),
            'ダ' => result.push_str("\u{FF80}\u{FF9E}"),
            'ヂ' => result.push_str("\u{FF81}\u{FF9E}"),
            'ヅ' => result.push_str("\u{FF82}\u{FF9E}"),
            'デ' => result.push_str("\u{FF83}\u{FF9E}"),
            'ド' => result.push_str("\u{FF84}\u{FF9E}"),
            'バ' => result.push_str("\u{FF8A}\u{FF9E}"),
            'ビ' => result.push_str("\u{FF8B}\u{FF9E}"),
            'ブ' => result.push_str("\u{FF8C}\u{FF9E}"),
            'ベ' => result.push_str("\u{FF8D}\u{FF9E}"),
            'ボ' => result.push_str("\u{FF8E}\u{FF9E}"),
            'ヴ' => result.push_str("\u{FF73}\u{FF9E}"),
            // Handakuten (p-row)
            'パ' => result.push_str("\u{FF8A}\u{FF9F}"),
            'ピ' => result.push_str("\u{FF8B}\u{FF9F}"),
            'プ' => result.push_str("\u{FF8C}\u{FF9F}"),
            'ペ' => result.push_str("\u{FF8D}\u{FF9F}"),
            'ポ' => result.push_str("\u{FF8E}\u{FF9F}"),
            _ => result.push(c),
        }
    }
    result
}

/// Convert half-width ASCII to full-width ASCII
pub fn ascii_to_fullwidth(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            // ASCII printable range 0x21-0x7E → full-width 0xFF01-0xFF5E
            '!'..='~' => std::char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c),
            // Space → full-width space
            ' ' => '\u{3000}',
            _ => c,
        })
        .collect()
}

/// Convert katakana to hiragana
pub fn katakana_to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            // Katakana range (U+30A1-U+30F6) -> Hiragana (U+3041-U+3096)
            '\u{30A1}'..='\u{30F6}' => std::char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hiragana_to_katakana() {
        assert_eq!(hiragana_to_katakana("あいうえお"), "アイウエオ");
        assert_eq!(hiragana_to_katakana("こんにちは"), "コンニチハ");
        assert_eq!(hiragana_to_katakana("きゃきゅきょ"), "キャキュキョ");
        assert_eq!(hiragana_to_katakana("がぎぐげご"), "ガギグゲゴ");
        assert_eq!(hiragana_to_katakana("ぱぴぷぺぽ"), "パピプペポ");

        // Mixed with non-hiragana should pass through
        assert_eq!(hiragana_to_katakana("abc123"), "abc123");
        assert_eq!(hiragana_to_katakana("あいうabc"), "アイウabc");
    }

    #[test]
    fn test_katakana_to_hiragana() {
        assert_eq!(katakana_to_hiragana("アイウエオ"), "あいうえお");
        assert_eq!(katakana_to_hiragana("コンニチハ"), "こんにちは");
        assert_eq!(katakana_to_hiragana("キャキュキョ"), "きゃきゅきょ");
    }

    #[test]
    fn test_round_trip() {
        let original = "こんにちは";
        let katakana = hiragana_to_katakana(original);
        let back = katakana_to_hiragana(&katakana);
        assert_eq!(original, back);
    }

    #[test]
    fn test_normalize_nfkc() {
        // Full-width ASCII should be converted to half-width
        assert_eq!(normalize_nfkc("（）"), "()");
        assert_eq!(normalize_nfkc("！？"), "!?");
        assert_eq!(normalize_nfkc("Ａｂｃ"), "Abc");
        assert_eq!(normalize_nfkc("０１２３"), "0123");

        // Full-width punctuation
        assert_eq!(normalize_nfkc("、。"), "、。"); // These are NOT full-width ASCII
        assert_eq!(normalize_nfkc("「」"), "「」"); // Japanese brackets preserved

        // Hiragana, Katakana, Kanji should be preserved
        assert_eq!(normalize_nfkc("あいうえお"), "あいうえお");
        assert_eq!(normalize_nfkc("アイウエオ"), "アイウエオ");
        assert_eq!(normalize_nfkc("漢字"), "漢字");

        // Mixed text
        assert_eq!(normalize_nfkc("（カッコ）テスト！"), "(カッコ)テスト!");

        // Special jinen tokens (Private Use Area U+EE00-U+EE02) should be preserved
        assert_eq!(normalize_nfkc("\u{ee00}"), "\u{ee00}");
        assert_eq!(normalize_nfkc("\u{ee01}"), "\u{ee01}");
        assert_eq!(normalize_nfkc("\u{ee02}"), "\u{ee02}");
        assert_eq!(
            normalize_nfkc("\u{ee02}context\u{ee00}input\u{ee01}"),
            "\u{ee02}context\u{ee00}input\u{ee01}"
        );
    }
}
