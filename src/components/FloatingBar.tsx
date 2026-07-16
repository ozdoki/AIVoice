import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Stop16Filled } from "@fluentui/react-icons";
import {
  type AppSettings,
  type Mode,
  type PolishPreset,
  type RecordingState,
  type SessionPhase,
  defaultSettings,
  formatHotkey,
} from "../types";
import { shouldClearFloatingBarTerminalTimer } from "../floatingBarTimerPolicy";
import { runFloatingBarTerminalPresentation } from "../floatingBarTerminalController";

interface SessionUiEvent {
  state: RecordingState;
  mode: Mode;
  polish_preset?: PolishPreset | null;
  phase?: SessionPhase;
  final_text: string | null;
  history_id: string | null;
  error: string | null;
}

interface LiveTranscriptStatusEvent {
  state: string;
  detail?: string | null;
}

const isTauri = "__TAURI_INTERNALS__" in window;

// 波形バーの基準ゲイン（中央ほど高く）
const BAR_GAINS = [0.42, 0.7, 0.92, 1.0, 0.92, 0.7, 0.42];
const BAR_MIN_H = 8;
const BAR_MAX_H = 38;
const WAVEFORM_DISPLAY_GAIN = 95;
const WAVEFORM_NOISE_FLOOR = 0.0008;
const POLISH_PRESETS: Array<{ value: PolishPreset; label: string }> = [
  { value: "slack", label: "Slack" },
  { value: "email", label: "Mail" },
  { value: "memo", label: "Memo" },
  { value: "prompt", label: "Prompt" },
  { value: "technical", label: "Tech" },
];
const FLOATING_BAR_COMPACT_HEIGHT = 104;
const FLOATING_BAR_POLISH_HEIGHT = 150;
const FLOATING_BAR_LIVE_HEIGHT = 170;
const FLOATING_BAR_LIVE_POLISH_HEIGHT = 214;

function getFloatingBarHeight(
  isRecording: boolean,
  nextMode: Mode,
  showLiveStrip: boolean
) {
  if (!isRecording) return FLOATING_BAR_COMPACT_HEIGHT;
  if (showLiveStrip && nextMode === "polish") return FLOATING_BAR_LIVE_POLISH_HEIGHT;
  if (showLiveStrip) return FLOATING_BAR_LIVE_HEIGHT;
  if (nextMode === "polish") return FLOATING_BAR_POLISH_HEIGHT;
  return FLOATING_BAR_COMPACT_HEIGHT;
}

function hasVisibleLiveStrip(
  showLiveTranscriptInFloatingBar: boolean,
  transcript: string,
  error: string
) {
  return (
    showLiveTranscriptInFloatingBar &&
    (Boolean(transcript.trim()) || Boolean(error.trim()))
  );
}

async function resizeAndPositionFloatingWindow(height: number) {
  await invoke("resize_and_position_floating_bar", { height });
}

