/*
 * Karukan fcitx5 addon implementation
 */

#include "karukan.h"

#include <fcitx-utils/i18n.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/utf8.h>
#include <fcitx/inputpanel.h>
#include <xkbcommon/xkbcommon-keysyms.h>

namespace fcitx {

// X11 modifier bitmask constants matching the Rust FFI boundary (KeyModifiers::*_MASK).
constexpr uint32_t kShiftMask = 1;    // ShiftMask
constexpr uint32_t kControlMask = 4;  // ControlMask
constexpr uint32_t kAltMask = 8;      // Mod1Mask
constexpr uint32_t kSuperMask = 64;   // Mod4Mask

// --- KarukanCandidateWord ---

KarukanCandidateWord::KarukanCandidateWord(KarukanEngine* engine, Text text, int index,
                                           const std::string& annotation)
    : CandidateWord(std::move(text)), engine_(engine), index_(index) {
    (void)annotation;  // Annotation is shown in aux text, not inline
}

void KarukanCandidateWord::select(InputContext* inputContext) const {
    engine_->selectCandidate(inputContext, index_);
}

// --- KarukanCandidateList ---

KarukanCandidateList::KarukanCandidateList(KarukanEngine* engine, InputContext* ic)
    : engine_(engine), ic_(ic) {
    setLayoutHint(CandidateLayoutHint::Vertical);
    setPageSize(9);
    // Set selection key labels (1-9)
    setSelectionKey(Key::keyListFromString("1 2 3 4 5 6 7 8 9"));
}

void KarukanCandidateList::updateCandidates(::KarukanEngine* rustEngine) {
    // Clear existing candidates
    while (totalSize() > 0) {
        remove(0);
    }

    uint32_t count = karukan_engine_get_candidate_count(rustEngine);
    uint32_t cursor = karukan_engine_get_candidate_cursor(rustEngine);

    for (uint32_t i = 0; i < count; i++) {
        const char* text = karukan_engine_get_candidate(rustEngine, i);
        if (text) {
            Text candidateText;
            candidateText.append(std::string(text));
            const char* ann = karukan_engine_get_candidate_annotation(rustEngine, i);
            std::string comment = (ann && ann[0] != '\0') ? std::string(ann) : "";
            append<KarukanCandidateWord>(engine_, std::move(candidateText), i, comment);
        }
    }

    if (count > 0 && cursor < count) {
        setGlobalCursorIndex(static_cast<int>(cursor));
    }
}

// --- KarukanState ---

KarukanState::KarukanState(KarukanEngine* engine, InputContext* ic) : engine_(engine), ic_(ic) {
    // Create Rust engine instance
    rustEngine_ = karukan_engine_new();
}

KarukanState::~KarukanState() {
    // Wait for any in-flight init thread before freeing rustEngine_; the
    // thread holds a raw pointer to it and will use-after-free otherwise.
    if (initThread_.joinable()) {
        initThread_.join();
    }
    if (rustEngine_) {
        karukan_engine_free(rustEngine_);
    }
}

void KarukanState::keyEvent(KeyEvent& keyEvent) {
    if (!rustEngine_) {
        return;
    }

    // Initialize on first use. Loading the model + dictionary on the fcitx5
    // main thread would block all XIM clients (notably alacritty) for several
    // seconds after a cold-cache reboot, since XIM is synchronous. Run the
    // init on a background thread; while it's running we consume keys to
    // suppress raw output but otherwise return immediately so XIM clients
    // don't freeze.
    if (!initCompleted_.load(std::memory_order_acquire)) {
        if (!initInProgress_.load(std::memory_order_acquire)) {
            // First key event: kick off background init and show loading aux.
            {
                auto& inputPanel = ic_->inputPanel();
                Text aux;
                aux.append("Karukan: Loading model...");
                inputPanel.setAuxUp(aux);
                ic_->updatePreedit();
                ic_->updateUserInterface(UserInterfaceComponent::InputPanel);
            }

            initInProgress_.store(true, std::memory_order_release);

            ::KarukanEngine* rustEngine = rustEngine_;
            EventDispatcher* dispatcher = engine_->eventDispatcher();
            auto icRef = ic_->watch();

            initThread_ = std::thread([this, rustEngine, dispatcher, icRef]() {
                int result = karukan_engine_init(rustEngine);
                initResult_.store(result, std::memory_order_relaxed);
                initCompleted_.store(true, std::memory_order_release);

                // Wake the main loop so the loading message refreshes even
                // if the user isn't pressing keys. Capture by value;
                // scheduleWithContext drops the call if the InputContext
                // (and our state) has been destroyed.
                dispatcher->scheduleWithContext(icRef, [icRef, result]() {
                    auto* ic = icRef.get();
                    if (!ic) {
                        return;
                    }
                    auto& inputPanel = ic->inputPanel();
                    if (result == 0) {
                        inputPanel.setAuxUp(Text());
                    } else {
                        Text aux;
                        aux.append("Karukan: Model load failed");
                        inputPanel.setAuxUp(aux);
                    }
                    ic->updatePreedit();
                    ic->updateUserInterface(UserInterfaceComponent::InputPanel);
                });
            });
        }

        // Init still running: swallow the key (don't pass through to the app)
        // so the user doesn't see raw romaji while waiting.
        keyEvent.filterAndAccept();
        return;
    }

    // Init completed: reap the thread once and clear the in-progress flag.
    if (initInProgress_.load(std::memory_order_acquire)) {
        if (initThread_.joinable()) {
            initThread_.join();
        }
        initInProgress_.store(false, std::memory_order_release);
        // If init failed, leave the failure aux in place and pass keys through.
        if (initResult_.load(std::memory_order_relaxed) != 0) {
            return;
        }
    }

    // Convert key event
    uint32_t keysym = keyEvent.key().sym();
    uint32_t state = 0;

    if (keyEvent.key().states().test(KeyState::Shift)) {
        state |= kShiftMask;
    }
    if (keyEvent.key().states().test(KeyState::Ctrl)) {
        state |= kControlMask;
    }
    if (keyEvent.key().states().test(KeyState::Alt)) {
        state |= kAltMask;
    }
    if (keyEvent.key().states().test(KeyState::Super)) {
        state |= kSuperMask;
    }

    int isRelease = keyEvent.isRelease() ? 1 : 0;

    // Capture surrounding text at input start (Empty state) for accurate context.
    // For apps without SurroundingText capability (terminals), this clears
    // the context so stale data doesn't persist.
    if (karukan_engine_is_empty(rustEngine_) && !isRelease) {
        if (ic_->capabilityFlags().test(CapabilityFlag::SurroundingText) &&
            ic_->surroundingText().isValid()) {
            const auto& surrounding = ic_->surroundingText();
            const std::string& text = surrounding.text();
            uint32_t cursor = surrounding.cursor();
            karukan_engine_set_surrounding_text(rustEngine_, text.c_str(), cursor);
        } else {
            karukan_engine_set_surrounding_text(rustEngine_, "", 0);
        }
    }

    // Process key through Rust engine
    handlingKeyEvent_ = true;
    int consumed = karukan_engine_process_key(rustEngine_, keysym, state, isRelease);

    if (consumed) {
        keyEvent.filterAndAccept();
    }

    // Always update UI: some not-consumed keys (e.g., Shift toggle) still
    // change engine state and produce UI actions. The has_* flags in the
    // Rust engine guard against unnecessary updates.
    updateUI();
    handlingKeyEvent_ = false;
}

void KarukanState::reset() {
    // If reset() is called re-entrantly during key event processing
    // (e.g. the application reacts to a preedit/candidate change by
    // calling im_context_reset()), skip it — the key handler is already
    // managing the engine state transition and will produce the correct UI.
    if (handlingKeyEvent_) {
        return;
    }

    if (rustEngine_) {
        karukan_engine_reset(rustEngine_);

        // If the engine was in Conversion state, reset() preserves the
        // selected candidate in the commit cache. Flush it to the app
        // so the user's conversion choice is not silently discarded.
        if (karukan_engine_has_commit(rustEngine_)) {
            const char* commitText = karukan_engine_get_commit(rustEngine_);
            if (commitText && karukan_engine_get_commit_len(rustEngine_) > 0) {
                ic_->commitString(commitText);
            }
        }
    }

    ic_->inputPanel().reset();
    ic_->updatePreedit();
    ic_->updateUserInterface(UserInterfaceComponent::InputPanel);
}

void KarukanState::updateUI() {
    if (!rustEngine_) {
        return;
    }

    auto& inputPanel = ic_->inputPanel();

    // On commit: send committed text, then reset the input panel to clear
    // preedit/candidates/aux in one shot.
    // New preedit/candidates/aux are re-set below if the engine produced them.
    if (karukan_engine_has_commit(rustEngine_)) {
        const char* commitText = karukan_engine_get_commit(rustEngine_);
        uint32_t commitLen = karukan_engine_get_commit_len(rustEngine_);
        if (commitText && commitLen > 0) {
            ic_->commitString(commitText);
        }
        inputPanel.reset();
    }

    // Set preedit (new input after commit, or a regular update)
    if (karukan_engine_has_preedit(rustEngine_)) {
        const char* preeditText = karukan_engine_get_preedit(rustEngine_);
        uint32_t preeditLen = karukan_engine_get_preedit_len(rustEngine_);
        uint32_t preeditCaret = karukan_engine_get_preedit_caret(rustEngine_);

        Text preedit;
        if (preeditText && preeditLen > 0) {
            preedit.append(std::string(preeditText, preeditLen), TextFormatFlag::Underline);
            preedit.setCursor(static_cast<int>(preeditCaret));
        }

        if (ic_->capabilityFlags().test(CapabilityFlag::Preedit)) {
            inputPanel.setClientPreedit(preedit);
        } else {
            inputPanel.setPreedit(preedit);
        }
    }

    // Aux text (reading hint shown above candidates)
    if (karukan_engine_has_aux(rustEngine_)) {
        const char* auxText = karukan_engine_get_aux(rustEngine_);
        uint32_t auxLen = karukan_engine_get_aux_len(rustEngine_);

        if (auxText && auxLen > 0) {
            Text aux;
            aux.append(std::string(auxText, auxLen));
            inputPanel.setAuxUp(aux);
        } else {
            inputPanel.setAuxUp(Text());
        }
    }

    // Candidates
    if (karukan_engine_has_candidates(rustEngine_)) {
        if (karukan_engine_should_hide_candidates(rustEngine_)) {
            inputPanel.setCandidateList(nullptr);
        } else {
            auto candidateList = std::make_unique<KarukanCandidateList>(engine_, ic_);
            candidateList->updateCandidates(rustEngine_);
            inputPanel.setCandidateList(std::move(candidateList));
        }
    }

    ic_->updatePreedit();
    ic_->updateUserInterface(UserInterfaceComponent::InputPanel);
}

// --- KarukanEngine ---

KarukanEngine::KarukanEngine(Instance* instance)
    : instance_(instance),
      factory_([this](InputContext& ic) { return new KarukanState(this, &ic); }) {
    instance_->inputContextManager().registerProperty("karukanState", &factory_);
    eventDispatcher_.attach(&instance_->eventLoop());
}

KarukanEngine::~KarukanEngine() {
    eventDispatcher_.detach();
}

void KarukanEngine::keyEvent(const InputMethodEntry& entry, KeyEvent& keyEvent) {
    FCITX_UNUSED(entry);

    auto* ic = keyEvent.inputContext();
    auto* state = ic->propertyFor(&factory_);

    state->keyEvent(keyEvent);
}

void KarukanEngine::reset(const InputMethodEntry& entry, InputContextEvent& event) {
    FCITX_UNUSED(entry);

    auto* ic = event.inputContext();
    auto* state = ic->propertyFor(&factory_);

    state->reset();
}

void KarukanEngine::activate(const InputMethodEntry& entry, InputContextEvent& event) {
    FCITX_UNUSED(entry);

    auto* ic = event.inputContext();
    auto* state = ic->propertyFor(&factory_);

    // Capture surrounding text on activation for accurate context.
    // For apps without SurroundingText capability, this clears the context.
    if (state->rustEngine()) {
        if (ic->capabilityFlags().test(CapabilityFlag::SurroundingText) &&
            ic->surroundingText().isValid()) {
            const auto& surrounding = ic->surroundingText();
            const std::string& text = surrounding.text();
            uint32_t cursor = surrounding.cursor();
            karukan_engine_set_surrounding_text(state->rustEngine(), text.c_str(), cursor);
        } else {
            karukan_engine_set_surrounding_text(state->rustEngine(), "", 0);
        }
    }
}

void KarukanEngine::deactivate(const InputMethodEntry& entry, InputContextEvent& event) {
    FCITX_UNUSED(entry);

    auto* ic = event.inputContext();
    auto* state = ic->propertyFor(&factory_);

    // Commit any pending input on deactivation (mozc-style behavior)
    // Uses commit_for_deactivate which cancels conversion first — so
    // pressing Esc (mapped as deactivation key) commits the original
    // hiragana instead of a half-finished conversion result.
    if (state->rustEngine()) {
        if (karukan_engine_commit_for_deactivate(state->rustEngine())) {
            const char* commitText = karukan_engine_get_commit(state->rustEngine());
            if (commitText && karukan_engine_get_commit_len(state->rustEngine()) > 0) {
                ic->commitString(commitText);
            }
        }
        // Persist learning cache on deactivation (azooKey-style)
        karukan_engine_save_learning(state->rustEngine());
    }

    // Invalidate fcitx5's surrounding text and clear Rust-side context
    // so stale data doesn't persist across sessions.
    ic->surroundingText().invalidate();
    if (state->rustEngine()) {
        karukan_engine_set_surrounding_text(state->rustEngine(), "", 0);
    }

    // reset() clears inputPanel (preedit/candidates/aux) and flushes UI
    state->reset();
}

void KarukanEngine::selectCandidate(InputContext* ic, int index) {
    auto* state = ic->propertyFor(&factory_);
    auto* rustEngine = state->rustEngine();

    if (!rustEngine) {
        return;
    }

    // Process the selection key (1-9)
    uint32_t keysym = XKB_KEY_1 + index;
    karukan_engine_process_key(rustEngine, keysym, 0, 0);

    state->updateUI();
}

}  // namespace fcitx

// Export the addon factory
FCITX_ADDON_FACTORY(fcitx::KarukanEngineFactory);
