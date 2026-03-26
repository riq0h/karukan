//! Conversion state handling (candidates, segments, commit)

use std::collections::HashSet;
use std::time::Instant;

use tracing::debug;

use super::*;

/// Maximum number of learning candidates to show
const MAX_LEARNING_CANDIDATES: usize = 3;

/// Helper for building a deduplicated list of conversion candidates.
struct CandidateBuilder {
    candidates: Vec<AnnotatedCandidate>,
    seen: HashSet<String>,
}

impl CandidateBuilder {
    fn new() -> Self {
        Self {
            candidates: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Push a candidate if its text hasn't been seen yet.
    fn push_if_new(&mut self, text: String, source: CandidateSource, reading: Option<String>) {
        if self.seen.insert(text.clone()) {
            self.candidates.push(AnnotatedCandidate {
                text,
                source,
                reading,
            });
        }
    }

    /// Push a pre-built `AnnotatedCandidate` if its text hasn't been seen yet.
    fn push_annotated_if_new(&mut self, ac: AnnotatedCandidate) {
        if self.seen.insert(ac.text.clone()) {
            self.candidates.push(ac);
        }
    }

    fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    fn into_candidates(self) -> Vec<AnnotatedCandidate> {
        self.candidates
    }
}

impl InputMethodEngine {
    /// Run kana-kanji conversion for a reading via llama.cpp model.
    ///
    /// Determines the conversion strategy (main model, light model, or parallel beam),
    /// dispatches to the appropriate model(s), measures latency, and records which model was used.
    fn run_kana_kanji_conversion(&mut self, reading: &str, num_candidates: usize) -> Vec<String> {
        let Some(converter) = self.converters.kanji.as_ref() else {
            return vec![];
        };
        let katakana = karukan_engine::kana::hiragana_to_katakana(reading);
        let api_context = self.truncate_context_for_api();
        let main_model_name = converter.model_display_name().to_string();

        let strategy = self.determine_strategy(reading, num_candidates);
        debug!(
            "convert: reading=\"{}\" api_context=\"{}\" candidates={} strategy={:?}",
            reading, api_context, num_candidates, strategy
        );

        let start = Instant::now();

        let candidates = match &strategy {
            ConversionStrategy::ParallelBeam { beam_width } => {
                let Some(light_converter) = self.converters.light_kanji.as_ref() else {
                    return vec![];
                };
                let bw = *beam_width;
                let (default_top1, light_candidates) = std::thread::scope(|s| {
                    let h_default = s.spawn(|| {
                        converter
                            .convert(&katakana, &api_context, 1)
                            .unwrap_or_default()
                    });
                    let h_beam = s.spawn(|| {
                        light_converter
                            .convert(&katakana, &api_context, bw)
                            .unwrap_or_default()
                    });
                    (
                        h_default.join().unwrap_or_default(),
                        h_beam.join().unwrap_or_default(),
                    )
                });
                Self::merge_candidates_dedup(default_top1, light_candidates, bw)
            }
            ConversionStrategy::LightModelOnly => {
                let Some(light_converter) = self.converters.light_kanji.as_ref() else {
                    return vec![];
                };
                light_converter
                    .convert(&katakana, &api_context, 1)
                    .unwrap_or_default()
            }
            ConversionStrategy::MainModelOnly => converter
                .convert(&katakana, &api_context, 1)
                .unwrap_or_default(),
            ConversionStrategy::MainModelBeam { beam_width } => converter
                .convert(&katakana, &api_context, *beam_width)
                .unwrap_or_default(),
        };

        self.metrics.conversion_ms = start.elapsed().as_millis() as u64;
        self.update_adaptive_model_flag(&strategy);

        self.metrics.model_name = match &strategy {
            ConversionStrategy::ParallelBeam { .. } => {
                let light_name = self
                    .converters
                    .light_kanji
                    .as_ref()
                    .map(|c| c.model_display_name().to_string())
                    .unwrap_or_default();
                format!("{}+{}", main_model_name, light_name)
            }
            ConversionStrategy::LightModelOnly => self
                .converters
                .light_kanji
                .as_ref()
                .map(|c| c.model_display_name().to_string())
                .unwrap_or(main_model_name),
            ConversionStrategy::MainModelOnly | ConversionStrategy::MainModelBeam { .. } => {
                main_model_name
            }
        };

        candidates
    }

    /// Run inference for auto-suggest and return candidates (raw strings).
    /// Initializes the kanji converter lazily. Falls back to the reading itself
    /// if no candidates are produced.
    pub(super) fn run_auto_suggest(&mut self, reading: &str, num_candidates: usize) -> Vec<String> {
        // Ensure kanji converter is initialized
        if self.converters.kanji.is_none()
            && let Err(e) = self.init_kanji_converter()
        {
            debug!("Failed to initialize kanji converter: {}", e);
            return vec![reading.to_string()];
        }

        let candidates = self.run_kana_kanji_conversion(reading, num_candidates);

        if candidates.is_empty() {
            vec![reading.to_string()]
        } else {
            candidates
        }
    }

    /// Start conversion using the current live-conversion result + dictionary candidates.
    ///
    /// Called when DOWN/TAB is pressed during live conversion.  Instead of
    /// Start kanji conversion
    pub(super) fn start_conversion(&mut self) -> EngineResult {
        // Flush any remaining romaji into composed_hiragana
        self.flush_romaji_to_composed();

        let full_text = self.input_buf.text.clone();

        // Selection-based partial conversion: convert only the selected range
        let (reading, remaining) = if let Some((sel_start, sel_end)) =
            self.input_buf.selection_range()
        {
            let before: String = full_text.chars().take(sel_start).collect();
            let selected: String = full_text
                .chars()
                .skip(sel_start)
                .take(sel_end - sel_start)
                .collect();
            let after: String = full_text.chars().skip(sel_end).collect();
            (selected, Some((before, after)))
        } else {
            (full_text, None)
        };
        self.remaining_after_conversion = remaining;
        self.input_buf.clear_selection();

        // Save auto-suggest/live conversion result before clearing state.
        // This ensures the candidate that was displayed during input is preserved
        // in the conversion candidate list even if the re-inference uses a different strategy.
        let prev_suggest_text = std::mem::take(&mut self.live.text);

        self.converters.romaji.reset();
        self.input_buf.cursor_pos = 0;

        if reading.is_empty() {
            return EngineResult::consumed();
        }

        // Get candidates from kanji converter (use full num_candidates for explicit conversion)
        let mut candidates = self.build_conversion_candidates(&reading, self.config.num_candidates);

        // If the previous auto-suggest result is not in the new candidates, insert it at the top
        // so it doesn't disappear when the conversion strategy changes.
        let seen: HashSet<&str> = candidates.iter().map(|c| c.text.as_str()).collect();
        if !prev_suggest_text.is_empty()
            && prev_suggest_text != reading
            && !seen.contains(prev_suggest_text.as_str())
        {
            candidates.insert(
                0,
                AnnotatedCandidate {
                    text: prev_suggest_text,
                    source: CandidateSource::Model,
                    reading: None,
                },
            );
        }

        if candidates.is_empty() {
            // No candidates, stay in hiragana mode
            let preedit = Preedit::with_text_underlined(&reading);
            self.state = InputState::Composing {
                preedit: preedit.clone(),
                romaji_buffer: String::new(),
            };
            return EngineResult::consumed().with_action(EngineAction::UpdatePreedit(preedit));
        }

        // Create candidate list with reading and source annotation
        let candidate_list = CandidateList::new(
            candidates
                .into_iter()
                .enumerate()
                .map(|(i, ac)| {
                    let label = ac.source.label();
                    let cand_reading = ac.reading.unwrap_or_else(|| reading.clone());
                    let mut c = if label.is_empty() {
                        Candidate::with_reading(&ac.text, &cand_reading)
                    } else {
                        Candidate {
                            text: ac.text,
                            reading: Some(cand_reading),
                            annotation: Some(label.to_string()),
                            index: 0,
                        }
                    };
                    c.index = i;
                    c
                })
                .collect(),
        );
        self.enter_conversion_state(&reading, candidate_list)
    }

    /// Transition to Conversion state with the given reading and candidate list.
    ///
    /// Sets up the preedit (highlighted selected text), updates the state, and
    /// returns an EngineResult with preedit, candidates, and aux text actions.
    fn enter_conversion_state(&mut self, reading: &str, candidates: CandidateList) -> EngineResult {
        let selected_text = candidates.selected_text().unwrap_or(reading).to_string();

        let preedit = Preedit::from_segments(
            vec![PreeditSegment::highlighted(&selected_text)],
            selected_text.chars().count(),
        );

        self.state = InputState::Conversion {
            preedit: preedit.clone(),
            candidates: candidates.clone(),
        };

        self.conversion_space_count = 1;
        let threshold = self.config.candidate_window_threshold;
        let show = threshold == 0 || self.conversion_space_count >= threshold;

        let mut result = EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit));
        if show {
            result = result
                .with_action(EngineAction::ShowCandidates(candidates.clone()))
                .with_action(EngineAction::UpdateAuxText(
                    self.format_aux_conversion_with_page(reading, Some(&candidates)),
                ));
        } else {
            result = result
                .with_action(EngineAction::HideCandidates)
                .with_action(EngineAction::HideAuxText);
        }
        result
    }

