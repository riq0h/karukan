//! Cursor movement and character deletion

use super::*;

impl InputMethodEngine {
    /// Common helper for cursor movement: clear live conversion and set the
    /// new display position. Nothing settles — unevaluated romaji stays
    /// live, so typing after a move can still combine with it. Moving does
    /// end a temporary alphabet word (the user left the word they were
    /// typing), so the next key is evaluated as romaji again.
    fn move_caret(&mut self, new_pos: impl FnOnce(&InputBuffer) -> usize) -> EngineResult {
        if self.mode.current() == InputMode::Alphabet {
            self.mode.exit_temporary();
        }
        self.live.shown = false;
        self.input_buf.set_cursor(new_pos(&self.input_buf));
        self.log_chunk_state("cursor");
        let preedit = self.set_composing_state();
        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()))
    }

    /// Handle backspace in composing mode
    pub(super) fn backspace_composing(&mut self) -> EngineResult {
        // Remove one display character before the cursor: a single-character
        // element vanishes whole, re-exposing the live element before it
        // (`ytko` → BS → `o` → 「yと」); きょ is truncated per character.
        let reading_before = self.input_buf.reading();
        if !self.edit_with_chunk_breaks(|e| e.input_buf.backspace(&e.converters.romaji)) {
            // Nothing to delete
            return EngineResult::consumed();
        }

        if let Some(result) = self.try_reset_if_empty() {
            return result;
        }

        // Reading unchanged (a live keystroke was popped): keep the
        // candidate window as-is
        if self.input_buf.reading() == reading_before {
            let preedit = self.set_composing_state();
            return EngineResult::consumed()
                .with_action(EngineAction::UpdatePreedit(preedit))
                .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()));
        }
        self.refresh_input_state()
    }

    /// Move caret left within the composition
    pub(super) fn move_caret_left(&mut self) -> EngineResult {
        self.move_caret(|buf| buf.cursor().saturating_sub(1))
    }

    /// Move caret right within the composition
    pub(super) fn move_caret_right(&mut self) -> EngineResult {
        self.move_caret(|buf| buf.cursor() + 1)
    }

    /// Handle delete key in composing mode
    pub(super) fn delete_composing(&mut self) -> EngineResult {
        if !self.edit_with_chunk_breaks(|e| e.input_buf.delete_at_cursor(&e.converters.romaji)) {
            return EngineResult::consumed();
        }

        if let Some(result) = self.try_reset_if_empty() {
            return result;
        }

        self.refresh_input_state()
    }

    /// Move caret to start of input
    pub(super) fn move_caret_home(&mut self) -> EngineResult {
        self.move_caret(|_| 0)
    }

    /// Move caret to end of input
    pub(super) fn move_caret_end(&mut self) -> EngineResult {
        self.move_caret(|buf| buf.char_count())
    }

    // --- Fork: Shift+Arrow selection for partial conversion ---------------

    /// Extend (or shrink) the selection to `new_pos`.
    ///
    /// Before selecting, any live-conversion display is baked into the
    /// buffer so the user selects within the text they can see. The original
    /// reading is kept in `partial.original_reading` so a kanji/katakana
    /// selection can be mapped back to its hiragana via character alignment.
    fn shift_select(&mut self, new_pos: impl FnOnce(&InputBuffer) -> usize) -> EngineResult {
        self.bake_live_for_selection();
        let pos = new_pos(&self.input_buf);
        self.input_buf.extend_selection_to(pos);
        let preedit = self.set_composing_state();
        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()))
    }

    /// Shift+Left: extend/shrink the selection one character left.
    pub(super) fn shift_select_left(&mut self) -> EngineResult {
        self.shift_select(|buf| buf.cursor().saturating_sub(1))
    }

    /// Shift+Right: extend/shrink the selection one character right.
    pub(super) fn shift_select_right(&mut self) -> EngineResult {
        self.shift_select(|buf| buf.cursor() + 1)
    }

    /// Shift+Home: select to the start of the composition.
    pub(super) fn shift_select_home(&mut self) -> EngineResult {
        self.shift_select(|_| 0)
    }

    /// Shift+End: select to the end of the composition.
    pub(super) fn shift_select_end(&mut self) -> EngineResult {
        self.shift_select(|buf| buf.char_count())
    }

    /// Bake the live-conversion text into the buffer so selection operates
    /// on the visible (converted) text rather than the hidden reading.
    /// Records the pre-bake reading so `start_conversion` can align a
    /// kanji selection back to its hiragana. No-op when nothing is shown.
    fn bake_live_for_selection(&mut self) {
        if !self.live.shown {
            return;
        }
        let live_text = self.live_text_with_pending();
        if live_text.is_empty() {
            return;
        }
        self.settle_romaji();
        let reading = self.input_buf.reading();
        if self.partial.original_reading.is_none() {
            self.partial.original_reading = Some(reading);
        }
        self.input_buf.replace_all_settled(&live_text);
        self.live.shown = false;
        self.chunks.clear();
        self.chunk_breaks.clear();
    }
}
