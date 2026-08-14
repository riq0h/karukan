//! Fork: `display.show_aux_text = false` hides the aux line entirely.

use super::*;

fn aux_of(result: &EngineResult) -> Option<String> {
    result.actions.iter().find_map(|a| match a {
        EngineAction::UpdateAuxText(text) => Some(text.clone()),
        _ => None,
    })
}

fn has_hide_aux(result: &EngineResult) -> bool {
    result
        .actions
        .iter()
        .any(|a| matches!(a, EngineAction::HideAuxText))
}

/// Engine with no model and the aux line disabled.
fn engine_without_aux() -> InputMethodEngine {
    let mut engine = InputMethodEngine::with_config(EngineConfig {
        show_aux_text: false,
        ..EngineConfig::default()
    });
    engine.converters.kanji = None;
    engine
}

#[test]
fn composing_aux_is_empty_when_disabled() {
    let mut e = engine_without_aux();
    let result = e.process_key(&press('a'));
    // The aux line renders empty, which the frontends clear
    assert_eq!(aux_of(&result).as_deref(), Some(""));
}

#[test]
fn composing_aux_is_populated_by_default() {
    let mut e = InputMethodEngine::new();
    e.converters.kanji = None;
    let result = e.process_key(&press('a'));
    let aux = aux_of(&result).expect("aux action expected");
    assert!(!aux.is_empty(), "aux should carry the mode indicator");
}

#[test]
fn conversion_aux_is_empty_when_disabled() {
    let mut e = engine_without_aux();
    e.process_key(&press('a'));
    let result = e.process_key(&press_key(Keysym::SPACE));
    assert!(matches!(e.state(), InputState::Conversion { .. }));
    assert_eq!(aux_of(&result).as_deref(), Some(""));
}

#[test]
fn live_conversion_toggle_hides_aux_when_disabled() {
    let mut e = engine_without_aux();
    // Ctrl+Shift+L would normally report `ライブ変換: ON/OFF`
    let result = e.process_key(&press_ctrl_shift(Keysym::KEY_L));
    assert!(has_hide_aux(&result), "toggle must not surface aux text");
    assert!(aux_of(&result).is_none());
}

#[test]
fn live_conversion_toggle_reports_by_default() {
    let mut e = InputMethodEngine::new();
    e.converters.kanji = None;
    let result = e.process_key(&press_ctrl_shift(Keysym::KEY_L));
    let aux = aux_of(&result).expect("toggle should report its new state");
    assert!(aux.contains("ライブ変換"), "got {aux}");
}

#[test]
fn verbose_toggle_stays_hidden_when_disabled() {
    let mut e = engine_without_aux();
    let result = e.process_key(&press_ctrl_shift(Keysym::KEY_V));
    assert!(has_hide_aux(&result));
    assert!(aux_of(&result).is_none());
}
