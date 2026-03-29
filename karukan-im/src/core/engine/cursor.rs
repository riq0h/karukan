//! Cursor movement and character deletion

use super::*;

impl InputMethodEngine {
    /// Common helper for cursor movement: flush romaji, clear live conversion, set new position.
    /// Also clears selection (plain cursor movement deselects).
    fn move_caret(&mut self, new_pos: usize) -> EngineResult {
        if !self.converters.romaji.buffer().is_empty() {
            self.flush_romaji_to_composed();
            self.converters.romaji.reset();
        }
        self.live.text.clear();
        self.input_buf.clear_selection();
        self.input_buf.cursor_pos = new_pos;
        let preedit = self.set_composing_state();
        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()))
    }

    /// Common helper for Shift+Arrow selection: flush romaji, set anchor if needed, move cursor.
    fn shift_select(&mut self, new_pos: usize) -> EngineResult {
        if !self.converters.romaji.buffer().is_empty() {
            self.flush_romaji_to_composed();
            self.converters.romaji.reset();
        }
        self.live.text.clear();
        // Set anchor at current position if not already set
        if self.input_buf.selection_anchor.is_none() {
            self.input_buf.selection_anchor = Some(self.input_buf.cursor_pos);
        }
        self.input_buf.cursor_pos = new_pos;
        let preedit = self.set_composing_state();
        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()))
    }

    /// Handle backspace in composing mode
    pub(super) fn backspace_composing(&mut self) -> EngineResult {
        // If romaji buffer is not empty, backspace from buffer (not from composed text)
        if !self.converters.romaji.buffer().is_empty() {
            let output_len_before = self.converters.romaji.output().chars().count();
            self.converters.romaji.backspace();
            let output_len_after = self.converters.romaji.output().chars().count();

            // If output shrank, a passthrough char was reclaimed into the buffer.
            // Remove it from input_buf too so it doesn't appear twice.
            if output_len_after < output_len_before && self.input_buf.cursor_pos > 0 {
                self.input_buf.remove_char_before_cursor();
            }

            if let Some(result) = self.try_reset_if_empty() {
                return result;
            }

            let preedit = self.set_composing_state();
            return EngineResult::consumed()
                .with_action(EngineAction::UpdatePreedit(preedit))
                .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()));
        }

        // Remove character before cursor from composed_hiragana
        if self.input_buf.cursor_pos > 0 {
            self.input_buf.remove_char_before_cursor();
        } else {
            // Nothing to delete
            return EngineResult::consumed();
        }

        // After deletion, if the character now before the cursor is an ASCII
        // lowercase letter (a passthrough consonant), reclaim it from input_buf
        // into the romaji buffer so it can combine with the next keystroke.
        // Example: "なmこ" → backspace "こ" → "なm" → reclaim 'm' into buffer
        //          → type 'a' → buffer "ma" → "ま" → "なま"
        if self.input_buf.cursor_pos > 0 && self.input_mode != InputMode::Alphabet {
            if let Some(prev_char) = self.input_buf.text.chars().nth(self.input_buf.cursor_pos - 1)
            {
                if prev_char.is_ascii_lowercase() {
                    self.input_buf.remove_char_before_cursor();
                    self.converters.romaji.reclaim_to_buffer(prev_char);
                }
            }
        }

        if let Some(result) = self.try_reset_if_empty() {
            return result;
        }

        self.refresh_input_state()
    }

    /// Move caret left within hiragana input
    pub(super) fn move_caret_left(&mut self) -> EngineResult {
        let new_pos = self.input_buf.cursor_pos.saturating_sub(1);
        self.move_caret(new_pos)
    }

    /// Move caret right within hiragana input
    pub(super) fn move_caret_right(&mut self) -> EngineResult {
        let total = self.input_buf.text.chars().count();
        let new_pos = (self.input_buf.cursor_pos + 1).min(total);
        self.move_caret(new_pos)
    }

    /// Handle delete key in hiragana mode
    pub(super) fn delete_composing(&mut self) -> EngineResult {
        // If romaji buffer is not empty, don't delete from composed (buffer is at cursor)
        if !self.converters.romaji.buffer().is_empty() {
            return EngineResult::consumed();
        }

        // Delete character at cursor position
        if self.input_buf.remove_char_at_cursor().is_none() {
            return EngineResult::consumed();
        }

        if let Some(result) = self.try_reset_if_empty() {
            return result;
        }

        self.refresh_input_state()
    }

    /// Move caret to start of input
    pub(super) fn move_caret_home(&mut self) -> EngineResult {
        self.move_caret(0)
    }

    /// Move caret to end of input
    pub(super) fn move_caret_end(&mut self) -> EngineResult {
        let total = self.input_buf.text.chars().count();
        self.move_caret(total)
    }

    /// Shift+Left: extend/shrink selection to the left
    pub(super) fn shift_select_left(&mut self) -> EngineResult {
        let new_pos = self.input_buf.cursor_pos.saturating_sub(1);
        self.shift_select(new_pos)
    }

    /// Shift+Right: extend/shrink selection to the right
    pub(super) fn shift_select_right(&mut self) -> EngineResult {
        let total = self.input_buf.text.chars().count();
        let new_pos = (self.input_buf.cursor_pos + 1).min(total);
        self.shift_select(new_pos)
    }

    /// Shift+Home: extend selection to the beginning
    pub(super) fn shift_select_home(&mut self) -> EngineResult {
        self.shift_select(0)
    }

    /// Shift+End: extend selection to the end
    pub(super) fn shift_select_end(&mut self) -> EngineResult {
        let total = self.input_buf.text.chars().count();
        self.shift_select(total)
    }
}
