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
fn typing_bakes_the_head_of_the_list_too() {
    // The head is a choice like any other: when the candidate the user
    // wanted is already first — which is what learning is for — Space lands
    // on it without the cursor ever moving. Requiring a move would leave
    // exactly that case unbaked.
    let mut engine = engine_in_conversion();
    assert_eq!(
        engine.candidates().and_then(|c| c.selected_text()),
        Some("合い")
    );

    let result = engine.process_key(&press('u'));
    assert_eq!(preedit_of(&result).as_deref(), Some("合いう"));
}

#[test]
fn typing_while_composing_is_untouched() {
    // Without Space there is no conversion window and nothing to keep, so
    // the reading grows as it always did — this is where the incremental
    // feel lives, and live conversion rewrites the preedit as you type.
    let mut engine = InputMethodEngine::new();
    seed_model_cache(&mut engine, "アイ", "", &["合い"]);
    engine.process_key(&press('a'));
    let result = engine.process_key(&press('i'));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
    assert_eq!(preedit_of(&result).as_deref(), Some("あい"));
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

#[test]
fn a_baked_surface_is_not_reconverted() {
    // The bake settles a surface, but it stays in the buffer and the buffer
    // is what live conversion reads. Unless the settled run is held out of
    // the model, the next keystroke feeds it back in and the model rewrites
    // it — 「サンテん」 comes back as 「さんてん」, undoing the choice.
    let mut engine = InputMethodEngine::new();
    engine.live.enabled = true;
    seed_model_cache(&mut engine, "アイ", "", &["藍"]);
    // Only the tail may reach the model, with the settled 藍 as its context.
    seed_model_cache(&mut engine, "ウ", "藍", &["卯"]);
    // The entry that answers if the settled 藍 is fed back in.
    seed_model_cache(&mut engine, "藍ウ", "", &["あいう"]);

    engine.process_key(&press('a'));
    engine.process_key(&press('i'));
    engine.process_key(&press_key(Keysym::SPACE));
    assert_eq!(
        engine.candidates().and_then(|c| c.selected_text()),
        Some("藍")
    );

    let result = engine.process_key(&press('u'));
    assert_eq!(preedit_of(&result).as_deref(), Some("藍卯"));

    let result = engine.process_key(&press_key(Keysym::RETURN));
    assert_eq!(committed(&result).as_deref(), Some("藍卯"));
}

