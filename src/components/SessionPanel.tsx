type RecordingState = "idle" | "recording" | "processing";

interface Props {
  state: RecordingState;
  lastText: string | null;
}

const stateLabel: Record<RecordingState, string> = {
  idle: "待機中",
  recording: "録音中",
  processing: "処理中",
};

const stateColor: Record<RecordingState, string> = {
  idle: "#555",
  recording: "#c33",
  processing: "#a80",
};

export function SessionPanel({ state, lastText }: Props) {
  return (
    <div
      style={{
        width: "100%",
        maxWidth: "320px",
        background: "#252525",
        borderRadius: "10px",
        padding: "1rem",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "0.5rem",
          marginBottom: "0.75rem",
        }}
      >
        <span
          style={{
            width: "10px",
            height: "10px",
            borderRadius: "50%",
            background: stateColor[state],
            display: "inline-block",
          }}
        />
        <span style={{ fontSize: "0.95rem" }}>{stateLabel[state]}</span>
      </div>
      {lastText && (
        <div>
          <p style={{ fontSize: "0.75rem", color: "#888", marginBottom: "0.3rem" }}>
            最後に注入したテキスト
          </p>
          <p
            style={{
              fontSize: "0.9rem",
              background: "#1a1a1a",
              padding: "0.5rem",
              borderRadius: "6px",
              wordBreak: "break-all",
            }}
          >
            {lastText}
          </p>
        </div>
      )}
    </div>
  );
}
