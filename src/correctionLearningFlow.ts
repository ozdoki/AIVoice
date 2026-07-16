import type { CorrectionArtifact } from "./types";

export type CorrectionEditCommit =
  | { ok: true; confirmedText: string; notice: string }
  | { ok: false; notice: string };

export function commitCorrectionEdit(draft: string): CorrectionEditCommit {
  if (!draft.trim()) {
    return { ok: false, notice: "修正後テキストが空です。" };
  }
  return {
    ok: true,
    confirmedText: draft,
    notice: "編集内容を画面に反映しました。まだ学習データには保存していません。",
  };
}

export function correctionPreviewArgs(
  historyId: string | null,
  confirmedText: string | null,
  originalText: string | null,
): { historyId: string; correctedText: string } | null {
  if (!historyId || !confirmedText || confirmedText === originalText) return null;
  return { historyId, correctedText: confirmedText };
}

export function correctionArtifactsForSubmission(
  artifacts: CorrectionArtifact[],
): CorrectionArtifact[] {
  return artifacts.length > 0 ? artifacts : [{ type: "none" }];
}

export function correctionSaveArgs(
  historyId: string | null,
  confirmedText: string | null,
  artifacts: CorrectionArtifact[],
): { historyId: string; correctedText: string; artifacts: CorrectionArtifact[] } | null {
  if (!historyId || !confirmedText) return null;
  return {
    historyId,
    correctedText: confirmedText,
    artifacts: correctionArtifactsForSubmission(artifacts),
  };
}
