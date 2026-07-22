import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  CorrectionArtifact,
  CorrectionCandidate,
  CorrectionPreview,
  ComparisonMode,
  CreateSelectedCorrectionResult,
  PrepareSelectedCorrectionResult,
  SelectedCorrectionCandidate,
  VocabularyCandidateAssociation,
} from "../types";
import { validateCorrectionArtifacts } from "../correctionArtifactValidation";
import {
  type CorrectionArtifactDraft,
  type SelectedCorrectionSourceRange,
  addContextualInsertionDraft,
  addManualDraft,
  artifactsFromDrafts,
  candidateIsSelected,
  draftSelectionCounts,
  editCandidateDraft,
  initialDraftsForPreview,
  legacySelectedPreviewArgs,
  legacySelectedSaveArtifacts,
  markDraftRevalidation,
  removeDraft,
  resetSelectedCorrectionPreviewState,
  setDraftsToNone,
  toggleCandidateDraft,
  toggleRevalidatedDraftSelection,
  updateDraft,
  updateDraftAssociation,
} from "../correctionCandidateState";

interface Props {
  prepared: PrepareSelectedCorrectionResult;
  onClose: () => void;
  onError: (message: string) => void;
}

export function SelectedCorrectionDialog({ prepared, onClose, onError }: Props) {
  const [historyId, setHistoryId] = useState(prepared.candidates[0]?.id ?? "");
  const [preview, setPreview] = useState<CorrectionPreview | null>(null);
  const [drafts, setDrafts] = useState<CorrectionArtifactDraft[]>([]);
  const [sourceRange, setSourceRange] = useState<SelectedCorrectionSourceRange | null>(null);
  const [comparisonMode, setComparisonMode] = useState<ComparisonMode>("full");
  const [recordId, setRecordId] = useState<string | null>(null);
  const [emptyOperation, setEmptyOperation] = useState<"none" | "delete" | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const errorRef = useRef<HTMLParagraphElement>(null);
  const sourceTextareaRef = useRef<HTMLTextAreaElement>(null);
  const previewRequestRef = useRef(0);
  const cancelInFlightRef = useRef(false);
  const mutationInFlightRef = useRef<"create" | "cancel" | null>(null);
  const candidate = prepared.candidates.find((item) => item.id === historyId) ?? null;
  const unchanged = candidate?.final_text === prepared.selected_text;
  const selectedDrafts = drafts.filter((draft) => draft.selected !== false);
  const artifacts = artifactsFromDrafts(selectedDrafts);
  const artifactsRequiringValidation = artifactsFromDrafts(
    selectedDrafts.filter((draft) => !draft.id.startsWith("persisted-unverified-")),
  );
  const submittedArtifacts: CorrectionArtifact[] = !prepared.multi_diff_enabled
    ? legacySelectedSaveArtifacts(artifacts)
    : artifacts;
  const vocabularyAssociations: VocabularyCandidateAssociation[] = selectedDrafts.flatMap((draft, artifactIndex) =>
    draft.artifact.type === "vocabulary" && draft.associatedCandidateId
      ? [{ artifact_index: artifactIndex, candidate_id: draft.associatedCandidateId }]
      : [],
  );
  const vocabularyAssociationError = prepared.multi_diff_enabled && drafts.some(
    (draft) => draft.artifact.type === "vocabulary"
      && !draft.id.startsWith("persisted-unverified-")
      && !draft.associatedCandidateId,
  ) ? "語彙候補には対応する差分候補を選択してください。" : null;
  const existingRecordChanged = Boolean(preview?.target_record_id) && (
    JSON.stringify(artifacts) !== JSON.stringify(preview?.persisted_artifacts ?? [])
    || preview?.corrected_text !== preview?.target_record_corrected_text
  );
  const operation = !prepared.multi_diff_enabled && preview
    ? "create"
    : preview?.target_record_id
    ? (artifacts.length > 0 ? (existingRecordChanged ? "update" : null) : emptyOperation)
    : (artifacts.length > 0 ? "create" : null);
  const artifactError = vocabularyAssociationError ?? (preview && candidate && artifactsRequiringValidation.length > 0
    ? validateCorrectionArtifacts(artifactsRequiringValidation, {
        appProcess: candidate.app_process,
        mode: candidate.mode,
      })
    : null);

  useEffect(() => {
    const first = dialogRef.current?.querySelector<HTMLElement>(
      "input:not([disabled]), button:not([disabled]), select:not([disabled]), textarea:not([disabled])"
    );
    (first ?? dialogRef.current)?.focus();
  }, []);

  useEffect(() => {
    previewRequestRef.current += 1;
    const empty = resetSelectedCorrectionPreviewState();
    setPreview(empty.preview);
    setDrafts(empty.drafts);
    setSourceRange(empty.sourceRange);
    setComparisonMode("full");
    const selectedCandidate = prepared.candidates.find((item) => item.id === historyId);
    setRecordId(selectedCandidate?.existing_records.length === 1 ? selectedCandidate.existing_records[0].id : null);
    setEmptyOperation(null);
    setBusy(false);
    setError(unchanged ? "元の出力と選択テキストが同じため保存できません。" : null);
  }, [historyId, prepared.token, unchanged]);

  const requestPreview = async (range: SelectedCorrectionSourceRange | null) => {
    if (!candidate || unchanged || busy) return;
    const requestId = ++previewRequestRef.current;
    setSourceRange(range);
    setPreview(null);
    setDrafts(setDraftsToNone());
    setError(null);
    setBusy(true);
    try {
      const previewArgs = prepared.multi_diff_enabled ? {
        token: prepared.token,
        historyId: candidate.id,
        comparisonMode,
        sourceStartUtf16: range?.start ?? null,
        sourceEndUtf16: range?.end ?? null,
        sourceDisplayFingerprint: candidate.source_display_fingerprint,
        recordId,
      } : legacySelectedPreviewArgs(prepared.token, candidate);
      const next = await invoke<CorrectionPreview>("preview_selected_correction", previewArgs);
      if (previewRequestRef.current !== requestId) return;
      setPreview(next);
      setRecordId(next.target_record_id);
      setDrafts(initialDraftsForPreview(next));
    } catch (previewError) {
      if (previewRequestRef.current === requestId) setError(String(previewError));
    } finally {
      if (previewRequestRef.current === requestId) setBusy(false);
    }
  };

  useEffect(() => {
    if (!prepared.multi_diff_enabled && candidate && !unchanged) void requestPreview(null);
    // The legacy contract previews once immediately after history selection.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [historyId, prepared.token]);

  const confirmSourceRange = async () => {
    const textarea = sourceTextareaRef.current;
    const start = textarea?.selectionStart ?? 0;
    const end = textarea?.selectionEnd ?? 0;
    if (start >= end) {
      setError("元の出力から学習する選択範囲を選択してください。");
      return;
    }
    await requestPreview({ start, end });
  };

  const sourceSelectionChanged = () => {
    setSourceRange((current) => (current ? null : current));
    setPreview((current) => (current ? null : current));
    setDrafts((current) => (current.length > 0 ? [] : current));
  };

  const changeRecord = (nextRecordId: string | null) => {
    previewRequestRef.current += 1;
    setRecordId(nextRecordId);
    const empty = resetSelectedCorrectionPreviewState();
    setPreview(empty.preview);
    setDrafts(empty.drafts);
    setSourceRange(empty.sourceRange);
    setEmptyOperation(null);
    setError(null);
  };

  const cancel = async () => {
    if (cancelInFlightRef.current || mutationInFlightRef.current !== null) return;
    cancelInFlightRef.current = true;
    mutationInFlightRef.current = "cancel";
    previewRequestRef.current += 1;
    setBusy(true);
    try {
      await invoke("cancel_selected_correction", { token: prepared.token });
      onClose();
    } catch (cancelError) {
      onError(String(cancelError));
      onClose();
    } finally {
      mutationInFlightRef.current = null;
    }
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        void cancel();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          "a[href], input:not([disabled]), button:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])"
        ) ?? []
      ).filter((element) => element.offsetParent !== null);
      if (focusable.length === 0) {
        event.preventDefault();
        dialogRef.current?.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  });

  const save = async () => {
    if (
      !candidate
      || !preview
      || unchanged
      || artifactError
      || !operation
      || busy
      || recordId !== preview.target_record_id
    ) return;
    if (mutationInFlightRef.current !== null) return;
    mutationInFlightRef.current = "create";
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<CreateSelectedCorrectionResult>("create_selected_correction", {
        token: prepared.token,
        historyId: candidate.id,
        comparisonMode,
        sourceStartUtf16: sourceRange?.start ?? null,
        sourceEndUtf16: sourceRange?.end ?? null,
        sourceDisplayFingerprint: candidate.source_display_fingerprint,
        previewFingerprint: preview.preview_fingerprint,
        idempotencyKey: preview.idempotency_key,
        recordId,
        operation,
        artifacts: submittedArtifacts,
        vocabularyAssociations,
      });
      if (result.focus_warning) onError(result.focus_warning);
      onClose();
    } catch (saveError) {
      setError(String(saveError));
      requestAnimationFrame(() => errorRef.current?.focus());
    } finally {
      mutationInFlightRef.current = null;
      setBusy(false);
    }
  };

  const revalidateDraft = async (draftId: string) => {
    if (!candidate || !preview || busy) return;
    const draft = drafts.find((item) => item.id === draftId);
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const validationDrafts = [
        ...drafts.filter((item) => item.id !== draftId && item.selected !== false),
        draft,
      ];
      await invoke("revalidate_selected_correction_artifacts", {
        token: prepared.token,
        historyId: candidate.id,
        comparisonMode,
        sourceStartUtf16: sourceRange?.start ?? null,
        sourceEndUtf16: sourceRange?.end ?? null,
        sourceDisplayFingerprint: candidate.source_display_fingerprint,
        previewFingerprint: preview.preview_fingerprint,
        idempotencyKey: preview.idempotency_key,
        recordId,
        artifacts: artifactsFromDrafts(validationDrafts.map((item) => ({ ...item, selected: true }))),
        vocabularyAssociations: validationDrafts.flatMap((item, artifactIndex) =>
          item.artifact.type === "vocabulary" && item.associatedCandidateId
            ? [{ artifact_index: artifactIndex, candidate_id: item.associatedCandidateId }]
            : [],
        ),
      });
      setDrafts((current) => markDraftRevalidation(current, draftId, true));
    } catch (validationError) {
      setDrafts((current) => markDraftRevalidation(current, draftId, false));
      setError(String(validationError));
      requestAnimationFrame(() => errorRef.current?.focus());
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop selected-correction-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) void cancel();
    }}>
      <div
        className="settings-dialog selected-correction-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="selected-correction-title"
        tabIndex={-1}
        ref={dialogRef}
      >
        <h2 id="selected-correction-title">選択テキストから修正を学習</h2>
        <p>修正元にする直近の音声入力を選んでください。候補が1件でも自動保存はしません。</p>
        {prepared.warning === "external_clipboard_change_preserved" && (
          <p className="dialog-warning" role="status">
            コピー中に別の更新があったため、外部のクリップボード更新を守り、元の内容は復元していません。
          </p>
        )}
        <fieldset className="selected-candidate-list">
          <legend>修正元の音声入力</legend>
          {prepared.candidates.map((item) => (
            <CandidateOption
              key={item.id}
              candidate={item}
              checked={item.id === historyId}
              onChange={() => setHistoryId(item.id)}
            />
          ))}
        </fieldset>
        {candidate && candidate.existing_records.length > 1 && (
          <label>
            編集する既存レコード
            <select value={recordId ?? ""} onChange={(event) => changeRecord(event.target.value || null)}>
              <option value="">選択してください</option>
              {candidate.existing_records.map((record) => (
                <option key={record.id} value={record.id}>
                  {record.status} / {new Date(record.updated_at * 1000).toLocaleString()} / {record.corrected_excerpt}
                </option>
              ))}
            </select>
          </label>
        )}
        {prepared.multi_diff_enabled && <fieldset>
          <legend>比較方式</legend>
          <label><input type="radio" checked={comparisonMode === "full"} onChange={() => { setComparisonMode("full"); setPreview(null); setSourceRange(null); }} />全文から差分を抽出</label>
          <label><input type="radio" checked={comparisonMode === "explicit_range"} onChange={() => { setComparisonMode("explicit_range"); setPreview(null); setSourceRange(null); }} />元の範囲を指定</label>
        </fieldset>}
        {candidate && (
          <div className="selected-text-comparison">
            <section>
              <h3>元の出力（{comparisonMode === "full" ? "全文比較" : "範囲指定"}）</h3>
              <textarea
                ref={sourceTextareaRef}
                className="latest-text"
                value={candidate.source_display_text}
                readOnly
                rows={4}
                aria-label="修正元の音声入力"
                onSelect={sourceSelectionChanged}
              />
              {comparisonMode === "full" ? (
                <button className="copy-button" onClick={() => void requestPreview(null)} disabled={busy || unchanged || (candidate.existing_records.length > 1 && !recordId)}>
                  全文を比較して候補を表示
                </button>
              ) : (
                <button className="copy-button" onClick={() => void confirmSourceRange()} disabled={busy || unchanged || (candidate.existing_records.length > 1 && !recordId)}>
                  選択範囲を確定して候補を表示
                </button>
              )}
              {sourceRange && (
                <small>選択範囲を確認しました（UTF-16: {sourceRange.start}–{sourceRange.end}）。</small>
              )}
              {preview && <button className="copy-button" onClick={() => dialogRef.current?.querySelector<HTMLInputElement>("input[data-candidate-checkbox]:not([disabled])")?.focus()}>候補一覧へ戻る</button>}
            </section>
            <section>
              <h3>選択テキスト（修正後）</h3>
              <div className="latest-text">{prepared.selected_text}</div>
            </section>
          </div>
        )}
        {preview && comparisonMode === "explicit_range" && (
          <section className="selected-text-comparison">
            <div>
              <h3>選択した元の抜粋</h3>
              <div className="latest-text">{preview.original_excerpt ?? ""}</div>
            </div>
            <div>
              <h3>選択した修正後の抜粋</h3>
              <div className="latest-text">{preview.corrected_excerpt ?? prepared.selected_text}</div>
            </div>
            <div>
              <h3>再構成した修正後全文</h3>
              <textarea className="latest-text" value={preview.corrected_text} readOnly rows={4} aria-label="再構成した修正後全文" />
            </div>
          </section>
        )}
        {preview && (
          <ArtifactEditor
            preview={preview}
            drafts={drafts}
            mode={candidate?.mode ?? "raw"}
            onChange={setDrafts}
            onRevalidate={(draftId) => void revalidateDraft(draftId)}
            onJumpToSource={() => sourceTextareaRef.current?.focus()}
          />
        )}
        {preview && (preview.warnings.length > 0 || preview.omitted_candidates > 0) && (
          <div className="dialog-warning" role="status">
            {preview.warnings.map((warning) => <small key={warning}>{warning}</small>)}
            {preview.omitted_candidates > 0 && (
              <small>先頭{preview.candidates.length}件を表示し、{preview.omitted_candidates}件を安全のため省略しました。</small>
            )}
          </div>
        )}
        {preview && preview.original_text !== preview.corrected_text && (
          <p className="dialog-warning" role="status">
            明示した選択範囲の抜粋を元に、修正後全文を再構成して保存します。置換候補は同じ置換元が複数ある場合など安全でないとき省略されます。
          </p>
        )}
        {artifactError && <p className="field-error" role="alert">{artifactError}</p>}
        {preview?.target_record_id && artifacts.length === 0 && (
          <fieldset>
            <legend>すべてのartifactを外す場合</legend>
            <label><input type="radio" checked={emptyOperation === "none"} onChange={() => setEmptyOperation("none")} />適用なし（none）へ更新</label>
            <label><input type="radio" checked={emptyOperation === "delete"} onChange={() => setEmptyOperation("delete")} />既存レコードを削除</label>
          </fieldset>
        )}
        <small>
          元の出力と選択テキストはローカルへ平文保存されます。語彙と文体例は外部APIへ送信される可能性があります。アプリ範囲の候補は表示中の入力先にだけ適用されます。
        </small>
        {error && <p className="dialog-error" role="alert" tabIndex={-1} ref={errorRef}>{error}</p>}
        <div className="dialog-actions">
          <button className="button primary" onClick={save} disabled={busy || !preview || unchanged || Boolean(artifactError) || !operation || artifacts.length > 16 || recordId !== preview?.target_record_id}>
            {preview?.target_record_id ? `変更${artifacts.length}件を保存` : `選択した${artifacts.length}件を保存`}
          </button>
          <button className="button secondary" onClick={() => void cancel()} disabled={busy}>
            キャンセル
          </button>
        </div>
      </div>
    </div>
  );
}

