import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Dismiss20Regular, Settings24Regular } from "@fluentui/react-icons";
import { ModeSwitch } from "./components/ModeSwitch";
import { SessionPanel } from "./components/SessionPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import {
  type AppSettings,
  type Mode,
  type RecordingState,
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
  const [lastError, setLastError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  useEffect(() => {
    if (!isTauri) return;
    invoke<AppSettings>("get_settings")
      .then((loaded) => {
        setSettings(loaded);
        setMode(loaded.mode);
        modeRef.current = loaded.mode;
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
          final_text: string | null;
          history_id: string | null;
          error: string | null;
        }>("session://state-changed", (event) => {
          const { state, final_text, error } = event.payload;
          setRecordingState(state);
          if (state === "idle") {
            setLastError(error ?? null);
            if (final_text) setLastText(final_text);
          }
        }),
        listen("hotkey://push-to-talk-down", () => {
          invoke("push_to_talk_down").catch((error) => setLastError(String(error)));
        }),
        listen("hotkey://push-to-talk-up", () => {
          invoke("push_to_talk_up").catch((error) => setLastError(String(error)));
        }),
        listen("hotkey://hands-free-toggle", () => {
          invoke("toggle_hands_free_recording").catch((error) =>
            setLastError(String(error))
          );
        }),
        listen("hotkey://toggle-mode", async () => {
          const next: Mode = modeRef.current === "raw" ? "polish" : "raw";
          try {
            await invoke("set_mode", { mode: next });
            modeRef.current = next;
            setMode(next);
            setSettings((current) => ({ ...current, mode: next }));
          } catch (error) {
            setLastError(String(error));
          }
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
          lastText={lastText}
          pushToTalk={settings.push_to_talk_hotkey}
          handsFree={settings.hands_free_hotkey}
          toggleMode={settings.toggle_mode_hotkey}
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
          onSaved={(next) => {
            setSettings(next);
            setMode(next.mode);
            modeRef.current = next.mode;
          }}
        />
      )}
    </main>
  );
}

export default App;
