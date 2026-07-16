import assert from "node:assert/strict";
import test from "node:test";
import {
  selectedVoiceEditPhaseAfterToggle,
  selectedVoiceEditPhaseFromBackendStatus,
  selectedVoiceEditPresentation,
  selectedVoiceEditToggleStarted,
} from "../src/selectedVoiceEditPresentation.ts";

test("selected voice edit presents idle, recording, and processing distinctly", () => {
  assert.deepEqual(selectedVoiceEditPresentation("idle"), { kind: "session" });
  assert.deepEqual(selectedVoiceEditPresentation("recording"), {
    kind: "recording",
    title: "選択テキストの編集指示を録音中",
    instruction: "F9で編集案生成 / Escapeでキャンセル",
  });
  assert.deepEqual(selectedVoiceEditPresentation("processing"), {
    kind: "processing",
    title: "編集案を生成中",
    instruction: "音声認識と編集処理を実行中。完了までお待ちください",
  });
});

test("starting a toggle advances recording to processing without hiding idle startup", () => {
  assert.equal(selectedVoiceEditToggleStarted("recording"), "processing");
  assert.equal(selectedVoiceEditToggleStarted("idle"), "idle");
});

test("backend recording remains visible while preview and idle return to session", () => {
  assert.equal(selectedVoiceEditPhaseFromBackendStatus("recording"), "recording");
  assert.equal(selectedVoiceEditPhaseFromBackendStatus("preview"), "idle");
  assert.equal(selectedVoiceEditPhaseFromBackendStatus("idle"), "idle");
});

test("toggle recording result reconciles Escape races with the latest backend status", () => {
  assert.equal(selectedVoiceEditPhaseAfterToggle("recording", "idle"), "idle");
  assert.equal(selectedVoiceEditPhaseAfterToggle("recording", "recording"), "recording");
});

test("toggle result remains a safe fallback when backend status cannot be read", () => {
  assert.equal(selectedVoiceEditPhaseAfterToggle("recording", undefined), "recording");
});

test("preview result always returns to the session presentation", () => {
  assert.equal(selectedVoiceEditPhaseAfterToggle("preview", "recording"), "idle");
  assert.equal(selectedVoiceEditPhaseAfterToggle("preview", "preview"), "idle");
  assert.equal(selectedVoiceEditPhaseAfterToggle("preview", undefined), "idle");
});
