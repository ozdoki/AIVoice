import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalPosition } from "@tauri-apps/api/dpi";

type RecordingState = "idle" | "recording" | "processing";
type Mode = "raw" | "polish";

interface SessionUiEvent {
  state: RecordingState;
  mode: Mode;
  final_text: string | null;
  error: string | null;
}

// 波形バーの基準ゲイン（中央ほど高く）
const BAR_GAINS = [0.5, 0.8, 1.0, 0.8, 0.5];
const BAR_MIN_H = 3;
const BAR_MAX_H = 22;

export function FloatingBar() {
  const [recordingState, setRecordingState] = useState<RecordingState>("idle");
  const [mode, setMode] = useState<Mode>("raw");
  const levelRef = useRef(0);
  const [displayLevel, setDisplayLevel] = useState(0);

  // 透明背景（ピル以外が透ける）
  useEffect(() => {
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
  }, []);

  // CSS アニメーション定義
  useEffect(() => {
    const style = document.createElement("style");
    style.textContent = `
      @keyframes rec-pulse {
        0%, 100% { opacity: 1; }
        50% { opacity: 0.35; }
      }
      @keyframes dot-bounce {
        0%, 80%, 100% { transform: scaleY(0.4); }
        40% { transform: scaleY(1.0); }
      }
    `;
    document.head.appendChild(style);
    return () => { document.head.removeChild(style); };
  }, []);

  // セッション状態リスナー
  useEffect(() => {
    const win = getCurrentWindow();
    let unlistener: (() => void) | undefined;

    listen<SessionUiEvent>("session://state-changed", async (event) => {
      const { state, mode: newMode } = event.payload;
      setRecordingState(state);
      setMode(newMode);

      if (state === "recording") {
        try {
          const monitor = await win.currentMonitor();
          if (monitor) {
            const scale = monitor.scaleFactor;
            const logW = monitor.size.width / scale;
            const logH = monitor.size.height / scale;
            // タスクバー（約48px）のちょい上に配置
            await win.setPosition(new LogicalPosition(logW / 2 - 150, logH - 116));
          }
        } catch { /* モニター取得失敗時はデフォルト位置 */ }
        await win.show();
      } else if (state === "idle") {
        levelRef.current = 0;
        setDisplayLevel(0);
        await win.hide();
      }
    }).then((off) => { unlistener = off; });

    return () => { unlistener?.(); };
  }, []);

  // 音量レベルリスナー（60fps でスムーズに追従）
  useEffect(() => {
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
    try { await invoke("stop_recording_session"); } catch (e) { console.error(e); }
  };

  const isRecording = recordingState === "recording";
  const isProcessing = recordingState === "processing";

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      width: "100%",
      height: "100%",
      background: "transparent",
    }}>
      {/* ピル本体 */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "0 14px",
          height: 44,
          width: 280,
          background: "rgba(14, 14, 14, 0.94)",
          borderRadius: 100,
          border: "1px solid rgba(255,255,255,0.09)",
          boxShadow: "0 4px 20px rgba(0,0,0,0.55)",
          userSelect: "none",
          WebkitAppRegion: "drag",
        } as React.CSSProperties}
      >
        {/* 録音中インジケーター（赤ドット） */}
        <span style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: isRecording ? "#ff3b30" : "#555",
          flexShrink: 0,
          animation: isRecording ? "rec-pulse 1.4s ease-in-out infinite" : "none",
          WebkitAppRegion: "no-drag",
        } as React.CSSProperties} />

        {/* 波形バー（録音中）/ 処理中ドット */}
        <div style={{
          display: "flex",
          alignItems: "center",
          gap: 3,
          height: BAR_MAX_H + 4,
          flex: 1,
          WebkitAppRegion: "no-drag",
        } as React.CSSProperties}>
          {isProcessing ? (
            // 処理中: 3つのバウンスドット
            [0, 1, 2].map((i) => (
              <span key={i} style={{
                display: "inline-block",
                width: 4,
                height: 16,
                borderRadius: 2,
                background: "rgba(255,255,255,0.45)",
                animation: `dot-bounce 1.1s ${i * 0.18}s ease-in-out infinite`,
                transformOrigin: "center",
              }} />
            ))
          ) : (
            // 録音中 / アイドル: 音量波形バー
            BAR_GAINS.map((gain, i) => {
              const h = isRecording
                ? Math.max(BAR_MIN_H, Math.min(BAR_MAX_H, displayLevel * 90 * gain + BAR_MIN_H))
                : BAR_MIN_H;
              return (
                <span key={i} style={{
                  display: "inline-block",
                  width: 3,
                  height: h,
                  borderRadius: 2,
                  background: isRecording
                    ? `rgba(255,255,255,${0.5 + gain * 0.5})`
                    : "rgba(255,255,255,0.18)",
                  transition: "height 55ms ease-out, background 200ms",
                  flexShrink: 0,
                }} />
              );
            })
          )}
        </div>

        {/* モードバッジ */}
        <span style={{
          fontSize: 10,
          color: "rgba(255,255,255,0.35)",
          letterSpacing: "0.03em",
          flexShrink: 0,
          WebkitAppRegion: "no-drag",
        } as React.CSSProperties}>
          {mode === "polish" ? "Polish" : "Raw"}
        </span>

        {/* 停止ボタン（録音中のみ） */}
        {isRecording && (
          <button
            onClick={handleStop}
            title="停止"
            style={{
              width: 22,
              height: 22,
              borderRadius: "50%",
              background: "rgba(255,59,48,0.18)",
              border: "1px solid rgba(255,59,48,0.4)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              cursor: "pointer",
              flexShrink: 0,
              padding: 0,
              WebkitAppRegion: "no-drag",
            } as React.CSSProperties}
          >
            <span style={{
              width: 7,
              height: 7,
              background: "#ff3b30",
              borderRadius: 1,
              display: "block",
            }} />
          </button>
        )}
      </div>
    </div>
  );
}
