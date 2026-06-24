import { invoke } from "@tauri-apps/api/core";
import { Mic24Regular, Sparkle24Regular } from "@fluentui/react-icons";

type Mode = "raw" | "polish";

interface Props {
  mode: Mode;
  onModeChange: (mode: Mode) => void;
}

export function ModeSwitch({ mode, onModeChange }: Props) {
  const toggle = async () => {
    const next: Mode = mode === "raw" ? "polish" : "raw";
    if ("__TAURI_INTERNALS__" in window) {
      await invoke("set_mode", { mode: next });
    }
    onModeChange(next);
  };

  return (
    <div className="mode-switch" role="group" aria-label="入力モード">
      <button
        className={`mode-option ${mode === "raw" ? "is-active" : ""}`}
        onClick={() => mode !== "raw" && toggle()}
        aria-pressed={mode === "raw"}
      >
        <Mic24Regular />
        <span>Raw</span>
      </button>
      <button
        className={`mode-option ${mode === "polish" ? "is-active" : ""}`}
        onClick={() => mode !== "polish" && toggle()}
        aria-pressed={mode === "polish"}
      >
        <Sparkle24Regular />
        <span>Polish</span>
      </button>
    </div>
  );
}
