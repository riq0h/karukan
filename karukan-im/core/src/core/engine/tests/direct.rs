//! Fork: F6-F10 direct conversion tests.

use super::*;

/// Type `text` as romaji into a fresh engine (no model), then press `key`
/// and return the committed text, if any.
fn direct_convert(input: &str, key: Keysym) -> Option<String> {
    let mut engine = InputMethodEngine::new();
    engine.converters.kanji = None;
    for ch in input.chars() {
        engine.process_key(&press(ch));
    }
    let result = engine.process_key(&press_key(key));
    result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    })
}

#[test]
fn f6_commits_hiragana() {
    assert_eq!(direct_convert("kana", Keysym::F6), Some("かな".to_string()));
}

#[test]
fn f7_commits_full_katakana() {
    assert_eq!(direct_convert("kana", Keysym::F7), Some("カナ".to_string()));
}

#[test]
fn f8_commits_half_katakana() {
    assert_eq!(
        direct_convert("kana", Keysym::F8),
        Some("ｶﾅ".to_string())
    );
}

#[test]
fn f8_handles_dakuten() {
    assert_eq!(
        direct_convert("gakkou", Keysym::F8),
        Some("ｶﾞｯｺｳ".to_string())
    );
}

#[test]
fn f9_commits_fullwidth_ascii() {
    // Latin passes through romaji conversion unchanged when no rule fires
    let mut engine = InputMethodEngine::new();
    engine.converters.kanji = None;
    // Shift+A enters alphabet mode; subsequent chars are recorded verbatim
    engine.process_key(&press_shift('A'));
    engine.process_key(&press('B'));
    engine.process_key(&press('C'));
    let result = engine.process_key(&press_key(Keysym::F9));
    let commit = result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(commit, Some("ＡＢＣ".to_string()));
}

#[test]
fn f10_commits_halfwidth_ascii() {
    let mut engine = InputMethodEngine::new();
    engine.converters.kanji = None;
    engine.process_key(&press_shift('A'));
    engine.process_key(&press('B'));
    engine.process_key(&press('C'));
    let result = engine.process_key(&press_key(Keysym::F10));
    let commit = result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(commit, Some("ABC".to_string()));
}

#[test]
fn function_key_on_empty_is_not_consumed() {
    let mut engine = InputMethodEngine::new();
    engine.converters.kanji = None;
    // Empty state: the key never reaches the composing handler, so it
    // passes through to the application.
    let result = engine.process_key(&press_key(Keysym::F6));
    assert!(!result.consumed);
}

#[test]
fn f7_from_conversion_state_uses_reading() {
    // Enter conversion, then F7: the reading (not the selected candidate)
    // is what gets converted to katakana.
    let mut engine = InputMethodEngine::new();
    engine.converters.kanji = None;
    for ch in "kana".chars() {
        engine.process_key(&press(ch));
    }
    engine.process_key(&press_key(Keysym::SPACE));
    assert!(matches!(engine.state(), InputState::Conversion { .. }));

    let result = engine.process_key(&press_key(Keysym::F7));
    let commit = result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(commit, Some("カナ".to_string()));
    assert!(matches!(engine.state(), InputState::Empty));
}

#[test]
fn f6_settles_pending_romaji() {
    // A trailing consonant settles before conversion: `kann` → `かん`
    assert_eq!(direct_convert("kann", Keysym::F6), Some("かん".to_string()));
}
