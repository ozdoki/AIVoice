import { useEffect, useState } from "react";
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
  const [recordingState, setRecordingState] = useState<RecordingState>("idle");
  const [lastText, setLastText] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    invoke<Mode>("get_mode").then(setMode).catch(console.error);
  }, []);

  useEffect(() => {
    const unlistenStart = listen("hotkey://start", () => {
      setRecordingState("recording");
    });

    const unlistenStop = listen("hotkey://stop", async () => {
      setRecordingState("processing");
      try {
        const text = await invoke<string>("start_mock_session");
        setLastText(text);
      } catch (e) {
        console.error(e);
      } finally {
        setRecordingState("idle");
      }
    });

    const unlistenToggle = listen("hotkey://toggle-mode", async () => {
      const next: Mode = mode === "raw" ? "polish" : "raw";
      await invoke("set_mode", { mode: next }).catch(console.error);
      setMode(next);
    });

    return () => {
      unlistenStart.then((f) => f());
      unlistenStop.then((f) => f());
      unlistenToggle.then((f) => f());
    };
  }, [mode]);

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

      <MockTrigger
        onResult={(text) => setLastText(text)}
        onStateChange={(s) => setRecordingState(s)}
      />

      {showSettings && <SettingsPanel onClose={() => setShowSettings(false)} />}
    </div>
  );
}

export default App;
