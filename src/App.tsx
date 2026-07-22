import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Dismiss20Regular, Settings24Regular } from "@fluentui/react-icons";
import { ModeSwitch } from "./components/ModeSwitch";
import { OnboardingPanel } from "./components/OnboardingPanel";
import { SessionPanel } from "./components/SessionPanel";
import { SelectedCorrectionDialog } from "./components/SelectedCorrectionDialog";
import { SelectedVoiceEditDialog } from "./components/SelectedVoiceEditDialog";
import { SettingsPanel } from "./components/SettingsPanel";
import {
  type SelectedVoiceEditBackendStatus,
  type SelectedVoiceEditPhase,
  selectedVoiceEditPhaseAfterToggle,
  selectedVoiceEditPresentation,
  selectedVoiceEditToggleStarted,
} from "./selectedVoiceEditPresentation";
import {
  type AppSettings,
  type Mode,
  type PolishState,
  type RecordingState,
  type PrepareSelectedCorrectionResult,
  type SessionPhase,
  type SelectedVoiceEditPreview,
  type SelectedVoiceEditToggleResult,
  defaultSettings,
} from "./types";

const isTauri = "__TAURI_INTERNALS__" in window;

function App() {
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [mode, setMode] = useState<Mode>("raw");
  const modeRef = useRef<Mode>("raw");
  const [recordingState, setRecordingState] = useState<RecordingState>("idle");
  const [lastText, setLastText] = useState<string | null>(
    isTauri ? null : "今日の打ち合わせは午後2時からです。"
  );
  const [lastRawText, setLastRawText] = useState<string | null>(
    isTauri ? null : "今日の打ち合わせは午後二時からです"
  );
  const [lastPolishState, setLastPolishState] = useState<PolishState | null>(
    isTauri ? null : "applied_changed"
  );
  const [lastHistoryId, setLastHistoryId] = useState<string | null>(null);
  const [lastResultMode, setLastResultMode] = useState<Mode>("raw");
  const [lastPolishPreset, setLastPolishPreset] = useState<string>("memo");
  const [sessionPhase, setSessionPhase] = useState<SessionPhase>("idle");
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [lastError, setLastError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [selectedCorrection, setSelectedCorrection] = useState<PrepareSelectedCorrectionResult | null>(null);
  const selectedPrepareBusyRef = useRef(false);
  const selectedCorrectionOpenRef = useRef(false);
  const [selectedVoiceEdit, setSelectedVoiceEdit] = useState<SelectedVoiceEditPreview | null>(null);
  const [selectedVoiceEditPhase, setSelectedVoiceEditPhaseState] = useState<SelectedVoiceEditPhase>("idle");
  const selectedVoiceEditPhaseRef = useRef<SelectedVoiceEditPhase>("idle");
  const selectedVoiceEditBusyRef = useRef(false);
  const cancelNoticeTimerRef = useRef<number | null>(null);

  const setSelectedVoiceEditPhase = (phase: SelectedVoiceEditPhase) => {
    selectedVoiceEditPhaseRef.current = phase;
    setSelectedVoiceEditPhaseState(phase);
  };

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    invoke<AppSettings>("get_settings")
      .then((loaded) => {
        setSettings(loaded);
        setMode(loaded.mode);
        modeRef.current = loaded.mode;
        if (!loaded.onboarding_completed || !loaded.has_api_key) {
          setShowOnboarding(true);
        }
      })
      .catch((error) => setLastError(String(error)));
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    let disposed = false;
    let unlisteners: Array<() => void> = [];

    const setup = async () => {
      const offs = await Promise.all([
        listen<{
          state: RecordingState;
          mode: Mode;
          polish_preset: string | null;
          phase?: SessionPhase;
          raw_text: string | null;
          final_text: string | null;
          history_id: string | null;
          polish_state: PolishState | null;
          error: string | null;
        }>("session://state-changed", (event) => {
          const {
            state,
            mode: resultMode,
            polish_preset,
            phase,
            raw_text,
            final_text,
            history_id,
            polish_state,
            error,
          } = event.payload;
          if (cancelNoticeTimerRef.current !== null) {
            window.clearTimeout(cancelNoticeTimerRef.current);
            cancelNoticeTimerRef.current = null;
          }
          setRecordingState(state);
          setSessionPhase(phase ?? (state === "recording" ? "recording" : state === "processing" ? "transcribing" : "idle"));
          if (state === "recording") {
            setRecordingStartedAt(Date.now());
            setLastPolishState(null);
            setLastHistoryId(null);
            if (error) setLastError(error);
          }
          if (state === "idle") {
            setSelectedVoiceEditPhase("idle");
            setRecordingStartedAt(null);
            setLastError(error ?? null);
            if (raw_text) setLastRawText(raw_text);
            if (final_text) setLastText(final_text);
            setLastHistoryId(history_id ?? null);
            setLastResultMode(resultMode);
            setLastPolishPreset(polish_preset ?? "memo");
            setLastPolishState(polish_state ?? null);
            if (phase === "cancelled") {
              cancelNoticeTimerRef.current = window.setTimeout(() => {
                setSessionPhase((current) => (current === "cancelled" ? "idle" : current));
                cancelNoticeTimerRef.current = null;
              }, 1000);
            }
          }
        }),
        listen<{ phase: SessionPhase }>("session://phase-changed", (event) => {
          setSessionPhase(event.payload.phase);
        }),
        listen("hotkey://push-to-talk-down", () => {
          invoke("push_to_talk_down").catch((error) => setLastError(String(error)));
        }),
        listen("hotkey://push-to-talk-up", () => {
          invoke("push_to_talk_up").catch((error) => setLastError(String(error)));
        }),
        listen("hotkey://hands-free-raw-toggle", () => {
          invoke("toggle_hands_free_recording_for_mode", { mode: "raw" }).catch((error) =>
            setLastError(String(error))
          );
        }),
        listen("hotkey://hands-free-polish-toggle", () => {
          invoke("toggle_hands_free_recording_for_mode", { mode: "polish" }).catch((error) =>
            setLastError(String(error))
          );
        }),
        listen("hotkey://cancel-recording", () => {
          invoke("cancel_recording_session")
            .then(() => setSelectedVoiceEditPhase("idle"))
            .catch((error) => {
              setSelectedVoiceEditPhase("idle");
              setLastError(String(error));
            });
        }),
        listen("hotkey://voice-edit-selected", () => {
          if (selectedVoiceEditBusyRef.current) return;
          selectedVoiceEditBusyRef.current = true;
          setSelectedVoiceEditPhase(
            selectedVoiceEditToggleStarted(selectedVoiceEditPhaseRef.current)
          );
          invoke<SelectedVoiceEditToggleResult>("toggle_selected_voice_edit")
            .then(async (result) => {
              const status = await invoke<SelectedVoiceEditBackendStatus>(
                "get_selected_voice_edit_status"
              ).catch(() => undefined);
              setLastError(result.status === "recording" ? result.warning : null);
              setSelectedVoiceEditPhase(selectedVoiceEditPhaseAfterToggle(result.status, status));
              if (result.status === "preview") setSelectedVoiceEdit(result.preview);
            })
            .catch((error) => {
              setSelectedVoiceEditPhase("idle");
              setLastError(String(error));
            })
            .finally(() => {
              selectedVoiceEditBusyRef.current = false;
            });
        }),
        listen("hotkey://learn-selected", () => {
          if (selectedPrepareBusyRef.current || selectedCorrectionOpenRef.current) return;
          selectedPrepareBusyRef.current = true;
          invoke<PrepareSelectedCorrectionResult>("prepare_selected_correction")
            .then((prepared) => {
              setLastError(null);
              selectedCorrectionOpenRef.current = true;
              setSelectedCorrection(prepared);
            })
            .catch(async (error) => {
              try {
                await invoke("show_main_for_selected_correction_error");
              } catch {
                // 元の固定エラーを優先する。選択本文はログへ出さない。
              }
              setLastError(String(error));
            })
            .finally(() => {
              selectedPrepareBusyRef.current = false;
            });
        }),
      ]);

      if (disposed) {
        offs.forEach((off) => off());
      } else {
        unlisteners = offs;
      }
    };

    setup().catch((error) => setLastError(String(error)));
    return () => {
      disposed = true;
      unlisteners.forEach((off) => off());
      if (cancelNoticeTimerRef.current !== null) {
        window.clearTimeout(cancelNoticeTimerRef.current);
        cancelNoticeTimerRef.current = null;
      }
    };
  }, []);

  const handleModeChange = (next: Mode) => {
    modeRef.current = next;
    setMode(next);
    setSettings((current) => ({ ...current, mode: next }));
  };

  const mainSessionPresentation = selectedVoiceEditPresentation(selectedVoiceEditPhase);

  return (
    <main className="app-shell">
      <header className="app-header">
        <h1 className="app-title">KoeType</h1>
        <button
          className="icon-button settings-button"
          onClick={() => setShowSettings(true)}
          title="設定"
          aria-label="設定を開く"
        >
          <Settings24Regular />
          <span>設定</span>
        </button>
      </header>

      <section className="app-content">
        <ModeSwitch mode={mode} onModeChange={handleModeChange} />
        {mainSessionPresentation.kind === "session" ? (
          <SessionPanel
            state={recordingState}
            phase={sessionPhase}
            lastText={lastText}
            rawText={lastRawText}
            polishState={lastPolishState}
            historyId={lastHistoryId}
            resultMode={lastResultMode}
            polishPreset={lastPolishPreset}
            correctionLearningMode={settings.correction_learning_mode}
            correctionLearningMultiDiffEnabled={settings.correction_learning_multi_diff_enabled}
            elapsedMs={recordingStartedAt ? now - recordingStartedAt : 0}
            pushToTalk={settings.push_to_talk_hotkey}
            handsFreeRaw={settings.hands_free_raw_hotkey}
            handsFreePolish={settings.hands_free_polish_hotkey}
          />
        ) : (
          <div className="selected-voice-edit-status" role="status" aria-live="polite">
            <span
              className={`selected-voice-edit-status-indicator is-${mainSessionPresentation.kind}`}
              aria-hidden="true"
            />
            <h2>{mainSessionPresentation.title}</h2>
            <p>{mainSessionPresentation.instruction}</p>
          </div>
        )}

        {lastError && (
          <div className="inline-error" role="alert">
            <span>{lastError}</span>
            <button
              className="icon-button"
              onClick={() => setLastError(null)}
              aria-label="エラーを閉じる"
            >
              <Dismiss20Regular />
            </button>
          </div>
        )}
      </section>

      {showSettings && (
        <SettingsPanel
          onClose={() => setShowSettings(false)}
          onOpenOnboarding={() => {
            setShowSettings(false);
            setShowOnboarding(true);
          }}
          onSaved={(next) => {
            setSettings(next);
            setMode(next.mode);
            modeRef.current = next.mode;
          }}
        />
      )}

      {selectedCorrection && (
        <SelectedCorrectionDialog
          prepared={selectedCorrection}
          onClose={() => {
            selectedCorrectionOpenRef.current = false;
            setSelectedCorrection(null);
          }}
          onError={setLastError}
        />
      )}

      {selectedVoiceEdit && (
        <SelectedVoiceEditDialog
          preview={selectedVoiceEdit}
          onClose={() => setSelectedVoiceEdit(null)}
          onError={setLastError}
        />
      )}

      {showOnboarding && (
        <OnboardingPanel
          settings={settings}
          recordingState={recordingState}
          sessionPhase={sessionPhase}
          onSettingsSaved={(next) => {
            setSettings(next);
            setMode(next.mode);
            modeRef.current = next.mode;
          }}
          onComplete={() => setShowOnboarding(false)}
          onClose={() => setShowOnboarding(false)}
        />
      )}
    </main>
  );
}

export default App;
