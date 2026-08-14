//! Fork: Shift+Arrow selection-based partial conversion tests.

use super::*;

fn commit_of(result: &EngineResult) -> Option<String> {
    result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    })
}

/// Engine with no model: conversion candidates come from the fallback
/// (hiragana/katakana) and rewriter paths, which is enough to exercise the
/// selection plumbing deterministically.
fn engine() -> InputMethodEngine {
    let mut engine = InputMethodEngine::new();
    engine.converters.kanji = None;
    engine.live.enabled = false;
    engine
}

fn type_str(engine: &mut InputMethodEngine, s: &str) {
    for ch in s.chars() {
        engine.process_key(&press(ch));
    }
}

#[test]
fn shift_right_selects_from_start() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::HOME));

    e.process_key(&press_shift_key(Keysym::RIGHT));
    assert_eq!(e.input_buf.selection_range(), Some((0, 1)));

    e.process_key(&press_shift_key(Keysym::RIGHT));
    assert_eq!(e.input_buf.selection_range(), Some((0, 2)));
}

#[test]
fn shift_left_selects_from_end() {
    let mut e = engine();
    type_str(&mut e, "aiu");

    e.process_key(&press_shift_key(Keysym::LEFT));
    assert_eq!(e.input_buf.selection_range(), Some((2, 3)));
}

#[test]
fn plain_arrow_clears_selection() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_shift_key(Keysym::LEFT));
    assert!(e.input_buf.selection_range().is_some());

    e.process_key(&press_key(Keysym::LEFT));
    assert_eq!(e.input_buf.selection_range(), None);
}

#[test]
fn shift_home_and_end_select_to_edges() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_shift_key(Keysym::HOME));
    assert_eq!(e.input_buf.selection_range(), Some((0, 3)));

    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::END));
    assert_eq!(e.input_buf.selection_range(), Some((0, 3)));
}

#[test]
fn space_converts_only_the_selection() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::RIGHT));

    e.process_key(&press_key(Keysym::SPACE));
    assert!(matches!(e.state(), InputState::Conversion { .. }));
    // The conversion covers just `あ`; `いう` is parked
    let reading = e.state().reading().unwrap();
    assert_eq!(reading, "あ");
}

#[test]
fn enter_bakes_partial_result_back_into_composition() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::RIGHT));
    e.process_key(&press_key(Keysym::SPACE));

    // Pick the katakana fallback so the bake is observable
    let result = loop {
        let r = e.process_key(&press_key(Keysym::SPACE));
        let selected = e
            .state()
            .candidates()
            .and_then(|c| c.selected())
            .map(|c| c.text.clone())
            .unwrap_or_default();
        if selected == "ア" {
            break r;
        }
        // Guard against an infinite loop if ア is absent
        if e.state().candidates().map(|c| c.cursor()).unwrap_or(0) == 0 {
            panic!("katakana candidate not found");
        }
    };
    let _ = result;

    let result = e.process_key(&press_key(Keysym::RETURN));
    // Baking returns to Composing, no commit yet
    assert!(commit_of(&result).is_none());
    assert!(matches!(e.state(), InputState::Composing { .. }));
    assert_eq!(e.input_buf.reading(), "アいう");
}

#[test]
fn final_enter_commits_the_whole_buffer() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::RIGHT));
    e.process_key(&press_key(Keysym::SPACE));
    e.process_key(&press_key(Keysym::RETURN));

    // Now a plain Enter with no selection commits everything
    let result = e.process_key(&press_key(Keysym::RETURN));
    let committed = commit_of(&result).expect("final Enter must commit");
    assert_eq!(committed.chars().count(), 3);
    assert!(matches!(e.state(), InputState::Empty));
}

#[test]
fn shift_arrow_from_conversion_bakes_and_selects() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::SPACE));
    assert!(matches!(e.state(), InputState::Conversion { .. }));

    // Shift+Left bakes the selected candidate, returns to Composing, selects
    e.process_key(&press_shift_key(Keysym::LEFT));
    assert!(matches!(e.state(), InputState::Composing { .. }));
    assert_eq!(e.input_buf.selection_range(), Some((2, 3)));
}

#[test]
fn reconvert_katakana_maps_back_to_hiragana() {
    // Bake a katakana result, then re-select it: the alignment must recover
    // the original hiragana so the conversion reading is kana, not katakana.
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::RIGHT));
    e.process_key(&press_key(Keysym::SPACE));

    // Advance to the katakana fallback and bake it
    for _ in 0..8 {
        let selected = e
            .state()
            .candidates()
            .and_then(|c| c.selected())
            .map(|c| c.text.clone())
            .unwrap_or_default();
        if selected == "ア" {
            break;
        }
        e.process_key(&press_key(Keysym::SPACE));
    }
    e.process_key(&press_key(Keysym::RETURN));
    assert_eq!(e.input_buf.reading(), "アいう");

    // Re-select the ア and convert: the reading behind it is あ
    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::RIGHT));
    e.process_key(&press_key(Keysym::SPACE));
    assert_eq!(e.state().reading().unwrap(), "あ");
}

#[test]
fn escape_cancels_partial_conversion() {
    let mut e = engine();
    type_str(&mut e, "aiu");
    e.process_key(&press_key(Keysym::HOME));
    e.process_key(&press_shift_key(Keysym::RIGHT));
    e.process_key(&press_key(Keysym::SPACE));

    e.process_key(&press_key(Keysym::ESCAPE));
    assert!(matches!(e.state(), InputState::Composing { .. }));
    assert_eq!(e.input_buf.reading(), "あいう");
}
