import assert from "node:assert/strict";
import test from "node:test";
import type { CorrectionCandidate } from "../src/types.ts";
import {
  artifactsFromDrafts,
  addContextualInsertionDraft,
  artifactIdentity,
  candidateIsSelected,
  editCandidateDraft,
  draftSelectionCounts,
  initialDraftsForPreview,
  legacySelectedPreviewArgs,
  legacySelectedSaveArtifacts,
  markDraftRevalidation,
  resetSelectedCorrectionPreviewState,
  toggleCandidateDraft,
  toggleRevalidatedDraftSelection,
  updateDraft,
  updateDraftAssociation,
} from "../src/correctionCandidateState.ts";

const candidate: CorrectionCandidate = {
  id: "replacement-0-stable",
  artifact: { type: "replacement", from: "Koe Type", to: "KoeType", scope: "global" },
  source_range: { start: 0, end: 1 },
  corrected_range: { start: 0, end: 1 },
  occurrence_count: 1,
  context_before: "",
  context_after: "",
  status: "eligible",
  reason_code: "none",
  reason: "",
  origin: "automatic",
  persistence_state: "new",
  warnings: [],
};

test("editing a selected candidate detaches it into an unselected manual draft", () => {
  assert.equal(artifactIdentity(candidate.artifact), "replacement\u0000global\u0000Koe Type\u0000KoeType");
  let drafts = toggleCandidateDraft([], candidate);
  assert.equal(candidateIsSelected(drafts, candidate), true);
  assert.equal(drafts[0].source, "candidate");
  drafts = updateDraft(drafts, candidate.id, {
    type: "replacement",
    from: "KoeType",
    to: "Koe Type",
    scope: "app",
  });
  assert.equal(candidateIsSelected(drafts, candidate), false);
  assert.equal(drafts[0].source, "manual");
  assert.deepEqual(artifactsFromDrafts(drafts), []);
  assert.equal(drafts[0].revalidation, "pending");
  drafts = toggleCandidateDraft(drafts, candidate);
  assert.equal(drafts.length, 2);
  assert.deepEqual(draftSelectionCounts(drafts), { selected: 1, pending: 1, invalid: 0 });
});

test("selection counts do not present pending or failed drafts as save targets", () => {
  const drafts = [
    { id: "selected", artifact: candidate.artifact, source: "candidate" as const, selected: true },
    { id: "pending", artifact: candidate.artifact, source: "manual" as const, selected: false, revalidation: "pending" as const },
    { id: "invalid", artifact: candidate.artifact, source: "manual" as const, selected: false, revalidation: "invalid" as const },
  ];
  assert.deepEqual(draftSelectionCounts(drafts), { selected: 1, pending: 1, invalid: 1 });
});

test("pure insertion can be converted into a contextual unselected replacement draft", () => {
  const insertion = {
    ...candidate,
    id: "insertion-1",
    artifact: { type: "none" as const },
    source_range: { start: 2, end: 2 },
    corrected_range: { start: 2, end: 4 },
    context_before: "前文",
    context_after: "後文",
    status: "unsupported" as const,
    reason_code: "pure_insertion" as const,
  };
  const preview = {
    corrected_text: "前文追加後文",
    app_process: "chrome.exe",
  } as Parameters<typeof addContextualInsertionDraft>[1];
  const drafts = addContextualInsertionDraft([], preview, insertion);
  assert.deepEqual(drafts[0].artifact, {
    type: "replacement",
    from: "前文後文",
    to: "前文追加後文",
    scope: "app",
  });
  assert.equal(drafts[0].selected, false);
  assert.equal(drafts[0].revalidation, "pending");
});

test("contextual insertion uses the preview's grapheme ranges", () => {
  const insertion = {
    ...candidate,
    id: "emoji-insertion",
    artifact: { type: "none" as const },
    source_range: { start: 1, end: 1 },
    corrected_range: { start: 1, end: 2 },
    context_before: "前",
    context_after: "後",
    status: "unsupported" as const,
    reason_code: "pure_insertion" as const,
  };
  const preview = {
    corrected_text: "前👨‍👩‍👧後",
    app_process: "",
  } as Parameters<typeof addContextualInsertionDraft>[1];
  const drafts = addContextualInsertionDraft([], preview, insertion);
  assert.deepEqual(drafts[0].artifact, {
    type: "replacement",
    from: "前後",
    to: "前👨‍👩‍👧後",
    scope: "global",
  });
});

