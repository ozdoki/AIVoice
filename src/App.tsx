import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ModeSwitch } from "./components/ModeSwitch";
import { SessionPanel } from "./components/SessionPanel";
import { MockTrigger } from "./components/MockTrigger";
import { SettingsPanel } from "./components/SettingsPanel";

type Mode = "raw" | "polish";
type RecordingState = "idle" | "recording" | "processing";

function App() {
  const [mode, setMode] = useState<Mode>("raw");
  const modeRef = useRef<Mode>("raw");
  const [recordingState, setRecordingState] = useState<RecordingState>("idle");
  const [lastText, setLastText] = useState<string | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);

  // modeRef を常に最新に保つ
  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  useEffect(() => {
    invoke<Mode>("get_mode").then((m) => {
      setMode(m);
      modeRef.current = m;
    }).catch(console.error);
  }, []);

  // ホットキーリスナーは1回だけ登録する（mode 変化で再登録しない）
  useEffect(() => {
    let disposed = false;
    let unlisteners: Array<() => void> = [];

    const setup = async () => {
      const offs = await Promise.all([
        listen("hotkey://start", async () => {
          try {
            await invoke("start_recording_session");
            setRecordingState("recording");
          } catch (e) {
            console.error(e);
          }
        }),
        listen("hotkey://stop", async () => {
          setRecordingState("processing");
          try {
            const text = await invoke<string>("stop_recording_session");
            setLastError(null);
            if (text) setLastText(text);
          } catch (e) {
            setLastError(String(e));
            console.error(e);
          } finally {
            setRecordingState("idle");
          }
        }),
        listen("hotkey://toggle-mode", async () => {
          const next: Mode = modeRef.current === "raw" ? "polish" : "raw";
          try {
            await invoke("set_mode", { mode: next });
            modeRef.current = next;
            setMode(next);
          } catch (e) {
            console.error(e);
          }
        }),
      ]);

      if (disposed) {
        offs.forEach((off) => off());
        return;
      }
      unlisteners = offs;
    };

    setup().catch(console.error);

    return () => {
      disposed = true;
      unlisteners.forEach((off) => off());
    };
  }, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: "1.5rem", paddingTop: "2rem" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "0.75rem" }}>
        <h1 style={{ fontSize: "1.5rem", letterSpacing: "0.05em" }}>AIVoice</h1>
        <button
          onClick={() => setShowSettings(true)}
          title="設定"
          style={{
            background: "transparent",
            border: "none",
            cursor: "pointer",
            fontSize: "1.2rem",
            color: "#888",
            padding: "0.2rem",
            lineHeight: 1,
          }}
        >
          ⚙
        </button>
      </div>

      <ModeSwitch mode={mode} onModeChange={setMode} />

      <SessionPanel state={recordingState} lastText={lastText} />

      {lastError && (
        <div style={{
          color: "#c00",
          background: "#fff0f0",
          border: "1px solid #fcc",
          borderRadius: "6px",
          padding: "0.5rem 1rem",
          fontSize: "0.85rem",
          maxWidth: "320px",
          textAlign: "center",
        }}>
          {lastError}
        </div>
      )}

      <MockTrigger
        onResult={(text) => setLastText(text)}
        onStateChange={(s) => setRecordingState(s)}
      />

      {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
    </div>
  );
}

export default App;
