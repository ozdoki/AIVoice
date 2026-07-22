import type { CorrectionArtifact, CorrectionCandidate, CorrectionPreview, SelectedCorrectionCandidate } from "./types";

export type CorrectionDraftSource = "candidate" | "manual";

export interface CorrectionArtifactDraft {
  id: string;
  artifact: CorrectionArtifact;
  source: CorrectionDraftSource;
  associatedCandidateId?: string;
  selected?: boolean;
  revalidation?: "pending" | "valid" | "invalid";
}

export function artifactIdentity(artifact: CorrectionArtifact): string {
  switch (artifact.type) {
    case "none":
      return "none";
    case "vocabulary":
      return `vocabulary\u0000${artifact.scope}\u0000${artifact.value}`;
    case "replacement":
      return `replacement\u0000${artifact.scope}\u0000${artifact.from}\u0000${artifact.to}`;
    case "style_example":
      return `style_example\u0000${artifact.input}\u0000${artifact.output}`;
  }
}

export function artifactsFromDrafts(drafts: CorrectionArtifactDraft[]): CorrectionArtifact[] {
  return drafts.filter((draft) => draft.selected !== false).map((draft) => draft.artifact);
}

export function draftSelectionCounts(drafts: CorrectionArtifactDraft[]) {
  return {
    selected: drafts.filter((draft) => draft.selected !== false).length,
    pending: drafts.filter((draft) => draft.revalidation === "pending").length,
    invalid: drafts.filter((draft) => draft.revalidation === "invalid").length,
  };
}

export function candidateIsSelected(
  drafts: CorrectionArtifactDraft[],
  candidate: CorrectionCandidate,
): boolean {
  return drafts.some((draft) => draft.id === candidate.id);
}

export function toggleCandidateDraft(
  drafts: CorrectionArtifactDraft[],
  candidate: CorrectionCandidate,
): CorrectionArtifactDraft[] {
  const exists = drafts.some((draft) => draft.id === candidate.id);
  if (exists) return drafts.filter((draft) => draft.id !== candidate.id);
  return [
    ...drafts,
    { id: candidate.id, artifact: candidate.artifact, source: "candidate", selected: true },
  ];
}

export function updateDraft(
  drafts: CorrectionArtifactDraft[],
  id: string,
  artifact: CorrectionArtifact,
): CorrectionArtifactDraft[] {
  return drafts.map((draft) => {
    if (draft.id !== id) return draft;
    if (draft.source === "manual") {
      return draft.revalidation
        ? { ...draft, artifact, selected: false, revalidation: "pending" }
        : { ...draft, artifact };
    }
    return {
      id: nextManualId(drafts, artifact.type),
      artifact,
      source: "manual",
      selected: false,
      revalidation: "pending",
    };
  });
}

export function markDraftRevalidation(
  drafts: CorrectionArtifactDraft[],
  id: string,
  valid: boolean,
): CorrectionArtifactDraft[] {
  return drafts.map((draft) => draft.id === id
    ? { ...draft, selected: false, revalidation: valid ? "valid" : "invalid" }
    : draft);
}

export function toggleRevalidatedDraftSelection(
  drafts: CorrectionArtifactDraft[],
  id: string,
): CorrectionArtifactDraft[] {
  return drafts.map((draft) => draft.id === id && draft.revalidation === "valid"
    ? { ...draft, selected: !draft.selected }
    : draft);
}

export function updateDraftAssociation(
  drafts: CorrectionArtifactDraft[],
  id: string,
  associatedCandidateId: string | undefined,
): CorrectionArtifactDraft[] {
  return drafts.map((draft) =>
    draft.id === id
      ? draft.revalidation
        ? { ...draft, associatedCandidateId, selected: false, revalidation: "pending" }
        : { ...draft, associatedCandidateId }
      : draft
  );
}

export function removeDraft(
  drafts: CorrectionArtifactDraft[],
  id: string,
): CorrectionArtifactDraft[] {
  return drafts.filter((draft) => draft.id !== id);
}

function nextManualId(drafts: CorrectionArtifactDraft[], type: CorrectionArtifact["type"]): string {
  let suffix = drafts.length;
  while (drafts.some((draft) => draft.id === `manual-${type}-${suffix}`)) suffix += 1;
  return `manual-${type}-${suffix}`;
}

export function editCandidateDraft(
  drafts: CorrectionArtifactDraft[],
  candidate: CorrectionCandidate,
): CorrectionArtifactDraft[] {
  if (drafts.length >= 16) return drafts;
  return [
    ...drafts.filter((draft) => draft.id !== candidate.id),
    {
      id: nextManualId(drafts, candidate.artifact.type),
      artifact: candidate.artifact,
      source: "manual",
      selected: false,
      revalidation: "pending",
    },
  ];
}