test("needs-review candidate enters an editable manual revalidation draft", () => {
  const needsReview = { ...candidate, id: "review-1", status: "needs_review" as const };
  const drafts = editCandidateDraft([], needsReview);
  assert.equal(drafts.length, 1);
  assert.equal(drafts[0].source, "manual");
  assert.equal(candidateIsSelected(drafts, needsReview), false);
  assert.deepEqual(drafts[0].artifact, needsReview.artifact);
  assert.deepEqual(artifactsFromDrafts(drafts), []);
  const failed = markDraftRevalidation(drafts, drafts[0].id, false);
  assert.deepEqual(artifactsFromDrafts(toggleRevalidatedDraftSelection(failed, drafts[0].id)), []);
  const valid = markDraftRevalidation(drafts, drafts[0].id, true);
  assert.deepEqual(artifactsFromDrafts(valid), []);
  const selected = toggleRevalidatedDraftSelection(valid, drafts[0].id);
  assert.deepEqual(artifactsFromDrafts(selected), [needsReview.artifact]);
  const editedAgain = updateDraft(selected, selected[0].id, {
    type: "replacement",
    from: "Koe Type revised",
    to: "KoeType",
    scope: "global",
  });
  assert.deepEqual(artifactsFromDrafts(editedAgain), []);
  assert.equal(editedAgain[0].revalidation, "pending");
});

test("duplicate persisted artifacts consume a verified candidate only once and preserve order", () => {
  const artifact = candidate.artifact;
  const preview = {
    candidates: [{ ...candidate, persistence_state: "persisted_verified" as const }],
    persisted_artifacts: [artifact, artifact],
  } as Parameters<typeof initialDraftsForPreview>[0];
  const drafts = initialDraftsForPreview(preview);
  assert.deepEqual(drafts.map((draft) => draft.id), [candidate.id, "persisted-unverified-0"]);
  assert.deepEqual(artifactsFromDrafts(drafts), [artifact, artifact]);
});

test("a persisted none record starts with no invisible selected draft", () => {
  const preview = {
    candidates: [],
    persisted_artifacts: [{ type: "none" as const }],
  } as Parameters<typeof initialDraftsForPreview>[0];
  const drafts = initialDraftsForPreview(preview);
  assert.deepEqual(drafts, []);
  assert.deepEqual(draftSelectionCounts(drafts), { selected: 0, pending: 0, invalid: 0 });
});

test("changing a validated vocabulary association returns the draft to pending and unselected", () => {
  const drafts = [{
    id: "manual-vocabulary-0",
    artifact: { type: "vocabulary" as const, value: "KoeType", scope: "global" as const },
    source: "manual" as const,
    associatedCandidateId: "candidate-a",
    selected: true,
    revalidation: "valid" as const,
  }];
  const changed = updateDraftAssociation(drafts, drafts[0].id, "candidate-b");
  assert.equal(changed[0].associatedCandidateId, "candidate-b");
  assert.equal(changed[0].selected, false);
  assert.equal(changed[0].revalidation, "pending");
  assert.deepEqual(artifactsFromDrafts(changed), []);
});

test("switching selected history resets source range, preview, and drafts", () => {
  const reset = resetSelectedCorrectionPreviewState();
  assert.deepEqual(reset, { sourceRange: null, preview: null, drafts: [] });
});

test("feature-off selected learning keeps the legacy full preview and none submission contract", () => {
  const selected = {
    id: "history-1",
    source_display_fingerprint: "source-fingerprint",
  } as Parameters<typeof legacySelectedPreviewArgs>[1];
  assert.deepEqual(legacySelectedPreviewArgs("token-1", selected), {
    token: "token-1",
    historyId: "history-1",
    comparisonMode: "full",
    sourceStartUtf16: null,
    sourceEndUtf16: null,
    sourceDisplayFingerprint: "source-fingerprint",
    recordId: null,
  });
  assert.deepEqual(legacySelectedSaveArtifacts([]), [{ type: "none" }]);
});