function CandidateOption({ candidate, checked, onChange }: {
  candidate: SelectedCorrectionCandidate;
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <label className="selected-candidate-option">
      <input type="radio" name="selected-history" checked={checked} onChange={onChange} />
      <span>
        <strong>{candidate.app_process || "入力先不明"}</strong>
        {!candidate.same_app && <em>別アプリ</em>}
        <small>{candidate.mode} / {candidate.polish_preset || "プリセットなし"}</small>
        <small>{new Date(candidate.created_at * 1000).toLocaleString()}</small>
        <span className="selected-candidate-excerpt">{candidate.final_text}</span>
      </span>
    </label>
  );
}

function ArtifactEditor({ preview, drafts, mode, onChange, onRevalidate, onJumpToSource }: {
  preview: CorrectionPreview;
  drafts: CorrectionArtifactDraft[];
  mode: "raw" | "polish";
  onChange: (drafts: CorrectionArtifactDraft[]) => void;
  onRevalidate: (draftId: string) => void;
  onJumpToSource: () => void;
}) {
  const counts = draftSelectionCounts(drafts);
  const eligibleCount = preview.candidates.filter((candidate) => candidate.status === "eligible").length;
  const needsReviewCount = preview.candidates.filter((candidate) => candidate.status === "needs_review").length;
  const unsupportedCount = preview.candidates.filter((candidate) => candidate.status === "unsupported").length;
  const toggle = (candidate: CorrectionCandidate) => {
    onChange(toggleCandidateDraft(drafts, candidate));
  };
  const update = (id: string, artifact: CorrectionArtifact) => onChange(updateDraft(drafts, id, artifact));
  const associateVocabulary = (id: string, candidateId: string) =>
    onChange(updateDraftAssociation(drafts, id, candidateId || undefined));
  const moveCandidateFocus = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" && (event.target as HTMLElement).matches("input[data-candidate-checkbox]")) {
      event.preventDefault();
      onJumpToSource();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const controls = Array.from(
      event.currentTarget.querySelectorAll<HTMLInputElement>("input[data-candidate-checkbox]:not([disabled])"),
    );
    const current = controls.indexOf(document.activeElement as HTMLInputElement);
    if (current < 0 || controls.length === 0) return;
    event.preventDefault();
    const delta = event.key === "ArrowDown" ? 1 : -1;
    controls[(current + delta + controls.length) % controls.length].focus();
  };

  return (
    <div className="correction-confirmation selected-artifacts" onKeyDown={moveCandidateFocus}>
      <p>適用する学習候補（明示的に選択してください）</p>
      <p>抽出結果：{preview.total_candidates}件（保存可能：{eligibleCount}件／要確認：{needsReviewCount}件／自動学習対象外：{unsupportedCount}件）</p>
      <p aria-live="polite">
        保存対象：{counts.selected}件
        {counts.pending > 0 && `／再検証待ち：${counts.pending}件`}
        {counts.invalid > 0 && `／再検証失敗：${counts.invalid}件`}
      </p>
      <small>この修正履歴から保存できる学習項目は最大16件です。</small>
      {preview.candidates.map((item, index) => (
        <label className="checkbox-row" key={item.id}>
          <input
            type="checkbox"
            data-candidate-checkbox
            aria-label={`${artifactLabel(item.artifact)} ${item.status} ${item.reason}`}
            checked={candidateIsSelected(drafts, item)}
            disabled={item.status !== "eligible" || (!candidateIsSelected(drafts, item) && drafts.length >= 16)}
            onChange={() => toggle(item)}
          />
          <span>
            候補{index + 1}：{artifactLabel(item.artifact)} / {candidateStatusLabel(item)}{item.persistence_state === "persisted_verified" && " / 保存済み"}{item.occurrence_count > 1 && ` (${item.occurrence_count}箇所)`}
            {item.reason && <small>{item.reason}</small>}
            <small>{item.context_before}［変更］{item.context_after}</small>
            {item.warnings.map((warning) => <small key={warning}>{warning}</small>)}
            {item.status === "needs_review" && (
              <button
                type="button"
                className="copy-button"
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  onChange(editCandidateDraft(drafts, item));
                }}
                disabled={drafts.length >= 16}
              >
                編集して再検証
              </button>
            )}
            {item.reason_code === "pure_insertion" && (
              <>
                <small>元の文章に置換元がないため、そのままでは追加位置を特定できません。</small>
                <button
                  type="button"
                  className="copy-button"
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    onChange(addContextualInsertionDraft(drafts, preview, item));
                  }}
                  disabled={drafts.length >= 16}
                >
                  前後を含む部分置換として追加
                </button>
              </>
            )}
          </span>
        </label>
      ))}
      {preview.persisted_unverified_artifacts.length > 0 && (
        <p className="dialog-warning" role="status">既存の未検証ルールは既定で保持します。削除する場合は下の候補編集から明示的に外してください。</p>
      )}
      <div className="latest-text-actions">
        <button className="copy-button" onClick={() => onChange(addManualDraft(drafts, preview, "vocabulary"))}>語彙を追加</button>
        <button className="copy-button" onClick={() => onChange(addManualDraft(drafts, preview, "replacement"))}>部分置換を追加</button>
        {mode === "polish" && <button className="copy-button" onClick={() => onChange(addManualDraft(drafts, preview, "style_example"))}>文体例を追加</button>}
      </div>
      {drafts.map((draft) => {
        const { id, artifact } = draft;
        return artifact.type === "none" ? null : (
        <div className="correction-artifact-editor" key={id}>
          {artifact.type === "vocabulary" && <><input aria-label="学習する語彙" value={artifact.value} maxLength={128} onChange={(event) => update(id, { ...artifact, value: event.target.value })} /><select aria-label="語彙の適用範囲" value={artifact.scope} onChange={(event) => update(id, { ...artifact, scope: event.target.value as "global" | "app" })}><option value="global">全アプリ</option><option value="app">このアプリ</option></select><select aria-label="語彙に対応する差分候補" value={draft.associatedCandidateId ?? ""} onChange={(event) => associateVocabulary(id, event.target.value)}><option value="">対応候補を選択</option>{preview.candidates.filter((candidate) => candidate.status !== "unsupported" && candidate.corrected_range.start < candidate.corrected_range.end).map((candidate) => <option key={candidate.id} value={candidate.id}>{artifactLabel(candidate.artifact)}</option>)}</select></>}
          {artifact.type === "replacement" && <><input aria-label="置換元" value={artifact.from} maxLength={128} onChange={(event) => update(id, { ...artifact, from: event.target.value })} /><span>→</span><input aria-label="置換先" value={artifact.to} maxLength={2000} onChange={(event) => update(id, { ...artifact, to: event.target.value })} /><select aria-label="置換の適用範囲" value={artifact.scope} onChange={(event) => update(id, { ...artifact, scope: event.target.value as "global" | "app" })}><option value="global">全アプリ</option><option value="app">このアプリ</option></select></>}
          {artifact.type === "style_example" && <><textarea aria-label="文体例の入力" value={artifact.input} maxLength={8000} onChange={(event) => update(id, { ...artifact, input: event.target.value })} /><textarea aria-label="文体例の出力" value={artifact.output} maxLength={8000} onChange={(event) => update(id, { ...artifact, output: event.target.value })} /></>}
          {draft.revalidation && <div>
            <button type="button" className="copy-button" onClick={() => onRevalidate(id)}>再検証</button>
            {draft.revalidation === "valid" ? (
              <label><input type="checkbox" checked={draft.selected === true} onChange={() => onChange(toggleRevalidatedDraftSelection(drafts, id))} />再検証済み候補を選択</label>
            ) : (
              <small>{draft.revalidation === "invalid" ? "再検証に失敗しました。編集して再試行してください。" : "編集後は再検証が必要です。"}</small>
            )}
          </div>}
          <button className="copy-button" onClick={() => onChange(removeDraft(drafts, id))}>候補を削除</button>
        </div>
        );
      })}
    </div>
  );
}

function artifactLabel(artifact: CorrectionArtifact): string {
  if (artifact.type === "replacement") return `置換: ${artifact.from} → ${artifact.to} (${artifact.scope})`;
  if (artifact.type === "vocabulary") return `語彙: ${artifact.value} (${artifact.scope})`;
  if (artifact.type === "style_example") return "Polish文体例";
  return "適用なし";
}

function candidateStatusLabel(candidate: CorrectionCandidate): string {
  if (candidate.status === "eligible") return "保存可能";
  if (candidate.status === "needs_review") return "要確認";
  if (candidate.reason_code === "pure_insertion") return "自動学習対象外（追加のみ）";
  if (candidate.reason_code === "pure_deletion") return "自動学習対象外（削除のみ）";
  return "自動学習対象外";
}
