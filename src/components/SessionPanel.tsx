import { Fragment, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Checkmark20Regular,
  Copy20Regular,
  Mic48Regular,
} from "@fluentui/react-icons";
import {
  type CorrectionArtifact,
  type CorrectionLearningMode,
  type CorrectionPreview,
  type HotkeyBinding,
  type Mode,
  type PolishState,
  type RecordingState,
  type SessionPhase,
  formatHotkey,
  hotkeyParts,
  isPolishFallback,
  polishStateDetail,
  polishStateLabel,
} from "../types";
import { validateCorrectionArtifacts } from "../correctionArtifactValidation";
import {
  commitCorrectionEdit,
  correctionArtifactsForSubmission,
  correctionPreviewArgs,
  correctionSaveArgs,
} from "../correctionLearningFlow";
import {
  addContextualInsertionDraft,
  candidateIsSelected,
  type CorrectionArtifactDraft,
  addManualDraft,
  artifactsFromDrafts,
  draftsFromDefaultArtifacts,
  draftSelectionCounts,
  editCandidateDraft,
  markDraftRevalidation,
  removeDraft,
  setDraftsToNone,
  toggleCandidateDraft,
  toggleRevalidatedDraftSelection,
  updateDraft,
} from "../correctionCandidateState";

interface Props {
  state: RecordingState;
  phase: SessionPhase;
  lastText: string | null;
  rawText: string | null;
  polishState: PolishState | null;
  historyId: string | null;
  resultMode: Mode;
  polishPreset: string;
  correctionLearningMode: CorrectionLearningMode;
  correctionLearningMultiDiffEnabled: boolean;
  elapsedMs: number;
  pushToTalk: HotkeyBinding;
  handsFreeRaw: HotkeyBinding;
  handsFreePolish: HotkeyBinding;
}

const stateLabel: Record<RecordingState, string> = {
  idle: "待機中",
  recording: "録音中",
  processing: "処理中",
};

const isTauri = "__TAURI_INTERNALS__" in window;

