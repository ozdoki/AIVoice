import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  SelectedVoiceEditPreview,
  SelectedVoiceEditReplaceResult,
} from "../types";
import { shouldFinalizeSelectedVoiceEditExpiry } from "../selectedVoiceEditExpiry";

interface Props {
  preview: SelectedVoiceEditPreview;
  onClose: () => void;
  onError: (message: string) => void;
}

export function SelectedVoiceEditDialog({ preview, onClose, onError }: Props) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const mutationRef = useRef<"replace" | "cancel" | null>(null);
  const expiryPendingRef = useRef(false);
  const expiryFinalizingRef = useRef(false);
  const completedRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("button:not([disabled])")?.focus();
  }, []);

  const finalizeExpiry = async () => {
    if (!shouldFinalizeSelectedVoiceEditExpiry(
      expiryPendingRef.current,
      mutationRef.current,
      completedRef.current,
      expiryFinalizingRef.current
    )) return;
    expiryFinalizingRef.current = true;
    mutationRef.current = "cancel";
    setBusy(true);
    try {
      await invoke("cancel_selected_voice_edit_preview", { token: preview.token });
    } catch {
      // backend側で既にscrub済みでも、UIの機密本文は期限どおり閉じる。
    } finally {
      completedRef.current = true;
      expiryPendingRef.current = false;
      mutationRef.current = null;
      expiryFinalizingRef.current = false;
      setBusy(false);
      onError("選択音声編集のpreviewは10分で期限切れになりました。");
      onClose();
    }
  };

  useEffect(() => {
    const timer = window.setTimeout(() => {
      expiryPendingRef.current = true;
      void finalizeExpiry();
    }, 10 * 60 * 1000);
    return () => window.clearTimeout(timer);
    // tokenごとに一度だけ期限を設定し、React側にも原文を残し続けない。
  }, [preview.token]);

  const cancel = async () => {
    if (mutationRef.current) return;
    mutationRef.current = "cancel";
    setBusy(true);
    try {
      await invoke("cancel_selected_voice_edit_preview", { token: preview.token });
    } catch (error) {
      onError(String(error));
    } finally {
      completedRef.current = true;
      expiryPendingRef.current = false;
      mutationRef.current = null;
      setBusy(false);
      onClose();
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
          "button:not([disabled]), [tabindex]:not([tabindex='-1'])"
        ) ?? []
      ).filter((element) => element.offsetParent !== null);
      if (!focusable.length) {
        event.preventDefault();
        dialogRef.current?.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!dialogRef.current?.contains(document.activeElement)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && document.activeElement === first) {
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

  const replace = async () => {
    if (mutationRef.current || !preview.replace_available) return;
    mutationRef.current = "replace";
    setBusy(true);
    setMessage(null);
    let replaced = false;
    try {
      const result = await invoke<SelectedVoiceEditReplaceResult>(
        "replace_selected_voice_edit",
        { token: preview.token }
      );
      if (result.replaced) {
        replaced = true;
      } else {
        setMessage(result.message);
      }
    } catch (error) {
      setMessage(String(error));
    } finally {
      mutationRef.current = null;
      setBusy(false);
      if (replaced) {
        completedRef.current = true;
        expiryPendingRef.current = false;
        onClose();
      } else if (shouldFinalizeSelectedVoiceEditExpiry(
        expiryPendingRef.current,
        mutationRef.current,
        completedRef.current,
        expiryFinalizingRef.current
      )) {
        void finalizeExpiry();
      }
    }
  };

  const copy = async () => {
    try {
      await invoke("copy_text", { text: preview.proposal });
      setMessage("編集案をクリップボードへコピーしました。");
    } catch (error) {
      setMessage(String(error));
    }
  };

  return (
    <div className="modal-backdrop selected-voice-edit-backdrop">
      <div
        className="settings-dialog selected-voice-edit-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="selected-voice-edit-title"
        tabIndex={-1}
        ref={dialogRef}
      >
        <h2 id="selected-voice-edit-title">選択テキストの音声編集案</h2>
        <p>
          置換前に内容を確認してください。原文と音声指示は設定中の外部APIへ送信されました。
          音声指示と編集案はこのpreviewの表示前にローカル平文履歴へ自動保存されています。
          原選択文は履歴・Recoveryへ別保存しません。
        </p>
        {preview.selection_warning === "external_clipboard_change_preserved" && (
          <p className="dialog-warning" role="status">
            選択取得中に外部のクリップボード更新を保全しました。この取得方式では安全な自動置換を行えません。
          </p>
        )}
        <div className="selected-text-comparison selected-voice-edit-comparison">
          <section><h3>原文</h3><div className="latest-text">{preview.original_text}</div></section>
          <section><h3>音声指示</h3><div className="latest-text">{preview.instruction}</div></section>
          <section><h3>編集案</h3><div className="latest-text">{preview.proposal}</div></section>
        </div>
        {!preview.replace_available && (
          <p className="dialog-warning" role="status">
            このアプリでは選択範囲を厳密に再確認できないため、Copyのみ利用できます。
          </p>
        )}
        {message && <p className="dialog-error" role="alert">{message}</p>}
        <div className="dialog-actions">
          <button className="button primary" onClick={() => void replace()} disabled={busy || !preview.replace_available}>
            元の選択範囲を置換
          </button>
          <button className="button secondary" onClick={() => void copy()} disabled={busy}>Copy</button>
          <button className="button secondary" onClick={() => void cancel()} disabled={busy}>キャンセル</button>
        </div>
      </div>
    </div>
  );
}
