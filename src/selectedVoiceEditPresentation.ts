export type SelectedVoiceEditPhase = "idle" | "recording" | "processing";
export type SelectedVoiceEditBackendStatus = "idle" | "recording" | "preview";
export type SelectedVoiceEditToggleStatus = "recording" | "preview";

export type SelectedVoiceEditPresentation = { kind: "session" } | {
  kind: "recording" | "processing";
  title: string;
  instruction: string;
};

export function selectedVoiceEditPresentation(
  phase: SelectedVoiceEditPhase
): SelectedVoiceEditPresentation {
  if (phase === "recording") {
    return {
      kind: "recording",
      title: "選択テキストの編集指示を録音中",
      instruction: "F9で編集案生成 / Escapeでキャンセル",
    };
  }
  if (phase === "processing") {
    return {
      kind: "processing",
      title: "編集案を生成中",
      instruction: "音声認識と編集処理を実行中。完了までお待ちください",
    };
  }
  return { kind: "session" };
}

export function selectedVoiceEditToggleStarted(
  current: SelectedVoiceEditPhase
): SelectedVoiceEditPhase {
  return current === "recording" ? "processing" : current;
}

export function selectedVoiceEditPhaseFromBackendStatus(
  status: SelectedVoiceEditBackendStatus
): SelectedVoiceEditPhase {
  return status === "recording" ? "recording" : "idle";
}

export function selectedVoiceEditPhaseAfterToggle(
  result: SelectedVoiceEditToggleStatus,
  backendStatus: SelectedVoiceEditBackendStatus | undefined
): SelectedVoiceEditPhase {
  if (result === "preview") return "idle";
  return backendStatus === undefined
    ? "recording"
    : selectedVoiceEditPhaseFromBackendStatus(backendStatus);
}
