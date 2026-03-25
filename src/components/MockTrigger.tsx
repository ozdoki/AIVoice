import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

interface Props {
  onResult: (text: string) => void;
  onStateChange: (state: "idle" | "processing") => void;
}

export function MockTrigger({ onResult, onStateChange }: Props) {
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleClick = async () => {
    setRunning(true);
    setError(null);
    onStateChange("processing");
    try {
      await invoke("start_recording_session");
      const result = await invoke<string>("stop_recording_session");
      onResult(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
      onStateChange("idle");
    }
  };

  return (
    <div style={{ textAlign: "center" }}>
      <button
        onClick={handleClick}
        disabled={running}
        style={{
          padding: "0.75rem 2.5rem",
          fontSize: "1rem",
          background: running ? "#444" : "#555",
          color: "#fff",
          border: "2px solid #777",
          borderRadius: "8px",
          cursor: running ? "not-allowed" : "pointer",
        }}
      >
        {running ? "処理中..." : "Mock Start"}
      </button>
      {error && (
        <p style={{ marginTop: "0.5rem", color: "#f55", fontSize: "0.8rem" }}>
          エラー: {error}
        </p>
      )}
      <p style={{ marginTop: "0.5rem", fontSize: "0.75rem", color: "#666" }}>
        フォーカスしたアプリにテキストを注入します
      </p>
    </div>
  );
}