export function FloatingBar() {
  const [recordingState, setRecordingState] = useState<RecordingState>(
    isTauri ? "idle" : "recording"
  );
  const [mode, setMode] = useState<Mode>("raw");
  const [phase, setPhase] = useState<SessionPhase>(isTauri ? "idle" : "recording");
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [activePolishPreset, setActivePolishPreset] = useState<PolishPreset>(
    defaultSettings.polish_preset
  );
  const [liveTranscript, setLiveTranscript] = useState("");
  const [liveTranscriptError, setLiveTranscriptError] = useState("");
  const levelRef = useRef(isTauri ? 0 : 0.32);
  const liveStripRef = useRef<HTMLDivElement>(null);
  const terminalHideTimerRef = useRef<number | null>(null);
  const sessionEventEpochRef = useRef(0);
  const [displayLevel, setDisplayLevel] = useState(isTauri ? 0 : 0.32);

  // 透明背景（ピル以外が透ける）
  useEffect(() => {
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => () => {
    if (
      shouldClearFloatingBarTerminalTimer("component_unmount") &&
      terminalHideTimerRef.current !== null
    ) {
      window.clearTimeout(terminalHideTimerRef.current);
      terminalHideTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    let unlistener: (() => void) | undefined;
    let disposed = false;
    const win = getCurrentWindow();

    invoke<AppSettings>("get_settings")
      .then((loaded) => {
        setSettings(loaded);
        setMode(loaded.mode);
      })
      .catch((error) => console.error(error));

    listen<AppSettings>("settings://changed", (event) => {
      setSettings(event.payload);
      setMode(event.payload.mode);
      setActivePolishPreset(event.payload.polish_preset);
      if (!event.payload.show_floating_bar) {
        win.hide().catch((error) => console.error(error));
      }
    }).then((off) => {
      if (disposed) off(); else unlistener = off;
    });

    return () => {
      disposed = true;
      unlistener?.();
    };
  }, []);

  // セッション状態リスナー
  useEffect(() => {
    if (!isTauri) return;

    const win = getCurrentWindow();
    let unlistener: (() => void) | undefined;
    let disposed = false;

    listen<SessionUiEvent>("session://state-changed", async (event) => {
      const eventEpoch = ++sessionEventEpochRef.current;
      const { state, mode: newMode, phase: nextPhase, polish_preset } = event.payload;
      if (
        shouldClearFloatingBarTerminalTimer("new_session_event") &&
        terminalHideTimerRef.current !== null
      ) {
        window.clearTimeout(terminalHideTimerRef.current);
        terminalHideTimerRef.current = null;
      }
      setRecordingState(state);
      setMode(newMode);
      if (polish_preset) {
        setActivePolishPreset(polish_preset);
      }
      setPhase(nextPhase ?? (state === "recording" ? "recording" : state === "processing" ? "transcribing" : "idle"));

      if (state === "recording" && settings.show_floating_bar) {
        setRecordingStartedAt(Date.now());
        setLiveTranscript("");
        setLiveTranscriptError("");
        try {
          await resizeAndPositionFloatingWindow(
            getFloatingBarHeight(
              true,
              newMode,
              false
            )
          );
        } catch { /* モニター取得失敗時はデフォルト位置 */ }
        if (disposed || sessionEventEpochRef.current !== eventEpoch) return;
        await win.show();
      } else if (state === "recording") {
        await win.hide();
      } else if (state === "idle") {
        levelRef.current = 0;
        setDisplayLevel(0);
        setLiveTranscript("");
        setLiveTranscriptError("");
        setActivePolishPreset(settings.polish_preset);
        setRecordingStartedAt(null);
        if (
          (nextPhase === "completed" || nextPhase === "cancelled" || nextPhase === "failed") &&
          settings.show_floating_bar
        ) {
          await runFloatingBarTerminalPresentation({
            epoch: eventEpoch,
            isCurrent: (captured) => !disposed && sessionEventEpochRef.current === captured,
            resize: async () => {
              try {
                await resizeAndPositionFloatingWindow(getFloatingBarHeight(false, newMode, false));
              } catch { /* モニター取得失敗時はデフォルト位置 */ }
            },
            show: () => win.show(),
            scheduleHide: () => {
              terminalHideTimerRef.current = window.setTimeout(() => {
                win.hide().catch((error) => console.error(error));
                terminalHideTimerRef.current = null;
              }, 1200);
            },
          });
        } else {
          await win.hide();
        }
      }
    }).then((off) => { if (disposed) off(); else unlistener = off; });

    return () => {
      disposed = true;
      unlistener?.();
      // 設定変更によるlistener再登録ではterminal hide timerを維持する。
    };
  }, [
    settings.polish_preset,
    settings.show_floating_bar,
    settings.show_live_transcript_in_floating_bar,
  ]);

  useEffect(() => {
    if (!isTauri || recordingState !== "recording" || !settings.show_floating_bar) return;

    resizeAndPositionFloatingWindow(
      getFloatingBarHeight(
        true,
        mode,
        hasVisibleLiveStrip(
          settings.show_live_transcript_in_floating_bar,
          liveTranscript,
          liveTranscriptError
        )
      )
    ).catch((error) => console.error(error));
  }, [
    liveTranscript,
    liveTranscriptError,
    mode,
    recordingState,
    settings.show_floating_bar,
    settings.show_live_transcript_in_floating_bar,
  ]);

  useEffect(() => {
    if (!isTauri) return;
    let partialOff: (() => void) | undefined;
    let statusOff: (() => void) | undefined;
    let disposed = false;

    listen<{ text: string }>("session://partial-text", (event) => {
      setLiveTranscript(event.payload.text);
      if (event.payload.text.trim()) {
        setLiveTranscriptError("");
      }
    }).then((off) => { if (disposed) off(); else partialOff = off; });

    listen<LiveTranscriptStatusEvent>("session://live-transcript-status", (event) => {
      if (event.payload.state === "error") {
        setLiveTranscriptError(event.payload.detail ?? "録音中の文字表示を開始できませんでした。");
      }
    }).then((off) => { if (disposed) off(); else statusOff = off; });

    return () => {
      disposed = true;
      partialOff?.();
      statusOff?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    let phaseOff: (() => void) | undefined;
    let disposed = false;

    listen<{ phase: SessionPhase }>("session://phase-changed", (event) => {
      setPhase(event.payload.phase);
    }).then((off) => { if (disposed) off(); else phaseOff = off; });

    return () => {
      disposed = true;
      phaseOff?.();
    };
  }, []);

  // 音量レベルリスナー（60fps でスムーズに追従）
  useEffect(() => {
    if (!isTauri) return;

    let unlistener: (() => void) | undefined;
    let disposed = false;
    let rafId: number;

    listen<number>("audio://level", (event) => {
      levelRef.current = event.payload;
    }).then((off) => { if (disposed) off(); else unlistener = off; });

    const tick = () => {
      setDisplayLevel((prev) => prev * 0.25 + levelRef.current * 0.75);
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);

    return () => {
      disposed = true;
      unlistener?.();
      cancelAnimationFrame(rafId);
    };
  }, []);

  const handleStop = async () => {
    if (!isTauri) {
      setRecordingState("idle");
      return;
    }
    try { await invoke("stop_recording_session"); } catch (e) { console.error(e); }
  };

  const isRecording = recordingState === "recording";
  const isProcessing = recordingState === "processing";
  const elapsedSeconds = recordingStartedAt
    ? Math.max(0, Math.floor((now - recordingStartedAt) / 1000))
    : 0;
  const phaseLabel: Record<SessionPhase, string> = {
    idle: "待機中",
    recording: "録音中",
    transcribing: "文字起こし中",
    polishing: "整形中",
    injecting: "注入中",
    completed: "完了",
    cancelled: "録音をキャンセルしました",
    failed: "失敗",
  };
  const showLiveTranscript =
    isRecording &&
    settings.show_live_transcript_in_floating_bar &&
    Boolean(liveTranscript.trim());
  const showLiveTranscriptError =
    isRecording &&
    settings.show_live_transcript_in_floating_bar &&
    Boolean(liveTranscriptError.trim()) &&
    !showLiveTranscript;
  const showPolishPresetControl = isRecording && mode === "polish";

  useEffect(() => {
    if (!showLiveTranscript) return;
    window.requestAnimationFrame(() => {
      const strip = liveStripRef.current;
      if (strip) {
        strip.scrollTop = strip.scrollHeight;
      }
    });
  }, [liveTranscript, showLiveTranscript]);

  return (
    <div className="floating-stage">
      <div className="floating-hint">
        <kbd>{formatHotkey(settings.push_to_talk_hotkey)}</kbd>
        <span>を長押しして音声入力</span>
      </div>
      <div className={`floating-pill ${isProcessing ? "is-processing" : ""}`}>
        <span className={`recording-dot ${isRecording ? "is-active" : ""}`} />
        <span className="floating-mode">{mode === "polish" ? "Polish" : "Raw"}</span>

        <div className="floating-waveform">
          {isProcessing ? (
            [0, 1, 2].map((i) => (
              <span key={i} className="processing-bar" style={{
                animation: `dot-bounce 1.1s ${i * 0.18}s ease-in-out infinite`,
              }} />
            ))
          ) : (
            BAR_GAINS.map((gain, i) => {
              const boostedLevel = Math.max(0, displayLevel - WAVEFORM_NOISE_FLOOR);
              const responsiveLevel = Math.min(
                1,
                Math.pow(boostedLevel * WAVEFORM_DISPLAY_GAIN, 0.45) * 1.45
              );
              const h = isRecording
                ? Math.max(
                    BAR_MIN_H,
                    Math.min(
                      BAR_MAX_H,
                      BAR_MIN_H + responsiveLevel * (BAR_MAX_H - BAR_MIN_H) * gain
                    )
                  )
                : BAR_MIN_H;
              return (
                <span key={i} className="waveform-bar" style={{
                  height: h,
                  opacity: isRecording ? 0.5 + gain * 0.5 : 0.2,
                }} />
              );
            })
          )}
        </div>

        <span className="floating-status">
          {phaseLabel[phase]}
          {isRecording && ` ${elapsedSeconds}s`}
        </span>

        {isRecording && (
          <button
            className="floating-stop"
            onClick={handleStop}
            title="停止"
            aria-label="録音を停止"
          >
            <Stop16Filled />
          </button>
        )}
      </div>
      {showLiveTranscript && (
        <div className="floating-live-strip" ref={liveStripRef}>
          <span>{liveTranscript}</span>
        </div>
      )}
      {showPolishPresetControl && (
        <div className="floating-preset-strip" aria-label="録音開始時に固定されたPolishプリセット" title="録音開始時に固定されます。次回の録音前に設定してください。">
          {POLISH_PRESETS.map((preset) => (
            <button
              key={preset.value}
              className={activePolishPreset === preset.value ? "is-active" : ""}
              disabled
              type="button"
            >
              {preset.label}
            </button>
          ))}
        </div>
      )}
      {showLiveTranscriptError && (
        <div className="floating-live-strip is-error">
          <span>ライブ文字表示: {liveTranscriptError}</span>
        </div>
      )}
    </div>
  );
}
