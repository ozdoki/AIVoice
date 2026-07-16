import assert from "node:assert/strict";
import test from "node:test";
import {
  commitCorrectionEdit,
  correctionArtifactsForSubmission,
  correctionPreviewArgs,
  correctionSaveArgs,
} from "../src/correctionLearningFlow.ts";

test("edited text flows into preview and explicit save command arguments", () => {
  const committed = commitCorrectionEdit("KoeTypeを起動する。\n次の行。");
  assert.equal(committed.ok, true);
  if (!committed.ok) return;

  const preview = correctionPreviewArgs(
    "history-42",
    committed.confirmedText,
    "Koe Typeを起動する。次の行。",
  );
  assert.deepEqual(preview, {
    historyId: "history-42",
    correctedText: "KoeTypeを起動する。\n次の行。",
  });

  const artifacts = [
    { type: "replacement", from: "Koe Type", to: "KoeType", scope: "global" },
  ] as const;
  assert.deepEqual(correctionSaveArgs("history-42", committed.confirmedText, [...artifacts]), {
    historyId: "history-42",
    correctedText: "KoeTypeを起動する。\n次の行。",
    artifacts,
  });
});

test("empty unchanged and artifact-free transitions remain conservative", () => {
  assert.deepEqual(commitCorrectionEdit(" \n "), {
    ok: false,
    notice: "修正後テキストが空です。",
  });
  assert.equal(correctionPreviewArgs("history-1", "same", "same"), null);
  assert.equal(correctionPreviewArgs(null, "changed", "same"), null);
  assert.deepEqual(correctionArtifactsForSubmission([]), [{ type: "none" }]);
  assert.deepEqual(correctionSaveArgs("history-1", "changed", []), {
    historyId: "history-1",
    correctedText: "changed",
    artifacts: [{ type: "none" }],
  });
});
