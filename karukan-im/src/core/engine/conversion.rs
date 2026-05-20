//! Conversion state handling (candidates, segments, commit)

use std::collections::HashSet;
use std::time::Instant;

use tracing::debug;

use super::*;

/// Maximum number of learning candidates to show
const MAX_LEARNING_CANDIDATES: usize = 3;

/// True iff every character is a hiragana (excluding hiragana-range punctuation
/// like `゛` and `゜`). Used to detect whether a selection can be used directly
/// as a reading or needs alignment-based mapping.
fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{3096}').contains(&c) || c == '\u{30FC}'
}

/// Mozc-style width/script annotation for a pure-kana candidate, or `None`
/// if the text mixes scripts or contains kanji/punctuation. Used to label
/// `あ` / `ア` / `ｱ` candidates in the conversion list.
fn width_annotation(text: &str) -> Option<&'static str> {
    if karukan_engine::is_pure_hiragana(text) {
        Some("[全]ひらがな")
    } else if karukan_engine::is_pure_full_katakana(text) {
        Some("[全]カタカナ")
    } else {
        None
    }
}

/// Helper for building a deduplicated list of conversion candidates.
///
/// Two push paths exist: [`push`] dedups by text (skips duplicates), and
/// [`push_force`] always inserts (used for learning candidates that should
/// appear at the top even if a later source re-emits the same text).
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
    fn push(&mut self, ac: AnnotatedCandidate) {
        if self.seen.insert(ac.text.clone()) {
            self.candidates.push(ac);
        }
    }

    /// Push a candidate unconditionally, marking its text as seen so later
    /// dedup'd inserts skip it. Use only for sources that should win over
    /// duplicates from later steps (e.g. learning cache).
    fn push_force(&mut self, ac: AnnotatedCandidate) {
        self.seen.insert(ac.text.clone());
        self.candidates.push(ac);
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
    ///
    /// Skips the model entirely when the reading has no hiragana/katakana — the
    /// model is trained on kana → kanji and hallucinates garbage (e.g. `「` → `w`)
    /// for symbol- or alphabet-only inputs. Rule-based variants from
    /// `SymbolRewriter` cover those cases instead.
    fn run_kana_kanji_conversion(&mut self, reading: &str, num_candidates: usize) -> Vec<String> {
        if !karukan_engine::contains_kana(reading) {
            return vec![];
        }
        let Some(converter) = self.converters.kanji.as_ref() else {
            return vec![];
        };
        let katakana = karukan_engine::hiragana_to_katakana(reading);
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

    /// Select from auto-suggest candidates (Tab/Down in Composing state).
    ///
    /// If auto-suggest candidates are available, enters Conversion state with those
    /// candidates (bypassing model re-inference). Falls back to `start_conversion()`
    /// if no candidates are stored.
    pub(super) fn select_auto_suggest(&mut self) -> EngineResult {
        let candidates = match self.suggest_candidates.take() {
            Some(c) if !c.is_empty() => c,
            _ => return self.start_conversion(false),
        };

        // Flush any remaining romaji
        self.flush_romaji_to_composed();

        let reading = self.input_buf.text.clone();
        self.converters.romaji.reset();
        self.input_buf.cursor_pos = 0;
        self.input_buf.clear_selection();
        self.live.text.clear();

        if reading.is_empty() {
            return EngineResult::consumed();
        }

        let candidate_list = CandidateList::new(candidates);

        let selected_text = candidate_list
            .selected_text()
            .unwrap_or(&reading)
            .to_string();

        let preedit = Preedit::from_segments(
            vec![PreeditSegment::highlighted(&selected_text)],
            selected_text.chars().count(),
        );

        self.state = InputState::Conversion {
            preedit: preedit.clone(),
            candidates: candidate_list.clone(),
        };

        // Force show candidate window (bypass threshold)
        self.conversion_space_count = self.config.candidate_window_threshold.max(1);

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::ShowCandidates(candidate_list.clone()))
            .with_action(EngineAction::UpdateAuxText(
                self.format_aux_conversion_with_page(&reading, Some(&candidate_list)),
            ))
    }

    /// Start kanji conversion for the current input buffer.
    ///
    /// Called when DOWN/SPACE/Shift+SPACE is pressed: flushes any pending romaji,
    /// resolves the reading, runs `build_conversion_candidates`, and transitions
    /// into the Conversion state. Supports selection-based partial conversion
    /// (Shift+Arrow then Space converts only the selected range).
    ///
    /// `skip_learning` is set by the Shift+Space path to omit learning-cache
    /// candidates (Space/Down keep the default learning-included behavior).
    pub(super) fn start_conversion(&mut self, skip_learning: bool) -> EngineResult {
        self.suggest_candidates = None;
        // Flush any remaining romaji into composed_hiragana
        self.flush_romaji_to_composed();

        let full_text = self.input_buf.text.clone();

        // Selection-based partial conversion: convert only the selected range.
        //
        // When the selection contains non-hiragana characters (e.g. the user
        // is re-converting a portion of an already-converted result like
        // "ペン" in "私はペンです"), align the surface against the original
        // hiragana reading saved in `original_composing_text` and convert the
        // mapped reading range instead of the surface text. For pure-hiragana
        // selections the surface itself is the reading, so the alignment step
        // is skipped.
        let (reading, remaining) =
            if let Some((sel_start, sel_end)) = self.input_buf.selection_range() {
                let selected: String = full_text
                    .chars()
                    .skip(sel_start)
                    .take(sel_end - sel_start)
                    .collect();
                let is_pure_hiragana =
                    !selected.is_empty() && selected.chars().all(is_hiragana);

                if is_pure_hiragana || self.original_composing_text.is_none() {
                    let before: String = full_text.chars().take(sel_start).collect();
                    let after: String = full_text.chars().skip(sel_end).collect();
                    (selected, Some((before, after)))
                } else {
                    let original = self.original_composing_text.as_ref().unwrap();
                    let segments = karukan_engine::align::align(&full_text, original);
                    let (r_start, r_end, expanded_start, expanded_end) =
                        karukan_engine::align::map_range(&segments, sel_start, sel_end);
                    let reading: String = original
                        .chars()
                        .skip(r_start)
                        .take(r_end - r_start)
                        .collect();
                    let before: String = full_text.chars().take(expanded_start).collect();
                    let after: String = full_text.chars().skip(expanded_end).collect();
                    (reading, Some((before, after)))
                }
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
        let mut candidates =
            self.build_conversion_candidates(&reading, self.config.num_candidates, skip_learning);

        // If the previous auto-suggest result is not in the new candidates, insert it at the top
        // so it doesn't disappear when the conversion strategy changes.
        let seen: HashSet<&str> = candidates.iter().map(|c| c.text.as_str()).collect();
        if !prev_suggest_text.is_empty()
            && prev_suggest_text != reading
            && !seen.contains(prev_suggest_text.as_str())
        {
            candidates.insert(
                0,
                AnnotatedCandidate::new(prev_suggest_text, CandidateSource::Model),
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

        // Map AnnotatedCandidate → public Candidate. The two annotation
        // slots are kept disjoint so descriptions never duplicate between the
        // aux text and the candidate's right-side comment:
        //   - `source_label` ← source.label() only (e.g. `🤖 AI`, `📚 辞書`)
        //   - `description`  ← the per-candidate description only
        //                      (e.g. `三点リーダ`, `[全]英大文字`)
        let candidate_list = CandidateList::new(
            candidates
                .into_iter()
                .map(|ac| {
                    let cand_reading = ac.reading.unwrap_or_else(|| reading.clone());
                    let label = ac.source.label();
                    Candidate {
                        text: ac.text,
                        reading: Some(cand_reading),
                        source_label: (!label.is_empty()).then(|| label.to_string()),
                        description: ac.description,
                    }
                })
                .collect(),
        );
        self.enter_conversion_state(&reading, candidate_list)
    }

    /// Build preedit for conversion state, including before/after text for partial conversion.
    ///
    /// When `remaining_after_conversion` is set (partial conversion), the preedit shows:
    ///   before(underline) + conversion_result(highlight) + after(underline)
    /// Otherwise, just the conversion result with highlight.
    fn build_conversion_preedit(&self, selected_text: &str) -> Preedit {
        if let Some((before, after)) = &self.remaining_after_conversion {
            let before_len = before.chars().count();
            let sel_len = selected_text.chars().count();
            let after_len = after.chars().count();
            let full = format!("{}{}{}", before, selected_text, after);
            let mut preedit = Preedit::with_text(&full);
            let mut attrs = Vec::new();
            if before_len > 0 {
                attrs.push(PreeditAttribute::underline(0, before_len));
            }
            attrs.push(PreeditAttribute::new(
                before_len,
                before_len + sel_len,
                AttributeType::Highlight,
            ));
            if after_len > 0 {
                attrs.push(PreeditAttribute::underline(
                    before_len + sel_len,
                    before_len + sel_len + after_len,
                ));
            }
            preedit.set_caret(before_len + sel_len);
            preedit
        } else {
            Preedit::from_segments(
                vec![PreeditSegment::highlighted(selected_text)],
                selected_text.chars().count(),
            )
        }
    }

    /// Transition to Conversion state with the given reading and candidate list.
    ///
    /// Sets up the preedit (highlighted selected text), updates the state, and
    /// returns an EngineResult with preedit, candidates, and aux text actions.
    fn enter_conversion_state(&mut self, reading: &str, candidates: CandidateList) -> EngineResult {
        let selected_text = candidates.selected_text().unwrap_or(reading).to_string();

        let preedit = self.build_conversion_preedit(&selected_text);

        self.state = InputState::Conversion {
            preedit: preedit.clone(),
            candidates: candidates.clone(),
        };

        self.conversion_space_count = 1;
        let threshold = self.config.candidate_window_threshold;
        let show = threshold == 0 || self.conversion_space_count >= threshold;

        let mut result = EngineResult::consumed().with_action(EngineAction::UpdatePreedit(preedit));
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
                    candidates.push(AnnotatedCandidate::new(
                        cand.surface.clone(),
                        CandidateSource::UserDictionary,
                    ));
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
                    candidates.push(AnnotatedCandidate::new(
                        cand.surface,
                        CandidateSource::Dictionary,
                    ));
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
    ///
    /// `skip_learning` suppresses the learning-cache step (1). Used by the Tab
    /// key path so users can escape a noisy learning history without losing
    /// access to dictionary/model candidates.
    pub(super) fn build_conversion_candidates(
        &mut self,
        reading: &str,
        num_candidates: usize,
        skip_learning: bool,
    ) -> Vec<AnnotatedCandidate> {
        // Try to initialize the kanji converter, but don't bail out if it
        // fails — symbol-only inputs (e.g. `。。。`) don't need the model and
        // we still want to produce dictionary, rewriter, and fallback candidates.
        // run_kana_kanji_conversion handles the converter-missing case.
        if self.converters.kanji.is_none()
            && let Err(e) = self.init_kanji_converter()
        {
            debug!("Failed to initialize kanji converter: {}", e);
        }

        let candidates = self.run_kana_kanji_conversion(reading, num_candidates);

        let hiragana = reading.to_string();
        let katakana = karukan_engine::hiragana_to_katakana(reading);

        // Priority: Learning → Model → System Dictionary → User Dictionary → Fallback
        let mut builder = CandidateBuilder::new();

        // 1. Learning cache candidates (highest priority).
        //    Force-inserted so they win against duplicate text from later sources.
        //    Skipped when the caller asks for a learning-free conversion (Shift+Space).
        if !skip_learning {
            for c in self.lookup_learning_candidates(reading) {
                // Exact matches have reading == input reading; use None to avoid redundancy
                let cand_reading = c.reading.filter(|r| r != reading);
                builder.push_force(
                    AnnotatedCandidate::new(c.text, CandidateSource::Learning)
                        .with_reading(cand_reading),
                );
            }
        }

        // 2. Dictionary candidates (user dict first, then system dict)
        let dict_results = self.search_dictionaries(reading, usize::MAX);
        // Insert user dictionary entries at the top (after learning)
        for ac in &dict_results {
            if ac.source == CandidateSource::UserDictionary {
                builder.push(ac.clone());
            }
        }

        // 3. Model inference results
        if candidates.is_empty() {
            // In emoji mode, defer the literal-fallback decision until
            // after rewriters have run — otherwise `:smile` would be
            // pinned to the top of the candidate list as a Fallback
            // and outrank the 😄 we surface in step 5/6.
            if builder.is_empty() && self.input_mode != InputMode::Emoji {
                builder.push(AnnotatedCandidate::new(
                    hiragana.clone(),
                    CandidateSource::Fallback,
                ));
            }
        } else {
            for text in candidates {
                builder.push(AnnotatedCandidate::new(text, CandidateSource::Model));
            }
        }

        // 4. System dictionary candidates (from search_dictionaries result)
        for ac in dict_results {
            if ac.source == CandidateSource::Dictionary {
                builder.push(ac);
            }
        }

        // 5/6. Hiragana/katakana fallback + rewriter variants.
        let rewriter_variants = self
            .converters
            .rewriters
            .rewrite_all(&[reading.to_string()]);
        if self.input_mode == InputMode::Emoji {
            for (variant, description) in rewriter_variants {
                builder.push(
                    AnnotatedCandidate::new(variant, CandidateSource::Rewriter)
                        .with_description(description),
                );
            }
        } else {
            builder.push(AnnotatedCandidate::new(hiragana, CandidateSource::Fallback));
            builder.push(AnnotatedCandidate::new(katakana, CandidateSource::Fallback));
            for (variant, description) in rewriter_variants {
                builder.push(
                    AnnotatedCandidate::new(variant, CandidateSource::Rewriter)
                        .with_description(description),
                );
            }
        }

        // 7. Enrich Fallback candidates with symbol descriptions.
        for c in &mut builder.candidates {
            if c.source == CandidateSource::Fallback
                && c.description.is_none()
                && let Some(desc) = karukan_engine::symbol_description(&c.text)
            {
                c.description = Some(desc.to_string());
            }
        }

        // 8. Attach mozc-style width annotations to any pure-kana candidate.
        for c in &mut builder.candidates {
            if c.description.is_none()
                && let Some(desc) = width_annotation(&c.text)
            {
                c.description = Some(desc.to_string());
            }
        }

        builder.into_candidates()
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
                    source_label: Some(label.clone()),
                    description: None,
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
                    source_label: Some(label.clone()),
                    description: None,
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
            .map(|ac| Candidate {
                text: ac.text,
                reading: Some(reading.to_string()),
                source_label: Some(ac.source.label().to_string()),
                description: None,
            })
            .collect()
    }

    /// Build rule-based rewriter variants for the reading itself (e.g. for
    /// symbol input `「` → `『`, `【`, `（`, ...). Used in the auto-suggest path
    /// so users see mozc-style symbol variants without pressing Space first.
    pub(super) fn lookup_rewriter_variants(&self, reading: &str) -> Vec<Candidate> {
        let source_label = CandidateSource::Rewriter.label().to_string();
        self.converters
            .rewriters
            .rewrite_all(&[reading.to_string()])
            .into_iter()
            .map(|(text, description)| Candidate {
                text,
                reading: Some(reading.to_string()),
                source_label: Some(source_label.clone()),
                description,
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
    pub(super) fn process_key_conversion(
        &mut self,
        key: &KeyEvent,
        shift_active: bool,
    ) -> EngineResult {
        match key.keysym {
            // Shift+Arrow: cancel conversion → return to Composing with selection
            Keysym::RIGHT if shift_active => self.conversion_to_selection_right(),
            Keysym::LEFT if shift_active => self.conversion_to_selection_left(),
            Keysym::HOME if shift_active => self.conversion_to_selection_home(),
            Keysym::END if shift_active => self.conversion_to_selection_end(),
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

        // Skip learning when the buffer is a `:shortcode` query — the
        // reading would be e.g. `:smile`, which isn't a hiragana key
        // and would corrupt the kana-keyed learning cache.
        if self.input_mode != InputMode::Emoji
            && let Some(reading) = &reading
        {
            self.record_learning(reading, &text);
        }

        // Check for remaining text from partial conversion
        let remaining = self.remaining_after_conversion.take();

        if let Some((before, after)) = remaining {
            // Partial conversion: bake the result into the composing buffer
            // so the user can continue converting other portions.
            return self.bake_partial_conversion(&before, &text, &after);
        }

        self.enter_empty_state();
        if self.input_mode == InputMode::Emoji {
            self.input_mode = InputMode::Hiragana;
        }

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

        if self.input_mode != InputMode::Emoji
            && let Some(reading) = &reading
        {
            self.record_learning(reading, &text);
        }

        // Build commit text: before + converted (after is discarded since
        // the user is starting fresh input, not continuing partial conversion)
        let commit_text = if let Some((before, _after)) = self.remaining_after_conversion.take() {
            format!("{}{}", before, text)
        } else {
            text
        };

        self.enter_empty_state();
        if self.input_mode == InputMode::Emoji {
            self.input_mode = InputMode::Hiragana;
        }

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
        // Restore full original text. input_buf.text is unchanged since
        // start_conversion() and already contains the full composing text
        // (including before/after portions of any partial selection).
        let remaining = self.remaining_after_conversion.take();
        let reading = self.input_buf.text.clone();
        debug!(
            "cancel_conversion: reading=\"{}\" remaining={:?}",
            reading, remaining
        );

        if reading.is_empty() {
            self.enter_empty_state();
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
            // Partial conversion: bake the result into the composing buffer
            return self.bake_partial_conversion(&before, &selected_text, &after);
        }

        self.enter_empty_state();

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
        let preedit = self.build_conversion_preedit(selected_text);

        if let Some(p) = self.state.preedit_mut() {
            *p = preedit.clone();
        }

        let reading = candidates
            .selected()
            .and_then(|c| c.reading.as_deref())
            .unwrap_or("");

        let threshold = self.config.candidate_window_threshold;
        let show = threshold == 0 || self.conversion_space_count >= threshold;

        let mut result = EngineResult::consumed().with_action(EngineAction::UpdatePreedit(preedit));
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
        let hw_katakana = karukan_engine::kana::hiragana_to_half_katakana(&text);
        self.commit_direct(hw_katakana)
    }

    /// F9: Commit as full-width ASCII (romaji)
    pub(super) fn direct_convert_fullwidth_ascii(&mut self) -> EngineResult {
        let raw = self.converters.romaji.raw_input().to_string();
        if raw.is_empty() {
            return EngineResult::not_consumed();
        }
        let fullwidth: String = raw
            .chars()
            .map(karukan_engine::kana::ascii_to_fullwidth_char)
            .collect();
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

    /// Bake a partial conversion result into the composing buffer.
    ///
    /// Instead of committing `before + converted` to the application, this replaces
    /// the selected portion in the composing buffer with the converted text and returns
    /// to Composing state. This allows the user to continue converting other portions
    /// regardless of selection direction (start, middle, or end).
    fn bake_partial_conversion(
        &mut self,
        before: &str,
        converted: &str,
        after: &str,
    ) -> EngineResult {
        // Save the original hiragana reading before the first bake.
        // input_buf.text is still unchanged at this point (start_conversion doesn't modify it).
        if self.original_composing_text.is_none() {
            self.original_composing_text = Some(self.input_buf.text.clone());
        }

        let new_text = format!("{}{}{}", before, converted, after);

        if new_text.is_empty() {
            self.enter_empty_state();
            return EngineResult::consumed()
                .with_action(EngineAction::UpdatePreedit(Preedit::new()))
                .with_action(EngineAction::HideCandidates)
                .with_action(EngineAction::HideAuxText);
        }

        self.input_buf.text = new_text;
        self.input_buf.cursor_pos = self.input_buf.text.chars().count();
        self.input_buf.clear_selection();
        self.converters.romaji.reset();
        for ch in self.input_buf.text.chars() {
            self.converters.romaji.push(ch);
        }
        self.live.text.clear();

        let preedit = self.set_composing_state();
        let mut result = EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(preedit))
            .with_action(EngineAction::HideCandidates);
        if self.config.auto_suggest {
            result = result.with_action(EngineAction::UpdateAuxText(self.format_aux_composing()));
        } else {
            result = result.with_action(EngineAction::HideAuxText);
        }
        result
    }

    /// Prepare for transitioning from Conversion back to Composing with selection.
    ///
    /// Bakes the current selected candidate into `input_buf` so the user can
    /// select within the already-converted text and re-convert a portion of
    /// it (mozc-style segment re-conversion). The original hiragana reading is
    /// preserved in `original_composing_text` for alignment-based mapping when
    /// the user later selects a kanji/katakana range and presses Space.
    fn prepare_cancel_for_selection(&mut self, cursor_at_start: bool) {
        self.conversion_space_count = 0;

        // Save the original reading for later alignment if not already set.
        if self.original_composing_text.is_none() {
            self.original_composing_text = Some(self.input_buf.text.clone());
        }

        // Bake the current candidate (or fall back to the raw reading if no
        // candidates are available, e.g. empty conversion state).
        let baked = self
            .selected_conversion_info()
            .map(|(t, _)| t)
            .unwrap_or_else(|| self.input_buf.text.clone());

        // Combine with any in-progress partial-conversion fragments so the
        // baked text reflects the full composing buffer the user is editing.
        let new_text = if let Some((before, after)) = self.remaining_after_conversion.take() {
            format!("{}{}{}", before, baked, after)
        } else {
            baked
        };

        self.input_buf.text = new_text;
        self.input_buf.cursor_pos = if cursor_at_start {
            0
        } else {
            self.input_buf.text.chars().count()
        };
        self.converters.romaji.reset();
        for ch in self.input_buf.text.chars() {
            self.converters.romaji.push(ch);
        }
        self.live.text.clear();
    }

    /// Shift+Right in Conversion: cancel conversion, place cursor at start, select right
    fn conversion_to_selection_right(&mut self) -> EngineResult {
        self.prepare_cancel_for_selection(true);
        self.shift_select_right()
    }

    /// Shift+Left in Conversion: cancel conversion, place cursor at end, select left
    fn conversion_to_selection_left(&mut self) -> EngineResult {
        self.prepare_cancel_for_selection(false);
        self.shift_select_left()
    }

    /// Shift+Home in Conversion: cancel conversion, place cursor at end, select to home
    fn conversion_to_selection_home(&mut self) -> EngineResult {
        self.prepare_cancel_for_selection(false);
        self.shift_select_home()
    }

    /// Shift+End in Conversion: cancel conversion, place cursor at start, select to end
    fn conversion_to_selection_end(&mut self) -> EngineResult {
        self.prepare_cancel_for_selection(true);
        self.shift_select_end()
    }

    /// Commit text directly and reset state
    fn commit_direct(&mut self, text: String) -> EngineResult {
        self.enter_empty_state();

        EngineResult::consumed()
            .with_action(EngineAction::UpdatePreedit(Preedit::new()))
            .with_action(EngineAction::HideCandidates)
            .with_action(EngineAction::HideAuxText)
            .with_action(EngineAction::Commit(text))
    }
}
