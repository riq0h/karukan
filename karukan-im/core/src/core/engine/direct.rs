//! Fork: F6-F10 direct conversion (MS-IME / ATOK style).
//!
//! The function keys commit the current composition in a fixed form without
//! going through the model:
//!
//! | Key | Form |
//! |-----|------|
//! | F6  | hiragana |
//! | F7  | full-width katakana |
//! | F8  | half-width katakana |
//! | F9  | full-width alphanumeric |
//! | F10 | half-width alphanumeric |
//!
//! The reading (`input_buf.reading()` after settling) is the source for all
//! five. F9/F10 operate on the ASCII already in the reading — in the
//! element-array model the original keystrokes are not retained once a rule
//! fires, so `かな` cannot be reversed to `kana`. Typing latin text in
//! alphabet mode (Shift+letter) records it verbatim, which is the case
//! F9/F10 are for.

use super::*;

impl InputMethodEngine {
    /// F6: commit as hiragana.
    pub(super) fn direct_convert_hiragana(&mut self) -> EngineResult {
        self.commit_direct(|reading| karukan_engine::katakana_to_hiragana(reading))
    }

    /// F7: commit as full-width katakana.
    pub(super) fn direct_convert_katakana(&mut self) -> EngineResult {
        self.commit_direct(|reading| karukan_engine::hiragana_to_katakana(reading))
    }

    /// F8: commit as half-width katakana.
    pub(super) fn direct_convert_half_katakana(&mut self) -> EngineResult {
        self.commit_direct(|reading| {
            let katakana = karukan_engine::hiragana_to_katakana(reading);
            karukan_engine::kana::katakana_to_half_width(&katakana)
        })
    }

    /// F9: commit as full-width alphanumeric.
    pub(super) fn direct_convert_fullwidth(&mut self) -> EngineResult {
        self.commit_direct(|reading| {
            reading
                .chars()
                .map(karukan_engine::kana::ascii_to_fullwidth_char)
                .collect()
        })
    }

    /// F10: commit as half-width alphanumeric.
    pub(super) fn direct_convert_halfwidth(&mut self) -> EngineResult {
        self.commit_direct(|reading| {
            reading
                .chars()
                .map(karukan_engine::kana::fullwidth_to_ascii_char)
                .collect()
        })
    }

    /// Run a direct conversion from the Conversion state: drop back to the
    /// untouched composition first, so the reading — not the selected
    /// candidate — is what gets transformed.
    pub(super) fn direct_convert_from_conversion(
        &mut self,
        convert: impl FnOnce(&mut Self) -> EngineResult,
    ) -> EngineResult {
        self.set_composing_state();
        convert(self)
    }

    /// Settle the composition, run `transform` over the reading, and commit
    /// the result. No-op (not consumed) when the composition is empty, so an
    /// unused function key falls through to the application.
    fn commit_direct(&mut self, transform: impl FnOnce(&str) -> String) -> EngineResult {
        self.settle_romaji();
        let reading = self.input_buf.reading();
        if reading.is_empty() {
            return EngineResult::not_consumed();
        }
        let text = transform(&reading);
        self.end_composition();

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(Preedit::new()))
            .with_action(EngineAction::Commit(text))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::HideAuxText)
    }
}