    /// Search user and system dictionaries for candidates matching a reading.
    ///
    /// User dictionary results come first (higher priority), then system dictionary
    /// results sorted by score. Duplicates are removed via HashSet.
    fn search_dictionaries(&self, reading: &str, limit: usize) -> Vec<AnnotatedCandidate> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        // User dictionary (higher priority)
        if let Some(dict) = &self.dicts.user
            && let Some(result) = dict.exact_match_search(reading)
        {
            for cand in result.candidates {
                if candidates.len() >= limit {
                    break;
                }
                if seen.insert(cand.surface.clone()) {
                    candidates.push(AnnotatedCandidate {
                        text: cand.surface.clone(),
                        source: CandidateSource::UserDictionary,
                        reading: None,
                    });
                }
            }
        }

        // System dictionary (sorted by score)
        if let Some(dict) = &self.dicts.system
            && let Some(result) = dict.exact_match_search(reading)
        {
            let mut dict_candidates: Vec<_> = result.candidates.to_vec();
            dict_candidates.sort_by(|a, b| a.score.total_cmp(&b.score));
            for cand in dict_candidates {
                if candidates.len() >= limit {
                    break;
                }
                if seen.insert(cand.surface.clone()) {
                    candidates.push(AnnotatedCandidate {
                        text: cand.surface,
                        source: CandidateSource::Dictionary,
                        reading: None,
                    });
                }
            }
        }

