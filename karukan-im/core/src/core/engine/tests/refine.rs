//! Fork: typing during conversion keeps a selection the user made.
//!
//! Upstream drops back to the untouched composition on every refining
//! keystroke (incremental-search style). That discards a candidate the user
//! deliberately moved to, reconverts the whole reading, and does not even
//! record the choice. Here an explicit selection is baked into the
//! composition first; an untouched list still refines as upstream does.

use super::*;

/// Conversion over 「あい」 with three model candidates, so the selection
/// has somewhere to move to. No converter is loaded, so the seeded cache
/// stands in for the model.
fn engine_in_conversion() -> InputMethodEngine {
    let mut engine = InputMethodEngine::new();
    seed_model_cache(&mut engine, "アイ", "", &["合い", "愛", "藍"]);
    engine.process_key(&press('a'));
    engine.process_key(&press('i'));
    engine.process_key(&press_key(Keysym::SPACE));
    assert!(matches!(engine.state(), InputState::Conversion { .. }));
    engine
}

fn preedit_of(result: &EngineResult) -> Option<String> {
    result.actions.iter().rev().find_map(|a| match a {
        EngineAction::UpdatePreedit(p) => Some(p.text().to_string()),
        _ => None,
    })
}

fn committed(result: &EngineResult) -> Option<String> {
    result.actions.iter().find_map(|a| match a {
        EngineAction::Commit(text) => Some(text.clone()),
        _ => None,
    })
}

#[test]
fn typing_after_moving_the_selection_bakes_it() {
    // 愛 was chosen deliberately, so the next keystroke must not throw it
    // away: it settles into the composition and typing continues after it.
    let mut engine = engine_in_conversion();
    engine.process_key(&press_key(Keysym::DOWN));
    assert_eq!(
        engine.candidates().and_then(|c| c.selected_text()),
        Some("愛")
    );

    let result = engine.process_key(&press('u'));
    assert_eq!(preedit_of(&result).as_deref(), Some("愛う"));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
}

#[test]
fn typing_without_moving_the_selection_still_refines() {
    // Nothing was chosen — the list is just open — so the upstream
    // incremental behaviour stands and the reading grows.
    let mut engine = engine_in_conversion();

    let result = engine.process_key(&press('u'));
    assert_eq!(preedit_of(&result).as_deref(), Some("あいう"));
}

#[test]
fn a_baked_selection_survives_to_the_commit() {
    // The whole composition commits at Enter, baked part included; nothing
    // reached the application before that.
    let mut engine = engine_in_conversion();
    engine.process_key(&press_key(Keysym::DOWN));
    let result = engine.process_key(&press('u'));
    assert!(
        committed(&result).is_none(),
        "nothing commits on a refining key"
    );

    let result = engine.process_key(&press_key(Keysym::RETURN));
    assert_eq!(committed(&result).as_deref(), Some("愛う"));
}

#[test]
fn the_commit_learns_against_the_original_reading() {
    // The baked surface is no longer the reading, so the learning record
    // has to use the hiragana from before the bake.
    let mut engine = engine_in_conversion();
    engine.learning = Some(LearningCache::new(LearningConfig::default()));
    engine.process_key(&press_key(Keysym::DOWN));
    engine.process_key(&press('u'));
    engine.process_key(&press_key(Keysym::RETURN));

    let cache = engine.learning.as_ref().expect("learning cache");
    let learned: Vec<String> = cache
        .lookup("あいう")
        .into_iter()
        .map(|(surface, _score)| surface)
        .collect();
    assert!(
        learned.iter().any(|s| s == "愛う"),
        "「あいう」→「愛う」 must be recorded, got {learned:?}"
    );
}

#[test]
fn backspace_still_cancels_instead_of_baking() {
    // Baking then deleting a character would eat part of the kanji that was
    // just settled, so Backspace keeps its old meaning.
    let mut engine = engine_in_conversion();
    engine.process_key(&press_key(Keysym::DOWN));

    let result = engine.process_key(&press_key(Keysym::BACKSPACE));
    assert_eq!(preedit_of(&result).as_deref(), Some("あい"));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
}

