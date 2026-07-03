import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowClockwise20Regular,
  Checkmark20Regular,
  Dismiss20Regular,
  Settings24Regular,
} from "@fluentui/react-icons";
import {
  type AppSettings,
  type DictionarySuggestion,
  type FocusedAppContext,
  type HistoryEntry,
  type HotkeyBinding,
  type RecoverySessionSummary,
  type SnippetEntry,
  type UsageDaySummary,
  defaultSettings,
  formatHotkey,
} from "../types";

interface AudioDevice {
  id: string;
  name: string;
}

interface Props {
  onClose: () => void;
  onOpenOnboarding: () => void;
  onSaved: (settings: AppSettings) => void;
}

const isTauri = "__TAURI_INTERNALS__" in window;

const settingsNavGroups = [
  {
    id: "settings-input",
    label: "入力",
    items: [
      ["settings-audio", "オーディオ"],
      ["settings-hotkeys", "ショートカット"],
      ["settings-general", "一般設定"],
    ],
  },
  {
    id: "settings-ai",
    label: "AI",
    items: [
      ["settings-api", "API"],
      ["settings-models", "モデル"],
      ["settings-custom", "カスタム指示"],
    ],
  },
  {
    id: "settings-dictionary-category",
    label: "辞書",
    items: [
      ["settings-dictionary", "辞書"],
      ["settings-snippets", "スニペット"],
    ],
  },
  {
    id: "settings-history-category",
    label: "履歴",
    items: [
      ["settings-history", "履歴"],
      ["settings-usage", "ステータス"],
    ],
  },
  {
    id: "settings-details",
    label: "詳細",
    items: [["settings-context", "コンテキスト"]],
  },
] as const;

function connectionKey(settings: AppSettings): string {
  return settings.api_base_url.trim();
}

function normalizeBrowserKey(key: string): string | null {
  if (/^[a-z0-9]$/i.test(key)) return key.toUpperCase();
  if (/^F([1-9]|1[0-9]|2[0-4])$/i.test(key)) return key.toUpperCase();
  const supported: Record<string, string> = {
    " ": "Space",
    Spacebar: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Escape: "Esc",
    Esc: "Esc",
    ArrowUp: "ArrowUp",
    ArrowDown: "ArrowDown",
    ArrowLeft: "ArrowLeft",
    ArrowRight: "ArrowRight",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
  };
  return supported[key] ?? null;
}

function bindingSignature(binding: HotkeyBinding): string {
  return `${binding.ctrl}:${binding.alt}:${binding.shift}:${binding.key.toUpperCase()}`;
}

