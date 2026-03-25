import { invoke } from "@tauri-apps/api/core";

type Mode = "raw" | "polish";

interface Props {
  mode: Mode;
  onModeChange: (mode: Mode) => void;
}

export function ModeSwitch({ mode, onModeChange }: Props) {
  const toggle = async () => {
    const next: Mode = mode === "raw" ? "polish" : "raw";
    await invoke("set_mode", { mode: next });
    onModeChange(next);
  };

  return (
    <div style={{ textAlign: "center" }}>
      <p style={{ marginBottom: "0.5rem", fontSize: "0.85rem", color: "#888" }}>
        現在のモード
      </p>
      <button
        onClick={toggle}
        style={{
          padding: "0.5rem 2rem",
          fontSize: "1.2rem",
          fontWeight: "bold",
          background: mode === "raw" ? "#2a6" : "#36a",
          color: "#fff",
          border: "none",
          borderRadius: "8px",
          cursor: "pointer",
        }}
      >
        {mode === "raw" ? "Raw" : "Polish"}
      </button>
      <p style={{ marginTop: "0.5rem", fontSize: "0.75rem", color: "#666" }}>
        クリックで切り替え
      </p>
    </div>
  );
}
