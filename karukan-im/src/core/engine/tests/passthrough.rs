use super::*;

#[test]
fn test_passthrough_no_double_counting() {
    // Regression test: typing '<' twice should produce "＜＜", not "＜＜＜".
    // '<' is converted to full-width '＜' by romaji rules.
    let mut engine = InputMethodEngine::new();

    // Type '<' in empty state → enters composing with "＜"
    engine.process_key(&press('<'));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
    assert_eq!(engine.preedit().unwrap().text(), "＜");

    // Type '<' again → appends another "＜"
    engine.process_key(&press('<'));
    assert_eq!(engine.preedit().unwrap().text(), "＜＜");
}

#[test]
fn test_thx_chars_not_lost() {
    // Regression test: typing "thx" should show "thx" in preedit, not lose chars.
    // The converter recursively passes through 't' and 'h', keeps 'x' in buffer.
    // The engine must pick up ALL chars from output delta, not just the last PassThrough.
    let mut engine = InputMethodEngine::new();

    // Type 't'
    engine.process_key(&press('t'));
    assert_eq!(engine.preedit().unwrap().text(), "t");

    // Type 'h'
    engine.process_key(&press('h'));
    assert_eq!(engine.preedit().unwrap().text(), "th");

    // Type 'x' → converter breaks "thx" into output="th" + buffer="x"
    engine.process_key(&press('x'));
    let preedit = engine.preedit().unwrap().text().to_string();
    assert_eq!(preedit, "thx", "Should show 'thx', not lose characters");

    // Commit should produce "thx"
    let result = engine.process_key(&press_key(Keysym::RETURN));
    let has_commit = result
        .actions
        .iter()
        .any(|a| matches!(a, EngineAction::Commit(text) if text == "thx"));
    assert!(has_commit, "Should commit 'thx'");
}

#[test]
fn test_passthrough_after_hiragana_no_double() {
    // Typing hiragana then '<' should append exactly one '<', not two
    let mut engine = InputMethodEngine::new();

    // Type "あ" (a)
    engine.process_key(&press('a'));
    assert_eq!(engine.preedit().unwrap().text(), "あ");

    // Type '<' while in hiragana input state → converted to full-width '＜'
    engine.process_key(&press('<'));
    let preedit = engine.preedit().unwrap().text().to_string();
    assert_eq!(preedit, "あ＜", "Should be 'あ＜', not 'あ＜＜'");

    // Type another '<'
    engine.process_key(&press('<'));
    let preedit = engine.preedit().unwrap().text().to_string();
    assert_eq!(preedit, "あ＜＜", "Should be 'あ＜＜', not 'あ＜＜＜'");
}

#[test]
fn test_digit_starts_input_mode() {
    // Typing a digit from Empty state should enter Composing,
    // not commit immediately. This allows typing "20世紀" etc.
    let mut engine = InputMethodEngine::new();

    // Type '2' from Empty state
    let result = engine.process_key(&press('2'));
    assert!(result.consumed);
    assert!(
        matches!(engine.state(), InputState::Composing { .. }),
        "Digit should enter Composing, not stay Empty"
    );
    assert_eq!(engine.preedit().unwrap().text(), "2");

    // Type '0'
    engine.process_key(&press('0'));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
    assert_eq!(engine.preedit().unwrap().text(), "20");

    // Type "seiki" -> "20せいき"
    engine.process_key(&press('s'));
    engine.process_key(&press('e'));
    engine.process_key(&press('i'));
    engine.process_key(&press('k'));
    engine.process_key(&press('i'));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
    assert_eq!(engine.preedit().unwrap().text(), "20せいき");

    // Commit should produce "20せいき"
    let result = engine.process_key(&press_key(Keysym::RETURN));
    let has_commit = result
        .actions
        .iter()
        .any(|a| matches!(a, EngineAction::Commit(text) if text == "20せいき"));
    assert!(has_commit, "Should commit '20せいき'");
}

#[test]
fn test_digit_in_middle_of_hiragana() {
    // Typing a digit while in Composing should keep the preedit
    let mut engine = InputMethodEngine::new();

    // Type "あ" then "2"
    engine.process_key(&press('a'));
    assert_eq!(engine.preedit().unwrap().text(), "あ");

    engine.process_key(&press('2'));
    assert!(matches!(engine.state(), InputState::Composing { .. }));
    assert_eq!(engine.preedit().unwrap().text(), "あ2");
}

