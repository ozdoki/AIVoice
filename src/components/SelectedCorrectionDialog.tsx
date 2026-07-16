import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  CorrectionArtifact,
  CorrectionPreview,
  PrepareSelectedCorrectionResult,
  SelectedCorrectionCandidate,
} from "../types";
import { validateCorrectionArtifacts } from "../correctionArtifactValidation";

interface Props {
  prepared: PrepareSelectedCorrectionResult;
  onClose: () => void;
  onError: (message: string) => void;
}

export function SelectedCorrectionDialog({ prepared, onClose, onError }: Props) {
  const [historyId, setHistoryId] = useState(prepared.candidates[0]?.id ?? "");
  const [preview, setPreview] = useState<CorrectionPreview | null>(null);
  const [artifacts, setArtifacts] = useState<CorrectionArtifact[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previewRequestRef = useRef(0);
  const cancelInFlightRef = useRef(false);
  const mutationInFlightRef = useRef<"create" | "cancel" | null>(null);
  const candidate = prepared.candidates.find((item) => item.id === historyId) ?? null;
  const unchanged = candidate?.final_text === prepared.selected_text;
  const submittedArtifacts: CorrectionArtifact[] = artifacts.length > 0 ? artifacts : [{ type: "none" }];
  const artifactError = preview && candidate
    ? validateCorrectionArtifacts(submittedArtifacts, {
        appProcess: candidate.app_process,
        mode: candidate.mode,
      })
    : null;

  useEffect(() => {
    const first = dialogRef.current?.querySelector<HTMLElement>(
      "input:not([disabled]), button:not([disabled]), select:not([disabled]), textarea:not([disabled])"
    );
    (first ?? dialogRef.current)?.focus();
  }, []);

  useEffect(() => {
    const requestId = ++previewRequestRef.current;
    setPreview(null);
    setArtifacts([]);
    setError(null);
    if (!historyId || unchanged) {
      if (unchanged) setError("元の出力と選択テキストが同じため保存できません。");
      return;
    }
    setBusy(true);
    invoke<CorrectionPreview>("preview_selected_correction", {
      token: prepared.token,
      historyId,
    })
      .then((next) => {
        if (previewRequestRef.current !== requestId) return;
        setPreview(next);
        setArtifacts(next.default_artifacts);
      })
      .catch((previewError) => {
        if (previewRequestRef.current === requestId) setError(String(previewError));
      })
      .finally(() => {
        if (previewRequestRef.current === requestId) setBusy(false);
      });
    return () => {
      if (previewRequestRef.current === requestId) previewRequestRef.current += 1;
    };
  }, [historyId, prepared.token, unchanged]);

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
    if (!candidate || !preview || unchanged || artifactError || busy) return;
    if (mutationInFlightRef.current !== null) return;
    mutationInFlightRef.current = "create";
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<{ focus_warning: string | null }>("create_selected_correction", {
        token: prepared.token,
        historyId: candidate.id,
        artifacts: submittedArtifacts,
      });
      if (result.focus_warning) onError(result.focus_warning);
      onClose();
    } catch (saveError) {
      setError(String(saveError));
    } finally {
      mutationInFlightRef.current = null;
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
        {candidate && (
          <div className="selected-text-comparison">
            <section>
              <h3>元の出力</h3>
              <div className="latest-text">{candidate.final_text}</div>
            </section>
            <section>
              <h3>選択テキスト（修正後）</h3>
              <div className="latest-text">{prepared.selected_text}</div>
            </section>
          </div>
        )}
        {preview && (
          <ArtifactEditor
            preview={preview}
            artifacts={artifacts}
            mode={candidate?.mode ?? "raw"}
            onChange={setArtifacts}
          />
        )}
        {artifactError && <p className="field-error" role="alert">{artifactError}</p>}
        <small>
          元の出力と選択テキストはローカルへ平文保存されます。語彙と文体例は外部APIへ送信される可能性があります。アプリ範囲の候補は表示中の入力先にだけ適用されます。
        </small>
        {error && <p className="dialog-error" role="alert">{error}</p>}
        <div className="dialog-actions">
          <button className="button primary" onClick={save} disabled={busy || !preview || unchanged || Boolean(artifactError)}>
            確認して保存
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

function ArtifactEditor({ preview, artifacts, mode, onChange }: {
  preview: CorrectionPreview;
  artifacts: CorrectionArtifact[];
  mode: "raw" | "polish";
  onChange: (artifacts: CorrectionArtifact[]) => void;
}) {
  const toggle = (candidate: CorrectionArtifact) => {
    const key = JSON.stringify(candidate);
    const exists = artifacts.some((item) => JSON.stringify(item) === key);
    if (exists) {
      const next = artifacts.filter((item) => JSON.stringify(item) !== key);
      onChange(next.length ? next : [{ type: "none" }]);
    } else {
      onChange([...artifacts.filter((item) => item.type !== "none"), candidate]);
    }
  };
  const update = (index: number, artifact: CorrectionArtifact) =>
    onChange(artifacts.map((item, itemIndex) => itemIndex === index ? artifact : item));

  return (
    <div className="correction-confirmation selected-artifacts">
      <p>適用する学習候補（明示的に選択してください）</p>
      {preview.candidates.map((item, index) => (
        <label className="checkbox-row" key={`${item.type}-${index}`}>
          <input type="checkbox" checked={artifacts.some((value) => JSON.stringify(value) === JSON.stringify(item))} onChange={() => toggle(item)} />
          <span>{artifactLabel(item)}</span>
        </label>
      ))}
      <label className="checkbox-row">
        <input type="radio" checked={artifacts.length === 1 && artifacts[0].type === "none"} onChange={() => onChange([{ type: "none" }])} />
        <span>適用なし（修正例だけ保存）</span>
      </label>
      <div className="latest-text-actions">
        <button className="copy-button" onClick={() => onChange([...artifacts.filter((item) => item.type !== "none"), { type: "vocabulary", value: "", scope: "global" }])}>語彙を追加</button>
        <button className="copy-button" onClick={() => onChange([...artifacts.filter((item) => item.type !== "none"), { type: "replacement", from: preview.original_text, to: preview.corrected_text, scope: "global" }])}>置換を追加</button>
        {mode === "polish" && <button className="copy-button" onClick={() => onChange([...artifacts.filter((item) => item.type !== "none"), { type: "style_example", input: preview.original_text, output: preview.corrected_text }])}>文体例を追加</button>}
      </div>
      {artifacts.map((artifact, index) => artifact.type === "none" ? null : (
        <div className="correction-artifact-editor" key={`${artifact.type}-${index}`}>
          {artifact.type === "vocabulary" && <><input aria-label="学習する語彙" value={artifact.value} maxLength={128} onChange={(event) => update(index, { ...artifact, value: event.target.value })} /><select aria-label="語彙の適用範囲" value={artifact.scope} onChange={(event) => update(index, { ...artifact, scope: event.target.value as "global" | "app" })}><option value="global">全アプリ</option><option value="app">このアプリ</option></select></>}
          {artifact.type === "replacement" && <><input aria-label="置換元" value={artifact.from} maxLength={128} onChange={(event) => update(index, { ...artifact, from: event.target.value })} /><span>→</span><input aria-label="置換先" value={artifact.to} maxLength={2000} onChange={(event) => update(index, { ...artifact, to: event.target.value })} /><select aria-label="置換の適用範囲" value={artifact.scope} onChange={(event) => update(index, { ...artifact, scope: event.target.value as "global" | "app" })}><option value="global">全アプリ</option><option value="app">このアプリ</option></select></>}
          {artifact.type === "style_example" && <><textarea aria-label="文体例の入力" value={artifact.input} maxLength={8000} onChange={(event) => update(index, { ...artifact, input: event.target.value })} /><textarea aria-label="文体例の出力" value={artifact.output} maxLength={8000} onChange={(event) => update(index, { ...artifact, output: event.target.value })} /></>}
          <button className="copy-button" onClick={() => { const next = artifacts.filter((_, itemIndex) => itemIndex !== index); onChange(next.length ? next : [{ type: "none" }]); }}>候補を削除</button>
        </div>
      ))}
    </div>
  );
}

function artifactLabel(artifact: CorrectionArtifact): string {
  if (artifact.type === "replacement") return `置換: ${artifact.from} → ${artifact.to} (${artifact.scope})`;
  if (artifact.type === "vocabulary") return `語彙: ${artifact.value} (${artifact.scope})`;
  if (artifact.type === "style_example") return "Polish文体例";
  return "適用なし";
}
