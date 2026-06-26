#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{c_char, c_int, c_uint};

use karukan_im::core::keycode::{KeyEvent, KeyModifiers, Keysym};

use super::{KarukanEngine, ffi_mut};

/// Process a key event
/// Returns 1 if the key was consumed, 0 if not
#[unsafe(no_mangle)]
pub extern "C" fn karukan_engine_process_key(
    engine: *mut KarukanEngine,
    keysym: c_uint,
    state: c_uint,
    is_release: c_int,
) -> c_int {
    let engine = ffi_mut!(engine, 0);
    engine.clear_flags();

    let modifiers = KeyModifiers::from_modifier_state(state);
    let key_event = KeyEvent::new(Keysym(keysym), modifiers, is_release == 0);
    let result = engine.engine.process_key(&key_event);

    engine.apply_actions(result.actions);
    engine.sync_timing();

    if result.consumed { 1 } else { 0 }
}

/// Reset the engine state.
///
/// Salvages pending text so the C++ caller can commit it before clearing
/// the UI.  Handles both Conversion state (selected candidate) and
/// Composing state (hiragana text — this covers the case where
/// cancel_conversion already ran but fcitx5 still fires a reset).
#[unsafe(no_mangle)]
pub extern "C" fn karukan_engine_reset(engine: *mut KarukanEngine) {
    let engine = ffi_mut!(engine);

    // Try to salvage text from Conversion first, then Composing.
    let salvage_text = engine.engine.commit_if_converting().or_else(|| {
        use karukan_im::InputState;
        if matches!(engine.engine.state(), InputState::Composing { .. }) {
            let text = engine.engine.composing_text().to_string();
            if text.is_empty() { None } else { Some(text) }
        } else {
            None
        }
    });

    engine.engine.reset();
    engine.preedit = super::PreeditCache::default();
    engine.candidates = super::CandidateCache::default();
    engine.aux = super::AuxCache::default();

    if let Some(text) = salvage_text {
        engine.commit.text = std::ffi::CString::new(text).unwrap_or_default();
        engine.commit.dirty = true;
    } else {
        engine.commit = super::CommitCache::default();
    }
}

/// Set the surrounding text context from the editor
/// This provides the actual text around the cursor for better conversion accuracy
#[unsafe(no_mangle)]
pub extern "C" fn karukan_engine_set_surrounding_text(
    engine: *mut KarukanEngine,
    text: *const c_char,
    cursor_pos: c_uint,
) {
    if text.is_null() {
        tracing::debug!("set_surrounding_text: text is null, skipping");
        return;
    }
    let engine = ffi_mut!(engine);
    // SAFETY: text pointer is non-null (checked above) and expected to be a valid C string from fcitx5
    let text_str = unsafe {
        match std::ffi::CStr::from_ptr(text).to_str() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("set_surrounding_text: invalid UTF-8: {}", e);
                engine.engine.set_surrounding_context("", "");
                return;
            }
        }
    };

    // cursor_pos from fcitx5's SurroundingText::cursor() is always a character
    // (code point) offset. Each frontend (Wayland, GTK, Qt) normalizes its native
    // unit to character offset before storing in SurroundingText.
    engine
        .engine
        .set_surrounding_text_at(text_str, cursor_pos as usize);
}