        candidates
    }

    /// Build conversion candidates for a reading from multiple sources.
    ///
    /// Combines learning cache, dictionaries, and model inference results
    /// with deduplication. Uses dynamic candidate count based on input token
    /// count for performance.
    ///
    /// Priority: Learning → User Dictionary → Model → System Dictionary → Fallback
    pub(super) fn build_conversion_candidates(
        &mut self,
        reading: &str,
        num_candidates: usize,
    ) -> Vec<AnnotatedCandidate> {
        // Ensure kanji converter is initialized
        if self.converters.kanji.is_none()
            && let Err(e) = self.init_kanji_converter()
        {
            debug!("Failed to initialize kanji converter: {}", e);
            return vec![AnnotatedCandidate {
                text: reading.to_string(),
                source: CandidateSource::Fallback,
                reading: None,
            }];
        }

        let candidates = self.run_kana_kanji_conversion(reading, num_candidates);

        let hiragana = reading.to_string();
        let katakana = Self::hiragana_to_katakana(reading);

        // Priority: Learning → Model → System Dictionary → User Dictionary → Fallback
        let mut builder = CandidateBuilder::new();

        // 1. Learning cache candidates (highest priority, exact match only)
        //    Prefix (predictive) matches are excluded during explicit conversion
        //    to avoid suggesting overly long candidates like "変換ウインドウ"
        //    for a reading of "へんかん". Prefix matches are still used in auto-suggest.
        for c in self.lookup_learning_exact(reading) {
            // Force-insert learning candidates (always included even if duplicate text)
            builder.seen.insert(c.text.clone());
            builder.candidates.push(AnnotatedCandidate {
                text: c.text,
                source: CandidateSource::Learning,
                // Exact matches have reading == input reading; use None to avoid redundancy
                reading: c.reading.filter(|r| r != reading),
            });
        }

        // 2. Model inference results (filter out overly long candidates)
        //    Kana-kanji conversion should produce output ≤ reading length in chars.
        //    Longer output typically means the model appended extra words
        //    (e.g. "へんかん" → "変換ウインドウ").
        let reading_len = reading.chars().count();
        if candidates.is_empty() {
            if builder.is_empty() {
                builder.push_if_new(hiragana.clone(), CandidateSource::Fallback, None);
            }
        } else {
            for text in candidates {
                if text.chars().count() <= reading_len {
                    builder.push_if_new(text, CandidateSource::Model, None);
                }
            }
        }

        // 3. Dictionary candidates
        let dict_results = self.search_dictionaries(reading, usize::MAX);

        // 3a. System dictionary
        for ac in &dict_results {
            if ac.source == CandidateSource::Dictionary {
                builder.push_annotated_if_new(ac.clone());
            }
        }

        // 3b. User dictionary (kaomoji/emoji etc. — lowest priority among dictionaries)
        for ac in dict_results {
            if ac.source == CandidateSource::UserDictionary {
                builder.push_annotated_if_new(ac);
            }
        }

        // 4. Append hiragana/katakana fallback if not already present
        builder.push_if_new(hiragana, CandidateSource::Fallback, None);
        builder.push_if_new(katakana, CandidateSource::Fallback, None);

        builder.into_candidates()
    }

    /// Look up learning cache candidates for a reading (exact match only, max 3).
    ///
    /// Used during explicit conversion (Space key) to avoid overly long prefix-match
    /// candidates. For auto-suggest display, use `lookup_learning_candidates` instead.
    fn lookup_learning_exact(&self, reading: &str) -> Vec<Candidate> {
        let Some(cache) = &self.learning else {
            return vec![];
        };
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut seen = HashSet::new();
        let label = CandidateSource::Learning.label().to_string();

        for (surface, _score) in cache.lookup(reading) {
            if candidates.len() >= MAX_LEARNING_CANDIDATES {
                break;
            }
            if seen.insert(surface.clone()) {
                candidates.push(Candidate {
                    text: surface,
                    reading: Some(reading.to_string()),
                    annotation: Some(label.clone()),
                    index: candidates.len(),
                });
            }
        }

        candidates
    }

    /// Look up learning cache candidates for a reading (exact + prefix match, max 3).
    ///
    /// Returns candidates from the learning cache suitable for auto-suggest display.
    pub(super) fn lookup_learning_candidates(&self, reading: &str) -> Vec<Candidate> {
        let Some(cache) = &self.learning else {
            return vec![];
        };
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut seen = HashSet::new();
        let label = CandidateSource::Learning.label().to_string();

        // Exact match
        for (surface, _score) in cache.lookup(reading) {
            if candidates.len() >= MAX_LEARNING_CANDIDATES {
                break;
            }
            if seen.insert(surface.clone()) {
                candidates.push(Candidate {
                    text: surface,
                    reading: Some(reading.to_string()),
                    annotation: Some(label.clone()),
                    index: candidates.len(),
                });
            }
        }

        // Prefix match (predictive)
        for (full_reading, surface, _score) in cache.prefix_lookup(reading) {
            if candidates.len() >= MAX_LEARNING_CANDIDATES {
                break;
            }
            if full_reading == reading {
                continue;
            }
            if seen.insert(surface.clone()) {
                candidates.push(Candidate {
                    text: surface,
                    reading: Some(full_reading),
                    annotation: Some(label.clone()),
                    index: candidates.len(),
                });
            }
        }

        candidates
    }

    /// Look up dictionary candidates for a reading (1 page, for live conversion display)
    ///
    /// Searches user dictionary first, then system dictionary.
    pub(super) fn lookup_dict_candidates(&self, reading: &str) -> Vec<Candidate> {
        self.search_dictionaries(reading, CandidateList::DEFAULT_PAGE_SIZE)
            .into_iter()
            .enumerate()
            .map(|(i, ac)| Candidate {
                text: ac.text,
                reading: Some(reading.to_string()),
                annotation: Some(ac.source.label().to_string()),
                index: i,
            })
            .collect()
    }

    /// Merge two candidate lists with deduplication
    /// Primary candidates come first, then secondary candidates that aren't duplicates
    pub(super) fn merge_candidates_dedup(
        primary: Vec<String>,
        secondary: Vec<String>,
        max_candidates: usize,
    ) -> Vec<String> {
        let mut seen = HashSet::new();
        primary
            .into_iter()
            .chain(secondary)
            .filter(|c| seen.insert(c.clone()))
            .take(max_candidates)
            .collect()
    }

    /// Process key in conversion state
    pub(super) fn process_key_conversion(&mut self, key: &KeyEvent) -> EngineResult {
        match key.keysym {
            Keysym::RETURN => self.commit_conversion(),
            Keysym::ESCAPE => self.cancel_conversion(),
            Keysym::F6 => self.direct_convert_hiragana(),
            Keysym::F7 => self.direct_convert_katakana(),
            Keysym::F8 => self.direct_convert_halfwidth_katakana(),
            Keysym::F9 => self.direct_convert_fullwidth_ascii(),
            Keysym::F10 => self.direct_convert_halfwidth_ascii(),
            Keysym::SPACE | Keysym::DOWN | Keysym::TAB => self.next_candidate(),
            Keysym::UP => self.prev_candidate(),
            Keysym::PAGE_DOWN => self.next_candidate_page(),
            Keysym::PAGE_UP => self.prev_candidate_page(),
            Keysym::BACKSPACE => self.backspace_conversion(),
            _ => {
                // Ctrl+N / Ctrl+P: emacs-style candidate navigation
                if key.modifiers.control_key && !key.modifiers.alt_key {
                    match key.keysym {
                        Keysym::KEY_N | Keysym::KEY_N_UPPER => return self.next_candidate(),
                        Keysym::KEY_P | Keysym::KEY_P_UPPER => return self.prev_candidate(),
                        _ => {}
                    }
                }

                // Check for digit selection (1-9)
                if let Some(digit) = key.keysym.digit_value() {
                    return self.select_candidate_by_digit(digit);
                }

                // Any printable character: commit current conversion and start new input
                if let Some(ch) = key.to_char()
                    && !key.modifiers.control_key
                    && !key.modifiers.alt_key
                {
                    return self.commit_conversion_and_continue(ch);
                }

                EngineResult::not_consumed()
            }
        }
    }

    /// Get selected text and reading from conversion state, or None if not in conversion
    fn selected_conversion_info(&self) -> Option<(String, Option<String>)> {
        match &self.state {
            InputState::Conversion { candidates, .. } => {
                let text = candidates.selected_text().unwrap_or("").to_string();
                let reading = candidates.selected().and_then(|c| c.reading.clone());
                Some((text, reading))
            }
            _ => None,
        }
    }

    /// Record a conversion selection in the learning cache.
    pub(super) fn record_learning(&mut self, reading: &str, surface: &str) {
        if let Some(cache) = &mut self.learning {
            cache.record(reading, surface);
        }
    }

    /// Commit the current conversion
    fn commit_conversion(&mut self) -> EngineResult {
        self.conversion_space_count = 0;
        let Some((text, reading)) = self.selected_conversion_info() else {
            return EngineResult::not_consumed();
        };

        if text.is_empty() {
            return EngineResult::consumed();
        }

        if let Some(reading) = &reading {
            self.record_learning(reading, &text);
        }

        // Check for remaining text from partial conversion
        let remaining = self.remaining_after_conversion.take();

        if let Some((before, after)) = remaining {
            // Partial conversion: commit `before + converted_text`, return `after` to composing
            let commit_text = format!("{}{}", before, text);
            self.input_buf.text = after;
            self.input_buf.cursor_pos = self.input_buf.text.chars().count();
            self.converters.romaji.reset();
            self.live.text.clear();
            let preedit = self.set_composing_state();

            if self.input_buf.text.is_empty() {
                // Nothing remaining — fully commit and return to empty
                self.state = InputState::Empty;
                self.input_buf.clear();
                return EngineResult::consumed()
                    .with_action(EngineAction::UpdatePreedit(Preedit::new()))
                    .with_action(EngineAction::HideCandidates)
                    .with_action(EngineAction::HideAuxText)
                    .with_action(EngineAction::Commit(commit_text));
            }

            let mut result = EngineResult::consumed()
                .with_action(EngineAction::Commit(commit_text))
                .with_action(EngineAction::UpdatePreedit(preedit))
                .with_action(EngineAction::HideCandidates);
            if self.config.auto_suggest {
                result =
                    result.with_action(EngineAction::UpdateAuxText(self.format_aux_composing()));
            } else {
                result = result.with_action(EngineAction::HideAuxText);
            }
            return result;
        }

        self.state = InputState::Empty;
        self.input_buf.text.clear();

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(Preedit::new()))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::HideAuxText)
            .with_action(EngineAction::Commit(text))
    }

    /// Commit current conversion and then process a new character as fresh input
    fn commit_conversion_and_continue(&mut self, ch: char) -> EngineResult {
        let Some((text, reading)) = self.selected_conversion_info() else {
            return EngineResult::not_consumed();
        };

        if let Some(reading) = &reading {
            self.record_learning(reading, &text);
        }

        // Build commit text including any before-selection prefix
        let commit_text = if let Some((before, _after)) = self.remaining_after_conversion.take() {
            format!("{}{}", before, text)
        } else {
            text
        };

        self.conversion_space_count = 0;
        self.state = InputState::Empty;
        self.input_buf.text.clear();

        // Start new input with the character
        let new_input_result = self.start_input(ch);

        // Combine: commit first, then new input actions
        let mut result = EngineResult::consumed()
            .with_action(EngineAction::Commit(commit_text))
            .with_action(EngineAction::HideCandidates);
        result.actions.extend(new_input_result.actions);
        result
    }

    /// Cancel conversion and return to hiragana
    pub(super) fn cancel_conversion(&mut self) -> EngineResult {
        self.conversion_space_count = 0;
        if !matches!(self.state, InputState::Conversion { .. }) {
            return EngineResult::not_consumed();
        }
        // Restore remaining text from partial conversion: before + reading + after
        let reading = if let Some((before, after)) = self.remaining_after_conversion.take() {
            format!("{}{}{}", before, self.input_buf.text, after)
        } else {
            self.input_buf.text.clone()
        };

        if reading.is_empty() {
            self.state = InputState::Empty;
            self.input_buf.clear();
            return EngineResult::consumed()
                .with_action(EngineAction::UpdatePreedit(Preedit::new()))
                .with_action(EngineAction::HideCandidates)
                .with_action(EngineAction::HideAuxText);
        }

        // Set up composed_hiragana with the reading
        self.input_buf.text = reading.clone();
        self.input_buf.cursor_pos = self.input_buf.text.chars().count();

        // Reset romaji converter and set output to reading
        self.converters.romaji.reset();
        // We need to push each character to rebuild the state
        for ch in reading.chars() {
            self.converters.romaji.push(ch);
        }

        let preedit = self.set_composing_state();

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::UpdateAuxText(self.format_aux_composing()))
    }

    /// Navigate candidates with the given operation, then update preedit
    fn navigate_candidate(&mut self, op: impl FnOnce(&mut CandidateList) -> bool) -> EngineResult {
        let (selected_text, candidates) = {
            let Some(candidates) = self.state.candidates_mut() else {
                return EngineResult::not_consumed();
            };
            op(candidates);
            let text = candidates.selected_text().unwrap_or("").to_string();
            (text, candidates.clone())
        };
        self.update_conversion_preedit(&selected_text, &candidates)
    }

    /// Select next candidate
    fn next_candidate(&mut self) -> EngineResult {
        self.conversion_space_count += 1;
        self.navigate_candidate(CandidateList::move_next)
    }

    /// Select previous candidate
    fn prev_candidate(&mut self) -> EngineResult {
        self.navigate_candidate(CandidateList::move_prev)
    }

    /// Go to next candidate page
    fn next_candidate_page(&mut self) -> EngineResult {
        self.navigate_candidate(CandidateList::next_page)
    }

    /// Go to previous candidate page
    fn prev_candidate_page(&mut self) -> EngineResult {
        self.navigate_candidate(CandidateList::prev_page)
    }

    /// Select candidate by digit (1-9)
    fn select_candidate_by_digit(&mut self, digit: usize) -> EngineResult {
        let (selected_text, reading) = {
            let candidates = match self.state.candidates_mut() {
                Some(c) => c,
                None => return EngineResult::not_consumed(),
            };

            if candidates.select_on_page(digit).is_none() {
                return EngineResult::consumed();
            }

            let text = candidates.selected_text().unwrap_or("").to_string();
            let reading = candidates.selected().and_then(|c| c.reading.clone());
            (text, reading)
        };

        // Record learning before committing
        if let Some(reading) = &reading {
            self.record_learning(reading, &selected_text);
        }

        // Commit immediately after digit selection
        self.conversion_space_count = 0;
        let remaining = self.remaining_after_conversion.take();

        if let Some((before, after)) = remaining {
            // Partial conversion: commit `before + selected`, return `after` to composing
            let commit_text = format!("{}{}", before, selected_text);
            self.input_buf.text = after;
            self.input_buf.cursor_pos = self.input_buf.text.chars().count();
            self.converters.romaji.reset();
            self.live.text.clear();
            let preedit = self.set_composing_state();

            if self.input_buf.text.is_empty() {
                self.state = InputState::Empty;
                self.input_buf.clear();
                return EngineResult::consumed()
                    .with_action(EngineAction::UpdatePreedit(Preedit::new()))
                    .with_action(EngineAction::HideCandidates)
                    .with_action(EngineAction::HideAuxText)
                    .with_action(EngineAction::Commit(commit_text));
            }

            let mut result = EngineResult::consumed()
                .with_action(EngineAction::Commit(commit_text))
                .with_action(EngineAction::UpdatePreedit(preedit))
                .with_action(EngineAction::HideCandidates);
            if self.config.auto_suggest {
                result =
                    result.with_action(EngineAction::UpdateAuxText(self.format_aux_composing()));
            } else {
                result = result.with_action(EngineAction::HideAuxText);
            }
            return result;
        }

        self.state = InputState::Empty;

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(Preedit::new()))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::HideAuxText)
            .with_action(EngineAction::Commit(selected_text))
    }

    /// Update preedit after candidate selection change
    fn update_conversion_preedit(
        &mut self,
        selected_text: &str,
        candidates: &CandidateList,
    ) -> EngineResult {
        let mut preedit = Preedit::with_text(selected_text);
        preedit.set_attributes(vec![PreeditAttribute::new(
            0,
            selected_text.chars().count(),
            AttributeType::Highlight,
        )]);

        if let Some(p) = self.state.preedit_mut() {
            *p = preedit.clone();
        }

        let reading = candidates
            .selected()
            .and_then(|c| c.reading.as_deref())
            .unwrap_or("");

        let threshold = self.config.candidate_window_threshold;
        let show = threshold == 0 || self.conversion_space_count >= threshold;

        let mut result = EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit));
        if show {
            result = result
                .with_action(EngineAction::ShowCandidates(candidates.clone()))
                .with_action(EngineAction::UpdateAuxText(
                    self.format_aux_conversion_with_page(reading, Some(candidates)),
                ));
        } else {
            result = result
                .with_action(EngineAction::HideCandidates)
                .with_action(EngineAction::HideAuxText);
        }
        result
    }

    /// Handle backspace in conversion mode
    fn backspace_conversion(&mut self) -> EngineResult {
        // Return to hiragana mode with the reading
        self.cancel_conversion()
    }

    /// F6: Commit as hiragana
    pub(super) fn direct_convert_hiragana(&mut self) -> EngineResult {
        let text = self.get_reading_for_direct_convert();
        if text.is_empty() {
            return EngineResult::not_consumed();
        }
        self.commit_direct(text)
    }

    /// F7: Commit as full-width katakana
    pub(super) fn direct_convert_katakana(&mut self) -> EngineResult {
        let text = self.get_reading_for_direct_convert();
        if text.is_empty() {
            return EngineResult::not_consumed();
        }
        let katakana = karukan_engine::hiragana_to_katakana(&text);
        self.commit_direct(katakana)
    }

    /// F8: Commit as half-width katakana
    pub(super) fn direct_convert_halfwidth_katakana(&mut self) -> EngineResult {
        let text = self.get_reading_for_direct_convert();
        if text.is_empty() {
            return EngineResult::not_consumed();
        }
        let hw_katakana = karukan_engine::hiragana_to_halfwidth_katakana(&text);
        self.commit_direct(hw_katakana)
    }

    /// F9: Commit as full-width ASCII (romaji)
    pub(super) fn direct_convert_fullwidth_ascii(&mut self) -> EngineResult {
        let raw = self.converters.romaji.raw_input().to_string();
        if raw.is_empty() {
            return EngineResult::not_consumed();
        }
        let fullwidth = karukan_engine::ascii_to_fullwidth(&raw);
        self.commit_direct(fullwidth)
    }

    /// F10: Commit as half-width ASCII (romaji)
    pub(super) fn direct_convert_halfwidth_ascii(&mut self) -> EngineResult {
        let raw = self.converters.romaji.raw_input().to_string();
        if raw.is_empty() {
            return EngineResult::not_consumed();
        }
        self.commit_direct(raw)
    }

    /// Get hiragana reading from current state (Composing or Conversion)
    fn get_reading_for_direct_convert(&mut self) -> String {
        match &self.state {
            InputState::Conversion { .. } => self.input_buf.text.clone(),
            InputState::Composing { .. } => {
                self.flush_romaji_to_composed();
                self.input_buf.text.clone()
            }
            _ => String::new(),
        }
    }

    /// Commit text directly and reset state
    fn commit_direct(&mut self, text: String) -> EngineResult {
        self.conversion_space_count = 0;
        self.remaining_after_conversion = None;
        self.state = InputState::Empty;
        self.input_buf.text.clear();
        self.converters.romaji.reset();

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(Preedit::new()))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::HideAuxText)
            .with_action(EngineAction::Commit(text))
    }
}