export function addManualDraft(
  drafts: CorrectionArtifactDraft[],
  preview: CorrectionPreview,
  type: Exclude<CorrectionArtifact["type"], "none">,
): CorrectionArtifactDraft[] {
  const scope = preview.app_process.trim() ? "app" : "global";
  const artifact: CorrectionArtifact =
    type === "vocabulary"
      ? { type, value: "", scope }
      : type === "replacement"
        ? { type, from: "", to: "", scope }
        : {
            type,
            input: preview.original_excerpt ?? preview.original_text,
            output: preview.corrected_excerpt ?? preview.corrected_text,
          };
  return [...drafts, { id: nextManualId(drafts, type), artifact, source: "manual", selected: true }];
}

export function addContextualInsertionDraft(
  drafts: CorrectionArtifactDraft[],
  preview: CorrectionPreview,
  candidate: CorrectionCandidate,
): CorrectionArtifactDraft[] {
  if (drafts.length >= 16 || candidate.reason_code !== "pure_insertion") return drafts;
  const inserted = splitGraphemes(preview.corrected_text)
    .slice(candidate.corrected_range.start, candidate.corrected_range.end)
    .join("");
  const from = `${candidate.context_before}${candidate.context_after}`;
  const to = `${candidate.context_before}${inserted}${candidate.context_after}`;
  const scope = preview.app_process.trim() ? "app" as const : "global" as const;
  const artifact: CorrectionArtifact = from && inserted
    ? { type: "replacement", from, to, scope }
    : { type: "replacement", from: "", to: "", scope };
  return [...drafts, {
    id: nextManualId(drafts, "replacement"),
    artifact,
    source: "manual",
    selected: false,
    revalidation: "pending",
  }];
}

function splitGraphemes(text: string): string[] {
  const Segmenter = (Intl as unknown as {
    Segmenter?: new (locale?: string, options?: { granularity: "grapheme" }) => {
      segment(value: string): Iterable<{ segment: string }>;
    };
  }).Segmenter;
  if (!Segmenter) return Array.from(text);
  return Array.from(new Segmenter("ja", { granularity: "grapheme" }).segment(text), (part) => part.segment);
}

export function setDraftsToNone(): CorrectionArtifactDraft[] {
  return [];
}

export function draftsFromDefaultArtifacts(preview: CorrectionPreview): CorrectionArtifactDraft[] {
  return preview.default_artifacts
    .filter((artifact) => artifact.type !== "none")
    .map((artifact, index) => ({ id: `legacy-default-${index}`, artifact, source: "candidate", selected: true }));
}

export function legacySelectedPreviewArgs(token: string, candidate: SelectedCorrectionCandidate) {
  return {
    token,
    historyId: candidate.id,
    comparisonMode: "full" as const,
    sourceStartUtf16: null,
    sourceEndUtf16: null,
    sourceDisplayFingerprint: candidate.source_display_fingerprint,
    recordId: null,
  };
}

export function legacySelectedSaveArtifacts(artifacts: CorrectionArtifact[]): CorrectionArtifact[] {
  return artifacts.length > 0 ? artifacts : [{ type: "none" }];
}

export function initialDraftsForPreview(preview: CorrectionPreview): CorrectionArtifactDraft[] {
  if (preview.persisted_artifacts.length === 0) return [];
  let unverifiedIndex = 0;
  const consumedCandidateIds = new Set<string>();
  return preview.persisted_artifacts
    .filter((artifact) => artifact.type !== "none")
    .map((artifact) => {
    const verified = preview.candidates.find(
      (candidate) => candidate.persistence_state === "persisted_verified"
        && !consumedCandidateIds.has(candidate.id)
        && artifactIdentity(candidate.artifact) === artifactIdentity(artifact),
    );
    if (verified) {
      consumedCandidateIds.add(verified.id);
      return { id: verified.id, artifact, source: "candidate" as const, selected: true };
    }
    const id = `persisted-unverified-${unverifiedIndex++}`;
    return { id, artifact, source: "candidate" as const, selected: true };
    });
}

export interface SelectedCorrectionSourceRange {
  start: number;
  end: number;
}

export interface SelectedCorrectionPreviewState {
  sourceRange: SelectedCorrectionSourceRange | null;
  preview: CorrectionPreview | null;
  drafts: CorrectionArtifactDraft[];
}

export function resetSelectedCorrectionPreviewState(): SelectedCorrectionPreviewState {
  return { sourceRange: null, preview: null, drafts: [] };
}
