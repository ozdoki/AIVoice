import { Fragment, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Checkmark20Regular,
  Copy20Regular,
  Mic48Regular,
} from "@fluentui/react-icons";
import {
  type HotkeyBinding,
  type PolishState,
  type RecordingState,
  type SessionPhase,
  formatHotkey,
  hotkeyParts,
  isPolishFallback,
  polishStateDetail,
  polishStateLabel,
} from "../types";

interface Props {
  state: RecordingState;
  phase: SessionPhase;
  lastText: string | null;
  rawText: string | null;
  polishState: PolishState | null;
  elapsedMs: number;
  pushToTalk: HotkeyBinding;
  handsFreeRaw: HotkeyBinding;
  handsFreePolish: HotkeyBinding;
}

const stateLabel: Record<RecordingState, string> = {
  idle: "待機中",
  recording: "録音中",
  processing: "処理中",
};

const isTauri = "__TAURI_INTERNALS__" in window;

export function SessionPanel({
  state,
  phase,
  lastText,
  rawText,
  polishState,
  elapsedMs,
  pushToTalk,
  handsFreeRaw,
  handsFreePolish,
}: Props) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const [copyError, setCopyError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);
  const [injectState, setInjectState] = useState<"idle" | "done" | "error">("idle");

  useEffect(() => {
    setCopyState("idle");
    setCopyError(null);
    setShowRaw(false);
    setInjectState("idle");
  }, [lastText]);

  const canCompareRaw = Boolean(rawText && lastText && rawText !== lastText);
  const stableText = showRaw && canCompareRaw ? rawText : lastText;
  const displayedText = stableText;
  const displayedLabel = showRaw && canCompareRaw ? "Raw テキスト" : "最後に入力したテキスト";
  const visiblePolishState = showRaw ? null : polishState;
  const polishLabel = polishStateLabel(visiblePolishState);
  const phaseLabel: Record<SessionPhase, string> = {
    idle: "待機中",
    recording: "録音中",
    transcribing: "文字起こし中",
    polishing: "整形中",
    injecting: "注入中",
    completed: "完了",
    failed: "失敗",
  };
  const elapsedSeconds = Math.max(0, Math.floor(elapsedMs / 1000));

  const stateHint =
    state === "idle"
      ? `録音を開始するには ${formatHotkey(pushToTalk)} を押してください`
      : state === "recording"
        ? "ショートカットを押すと録音を停止します"
        : "音声をテキストに変換しています";

  const handleCopy = async () => {
    if (!displayedText) return;
    try {
      if (isTauri) {
        await invoke("copy_text", { text: displayedText });
      } else {
        await navigator.clipboard?.writeText(displayedText);
      }
      setCopyError(null);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1600);
    } catch (error) {
      setCopyError(String(error));
      setCopyState("error");
    }
  };

  const handleInject = async () => {
    if (!displayedText || !isTauri) return;
    try {
      await invoke("inject_text", { text: displayedText });
      setInjectState("done");
      window.setTimeout(() => setInjectState("idle"), 1600);
    } catch (error) {
      setCopyError(`再注入に失敗しました: ${error}`);
      setInjectState("error");
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
        <div className="session-phase-row" aria-label="現在の処理段階">
          <span className={`phase-chip phase-${phase}`}>{phaseLabel[phase]}</span>
          {state !== "idle" && <span className="elapsed-time">{elapsedSeconds}秒</span>}
        </div>
      </div>

      <div className="latest-text-section">
        <div className="section-heading">
          <div className="section-label-row">
            <p className="section-label">{displayedLabel}</p>
            {polishLabel && (
              <span
                className={`polish-result-chip ${
                  isPolishFallback(visiblePolishState) ? "is-fallback" : ""
                }`}
                title={polishStateDetail(visiblePolishState)}
              >
                {polishLabel}
              </span>
            )}
          </div>
          <div className="latest-text-actions">
            {canCompareRaw && (
              <button
                className="copy-button"
                onClick={() => setShowRaw((current) => !current)}
                disabled={state !== "idle"}
              >
                {showRaw ? "Final" : "Raw"}
              </button>
            )}
            <button
              className={`copy-button ${injectState === "done" ? "is-copied" : ""}`}
              onClick={handleInject}
              disabled={!displayedText || state !== "idle"}
            >
              {injectState === "done" ? "再注入済み" : "再注入"}
            </button>
            <button
              className={`copy-button ${copyState === "copied" ? "is-copied" : ""}`}
              onClick={handleCopy}
              disabled={!displayedText}
            >
              {copyState === "copied" ? <Checkmark20Regular /> : <Copy20Regular />}
              {copyState === "copied" ? "コピー済み" : "コピー"}
            </button>
          </div>
        </div>
        <div className={`latest-text ${displayedText ? "" : "is-empty"}`}>
          {displayedText ?? "音声入力が完了すると、ここにテキストが表示されます。"}
        </div>
        {copyError && (
          <p className="field-error" role="alert">
            コピーに失敗しました: {copyError}
          </p>
        )}
      </div>

      <div className="shortcut-strip">
        <Shortcut binding={pushToTalk} label="押している間録音" />
        <Shortcut binding={handsFreeRaw} label="Rawハンズフリー" />
        <Shortcut binding={handsFreePolish} label="Polishハンズフリー" />
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