export function SessionPanel({
  state,
  phase,
  lastText,
  rawText,
  polishState,
  historyId,
  resultMode,
  polishPreset,
  correctionLearningMode,
  correctionLearningMultiDiffEnabled,
  elapsedMs,
  pushToTalk,
  handsFreeRaw,
  handsFreePolish,
}: Props) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const [copyError, setCopyError] = useState<string | null>(null);
  const [injectError, setInjectError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);
  const [injectState, setInjectState] = useState<"idle" | "done" | "error">("idle");
  const [confirmedText, setConfirmedText] = useState<string | null>(lastText);
  const [isEditing, setIsEditing] = useState(false);
  const [editDraft, setEditDraft] = useState("");
  const [correctionPreview, setCorrectionPreview] = useState<CorrectionPreview | null>(null);
  const [learningDrafts, setLearningDrafts] = useState<CorrectionArtifactDraft[]>([]);
  const learningArtifacts = artifactsFromDrafts(learningDrafts);
  const learningDraftCounts = draftSelectionCounts(learningDrafts);
  const hasUnselectedRevalidationDraft = learningDrafts.some(
    (draft) => draft.revalidation !== undefined && draft.selected !== true,
  );
  const [learningBusy, setLearningBusy] = useState(false);
  const [learningNotice, setLearningNotice] = useState<string | null>(null);
  const confirmationRef = useRef<HTMLDivElement>(null);
  const confirmationReturnFocusRef = useRef<HTMLElement | null>(null);
  const resultIdentity =
    historyId ?? `${lastText ?? ""}\u0000${rawText ?? ""}\u0000${resultMode}\u0000${polishPreset}`;
  const submittedLearningArtifacts = correctionArtifactsForSubmission(learningArtifacts);
  const learningArtifactError = correctionPreview
    ? validateCorrectionArtifacts(submittedLearningArtifacts, { appProcess: correctionPreview.app_process, mode: resultMode })
    : null;

  useEffect(() => {
    setCopyState("idle");
    setCopyError(null);
    setInjectError(null);
    setShowRaw(false);
    setInjectState("idle");
    setConfirmedText(lastText);
    setIsEditing(false);
    setEditDraft(lastText ?? "");
    setCorrectionPreview(null);
    setLearningDrafts(setDraftsToNone());
    setLearningNotice(null);
  }, [resultIdentity]);

  useEffect(() => {
    if (!correctionPreview) return;
    confirmationRef.current?.focus();
  }, [correctionPreview]);

  const canCompareRaw = Boolean(rawText && lastText && rawText !== lastText);
  const stableText = showRaw && canCompareRaw ? rawText : confirmedText;
  const displayedText = stableText;
  const displayedLabel = showRaw && canCompareRaw ? "Raw テキスト" : "最後に入力したテキスト";
  const visiblePolishState = showRaw ? null : polishState;
  const polishLabel = polishStateLabel(visiblePolishState);
  const phaseLabel: Record<SessionPhase, string> = {
    idle: "待機中",
    recording: "録音中",
    transcribing: "文字起こし中",
    polishing: "整形中",
    injecting: "注入中",
    completed: "完了",
    cancelled: "録音をキャンセルしました",
    failed: "失敗",
  };
  const elapsedSeconds = Math.max(0, Math.floor(elapsedMs / 1000));

  const stateHint =
    state === "idle"
      ? `録音を開始するには ${formatHotkey(pushToTalk)} を押してください`
      : state === "recording"
        ? "ショートカットを押すと録音を停止します"
        : "音声をテキストに変換しています";

  const handleCopy = async () => {
    if (!displayedText) return;
    try {
      if (isTauri) {
        await invoke("copy_text", { text: displayedText });
      } else {
        await navigator.clipboard?.writeText(displayedText);
      }
      setCopyError(null);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 1600);
    } catch (error) {
      setCopyError(`コピーに失敗しました: ${error}`);
      setCopyState("error");
    }
  };

  const handleInject = async () => {
    if (!displayedText || !isTauri) return;
    try {
      const warning = await invoke<string | null>("inject_text", { text: displayedText });
      setInjectError(warning);
      setInjectState("done");
      window.setTimeout(() => setInjectState("idle"), 1600);
    } catch (error) {
      setInjectError(`再注入に失敗しました: ${error}`);
      setInjectState("error");
    }
  };

  const handleSaveEdit = () => {
    const transition = commitCorrectionEdit(editDraft);
    if (!transition.ok) {
      setLearningNotice(transition.notice);
      return;
    }
    setConfirmedText(transition.confirmedText);
    setIsEditing(false);
    setCorrectionPreview(null);
    setLearningDrafts(setDraftsToNone());
    setLearningNotice(transition.notice);
  };

  const handlePreviewLearning = async () => {
    const args = correctionPreviewArgs(historyId, confirmedText, lastText);
    if (!args || !isTauri) return;
    setLearningBusy(true);
    setLearningNotice(null);
    try {
      confirmationReturnFocusRef.current = document.activeElement as HTMLElement | null;
      const preview = await invoke<CorrectionPreview>("preview_correction", args);
      setCorrectionPreview(preview);
      setLearningDrafts(
        correctionLearningMultiDiffEnabled ? [] : draftsFromDefaultArtifacts(preview),
      );
    } catch (error) {
      setLearningNotice(String(error));
    } finally {
      setLearningBusy(false);
    }
  };

  const closeCorrectionPreview = () => {
    setCorrectionPreview(null);
    window.setTimeout(() => confirmationReturnFocusRef.current?.focus(), 0);
  };

  const updateLearningArtifact = (id: string, artifact: CorrectionArtifact) => {
    setLearningDrafts((current) => updateDraft(current, id, artifact));
  };

  const revalidateLearningDraft = async (draftId: string) => {
    const draft = learningDrafts.find((item) => item.id === draftId);
    if (!draft || !historyId || !confirmedText || !isTauri) return;
    setLearningBusy(true);
    setLearningNotice(null);
    try {
      await invoke("revalidate_correction_artifacts", {
        historyId,
        correctedText: confirmedText,
        artifacts: [
          ...artifactsFromDrafts(learningDrafts.filter((item) => item.id !== draftId)),
          draft.artifact,
        ],
      });
      setLearningDrafts((current) => markDraftRevalidation(current, draftId, true));
    } catch (error) {
      setLearningDrafts((current) => markDraftRevalidation(current, draftId, false));
      setLearningNotice(String(error));
    } finally {
      setLearningBusy(false);
    }
  };

  const handleSaveLearning = async () => {
    const args = correctionSaveArgs(historyId, confirmedText, learningArtifacts);
    if (!args || !isTauri || learningArtifactError || hasUnselectedRevalidationDraft) return;
    setLearningBusy(true);
    setLearningNotice(null);
    try {
      await invoke("create_correction", args);
      closeCorrectionPreview();
      setLearningNotice("確認した修正内容をローカルに保存しました。");
    } catch (error) {
      setLearningNotice(String(error));
    } finally {
      setLearningBusy(false);
    }
  };

  return (
    <div className={`session-panel state-${state}`}>
      <div className="session-status" aria-live="polite">
        <div className="microphone-orbit">
          <Mic48Regular />
        </div>
        <h2>{stateLabel[state]}</h2>
        <p>{stateHint}</p>
        <div className="session-phase-row" aria-label="現在の処理段階">
          <span className={`phase-chip phase-${phase}`}>{phaseLabel[phase]}</span>
          {state !== "idle" && <span className="elapsed-time">{elapsedSeconds}秒</span>}
        </div>
      </div>

      <div className="latest-text-section">
        <div className="section-heading">
          <div className="section-label-row">
            <p className="section-label">{displayedLabel}</p>
            {polishLabel && (
              <span
                className={`polish-result-chip ${
                  isPolishFallback(visiblePolishState) ? "is-fallback" : ""
                }`}
                title={polishStateDetail(visiblePolishState)}
              >
                {polishLabel}
              </span>
            )}
          </div>
          <div className="latest-text-actions">
            {canCompareRaw && (
              <button
                className="copy-button"
                onClick={() => setShowRaw((current) => !current)}
                disabled={state !== "idle"}
              >
                {showRaw ? "Final" : "Raw"}
              </button>
            )}
            {!showRaw && confirmedText && !isEditing && (
              <button
                className="copy-button"
                onClick={() => {
                  setEditDraft(confirmedText);
                  setIsEditing(true);
                  setLearningNotice(null);
                }}
                disabled={state !== "idle"}
              >
                編集
              </button>
            )}
            <button
              className={`copy-button ${injectState === "done" ? "is-copied" : ""}`}
              onClick={handleInject}
              disabled={!displayedText || state !== "idle"}
            >
              {injectState === "done" ? "再注入済み" : "再注入"}
            </button>
            <button
              className={`copy-button ${copyState === "copied" ? "is-copied" : ""}`}
              onClick={handleCopy}
              disabled={!displayedText}
            >
              {copyState === "copied" ? <Checkmark20Regular /> : <Copy20Regular />}
              {copyState === "copied" ? "コピー済み" : "コピー"}
            </button>
          </div>
        </div>
        {isEditing && !showRaw ? (
          <div className="correction-editor">
            <textarea
              value={editDraft}
              onChange={(event) => setEditDraft(event.target.value)}
              maxLength={20000}
              rows={7}
              aria-label="修正後テキスト"
            />
            <div className="latest-text-actions">
              <button className="copy-button" onClick={handleSaveEdit}>編集を確定</button>
              <button
                className="copy-button"
                onClick={() => {
                  setIsEditing(false);
                  setEditDraft(confirmedText ?? "");
                }}
              >
                キャンセル
              </button>
            </div>
            <small>編集を確定しただけでは学習データへ保存されません。</small>
          </div>
        ) : (
          <div className={`latest-text ${displayedText ? "" : "is-empty"}`}>
            {displayedText ?? "音声入力が完了すると、ここにテキストが表示されます。"}
          </div>
        )}
        {!showRaw && confirmedText && confirmedText !== lastText && !isEditing && (
          <div className="correction-learning-actions">
            <button
              className="copy-button"
              onClick={handlePreviewLearning}
              disabled={
                learningBusy ||
                correctionLearningMode === "off" ||
                !historyId ||
                state !== "idle"
              }
              title={
                correctionLearningMode === "off"
                  ? "設定で「保存前に確認」を有効にしてください"
                  : !historyId
                    ? "元の履歴がないため学習できません"
                    : undefined
              }
            >
              修正を学習
            </button>
            {correctionLearningMode === "off" && <small>修正学習は設定でオフです。</small>}
          </div>
        )}
        {correctionPreview && (
          <div
            className="correction-confirmation"
            role="dialog"
            aria-label="修正学習の確認"
            tabIndex={-1}
            ref={confirmationRef}
            onKeyDown={(event) => {
              if (event.key === "Escape") closeCorrectionPreview();
            }}
          >
            <h3>保存前に確認</h3>
            <p><strong>元の出力</strong></p>
            <div className="latest-text">{correctionPreview.original_text}</div>
            <p><strong>修正後</strong></p>
            <div className="latest-text">{correctionPreview.corrected_text}</div>
            <small>
              分類: {correctionPreview.classification} / モード: {resultMode} / プリセット: {polishPreset}
            </small>
            <p>適用する候補（複数選択可）</p>
            <p>
              抽出結果：{correctionPreview.total_candidates}件（保存可能：
              {correctionPreview.candidates.filter((candidate) => candidate.status === "eligible").length}件／要確認：
              {correctionPreview.candidates.filter((candidate) => candidate.status === "needs_review").length}件／自動学習対象外：
              {correctionPreview.candidates.filter((candidate) => candidate.status === "unsupported").length}件）
            </p>
            <p aria-live="polite">
              保存対象：{learningDraftCounts.selected}件
              {learningDraftCounts.pending > 0 && `／再検証待ち：${learningDraftCounts.pending}件`}
              {learningDraftCounts.invalid > 0 && `／再検証失敗：${learningDraftCounts.invalid}件`}
            </p>
            <small>この修正履歴から保存できる学習項目は最大16件です。</small>
            {correctionPreview.candidates.length === 0 ? (
              <small>安全に適用できる候補を判定できないため、既定は「適用なし」です。</small>
            ) : (
              correctionPreview.candidates.map((candidate, index) => (
                <label key={candidate.id} className="checkbox-row">
                  <input
                    type="checkbox"
                    checked={candidateIsSelected(learningDrafts, candidate)}
                    disabled={candidate.status !== "eligible"}
                    onChange={() => setLearningDrafts((current) => toggleCandidateDraft(current, candidate))}
                  />
                  <span>
                    候補{index + 1}：{candidate.artifact.type === "replacement"
                      ? `置換: ${candidate.artifact.from} → ${candidate.artifact.to} (${candidate.artifact.scope})`
                      : candidate.artifact.type === "vocabulary"
                        ? `語彙: ${candidate.artifact.value} (${candidate.artifact.scope})`
                        : candidate.artifact.type === "style_example"
                          ? "Polish文体例"
                          : "適用なし"} / {correctionCandidateStatusLabel(candidate)}
                    {candidate.occurrence_count > 1 && ` (${candidate.occurrence_count}箇所)`}
                    {candidate.status !== "eligible" && <small>{candidate.reason || "安全確認が必要なため選択できません。"}</small>}
                    {candidate.warnings.map((warning) => <small key={warning}>{warning}</small>)}
                    {candidate.status === "needs_review" && (
                      <button
                        type="button"
                        className="copy-button"
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          setLearningDrafts((current) => editCandidateDraft(current, candidate));
                        }}
                        disabled={learningDrafts.length >= 16}
                      >
                        編集して再検証
                      </button>
                    )}
                    {candidate.reason_code === "pure_insertion" && (
                      <>
                        <small>元の文章に置換元がないため、そのままでは追加位置を特定できません。</small>
                        <button
                          type="button"
                          className="copy-button"
                          onClick={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            setLearningDrafts((current) =>
                              addContextualInsertionDraft(current, correctionPreview, candidate)
                            );
                          }}
                          disabled={learningDrafts.length >= 16}
                        >
                          前後を含む部分置換として追加
                        </button>
                      </>
                    )}
                  </span>
                </label>
              ))
            )}
            {(correctionPreview.warnings.length > 0 || correctionPreview.omitted_candidates > 0) && (
              <div className="dialog-warning" role="status">
                {correctionPreview.warnings.map((warning) => <small key={warning}>{warning}</small>)}
                {correctionPreview.omitted_candidates > 0 && (
                  <small>先頭{correctionPreview.candidates.length}件を表示し、{correctionPreview.omitted_candidates}件を安全のため省略しました。</small>
                )}
              </div>
            )}
            <label className="checkbox-row">
              <input
                type="radio"
                checked={learningDrafts.length === 0}
                onChange={() => setLearningDrafts(setDraftsToNone())}
              />
              <span>適用なし（修正例だけ保存）</span>
            </label>
            <div className="latest-text-actions">
              <button
                className="copy-button"
                onClick={() =>
                  setLearningDrafts((current) =>
                    correctionPreview ? addManualDraft(current, correctionPreview, "vocabulary") : current
                  )
                }
              >
                語彙を追加
              </button>
              <button
                className="copy-button"
                onClick={() =>
                  setLearningDrafts((current) =>
                    correctionPreview ? addManualDraft(current, correctionPreview, "replacement") : current
                  )
                }
              >
                部分置換を追加
              </button>
              {resultMode === "polish" && (
                <button
                  className="copy-button"
                  onClick={() =>
                    setLearningDrafts((current) =>
                      correctionPreview ? addManualDraft(current, correctionPreview, "style_example") : current
                    )
                  }
                >
                  文体例を追加
                </button>
              )}
            </div>
            {learningDrafts.map((draft) => {
              const { id, artifact } = draft;
              if (artifact.type === "none") return null;
              return (
                <div className="correction-artifact-editor" key={id}>
                  {artifact.type === "vocabulary" && (
                    <>
                      <input
                        value={artifact.value}
                        maxLength={128}
                        placeholder="学習する語彙"
                        onChange={(event) =>
                          updateLearningArtifact(id, { ...artifact, value: event.target.value })
                        }
                      />
                      <select
                        value={artifact.scope}
                        onChange={(event) =>
                          updateLearningArtifact(id, {
                            ...artifact,
                            scope: event.target.value as "global" | "app",
                          })
                        }
                      >
                        <option value="global">全アプリ</option>
                        <option value="app">このアプリ</option>
                      </select>
                    </>
                  )}
                  {artifact.type === "replacement" && (
                    <>
                      <input
                        value={artifact.from}
                        maxLength={128}
                        aria-label="置換元"
                        onChange={(event) =>
                          updateLearningArtifact(id, { ...artifact, from: event.target.value })
                        }
                      />
                      <span>→</span>
                      <input
                        value={artifact.to}
                        maxLength={2000}
                        aria-label="置換先"
                        onChange={(event) =>
                          updateLearningArtifact(id, { ...artifact, to: event.target.value })
                        }
                      />
                      <select
                        value={artifact.scope}
                        onChange={(event) =>
                          updateLearningArtifact(id, {
                            ...artifact,
                            scope: event.target.value as "global" | "app",
                          })
                        }
                      >
                        <option value="global">全アプリ</option>
                        <option value="app">このアプリ</option>
                      </select>
                    </>
                  )}
                  {artifact.type === "style_example" && (
                    <>
                      <textarea
                        value={artifact.input}
                        maxLength={8000}
                        aria-label="文体例の入力"
                        onChange={(event) =>
                          updateLearningArtifact(id, { ...artifact, input: event.target.value })
                        }
                      />
                      <textarea
                        value={artifact.output}
                        maxLength={8000}
                        aria-label="文体例の出力"
                        onChange={(event) =>
                          updateLearningArtifact(id, { ...artifact, output: event.target.value })
                        }
                      />
                    </>
                  )}
                  {draft.revalidation && (
                    <div>
                      <button className="copy-button" onClick={() => void revalidateLearningDraft(id)} disabled={learningBusy}>
                        再検証
                      </button>
                      {draft.revalidation === "valid" ? (
                        <label>
                          <input
                            type="checkbox"
                            checked={draft.selected === true}
                            onChange={() => setLearningDrafts((current) => toggleRevalidatedDraftSelection(current, id))}
                          />
                          再検証済み候補を選択
                        </label>
                      ) : (
                        <small>{draft.revalidation === "invalid" ? "再検証に失敗しました。" : "編集後は再検証が必要です。"}</small>
                      )}
                    </div>
                  )}
                  <button
                    className="copy-button"
                    onClick={() =>
                      setLearningDrafts((current) => removeDraft(current, id))
                    }
                  >
                    候補を削除
                  </button>
                </div>
              );
            })}
            <small>
              Raw・元の出力・修正後はローカルへ平文保存されます。語彙と文体例は外部APIへ送信され得ます。置換は端末内だけで適用します。
            </small>
            {learningArtifactError && <p className="field-error" role="alert">{learningArtifactError}</p>}
            <div className="latest-text-actions">
              <button className="copy-button" onClick={handleSaveLearning} disabled={learningBusy || Boolean(learningArtifactError) || hasUnselectedRevalidationDraft}>
                確認して保存
              </button>
              <button className="copy-button" onClick={closeCorrectionPreview}>
                保存しない
              </button>
            </div>
          </div>
        )}
        {learningNotice && <p className="field-help" role="status">{learningNotice}</p>}
        {copyError && (
          <p className="field-error" role="alert">
            {copyError}
          </p>
        )}
        {injectError && (
          <p className="field-error" role="alert">
            {injectError}
          </p>
        )}
      </div>

      <div className="shortcut-strip">
        <Shortcut binding={pushToTalk} label="押している間録音" />
        <Shortcut binding={handsFreeRaw} label="Rawハンズフリー" />
        <Shortcut binding={handsFreePolish} label="Polishハンズフリー" />
      </div>
    </div>
  );
}

function correctionCandidateStatusLabel(candidate: CorrectionPreview["candidates"][number]): string {
  if (candidate.status === "eligible") return "保存可能";
  if (candidate.status === "needs_review") return "要確認";
  if (candidate.reason_code === "pure_insertion") return "自動学習対象外（追加のみ）";
  if (candidate.reason_code === "pure_deletion") return "自動学習対象外（削除のみ）";
  return "自動学習対象外";
}

function Shortcut({
  binding,
  label,
}: {
  binding: HotkeyBinding;
  label: string;
}) {
  return (
    <div className="shortcut">
      <div className="shortcut-keys">
        {hotkeyParts(binding).map((key, index, keys) => (
          <Fragment key={`${key}-${index}`}>
            <kbd>{key}</kbd>
            {index < keys.length - 1 && <span className="plus">+</span>}
          </Fragment>
        ))}
      </div>
      <span className="shortcut-label">{label}</span>
    </div>
  );
}
