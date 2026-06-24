import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalPosition } from "@tauri-apps/api/dpi";
import { Stop16Filled } from "@fluentui/react-icons";
import { type AppSettings, defaultSettings, formatHotkey } from "../types";

type RecordingState = "idle" | "recording" | "processing";
type Mode = "raw" | "polish";

interface SessionUiEvent {
  state: RecordingState;
  mode: Mode;
  final_text: string | null;
  history_id: string | null;
  error: string | null;
}

const isTauri = "__TAURI_INTERNALS__" in window;

// 波形バーの基準ゲイン（中央ほど高く）
const BAR_GAINS = [0.42, 0.7, 0.92, 1.0, 0.92, 0.7, 0.42];
const BAR_MIN_H = 5;
const BAR_MAX_H = 36;

export function FloatingBar() {
  const [recordingState, setRecordingState] = useState<RecordingState>(
    isTauri ? "idle" : "recording"
  );
  const [mode, setMode] = useState<Mode>("raw");
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const levelRef = useRef(isTauri ? 0 : 0.32);
  const [displayLevel, setDisplayLevel] = useState(isTauri ? 0 : 0.32);

  // 透明背景（ピル以外が透ける）
  useEffect(() => {
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    let unlistener: (() => void) | undefined;
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
      if (!event.payload.show_floating_bar) {
        win.hide().catch((error) => console.error(error));
      }
    }).then((off) => {
      unlistener = off;
    });

    return () => {
      unlistener?.();
    };
  }, []);

  // セッション状態リスナー
  useEffect(() => {
    if (!isTauri) return;

    const win = getCurrentWindow();
    let unlistener: (() => void) | undefined;

    listen<SessionUiEvent>("session://state-changed", async (event) => {
      const { state, mode: newMode } = event.payload;
      setRecordingState(state);
      setMode(newMode);

      if (state === "recording" && settings.show_floating_bar) {
        try {
          const monitor = await currentMonitor();
          if (monitor) {
            const scale = monitor.scaleFactor;
            const logW = monitor.size.width / scale;
            const logH = monitor.size.height / scale;
            // タスクバー（約48px）のちょい上に配置
            await win.setPosition(new LogicalPosition(logW / 2 - 190, logH - 156));
          }
        } catch { /* モニター取得失敗時はデフォルト位置 */ }
        await win.show();
      } else if (state === "recording") {
        await win.hide();
      } else if (state === "idle") {
        levelRef.current = 0;
        setDisplayLevel(0);
        await win.hide();
      }
    }).then((off) => { unlistener = off; });

    return () => { unlistener?.(); };
  }, [settings.show_floating_bar]);

  // 音量レベルリスナー（60fps でスムーズに追従）
  useEffect(() => {
    if (!isTauri) return;

    let unlistener: (() => void) | undefined;
    let rafId: number;

    listen<number>("audio://level", (event) => {
      levelRef.current = event.payload;
    }).then((off) => { unlistener = off; });

    const tick = () => {
      setDisplayLevel((prev) => prev * 0.6 + levelRef.current * 0.4);
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);

    return () => {
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
              const responsiveLevel = Math.min(1, Math.sqrt(Math.max(0, displayLevel)) * 1.2);
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
    </div>
  );
}