function historyDictionaryCandidate(
  item: HistoryEntry,
  dictionaryWords: string[]
): string | null {
  const text = `${item.final_text} ${item.raw_text}`;
  const matches = text.match(/[A-Za-z][A-Za-z0-9._/+@#-]{1,63}|[\u30A0-\u30FF]{3,}/g) ?? [];
  return (
    matches.find(
      (candidate) =>
        !dictionaryWords.some((word) => word.toLowerCase() === candidate.toLowerCase())
    ) ?? null
  );
}

export function SettingsPanel({ onClose, onOpenOnboarding, onSaved }: Props) {
  const apiKeyInputRef = useRef<HTMLInputElement>(null);
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoadedFor, setModelsLoadedFor] = useState<string | null>(null);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [apiStatus, setApiStatus] = useState<string | null>(null);
  const [apiKeyDraftPresent, setApiKeyDraftPresent] = useState(false);
  const [apiKeyImportPath, setApiKeyImportPath] = useState("");
  const [apiKeyBusy, setApiKeyBusy] = useState(false);
  const [dictionaryWords, setDictionaryWords] = useState<string[]>([]);
  const [dictionarySuggestions, setDictionarySuggestions] = useState<DictionarySuggestion[]>([]);
  const [dictionaryDraft, setDictionaryDraft] = useState("");
  const [dictionaryLimit, setDictionaryLimit] = useState(800);
  const [snippets, setSnippets] = useState<SnippetEntry[]>([]);
  const [snippetCueDraft, setSnippetCueDraft] = useState("");
  const [snippetTextDraft, setSnippetTextDraft] = useState("");
  const [snippetLimit, setSnippetLimit] = useState(100);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [historySearch, setHistorySearch] = useState("");
  const [historyRawVisible, setHistoryRawVisible] = useState<Record<string, boolean>>({});
  const [recoverySessions, setRecoverySessions] = useState<RecoverySessionSummary[]>([]);
  const [usage, setUsage] = useState<UsageDaySummary[]>([]);
  const [focusedContext, setFocusedContext] = useState<FocusedAppContext | null>(null);
  const [dataBusy, setDataBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  const loadDevices = async () => {
    setDevicesLoading(true);
    try {
      const next = isTauri
        ? await invoke<AudioDevice[]>("list_audio_devices")
        : [{ id: "preview-default", name: "内蔵マイク" }];
      setDevices(next);
    } catch (loadError) {
      setError(`マイクデバイスを取得できませんでした: ${loadError}`);
    } finally {
      setDevicesLoading(false);
    }
  };

  const getApiKeyDraft = () => apiKeyInputRef.current?.value.trim() ?? "";

  const clearApiKeyDraft = () => {
    if (apiKeyInputRef.current) {
      apiKeyInputRef.current.value = "";
    }
    setApiKeyDraftPresent(false);
  };

  const loadModels = async (source: AppSettings, apiKeyOverride = "") => {
    setModelsLoading(true);
    setModelError(null);
    const target = connectionKey(source);
    try {
      const next = isTauri
        ? await invoke<string[]>("list_models", {
            baseUrl: source.api_base_url,
            apiKey: apiKeyOverride,
          })
        : [
            "gpt-realtime-whisper",
            "gpt-4o-mini-transcribe",
            "gpt-4o-transcribe",
            "whisper-1",
            "gpt-4o-mini",
            "gpt-4.1-mini",
          ];
      setModels(next);
      setModelsLoadedFor(target);
    } catch (loadError) {
      setModels([]);
      setModelsLoadedFor(target);
      setModelError(String(loadError));
    } finally {
      setModelsLoading(false);
    }
  };

  const loadLocalData = async () => {
    try {
      if (!isTauri) {
        setDictionaryWords(["Obsidian", "AIVoice", "Antigravity"]);
        setDictionarySuggestions([
          { word: "Slack", count: 3 },
          { word: "Cursor", count: 2 },
          { word: "Whisper", count: 1 },
        ]);
        setSnippets([
          {
            id: "preview-snippet",
            cue: "署名",
            text: "山田太郎\nhttps://example.com",
            created_at: Math.floor(Date.now() / 1000),
          },
        ]);
        setHistory([
          {
            id: "preview-history",
            raw_text: "今日の打ち合わせは午後二時からです",
            final_text: "今日の打ち合わせは午後2時からです。",
            mode: "polish",
            duration_ms: 4200,
            created_at: Math.floor(Date.now() / 1000),
            error: null,
            status: "success",
            pinned: false,
          },
        ]);
        setRecoverySessions([]);
        setUsage([
          {
            day: new Date().toISOString().slice(0, 10),
            sessions: 3,
            words: 124,
            characters: 312,
            audio_seconds: 68,
            asr_cost_usd: 0.0068,
            polish_cost_usd: 0.0002,
            models: ["whisper-1", "gpt-4o-mini"],
          },
        ]);
        return;
      }
      const [
        words,
        suggestions,
        snippetItems,
        historyItems,
        usageItems,
        recoveryItems,
        limit,
        snippetsMax,
      ] = await Promise.all([
        invoke<string[]>("get_dictionary"),
        invoke<DictionarySuggestion[]>("get_dictionary_suggestions"),
        invoke<SnippetEntry[]>("get_snippets"),
        invoke<HistoryEntry[]>("get_history"),
        invoke<UsageDaySummary[]>("get_usage_summary"),
        invoke<RecoverySessionSummary[]>("get_recovery_sessions"),
        invoke<number>("dictionary_limit"),
        invoke<number>("snippet_limit"),
      ]);
      setDictionaryWords(words);
      setDictionarySuggestions(suggestions);
      setSnippets(snippetItems);
      setHistory(historyItems);
      setUsage(usageItems);
      setRecoverySessions(recoveryItems);
      setDictionaryLimit(limit);
      setSnippetLimit(snippetsMax);
    } catch (loadError) {
      setError(String(loadError));
    }
  };

  useEffect(() => {
    const initialize = async () => {
      if (!isTauri) {
        setSettings(defaultSettings);
        await Promise.all([loadDevices(), loadModels(defaultSettings), loadLocalData()]);
        return;
      }
      try {
        const loaded = await invoke<AppSettings>("get_settings");
        setSettings(loaded);
        await Promise.all([loadDevices(), loadModels(loaded), loadLocalData()]);
      } catch (loadError) {
        setError(String(loadError));
      }
    };
    initialize();
  }, []);

  const updateConnection = (
    key: "api_base_url",
    value: string
  ) => {
    setSettings((current) => ({ ...current, [key]: value }));
    setModelsLoadedFor(null);
    setModelError(null);
    setApiStatus(null);
  };

  const handleApiKeyDraftChange = () => {
    setApiKeyDraftPresent(Boolean(getApiKeyDraft()));
    setModelsLoadedFor(null);
    setModelError(null);
    setApiStatus(null);
  };

  const updateHotkey = (
    key:
      | "push_to_talk_hotkey"
      | "hands_free_hotkey"
      | "toggle_mode_hotkey",
    binding: HotkeyBinding
  ) => {
    setSettings((current) => ({ ...current, [key]: binding }));
    setHotkeyError(null);
  };

  const modelsReady =
    modelsLoadedFor === connectionKey(settings) &&
    !modelsLoading &&
    !modelError &&
    models.length > 0;
  const selectedDeviceMissing =
    Boolean(settings.device_id) &&
    !devices.some((device) => device.id === settings.device_id);

  const validateHotkeys = (): string | null => {
    const bindings = [
      settings.push_to_talk_hotkey,
      settings.hands_free_hotkey,
      settings.toggle_mode_hotkey,
    ];
    if (bindings.some((binding) => !binding.key)) {
      return "すべてのショートカットを設定してください。";
    }
    if (new Set(bindings.map(bindingSignature)).size !== bindings.length) {
      return "ショートカットキーが重複しています。";
    }
    return null;
  };

  const handleSave = async () => {
    const validationError = validateHotkeys();
    if (validationError) {
      setHotkeyError(validationError);
      return;
    }
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      if (isTauri) {
        await invoke("save_settings", {
          newSettings: { ...settings, api_key: "" },
        });
      }
      onSaved(settings);
      setSaved(true);
      window.setTimeout(onClose, 800);
    } catch (saveError) {
      setError(`保存できませんでした: ${saveError}`);
    } finally {
      setSaving(false);
    }
  };

  const modelOptions = (current: string) => {
    const values = models.includes(current) ? models : [current, ...models];
    return values.map((model) => (
      <option key={model} value={model}>
        {model}
        {!models.includes(model) ? "（現在値）" : ""}
      </option>
    ));
  };

  const handleSaveApiKey = async () => {
    const apiKey = getApiKeyDraft();
    if (!apiKey) {
      setApiStatus(null);
      setError("保存するAPIキーを入力してください。");
      return;
    }
    setApiKeyBusy(true);
    setError(null);
    setApiStatus(null);
    try {
      const next = isTauri
        ? await invoke<AppSettings>("save_api_key", { apiKey })
        : { ...settings, api_key: "", has_api_key: true };
      setSettings(next);
      clearApiKeyDraft();
      setModelsLoadedFor(null);
      setApiStatus("APIキーを保存しました。");
    } catch (saveError) {
      setError(`APIキーを保存できませんでした: ${saveError}`);
    } finally {
      setApiKeyBusy(false);
    }
  };

  const handleDeleteApiKey = async () => {
    setApiKeyBusy(true);
    setError(null);
    setApiStatus(null);
    try {
      const next = isTauri
        ? await invoke<AppSettings>("delete_api_key")
        : { ...settings, api_key: "", has_api_key: false };
      setSettings(next);
      clearApiKeyDraft();
      setModels([]);
      setModelsLoadedFor(null);
      setApiStatus("保存済みAPIキーを削除しました。");
    } catch (deleteError) {
      setError(`APIキーを削除できませんでした: ${deleteError}`);
    } finally {
      setApiKeyBusy(false);
    }
  };

  const handleTestApiConnection = async () => {
    setApiKeyBusy(true);
    setError(null);
    setApiStatus(null);
    try {
      const count = isTauri
        ? await invoke<number>("test_api_connection", {
            baseUrl: settings.api_base_url,
            apiKeyOverride: getApiKeyDraft() || null,
          })
        : 3;
      setApiStatus(`接続できました。モデル ${count} 件を確認しました。`);
    } catch (testError) {
      setError(`接続テストに失敗しました: ${testError}`);
    } finally {
      setApiKeyBusy(false);
    }
  };

  const handleImportApiKey = async () => {
    const path = apiKeyImportPath.trim();
    if (!path) {
      setApiStatus(null);
      setError("取り込み元ファイルのパスを入力してください。");
      return;
    }
    setApiKeyBusy(true);
    setError(null);
    setApiStatus(null);
    try {
      const next = isTauri
        ? await invoke<AppSettings>("import_api_key_from_env_file", { path })
        : { ...settings, api_key: "", has_api_key: true };
      setSettings(next);
      setApiKeyImportPath("");
      clearApiKeyDraft();
      setModelsLoadedFor(null);
      setApiStatus("APIキーをWindows Credential Managerへ取り込み、一時ファイルを削除しました。");
    } catch (importError) {
      setError(`APIキーの取り込みに失敗しました: ${importError}`);
    } finally {
      setApiKeyBusy(false);
    }
  };

  const handleAddDictionaryWord = async () => {
    const word = dictionaryDraft.trim();
    if (!word) return;
    await addDictionaryWord(word);
    setDictionaryDraft("");
  };

  const addDictionaryWord = async (word: string) => {
    setDataBusy(true);
    setError(null);
    setStatusMessage(null);
    try {
      const next = isTauri
        ? await invoke<string[]>("add_dictionary_word", { word })
        : [...new Set([...dictionaryWords, word])].sort();
      setDictionaryWords(next);
      setDictionarySuggestions((current) =>
        current.filter((item) => item.word.toLowerCase() !== word.toLowerCase())
      );
      setStatusMessage(`${word} を辞書に追加しました。`);
    } catch (addError) {
      setError(`辞書に追加できませんでした: ${addError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleRemoveDictionaryWord = async (word: string) => {
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<string[]>("remove_dictionary_word", { word })
        : dictionaryWords.filter((item) => item !== word);
      setDictionaryWords(next);
      if (isTauri) {
        const suggestions = await invoke<DictionarySuggestion[]>("get_dictionary_suggestions");
        setDictionarySuggestions(suggestions);
      }
    } catch (removeError) {
      setError(`辞書から削除できませんでした: ${removeError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleAddSnippet = async () => {
    const cue = snippetCueDraft.trim();
    const text = snippetTextDraft.trim();
    if (!cue || !text) return;
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<SnippetEntry[]>("add_snippet", { cue, text })
        : [
            {
              id: `preview-snippet-${Date.now()}`,
              cue,
              text,
              created_at: Math.floor(Date.now() / 1000),
            },
            ...snippets.filter((item) => item.cue.toLowerCase() !== cue.toLowerCase()),
          ];
      setSnippets(next);
      setSnippetCueDraft("");
      setSnippetTextDraft("");
    } catch (addError) {
      setError(`スニペットを追加できませんでした: ${addError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleRemoveSnippet = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<SnippetEntry[]>("remove_snippet", { id })
        : snippets.filter((item) => item.id !== id);
      setSnippets(next);
    } catch (removeError) {
      setError(`スニペットを削除できませんでした: ${removeError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleDeleteHistory = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<HistoryEntry[]>("delete_history_item", { id })
        : history.filter((item) => item.id !== id);
      setHistory(next);
      setHistoryRawVisible((current) => {
        const { [id]: _removed, ...rest } = current;
        return rest;
      });
      if (isTauri) {
        const suggestions = await invoke<DictionarySuggestion[]>("get_dictionary_suggestions");
        setDictionarySuggestions(suggestions);
      }
    } catch (deleteError) {
      setError(`履歴を削除できませんでした: ${deleteError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleToggleHistoryPin = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<HistoryEntry[]>("toggle_history_pin", { id })
        : history.map((item) =>
            item.id === id ? { ...item, pinned: !item.pinned } : item
          );
      setHistory(next);
    } catch (pinError) {
      setError(`履歴のピン留めを変更できませんでした: ${pinError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleRerunHistoryPolish = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<HistoryEntry[]>("rerun_history_polish", { id })
        : history.map((item) =>
            item.id === id
              ? {
                  ...item,
                  mode: "polish" as const,
                  final_text: `${item.final_text}\n\nPolish preview`,
                  status: "success" as const,
                  error: null,
                }
              : item
          );
      setHistory(next);
    } catch (polishError) {
      setError(`Polishを再実行できませんでした: ${polishError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleAddHistoryCandidate = async (item: HistoryEntry) => {
    const candidate = historyDictionaryCandidate(item, dictionaryWords);
    if (!candidate) {
      setError("この履歴から追加できる辞書候補が見つかりませんでした。");
      return;
    }
    await addDictionaryWord(candidate);
  };

  const handleClearHistory = async () => {
    setDataBusy(true);
    setError(null);
    try {
      if (isTauri) {
        await invoke("clear_history");
      }
      setHistory([]);
      setDictionarySuggestions([]);
    } catch (deleteError) {
      setError(`履歴を全削除できませんでした: ${deleteError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleCopyHistory = async (text: string) => {
    if (!text) return;
    try {
      if (isTauri) {
        await invoke("copy_text", { text });
      } else {
        await navigator.clipboard?.writeText(text);
      }
    } catch (copyError) {
      setError(`履歴をコピーできませんでした: ${copyError}`);
    }
  };

  const handleInjectHistory = async (text: string) => {
    if (!text) return;
    try {
      if (isTauri) {
        await invoke("inject_text", { text });
      }
    } catch (injectError) {
      setError(`履歴を再注入できませんでした: ${injectError}`);
    }
  };

  const handleRetryRecovery = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      if (!isTauri) return;
      const updated = await invoke<RecoverySessionSummary>("retry_recovery_session", { id });
      setRecoverySessions((current) =>
        current.map((item) => (item.id === id ? updated : item))
      );
    } catch (retryError) {
      setError(`未完了の録音を再文字起こしできませんでした: ${retryError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleInjectRecovery = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      if (!isTauri) return;
      const updated = await invoke<RecoverySessionSummary>("inject_recovery_session", { id });
      setRecoverySessions((current) =>
        current.map((item) => (item.id === id ? updated : item))
      );
    } catch (injectError) {
      setError(`未完了の録音を再注入できませんでした: ${injectError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleSaveRecoveryToHistory = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      if (!isTauri) return;
      const saved = await invoke<HistoryEntry>("save_recovery_session_to_history", { id });
      setHistory((current) => [saved, ...current]);
      setRecoverySessions((current) => current.filter((item) => item.id !== id));
    } catch (saveError) {
      setError(`未完了の録音を履歴へ保存できませんでした: ${saveError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleDeleteRecovery = async (id: string) => {
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<RecoverySessionSummary[]>("delete_recovery_session", { id })
        : recoverySessions.filter((item) => item.id !== id);
      setRecoverySessions(next);
    } catch (deleteError) {
      setError(`未完了の録音を削除できませんでした: ${deleteError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handlePreviewContext = async () => {
    setDataBusy(true);
    setError(null);
    try {
      const context = isTauri
        ? await invoke<FocusedAppContext | null>("get_focused_app_context")
        : { process_name: "Code.exe", window_title: "AIVoice" };
      setFocusedContext(context);
    } catch (contextError) {
      setError(`コンテキストを取得できませんでした: ${contextError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const formatHistoryTime = (seconds: number) =>
    new Date(seconds * 1000).toLocaleString();

  const recoveryStatusLabel = (status: RecoverySessionSummary["status"]) => {
    switch (status) {
      case "recording":
        return "録音中";
      case "captured":
        return "録音済み";
      case "transcribing":
        return "文字起こし中";
      case "text_ready":
        return "テキスト復元済み";
      case "failed":
        return "失敗";
      case "orphaned":
        return "中断";
      case "completed":
        return "完了";
      default:
        return status;
    }
  };

  const formatCost = (value: number) => `$${value.toFixed(4)}`;
  const historyQuery = historySearch.trim().toLowerCase();
  const visibleHistory = history
    .filter((item) => {
      if (!historyQuery) return true;
      return [item.raw_text, item.final_text, item.error ?? "", item.mode]
        .join(" ")
        .toLowerCase()
        .includes(historyQuery);
    })
    .sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      return b.created_at - a.created_at;
    });

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <section
        className="settings-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <div>
            <Settings24Regular />
            <h2 id="settings-title">設定</h2>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="設定を閉じる">
            <Dismiss20Regular />
          </button>
        </header>

        {error && (
          <div className="dialog-error" role="alert">
            <span>{error}</span>
            <button
              className="icon-button"
              onClick={() => setError(null)}
              aria-label="エラーを閉じる"
            >
              <Dismiss20Regular />
            </button>
          </div>
        )}
        {statusMessage && (
          <p className="field-success settings-status">{statusMessage}</p>
        )}

        <div className="dialog-body settings-body-with-nav">
          <nav className="settings-side-nav" aria-label="設定カテゴリ">
            {settingsNavGroups.map((group) => (
              <div className="settings-nav-group" key={group.id}>
                <button
                  className="settings-nav-parent"
                  type="button"
                  onClick={() =>
                    document.getElementById(group.id)?.scrollIntoView({ block: "start" })
                  }
                >
                  {group.label}
                </button>
                <div className="settings-nav-children">
                  {group.items.map(([id, label]) => (
                    <button
                      key={id}
                      className="settings-nav-child"
                      type="button"
                      onClick={() =>
                        document.getElementById(id)?.scrollIntoView({ block: "start" })
                      }
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </nav>
          <div className="settings-pane">
          <SettingsCategory
            id="settings-ai"
            title="AI"
            description="APIキー、モデル、Polishの出力品質をまとめて調整します。"
          />
          <SettingsSection id="settings-api" title="API">
            <FormField label="API Base URL">
              <input
                value={settings.api_base_url}
                onChange={(event) =>
                  updateConnection("api_base_url", event.target.value)
                }
              />
            </FormField>
            <FormField label="API Key">
              <input
                ref={apiKeyInputRef}
                type="password"
                placeholder={settings.has_api_key ? "保存済み（変更時のみ入力）" : ""}
                onChange={handleApiKeyDraftChange}
              />
            </FormField>
            <p className="settings-note">
              APIキーはWindows Credential Managerに保存されます。保存済みキーは画面には表示しません。
            </p>
            <div className="api-key-actions">
              <button
                className="button secondary compact"
                onClick={handleSaveApiKey}
                disabled={apiKeyBusy || !apiKeyDraftPresent}
              >
                APIキーを保存
              </button>
              <button
                className="button secondary compact"
                onClick={handleTestApiConnection}
                disabled={apiKeyBusy || (!settings.has_api_key && !apiKeyDraftPresent)}
              >
                接続テスト
              </button>
              <button
                className="button danger compact"
                onClick={handleDeleteApiKey}
                disabled={apiKeyBusy || !settings.has_api_key}
              >
                保存済みキーを削除
              </button>
            </div>
            <FormField label="一時envファイルの取り込みパス">
              <input
                value={apiKeyImportPath}
                placeholder=".codex/openai-api-key.env"
                onChange={(event) => {
                  setApiKeyImportPath(event.target.value);
                  setApiStatus(null);
                  setError(null);
                }}
              />
            </FormField>
            <div className="api-key-actions">
              <button
                className="button secondary compact"
                onClick={handleImportApiKey}
                disabled={apiKeyBusy || !apiKeyImportPath.trim()}
              >
                OPENAI_API_KEYを取り込み
              </button>
            </div>
            {apiStatus && <p className="field-success">{apiStatus}</p>}
            <p className="settings-note">
              OpenAI Developersで新しいキーを作成した場合は、表示されたキーをここへ一度だけ貼り付けて保存してください。.env.local などの平文保存は推奨しません。
            </p>
            <p className="settings-note">
              Codex / OpenAI Developers連携では、ワークスペース内の一時envファイルからOPENAI_API_KEYだけを読み取り、Windows Credential Managerへ保存した後に一時ファイルを削除します。保存済みキーは再表示できません。
            </p>
          </SettingsSection>

          <SettingsSection
            id="settings-models"
            title="モデル"
            action={
              <button
                className="section-action"
                onClick={() => loadModels(settings, getApiKeyDraft())}
                disabled={modelsLoading}
              >
                <ArrowClockwise20Regular />
                {modelsLoading ? "取得中" : "再取得"}
              </button>
            }
          >
            <FormField label="ASR Model">
              <select
                value={settings.api_model}
                disabled={!modelsReady}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    api_model: event.target.value,
                  }))
                }
              >
                {modelOptions(settings.api_model)}
              </select>
            </FormField>
            <FormField label="Polish Model">
              <select
                value={settings.polish_model}
                disabled={!modelsReady}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    polish_model: event.target.value,
                  }))
                }
              >
                {modelOptions(settings.polish_model)}
              </select>
            </FormField>
            {!modelsReady && !modelError && (
              <p className="settings-note">
                API URLまたはキーを変更した場合は、モデルを再取得してください。
              </p>
            )}
            {modelError && (
              <p className="field-error" role="alert">
                モデル一覧を取得できませんでした: {modelError}
              </p>
            )}
          </SettingsSection>

          <SettingsCategory
            id="settings-input"
            title="入力"
            description="毎日の音声入力で触るマイク、ショートカット、表示をまとめています。"
          />
          <SettingsSection
            id="settings-audio"
            title="オーディオ"
            action={
              <button
                className="section-action"
                onClick={loadDevices}
                disabled={devicesLoading}
              >
                <ArrowClockwise20Regular />
                {devicesLoading ? "読込中" : "再読込"}
              </button>
            }
          >
            <FormField label="マイクデバイス">
              <select
                value={settings.device_id ?? ""}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    device_id: event.target.value || null,
                  }))
                }
              >
                <option value="">既定の入力デバイス</option>
                {selectedDeviceMissing && settings.device_id && (
                  <option value={settings.device_id}>
                    未接続の保存済みデバイス
                  </option>
                )}
                {devices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name}
                  </option>
                ))}
              </select>
            </FormField>
            {selectedDeviceMissing && (
              <p className="field-warning">
                保存済みのマイクが接続されていません。録音時は既定デバイスを使用します。
              </p>
            )}
          </SettingsSection>

          <SettingsSection id="settings-hotkeys" title="ショートカット">
            <HotkeyField
              label="押している間録音"
              value={settings.push_to_talk_hotkey}
              onChange={(binding) =>
                updateHotkey("push_to_talk_hotkey", binding)
              }
              onError={setHotkeyError}
            />
            <HotkeyField
              label="ハンズフリー録音"
              value={settings.hands_free_hotkey}
              onChange={(binding) => updateHotkey("hands_free_hotkey", binding)}
              onError={setHotkeyError}
            />
            <HotkeyField
              label="モード切替"
              value={settings.toggle_mode_hotkey}
              onChange={(binding) => updateHotkey("toggle_mode_hotkey", binding)}
              onError={setHotkeyError}
            />
            <p className="settings-note">
              欄を選択して任意のキーを押してください。英数字、F1〜F24、Space、Enter、
              Tab、Esc、矢印・編集キーに対応しています。
            </p>
            {hotkeyError && (
              <p className="field-error" role="alert">
                {hotkeyError}
              </p>
            )}
          </SettingsSection>

          <SettingsSection id="settings-custom" title="カスタム指示">
            <FormField label="Polish プリセット">
              <select
                value={settings.polish_preset}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    polish_preset: event.target.value as AppSettings["polish_preset"],
                  }))
                }
              >
                <option value="slack">Slack風（短く自然に）</option>
                <option value="email">メール風（丁寧に整える）</option>
                <option value="memo">メモ風（要点を読みやすく）</option>
                <option value="prompt">AIプロンプト風（指示を明確に）</option>
                <option value="technical">技術メモ風（固有名詞と記号を保持）</option>
              </select>
            </FormField>
            <FormField label="Polish モードの出力指示">
              <textarea
                className="settings-textarea"
                value={settings.custom_polish_instructions}
                placeholder="例: Slackでは短く自然に。メールでは段落を整えて丁寧に。"
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    custom_polish_instructions: event.target.value,
                  }))
                }
              />
            </FormField>
            <p className="settings-note">
              プリセットと追加指示はPolishモードのときだけ使用します。本文だけを出力する制約は固定で維持されます。
            </p>
          </SettingsSection>

          <SettingsCategory
            id="settings-dictionary-category"
            title="辞書"
            description="固有名詞、技術語、定型文を個人用に管理します。"
          />
          <SettingsSection id="settings-dictionary" title="辞書">
            <div className="dictionary-header">
              <span>{dictionaryWords.length}/{dictionaryLimit} 語</span>
              <div className="dictionary-add">
                <input
                  value={dictionaryDraft}
                  placeholder="カスタムワード"
                  onChange={(event) => setDictionaryDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      handleAddDictionaryWord();
                    }
                  }}
                />
                <button
                  className="button secondary compact"
                  onClick={handleAddDictionaryWord}
                  disabled={dataBusy || !dictionaryDraft.trim()}
                >
                  追加
                </button>
              </div>
            </div>
            <div className="dictionary-list">
              {dictionaryWords.length === 0 ? (
                <p className="settings-note">登録済みの単語はありません。</p>
              ) : (
                dictionaryWords.map((word) => (
                  <div className="dictionary-row" key={word}>
                    <span>{word}</span>
                    <button
                      className="text-button"
                      onClick={() => handleRemoveDictionaryWord(word)}
                      disabled={dataBusy}
                    >
                      削除
                    </button>
                  </div>
                ))
              )}
            </div>
            <div className="dictionary-suggestions">
              <div className="section-toolbar">
                <span>履歴からの候補</span>
                <span>{dictionarySuggestions.length} 件</span>
              </div>
              {dictionarySuggestions.length === 0 ? (
                <p className="settings-note">
                  履歴に固有名詞や技術語の候補が見つかると、ここから辞書へ追加できます。
                </p>
              ) : (
                <div className="suggestion-chips">
                  {dictionarySuggestions.map((item) => (
                    <button
                      className="suggestion-chip"
                      key={item.word}
                      onClick={() => addDictionaryWord(item.word)}
                      disabled={dataBusy || dictionaryWords.length >= dictionaryLimit}
                      title={`${item.count} 回出現`}
                    >
                      <span>{item.word}</span>
                      <small>{item.count}</small>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <p className="settings-note">
              辞書は最大 {dictionaryLimit} 語までです。ASRの補助プロンプトとPolishの固有名詞候補として使います。
            </p>
          </SettingsSection>

          <SettingsSection id="settings-snippets" title="スニペット">
            <div className="section-toolbar">
              <span>{snippets.length}/{snippetLimit} 件</span>
              <span>音声キューをfinal text内で展開します</span>
            </div>
            <div className="snippet-editor">
              <FormField label="音声キュー">
                <input
                  value={snippetCueDraft}
                  placeholder="例: 署名"
                  onChange={(event) => setSnippetCueDraft(event.target.value)}
                />
              </FormField>
              <FormField label="展開テキスト">
                <textarea
                  className="settings-textarea"
                  value={snippetTextDraft}
                  placeholder="例: 山田太郎&#10;https://example.com"
                  onChange={(event) => setSnippetTextDraft(event.target.value)}
                />
              </FormField>
              <button
                className="button secondary compact"
                onClick={handleAddSnippet}
                disabled={
                  dataBusy ||
                  !snippetCueDraft.trim() ||
                  !snippetTextDraft.trim() ||
                  snippets.length >= snippetLimit
                }
              >
                追加
              </button>
            </div>
            <div className="snippet-list">
              {snippets.length === 0 ? (
                <p className="settings-note">登録済みのスニペットはありません。</p>
              ) : (
                snippets.map((snippet) => (
                  <article className="snippet-row" key={snippet.id}>
                    <div>
                      <strong>{snippet.cue}</strong>
                      <p>{snippet.text}</p>
                    </div>
                    <button
                      className="text-button"
                      onClick={() => handleRemoveSnippet(snippet.id)}
                      disabled={dataBusy}
                    >
                      削除
                    </button>
                  </article>
                ))
              )}
            </div>
            <p className="settings-note">
              Raw/Polishの処理後、入力先へ注入する直前に展開します。Polishによる書き換え対象にはしません。
            </p>
          </SettingsSection>

          <SettingsCategory
            id="settings-details"
            title="詳細"
            description="コンテキスト確認や利用量など、必要な時だけ見る項目です。"
          />
          <SettingsSection id="settings-context" title="コンテキスト">
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={settings.deep_context_enabled}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    deep_context_enabled: event.target.checked,
                  }))
                }
              />
              <span>前面アプリ名とウィンドウタイトルをプロンプト補助に使う</span>
            </label>
            <p className="settings-note">
              取得対象は前面アプリ名とウィンドウタイトルのみです。入力欄本文は読み取りません。Slack、メール、ブラウザ、IDE系ではPolishの文体補助にも使います。取得したコンテキストは保存せず、文字起こし/Polishリクエスト時だけ使用します。
            </p>
            <button
              className="button secondary compact"
              onClick={handlePreviewContext}
              disabled={dataBusy}
            >
              現在のコンテキストを確認
            </button>
            {focusedContext && (
              <div className="context-preview">
                <span>{focusedContext.process_name || "アプリ名不明"}</span>
                <small>{focusedContext.window_title || "タイトルなし"}</small>
              </div>
            )}
          </SettingsSection>

          <SettingsSection id="settings-general" title="一般設定">
            <div className="section-toolbar">
              <span>初回設定を後から確認し直せます。</span>
              <button className="button secondary compact" onClick={onOpenOnboarding}>
                初回セットアップを再実行
              </button>
            </div>
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={settings.show_floating_bar}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    show_floating_bar: event.target.checked,
                  }))
                }
              />
              <span>録音中にフローティングバーを表示する</span>
            </label>
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={settings.launch_at_login}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    launch_at_login: event.target.checked,
                  }))
                }
              />
              <span>ログイン時にAIVoiceを起動する</span>
            </label>
          </SettingsSection>

          <SettingsCategory
            id="settings-history-category"
            title="履歴"
            description="過去の入力、未完了の録音、利用量を確認します。"
          />
          <SettingsSection id="settings-history" title="履歴">
            <div className="section-toolbar">
              <span>最大200件まで保存します。古い履歴から自動的に整理されます。</span>
              <button
                className="button danger compact"
                onClick={handleClearHistory}
                disabled={dataBusy || history.length === 0}
              >
                全削除
              </button>
            </div>
            <div className="history-tools">
              <input
                className="history-search"
                value={historySearch}
                placeholder="履歴を検索"
                onChange={(event) => setHistorySearch(event.target.value)}
              />
              <span>{visibleHistory.length} 件</span>
            </div>
            {recoverySessions.length > 0 && (
              <div className="history-list recovery-list">
                <p className="settings-note">
                  未完了の録音があります。成功した録音の音声は削除済みで、失敗・中断した録音だけ復元候補として残ります。
                </p>
                {recoverySessions.map((item) => {
                  const text = item.final_text || item.raw_text;
                  return (
                    <article className="history-card recovery-card" key={item.id}>
                      <div className="history-meta">
                        <span>{formatHistoryTime(item.created_at)}</span>
                        <span>{item.mode === "polish" ? "Polish" : "Raw"}</span>
                        <span>{recoveryStatusLabel(item.status)}</span>
                        <span>{Math.round(item.duration_ms / 1000)}秒</span>
                      </div>
                      <p>{text || item.error || "音声のみ保存されています。"}</p>
                      <div className="history-actions">
                        <button
                          className="button secondary compact"
                          onClick={() => handleRetryRecovery(item.id)}
                          disabled={dataBusy || !item.can_retry}
                        >
                          再文字起こし
                        </button>
                        <button
                          className="button secondary compact"
                          onClick={() => handleCopyHistory(text)}
                          disabled={!text}
                        >
                          コピー
                        </button>
                        <button
                          className="button secondary compact"
                          onClick={() => handleInjectRecovery(item.id)}
                          disabled={dataBusy || !item.final_text}
                        >
                          再注入
                        </button>
                        <button
                          className="button secondary compact"
                          onClick={() => handleSaveRecoveryToHistory(item.id)}
                          disabled={dataBusy || !item.final_text}
                        >
                          履歴へ保存
                        </button>
                        <button
                          className="button danger compact"
                          onClick={() => handleDeleteRecovery(item.id)}
                          disabled={dataBusy}
                        >
                          削除
                        </button>
                      </div>
                    </article>
                  );
                })}
              </div>
            )}
            <div className="history-list">
              {history.length === 0 ? (
                <p className="settings-note">履歴はまだありません。</p>
              ) : visibleHistory.length === 0 ? (
                <p className="settings-note">検索条件に一致する履歴はありません。</p>
              ) : (
                visibleHistory.slice(0, 50).map((item) => {
                  const canShowRaw = Boolean(item.raw_text && item.raw_text !== item.final_text);
                  const showRawText = Boolean(historyRawVisible[item.id] && canShowRaw);
                  const text = showRawText ? item.raw_text : item.final_text;
                  const dictionaryCandidate = historyDictionaryCandidate(item, dictionaryWords);
                  return (
                    <article
                      className={`history-card ${item.pinned ? "is-pinned" : ""} ${
                        item.status === "error" ? "is-error" : ""
                      }`}
                      key={item.id}
                    >
                      <div className="history-meta">
                        <span>{formatHistoryTime(item.created_at)}</span>
                        <span>{item.mode === "polish" ? "Polish" : "Raw"}</span>
                        <span>{item.status === "error" ? "失敗" : "成功"}</span>
                        {item.pinned && <span>ピン留め</span>}
                        <span>{Math.round(item.duration_ms / 1000)}秒</span>
                      </div>
                      <p>{text || item.error || "テキストなし"}</p>
                      <div className="history-actions">
                        <button
                          className="button secondary compact"
                          onClick={() => handleToggleHistoryPin(item.id)}
                          disabled={dataBusy}
                        >
                          {item.pinned ? "ピン解除" : "ピン留め"}
                        </button>
                        {canShowRaw && (
                          <button
                            className="button secondary compact"
                            onClick={() =>
                              setHistoryRawVisible((current) => ({
                                ...current,
                                [item.id]: !current[item.id],
                              }))
                            }
                          >
                            {showRawText ? "Final" : "Raw"}
                          </button>
                        )}
                        <button
                          className="button secondary compact"
                          onClick={() => handleCopyHistory(text)}
                          disabled={!text}
                        >
                          コピー
                        </button>
                        <button
                          className="button secondary compact"
                          onClick={() => handleInjectHistory(text)}
                          disabled={!text}
                        >
                          再注入
                        </button>
                        <button
                          className="button secondary compact"
                          onClick={() => handleAddHistoryCandidate(item)}
                          disabled={dataBusy || !dictionaryCandidate}
                          title={dictionaryCandidate ? `${dictionaryCandidate} を追加` : "候補なし"}
                        >
                          {dictionaryCandidate ? `辞書追加: ${dictionaryCandidate}` : "辞書候補なし"}
                        </button>
                        <button
                          className="button secondary compact"
                          onClick={() => handleRerunHistoryPolish(item.id)}
                          disabled={dataBusy || !(item.raw_text || item.final_text)}
                        >
                          Polish再実行
                        </button>
                        <button
                          className="button danger compact"
                          onClick={() => handleDeleteHistory(item.id)}
                          disabled={dataBusy}
                        >
                          削除
                        </button>
                      </div>
                    </article>
                  );
                })
              )}
            </div>
          </SettingsSection>

          <SettingsSection id="settings-usage" title="ステータス">
            <div className="usage-list">
              {usage.length === 0 ? (
                <p className="settings-note">利用量の記録はまだありません。</p>
              ) : (
                usage.slice(0, 14).map((day) => (
                  <div className="usage-row" key={day.day}>
                    <div>
                      <strong>{day.day}</strong>
                      <small>{day.models.join(", ") || "モデル未記録"}</small>
                    </div>
                    <span>{day.sessions} 回</span>
                    <span>{day.words} words</span>
                    <span>{day.characters} 文字</span>
                    <span>
                      {formatCost(day.asr_cost_usd + day.polish_cost_usd)}
                    </span>
                  </div>
                ))
              )}
            </div>
            <p className="settings-note">
              料金はローカル推定です。OpenAI公式価格を2026-06-24時点で確認し、ASRは分単価、Polishは推定トークン数で概算しています。実際の請求額はプロバイダ、モデル、トークン化、割引、リージョン設定に依存します。
            </p>
          </SettingsSection>
          </div>
        </div>

        <footer className="dialog-footer">
          <button className="button secondary" onClick={onClose}>
            キャンセル
          </button>
          <button
            className={`button primary ${saved ? "is-saved" : ""}`}
            onClick={handleSave}
            disabled={saving}
          >
            {saved && <Checkmark20Regular />}
            {saved ? "保存済み" : saving ? "保存中..." : "保存"}
          </button>
        </footer>
      </section>
    </div>
  );
}

function SettingsSection({
  id,
  title,
  action,
  children,
}: {
  id?: string;
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section id={id} className="settings-section">
      <div className="settings-section-header">
        <h3>{title}</h3>
        {action}
      </div>
      {children}
    </section>
  );
}

function SettingsCategory({
  id,
  title,
  description,
}: {
  id: string;
  title: string;
  description: string;
}) {
  return (
    <div id={id} className="settings-category">
      <h3>{title}</h3>
      <p>{description}</p>
    </div>
  );
}

function FormField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="form-field">
      <label>{label}</label>
      {children}
    </div>
  );
}

function HotkeyField({
  label,
  value,
  onChange,
  onError,
}: {
  label: string;
  value: HotkeyBinding;
  onChange: (binding: HotkeyBinding) => void;
  onError: (error: string | null) => void;
}) {
  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    event.preventDefault();
    event.stopPropagation();
    if (event.metaKey) {
      onError("Windowsキーを含むショートカットには対応していません。");
      return;
    }
    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;
    const key = normalizeBrowserKey(event.key);
    if (!key) {
      onError(`未対応のキーです: ${event.key}`);
      return;
    }
    onChange({
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      key,
    });
  };

  return (
    <div className="form-field hotkey-field">
      <label>{label}</label>
      <input
        readOnly
        value={formatHotkey(value)}
        onKeyDown={handleKeyDown}
        onFocus={(event) => event.currentTarget.select()}
        aria-label={`${label}のショートカット`}
      />
    </div>
  );
}
