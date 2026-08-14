//! Fork: selection-based partial conversion (Shift+Arrow → Space).
//!
//! Shift+Arrow selects a span of the composition; Space converts only that
//! span. Committing (Enter) bakes the result back into the composition
//! instead of committing to the application, so other spans can be converted
//! next. A final Enter with nothing selected commits the whole buffer.
//!
//! **Re-converting already-converted text.** The selection can cover kanji
//! or katakana that a previous conversion (or live conversion) produced. The
//! model needs hiragana, so the surface is aligned character-by-character
//! against `partial.original_reading` — the composition's reading before any
//! result was baked in — and the corresponding reading span is converted
//! instead. Kana in the surface act as anchors; a run of kanji between two
//! anchors is atomic, so `会議室` re-converts as a whole but not as `会議`
//! plus `室`.

use super::*;

impl InputMethodEngine {
    /// Convert only the selected span, parking the rest in
    /// `partial.remaining`.
    pub(super) fn start_partial_conversion(&mut self, learning: LearningLookup) -> EngineResult {
        self.settle_romaji();
        let Some((sel_start, sel_end)) = self.input_buf.selection_range() else {
            return self.start_conversion(learning);
        };
        let full = self.input_buf.reading();

        let (reading, before, after) = self.resolve_selection(&full, sel_start, sel_end);
        self.input_buf.clear_selection();
        self.live.shown = false;

        if reading.is_empty() {
            return EngineResult::consumed();
        }

        self.partial.remaining = Some((before, after));

        let candidates = self.build_conversion_candidates(
            &reading,
            &reading,
            "",
            self.config.num_candidates,
            learning,
        );
        if candidates.is_empty() {
            self.partial.remaining = None;
            let preedit = self.set_composing_state();
            return EngineResult::consumed().with_action(EngineAction::UpdatePreedit(preedit));
        }

        let candidate_list = Self::to_conversion_candidate_list(candidates, &reading);
        self.enter_conversion_state(&reading, candidate_list)
    }

    /// Resolve a selection into `(reading_to_convert, before, after)`.
    ///
    /// A pure-hiragana selection is its own reading. Otherwise the surface is
    /// aligned against `partial.original_reading` to recover the hiragana,
    /// and the selection is widened to whole alignment segments so a kanji
    /// compound is never split mid-way.
    fn resolve_selection(
        &self,
        full: &str,
        sel_start: usize,
        sel_end: usize,
    ) -> (String, String, String) {
        let selected: String = full
            .chars()
            .skip(sel_start)
            .take(sel_end - sel_start)
            .collect();
        let take_span = |start: usize, end: usize| {
            let before: String = full.chars().take(start).collect();
            let after: String = full.chars().skip(end).collect();
            (before, after)
        };

        let pure_hiragana = !selected.is_empty() && selected.chars().all(is_hiragana_char);
        let Some(original) = self.partial.original_reading.as_ref().filter(|_| !pure_hiragana)
        else {
            let (before, after) = take_span(sel_start, sel_end);
            return (selected, before, after);
        };

        let segments = karukan_engine::align::align(full, original);
        let (r_start, r_end, wide_start, wide_end) =
            karukan_engine::align::map_range(&segments, sel_start, sel_end);
        let reading: String = original
            .chars()
            .skip(r_start)
            .take(r_end.saturating_sub(r_start))
            .collect();
        let (before, after) = take_span(wide_start, wide_end);
        (reading, before, after)
    }

    /// Bake a committed partial conversion back into the composition: the
    /// buffer becomes `before + converted + after` and the engine returns to
    /// Composing so the user can convert another span. The pre-bake reading
    /// is preserved for aligning the next selection.
    pub(super) fn bake_partial_conversion(&mut self, converted: &str) -> EngineResult {
        let Some((before, after)) = self.partial.remaining.take() else {
            return EngineResult::not_consumed();
        };
        // Remember the reading behind the text being replaced, so a later
        // selection over this kanji can still be mapped back.
        if self.partial.original_reading.is_none() {
            self.partial.original_reading = Some(self.input_buf.reading());
        }

        let baked = format!("{before}{converted}{after}");
        if baked.is_empty() {
            self.end_composition();
            return EngineResult::consumed()
                .with_action(EngineAction::UpdatePreedit(Preedit::new()))
                .with_action(EngineAction::HideCandidates)
                .with_action(EngineAction::HideAuxText);
        }

        self.input_buf.replace_all_settled(&baked);
        self.live.shown = false;
        self.chunks.clear();
        self.chunk_breaks.clear();

        let preedit = self.set_composing_state();
        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()))
    }

    /// Whether a partial conversion is in flight (Enter should bake rather
    /// than commit).
    pub(super) fn has_partial_conversion(&self) -> bool {
        self.partial.remaining.is_some()
    }

    /// Shift+Arrow from the Conversion state: bake the selected candidate
    /// into the composition, then start selecting inside it. `from_start`
    /// puts the caret at the beginning (Shift+Right grows rightward);
    /// otherwise it sits at the end (Shift+Left grows leftward).
    pub(super) fn conversion_to_selection(&mut self, from_start: bool) -> EngineResult {
        self.bake_conversion_for_selection();
        let pos = if from_start {
            0
        } else {
            self.input_buf.char_count()
        };
        self.input_buf.set_cursor(pos);
        if from_start {
            self.shift_select_right()
        } else {
            self.shift_select_left()
        }
    }

    /// Shift+Home from Conversion: bake, then select to the start.
    pub(super) fn conversion_to_selection_home(&mut self) -> EngineResult {
        self.bake_conversion_for_selection();
        self.input_buf.set_cursor(self.input_buf.char_count());
        self.shift_select_home()
    }

    /// Shift+End from Conversion: bake, then select to the end.
    pub(super) fn conversion_to_selection_end(&mut self) -> EngineResult {
        self.bake_conversion_for_selection();
        self.input_buf.set_cursor(0);
        self.shift_select_end()
    }

    /// Fold the selected candidate (plus any parked before/after text) back
    /// into the composition and return to Composing, keeping the pre-bake
    /// reading for alignment.
    fn bake_conversion_for_selection(&mut self) {
        let Some((text, _)) = self.selected_conversion_info() else {
            return;
        };
        if self.partial.original_reading.is_none() {
            self.partial.original_reading = Some(self.input_buf.reading());
        }
        let baked = match self.partial.remaining.take() {
            Some((before, after)) => format!("{before}{text}{after}"),
            None => text,
        };
        self.input_buf.replace_all_settled(&baked);
        self.live.shown = false;
        self.chunks.clear();
        self.chunk_breaks.clear();
        self.set_composing_state();
    }

    /// The reading to learn against when the whole composition is finally
    /// committed: the pre-bake hiragana if any partial conversion happened,
    /// otherwise the current reading.
    pub(super) fn take_learning_reading(&mut self, fallback: String) -> String {
        self.partial.original_reading.take().unwrap_or(fallback)
    }
}

/// Hiragana (excluding the combining marks in the block's tail), plus the
/// prolonged sound mark that shows up inside kana readings.
fn is_hiragana_char(c: char) -> bool {
    ('\u{3041}'..='\u{3096}').contains(&c) || c == '\u{30FC}'
}
