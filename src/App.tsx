import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Dismiss20Regular, Settings24Regular } from "@fluentui/react-icons";
import { ModeSwitch } from "./components/ModeSwitch";
import { OnboardingPanel } from "./components/OnboardingPanel";
import { SessionPanel } from "./components/SessionPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import {
  type AppSettings,
  type Mode,
  type RecordingState,
  type SessionPhase,
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
  const [sessionPhase, setSessionPhase] = useState<SessionPhase>("idle");
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [lastError, setLastError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);

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
          phase?: SessionPhase;
          raw_text: string | null;
          final_text: string | null;
          history_id: string | null;
          error: string | null;
        }>("session://state-changed", (event) => {
          const { state, phase, raw_text, final_text, error } = event.payload;
          setRecordingState(state);
          setSessionPhase(phase ?? (state === "recording" ? "recording" : state === "processing" ? "transcribing" : "idle"));
          if (state === "recording") {
            setRecordingStartedAt(Date.now());
          }
          if (state === "idle") {
            setRecordingStartedAt(null);
            setLastError(error ?? null);
            if (raw_text) setLastRawText(raw_text);
            if (final_text) setLastText(final_text);
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
    };
  }, []);

  const handleModeChange = (next: Mode) => {
    modeRef.current = next;
    setMode(next);
    setSettings((current) => ({ ...current, mode: next }));
  };

  return (
    <main className="app-shell">
      <header className="app-header">
        <h1 className="app-title">AIVoice</h1>
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
        <SessionPanel
          state={recordingState}
          phase={sessionPhase}
          lastText={lastText}
          rawText={lastRawText}
          elapsedMs={recordingStartedAt ? now - recordingStartedAt : 0}
          pushToTalk={settings.push_to_talk_hotkey}
          handsFreeRaw={settings.hands_free_raw_hotkey}
          handsFreePolish={settings.hands_free_polish_hotkey}
        />

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

      {showOnboarding && (
        <OnboardingPanel
          settings={settings}
          recordingState={recordingState}
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