#[test]
fn test_backspace_reclaims_passthrough_for_romaji() {
    // Typing "hs" passes 'h' through, then backspacing 's' should reclaim 'h'
    // into the romaji buffer so that typing 'a' produces "は", not "hあ".
    let mut engine = InputMethodEngine::new();

    // Type 'h' → buffered
    engine.process_key(&press('h'));
    assert_eq!(engine.preedit().unwrap().text(), "h");

    // Type 's' → 'h' passes through (no "hs" rule), 's' buffered
    engine.process_key(&press('s'));
    assert_eq!(engine.preedit().unwrap().text(), "hs");

    // Backspace → remove 's', reclaim 'h' back to buffer
    engine.process_key(&press_key(Keysym::BACKSPACE));
    assert_eq!(engine.preedit().unwrap().text(), "h");

    // Type 'a' → 'h' + 'a' = "は"
    engine.process_key(&press('a'));
    assert_eq!(
        engine.preedit().unwrap().text(),
        "は",
        "Should produce は, not hあ"
    );
}

#[test]
fn test_backspace_reclaim_with_preceding_hiragana() {
    // Same scenario but with hiragana already in the buffer
    let mut engine = InputMethodEngine::new();

    // Type "ka" → "か"
    engine.process_key(&press('k'));
    engine.process_key(&press('a'));
    assert_eq!(engine.preedit().unwrap().text(), "か");

    // Type "hs" → "かhs" (h passed through, s buffered)
    engine.process_key(&press('h'));
    engine.process_key(&press('s'));
    assert_eq!(engine.preedit().unwrap().text(), "かhs");

    // Backspace → "かh" (s removed, h reclaimed)
    engine.process_key(&press_key(Keysym::BACKSPACE));
    assert_eq!(engine.preedit().unwrap().text(), "かh");

    // Type 'a' → "かは"
    engine.process_key(&press('a'));
    assert_eq!(engine.preedit().unwrap().text(), "かは");
}

#[test]
fn test_backspace_reclaims_passthrough_after_hiragana_deletion() {
    // Typing "namko" produces "なmこ" (m passed through, ko → こ).
    // Deleting "こ" should reclaim 'm' from input_buf into romaji buffer
    // so that typing "a" produces "なま", not "なmあ".
    let mut engine = InputMethodEngine::new();

    // Type "namko" → "なmこ"
    for ch in "namko".chars() {
        engine.process_key(&press(ch));
    }
    assert_eq!(engine.preedit().unwrap().text(), "なmこ");

    // Backspace → delete "こ", reclaim 'm' into buffer → display "なm"
    engine.process_key(&press_key(Keysym::BACKSPACE));
    assert_eq!(engine.preedit().unwrap().text(), "なm");

    // Type 'a' → buffer "ma" → "ま" → "なま"
    engine.process_key(&press('a'));
    assert_eq!(
        engine.preedit().unwrap().text(),
        "なま",
        "Should produce なま, not なmあ"
    );

    // Continue with "ko" → "なまこ"
    engine.process_key(&press('k'));
    engine.process_key(&press('o'));
    assert_eq!(engine.preedit().unwrap().text(), "なまこ");
}

#[test]
fn test_backspace_reclaim_n_before_consonant() {
    // Typing "ns" triggers n-before-consonant (ん + s).
    // Backspacing 's' from romaji buffer reclaims 'ん' → 'n'.
    // Then typing 'a' gives "な", not "んあ".
    let mut engine = InputMethodEngine::new();

    // Type "ns" → "ん" + buffer="s"
    engine.process_key(&press('n'));
    engine.process_key(&press('s'));
    assert_eq!(engine.preedit().unwrap().text(), "んs");

    // Backspace → reclaim ん → n in buffer
    engine.process_key(&press_key(Keysym::BACKSPACE));
    assert_eq!(engine.preedit().unwrap().text(), "n");

    // Type 'a' → "な"
    engine.process_key(&press('a'));
    assert_eq!(
        engine.preedit().unwrap().text(),
        "な",
        "Should produce な, not んあ"
    );
}
