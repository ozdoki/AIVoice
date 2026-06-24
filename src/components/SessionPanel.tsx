import { Fragment, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Checkmark20Regular,
  Copy20Regular,
  Mic48Regular,
} from "@fluentui/react-icons";
import {
  type HotkeyBinding,
  type RecordingState,
  formatHotkey,
  hotkeyParts,
} from "../types";

interface Props {
  state: RecordingState;
  lastText: string | null;
  pushToTalk: HotkeyBinding;
  handsFree: HotkeyBinding;
  toggleMode: HotkeyBinding;
}

const stateLabel: Record<RecordingState, string> = {
  idle: "待機中",
  recording: "録音中",
  processing: "処理中",
};

const isTauri = "__TAURI_INTERNALS__" in window;

export function SessionPanel({
  state,
  lastText,
  pushToTalk,
  handsFree,
  toggleMode,
}: Props) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const [copyError, setCopyError] = useState<string | null>(null);

  useEffect(() => {
    setCopyState("idle");
    setCopyError(null);
  }, [lastText]);

  const stateHint =
    state === "idle"
      ? `録音を開始するには ${formatHotkey(pushToTalk)} を押してください`
      : state === "recording"
        ? "ショートカットを押すと録音を停止します"
        : "音声をテキストに変換しています";

  const handleCopy = async () => {
    if (!lastText) return;
    try {
      if (isTauri) {
        await invoke("copy_text", { text: lastText });
      } else {
        await navigator.clipboard?.writeText(lastText);
      }
      setCopyError(null);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1600);
    } catch (error) {
      setCopyError(String(error));
      setCopyState("error");
    }
  };

  return (
    <div className={`session-panel state-${state}`}>
      <div className="session-status" aria-live="polite">
        <div className="microphone-orbit">
          <Mic48Regular />
        </div>
        <h2>{stateLabel[state]}</h2>
        <p>{stateHint}</p>
      </div>

      <div className="latest-text-section">
        <div className="section-heading">
          <p className="section-label">最後に入力したテキスト</p>
          <button
            className={`copy-button ${copyState === "copied" ? "is-copied" : ""}`}
            onClick={handleCopy}
            disabled={!lastText}
          >
            {copyState === "copied" ? <Checkmark20Regular /> : <Copy20Regular />}
            {copyState === "copied" ? "コピー済み" : "コピー"}
          </button>
        </div>
        <div className={`latest-text ${lastText ? "" : "is-empty"}`}>
          {lastText ?? "音声入力が完了すると、ここにテキストが表示されます。"}
        </div>
        {copyError && (
          <p className="field-error" role="alert">
            コピーに失敗しました: {copyError}
          </p>
        )}
      </div>

      <div className="shortcut-strip">
        <Shortcut binding={pushToTalk} label="押している間録音" />
        <Shortcut binding={handsFree} label="ハンズフリー" />
        <Shortcut binding={toggleMode} label="モード切替" />
      </div>
    </div>
  );
}

function Shortcut({
  binding,
  label,
}: {
  binding: HotkeyBinding;
  label: string;
}) {
  return (
    <div className="shortcut">
      <div className="shortcut-keys">
        {hotkeyParts(binding).map((key, index, keys) => (
          <Fragment key={`${key}-${index}`}>
            <kbd>{key}</kbd>
            {index < keys.length - 1 && <span className="plus">+</span>}
          </Fragment>
        ))}
      </div>
      <span className="shortcut-label">{label}</span>
    </div>
  );
}
