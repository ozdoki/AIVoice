import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowClockwise20Regular,
  Checkmark20Regular,
  Dismiss20Regular,
  Settings24Regular,
} from "@fluentui/react-icons";
import {
  type AppProfile,
  type AppProfileInput,
  type AppSettings,
  type CorrectionArtifact,
  type CorrectionRecord,
  type DictionarySuggestion,
  type DataProcessingSummary,
  type EffectiveAppProfile,
  type EffectiveSource,
  type FocusedAppContext,
  type HistoryEntry,
  type HotkeyBinding,
  type RecoverySessionSummary,
  type ProfileMutationResult,
  type SnippetEntry,
  type UsageDaySummary,
  defaultSettings,
  formatHotkey,
  isPolishFallback,
  polishStateDetail,
  polishStateLabel,
} from "../types";
import { validateCorrectionArtifacts } from "../correctionArtifactValidation";

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
const LIVE_TRANSCRIPT_MODEL = "gpt-realtime-whisper";

function emptyAppProfileDraft(processName = ""): AppProfileInput {
  return {
    enabled: true,
    name: "",
    process_name: processName,
    title_condition: null,
    priority: 0,
    overrides: {
      mode: null,
      polish_preset: null,
      language_mode: null,
    },
  };
}

function effectiveSourceLabel(source: EffectiveSource): string {
  if (source.kind === "profile") return `プロファイル: ${source.profile_name ?? source.profile_id}`;
  if (source.kind === "suggestion") return "既存のアプリ推奨";
  if (source.kind === "hotkey_override") return "今回のショートカット指定";
  return "全体設定";
}

function supportsLiveTranscriptModel(model: string): boolean {
  return model.trim() === LIVE_TRANSCRIPT_MODEL;
}

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
      ["settings-corrections", "修正学習"],
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
    items: [
      ["settings-context", "コンテキスト"],
      ["settings-app-profiles", "アプリ別プロファイル"],
      ["settings-data-flow", "データ処理経路"],
    ],
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
  if (item.operation_kind === "selected_voice_edit") return null;
  const text = `${item.final_text} ${item.raw_text}`;
  const matches = text.match(/[A-Za-z][A-Za-z0-9._/+@#-]{1,63}|[\u30A0-\u30FF]{3,}/g) ?? [];
  return (
    matches.find(
      (candidate) =>
        !dictionaryWords.some((word) => word.toLowerCase() === candidate.toLowerCase())
    ) ?? null
  );
}

function correctionArtifactWarning(item: CorrectionRecord): string | null {
  const originalChars = Array.from(item.original_text).length;
  const fullTextLike = item.artifacts.some((artifact) => {
    if (artifact.type !== "replacement") return false;
    const fromChars = Array.from(artifact.from).length;
    const sentenceCount = (artifact.from.match(/[。！？]|[.!?](?=\s|$)/gu) ?? []).length;
    return (
      artifact.from === item.original_text ||
      (originalChars > 0 && fromChars / originalChars >= 0.6) ||
      /\r?\n/.test(artifact.from) ||
      sentenceCount >= 2
    );
  });
  return fullTextLike
    ? "全文に近い置換は入力全体へ広く適用されます。必要なら短い部分置換へ分割してください。"
    : null;
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
  const [corrections, setCorrections] = useState<CorrectionRecord[]>([]);
  const [correctionLoadError, setCorrectionLoadError] = useState<string | null>(null);
  const [correctionTypeFilter, setCorrectionTypeFilter] = useState("all");
  const [correctionScopeFilter, setCorrectionScopeFilter] = useState("all");
  const [correctionStatusFilter, setCorrectionStatusFilter] = useState("all");
  const [correctionSourceFilter, setCorrectionSourceFilter] = useState("");
  const [editingCorrectionId, setEditingCorrectionId] = useState<string | null>(null);
  const [editingCorrectionText, setEditingCorrectionText] = useState("");
  const [editingCorrectionArtifacts, setEditingCorrectionArtifacts] = useState<CorrectionArtifact[]>([]);
  const [recoverySessions, setRecoverySessions] = useState<RecoverySessionSummary[]>([]);
  const [usage, setUsage] = useState<UsageDaySummary[]>([]);
  const [focusedContext, setFocusedContext] = useState<FocusedAppContext | null>(null);
  const [appProfiles, setAppProfiles] = useState<AppProfile[]>([]);
  const [appProfileDraft, setAppProfileDraft] = useState<AppProfileInput>(emptyAppProfileDraft());
  const [editingAppProfileId, setEditingAppProfileId] = useState<string | null>(null);
  const [profileCurrentTitle, setProfileCurrentTitle] = useState("");
  const [profileLoadError, setProfileLoadError] = useState<string | null>(null);
  const [profileWarnings, setProfileWarnings] = useState<string[]>([]);
  const [effectiveAppProfile, setEffectiveAppProfile] = useState<EffectiveAppProfile | null>(null);
  const [profileBusy, setProfileBusy] = useState(false);
  const [dataFlow, setDataFlow] = useState<DataProcessingSummary | null>(null);
  const [dataBusy, setDataBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);

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
            "gpt-transcribe",
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
        setDictionaryWords(["Obsidian", "KoeType", "Antigravity"]);
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
            polish_state: "applied_changed",
            pinned: false,
            polish_preset: "memo",
            app_process: "notepad.exe",
            operation_kind: "dictation",
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

  const loadDataFlow = async () => {
    if (!isTauri) return;
    try {
      setDataFlow(await invoke<DataProcessingSummary>("get_data_processing_summary"));
    } catch (loadError) {
      setError(`データ処理経路を取得できませんでした: ${loadError}`);
    }
  };

  const loadCorrections = async () => {
    if (!isTauri) return;
    try {
      setCorrections(await invoke<CorrectionRecord[]>("get_corrections"));
      setCorrectionLoadError(null);
    } catch (loadError) {
      setCorrectionLoadError(String(loadError));
    }
  };

  const loadAppProfiles = async () => {
    if (!isTauri) return;
    try {
      setAppProfiles(await invoke<AppProfile[]>("get_app_profiles"));
      setProfileLoadError(null);
    } catch (loadError) {
      setProfileLoadError(String(loadError));
    }
  };

  useEffect(() => {
    const initialize = async () => {
      if (!isTauri) {
        setSettings(defaultSettings);
        await Promise.all([loadDevices(), loadModels(defaultSettings), loadLocalData()]);
        setInitialLoading(false);
        return;
      }
      try {
        const loaded = await invoke<AppSettings>("get_settings");
        setSettings(loaded);
        await Promise.all([
          loadDevices(),
          loadModels(loaded),
          loadLocalData(),
          loadDataFlow(),
          loadCorrections(),
          loadAppProfiles(),
        ]);
      } catch (loadError) {
        setError(String(loadError));
      } finally {
        setInitialLoading(false);
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
      | "hands_free_raw_hotkey"
      | "hands_free_polish_hotkey"
      | "learn_selected_hotkey"
      | "voice_edit_selected_hotkey",
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
      settings.hands_free_raw_hotkey,
      settings.hands_free_polish_hotkey,
      settings.learn_selected_hotkey,
      settings.voice_edit_selected_hotkey,
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

  const handleAsrModelChange = (model: string) => {
    setSettings((current) => ({
      ...current,
      api_model: model,
      show_live_transcript_in_floating_bar:
        supportsLiveTranscriptModel(model) && current.show_live_transcript_in_floating_bar,
    }));
  };

  const handleLiveTranscriptToggle = (enabled: boolean) => {
    setSettings((current) => ({
      ...current,
      api_model: enabled ? LIVE_TRANSCRIPT_MODEL : current.api_model,
      show_live_transcript_in_floating_bar: enabled,
    }));
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

  const handleCorrectionStatus = async (id: string, status: "active" | "undone") => {
    setDataBusy(true);
    setError(null);
    try {
      const command = status === "active" ? "reactivate_correction" : "undo_correction";
      setCorrections(await invoke<CorrectionRecord[]>(command, { id }));
      await loadDataFlow();
    } catch (actionError) {
      setError(`修正学習の状態を変更できませんでした: ${actionError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleUpdateCorrection = async (item: CorrectionRecord) => {
    const artifactError = validateCorrectionArtifacts(editingCorrectionArtifacts, {
      appProcess: item.app_process,
      mode: item.mode,
    });
    if (artifactError) {
      setError(artifactError);
      return;
    }
    setDataBusy(true);
    setError(null);
    try {
      const updated = await invoke<CorrectionRecord>("update_correction", {
        id: item.id,
        correctedText: editingCorrectionText,
        artifacts: editingCorrectionArtifacts,
      });
      setCorrections((current) =>
        current.map((candidate) => (candidate.id === updated.id ? updated : candidate))
      );
      await loadDataFlow();
      setEditingCorrectionId(null);
      setEditingCorrectionArtifacts([]);
    } catch (updateError) {
      setError(`修正学習を更新できませんでした: ${updateError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const updateEditingArtifact = (index: number, artifact: CorrectionArtifact) => {
    setEditingCorrectionArtifacts((current) =>
      current.map((item, itemIndex) => (itemIndex === index ? artifact : item))
    );
  };

  const changeEditingArtifactType = (
    index: number,
    type: CorrectionArtifact["type"],
    item: CorrectionRecord
  ) => {
    const artifact: CorrectionArtifact =
      type === "vocabulary"
        ? { type, value: "", scope: "global" }
        : type === "replacement"
          ? { type, from: item.original_text, to: editingCorrectionText, scope: "global" }
          : type === "style_example"
            ? { type, input: item.original_text, output: editingCorrectionText }
            : { type: "none" };
    setEditingCorrectionArtifacts((current) => {
      if (type === "none") return [artifact];
      return current
        .map((candidate, itemIndex) => (itemIndex === index ? artifact : candidate))
        .filter((candidate) => candidate.type !== "none");
    });
  };

  const handleDeleteCorrection = async (id: string) => {
    if (!window.confirm("この修正学習を完全に削除します。元に戻せません。")) return;
    setDataBusy(true);
    setError(null);
    try {
      setCorrections(await invoke<CorrectionRecord[]>("delete_correction", { id }));
      await loadDataFlow();
    } catch (deleteError) {
      setError(`修正学習を削除できませんでした: ${deleteError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleClearCorrections = async () => {
    if (!window.confirm("修正学習をすべて削除します。破損データも空のストアへ置き換わり、元に戻せません。")) {
      return;
    }
    setDataBusy(true);
    setError(null);
    try {
      await invoke("clear_corrections");
      setCorrections([]);
      setCorrectionLoadError(null);
      await loadDataFlow();
    } catch (clearError) {
      setError(`修正学習を全消去できませんでした: ${clearError}`);
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
        const warning = await invoke<string | null>("inject_text", { text });
        if (warning) setError(warning);
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
      if (updated.injection_warning) setError(updated.injection_warning);
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
        : { process_name: "Code.exe", window_title: "KoeType" };
      setFocusedContext(context);
    } catch (contextError) {
      setError(`コンテキストを取得できませんでした: ${contextError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleUseCurrentTarget = async () => {
    setProfileBusy(true);
    setError(null);
    try {
      const current = isTauri
        ? await invoke<FocusedAppContext | null>("get_focused_app_context")
        : { process_name: "Code.exe", window_title: "README.md - KoeType" };
      if (!current?.process_name) throw new Error("入力先アプリを取得できませんでした。");
      setProfileCurrentTitle(current.window_title);
      setEditingAppProfileId(null);
      setAppProfileDraft({
        ...emptyAppProfileDraft(current.process_name),
        name: current.process_name.replace(/\.exe$/i, ""),
      });
      setProfileWarnings([]);
    } catch (targetError) {
      setError(`現在の対象から作成できませんでした: ${targetError}`);
    } finally {
      setProfileBusy(false);
    }
  };

  const handleEditAppProfile = (profile: AppProfile) => {
    setEditingAppProfileId(profile.id);
    setProfileCurrentTitle("");
    setProfileWarnings([]);
    setAppProfileDraft({
      enabled: profile.enabled,
      name: profile.name,
      process_name: profile.process_name,
      title_condition: profile.title_condition,
      priority: profile.priority,
      overrides: { ...profile.overrides },
    });
  };

  const handleSaveAppProfile = async () => {
    setProfileBusy(true);
    setError(null);
    setProfileWarnings([]);
    try {
      if (!isTauri) return;
      const warnings = await invoke<string[]>("get_app_profile_conflict_warnings", {
        input: appProfileDraft,
        excludeId: editingAppProfileId,
      });
      setProfileWarnings(warnings);
      if (warnings.length > 0 && !window.confirm(`${warnings.join("\n")}\n\nこの優先規則で保存しますか？`)) {
        return;
      }
      const result = editingAppProfileId
        ? await invoke<ProfileMutationResult>("update_app_profile", {
            id: editingAppProfileId,
            input: appProfileDraft,
          })
        : await invoke<ProfileMutationResult>("create_app_profile", { input: appProfileDraft });
      setProfileWarnings(result.warnings);
      await Promise.all([loadAppProfiles(), loadDataFlow()]);
      setEffectiveAppProfile(null);
      setEditingAppProfileId(null);
      setProfileCurrentTitle("");
      setAppProfileDraft(emptyAppProfileDraft());
      setStatusMessage("アプリ別プロファイルを保存しました。");
    } catch (saveError) {
      setError(`アプリ別プロファイルを保存できませんでした: ${saveError}`);
    } finally {
      setProfileBusy(false);
    }
  };

  const handleToggleAppProfile = async (profile: AppProfile) => {
    setProfileBusy(true);
    setError(null);
    try {
      if (!isTauri) return;
      const updated = await invoke<AppProfile>("set_app_profile_enabled", {
        id: profile.id,
        enabled: !profile.enabled,
      });
      setAppProfiles((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setEffectiveAppProfile(null);
      await loadDataFlow();
    } catch (toggleError) {
      setError(`プロファイルを切り替えられませんでした: ${toggleError}`);
    } finally {
      setProfileBusy(false);
    }
  };

  const handleDeleteAppProfile = async (id: string) => {
    if (!window.confirm("このアプリ別プロファイルを削除しますか？")) return;
    setProfileBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<AppProfile[]>("delete_app_profile", { id })
        : appProfiles.filter((item) => item.id !== id);
      setAppProfiles(next);
      setEffectiveAppProfile(null);
      await loadDataFlow();
      if (editingAppProfileId === id) {
        setEditingAppProfileId(null);
        setAppProfileDraft(emptyAppProfileDraft());
      }
    } catch (deleteError) {
      setError(`プロファイルを削除できませんでした: ${deleteError}`);
    } finally {
      setProfileBusy(false);
    }
  };

  const handleClearAppProfiles = async () => {
    if (!window.confirm("破損データを含むすべてのアプリ別プロファイルを消去しますか？元に戻せません。")) return;
    setProfileBusy(true);
    try {
      if (isTauri) await invoke("clear_app_profiles");
      setAppProfiles([]);
      setProfileLoadError(null);
      setEffectiveAppProfile(null);
      setEditingAppProfileId(null);
      setProfileCurrentTitle("");
      setAppProfileDraft(emptyAppProfileDraft());
      setProfileWarnings([]);
      await loadDataFlow();
      setStatusMessage("アプリ別プロファイルを全消去しました。");
    } catch (clearError) {
      setError(`プロファイルを全消去できませんでした: ${clearError}`);
    } finally {
      setProfileBusy(false);
    }
  };

  const handlePreviewEffectiveAppProfile = async () => {
    setProfileBusy(true);
    setError(null);
    try {
      if (!isTauri) return;
      setEffectiveAppProfile(await invoke<EffectiveAppProfile>("preview_current_app_profile"));
    } catch (previewError) {
      setError(`有効設定を確認できませんでした: ${previewError}`);
    } finally {
      setProfileBusy(false);
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
  const correctionSourceQuery = correctionSourceFilter.trim().toLowerCase();
  const visibleCorrections = corrections.filter((item) => {
    const types = item.artifacts.map((artifact) => artifact.type);
    const scopes = item.artifacts.flatMap((artifact) =>
      artifact.type === "vocabulary" || artifact.type === "replacement" ? [artifact.scope] : []
    );
    return (
      (correctionTypeFilter === "all" || types.includes(correctionTypeFilter as CorrectionArtifact["type"])) &&
      (correctionScopeFilter === "all" || scopes.includes(correctionScopeFilter as "global" | "app")) &&
      (correctionStatusFilter === "all" || item.status === correctionStatusFilter) &&
      (!correctionSourceQuery ||
        [item.source_history_id, item.app_process, item.mode, item.polish_preset]
          .join(" ")
          .toLowerCase()
          .includes(correctionSourceQuery))
    );
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
        {initialLoading && <p className="settings-note" role="status">保存済み設定を読み込んでいます…</p>}

        <fieldset className="settings-loading-fieldset" disabled={initialLoading}>
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
                onChange={(event) => handleAsrModelChange(event.target.value)}
              >
                {modelOptions(settings.api_model)}
              </select>
            </FormField>
            <FormField label="音声認識の言語">
              <select
                value={settings.language_mode}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    language_mode: event.target.value as AppSettings["language_mode"],
                  }))
                }
              >
                <option value="auto">Auto（日本語・英語・混在を自動判定）</option>
                <option value="ja">日本語</option>
                <option value="en">英語</option>
              </select>
            </FormField>
            <p className="settings-note">
              Autoでは言語ヒントをAPIへ送りません。日本語・英語は認識のヒントであり、翻訳や出力言語の強制ではありません。OpenAI互換APIではプロバイダ側の対応状況に依存します。
            </p>
            {settings.show_live_transcript_in_floating_bar &&
              !supportsLiveTranscriptModel(settings.api_model) && (
                <p className="field-warning">
                  録音中の文字表示は {LIVE_TRANSCRIPT_MODEL} でのみ有効です。保存前にモデルを切り替えてください。
                </p>
              )}
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

          <SettingsSection id="settings-corrections" title="修正学習">
            <FormField label="動作">
              <select
                value={settings.correction_learning_mode}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    correction_learning_mode: event.target.value as "off" | "ask",
                  }))
                }
              >
                <option value="off">オフ</option>
                <option value="ask">保存前に確認</option>
              </select>
            </FormField>
            <p className="settings-note">
              オフでは保存済み学習もASR・置換・Polishへ一切適用しません。自動学習はなく、確認画面で選んだ内容だけを保存します。
            </p>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.correction_learning_multi_diff_enabled}
                onChange={(event) => setSettings((current) => ({
                  ...current,
                  correction_learning_multi_diff_enabled: event.target.checked,
                }))}
              />
              <span>複数差分の抽出と既存レコード再編集（段階導入）</span>
            </label>
            <p className="settings-note">
              Raw・元の出力・修正後はローカルへ平文保存され、暗号化は保証されません。語彙とPolish文体例は外部APIへ送信され得ます。決定的置換は端末内だけで適用します。
            </p>
            <div className="section-toolbar">
              <span>{visibleCorrections.length}/{corrections.length} 件を表示</span>
              <button className="button danger compact" onClick={handleClearCorrections} disabled={dataBusy}>
                全消去・破損を復旧
              </button>
            </div>
            {correctionLoadError && (
              <div className="inline-error" role="alert">
                <span>{correctionLoadError}</span>
                <small>通常操作では上書きしていません。「全消去・破損を復旧」だけが明示的な復旧経路です。</small>
              </div>
            )}
            <div className="history-tools correction-filters">
              <select aria-label="修正学習のタイプ" value={correctionTypeFilter} onChange={(event) => setCorrectionTypeFilter(event.target.value)}>
                <option value="all">全タイプ</option>
                <option value="vocabulary">語彙</option>
                <option value="replacement">置換</option>
                <option value="style_example">文体例</option>
                <option value="none">適用なし</option>
              </select>
              <select aria-label="修正学習の適用範囲" value={correctionScopeFilter} onChange={(event) => setCorrectionScopeFilter(event.target.value)}>
                <option value="all">全スコープ</option>
                <option value="global">全アプリ</option>
                <option value="app">アプリ別</option>
              </select>
              <select aria-label="修正学習の状態" value={correctionStatusFilter} onChange={(event) => setCorrectionStatusFilter(event.target.value)}>
                <option value="all">全状態</option>
                <option value="active">有効</option>
                <option value="undone">取り消し済み</option>
              </select>
              <input
                aria-label="修正学習の履歴ID・アプリ・モード検索"
                value={correctionSourceFilter}
                placeholder="履歴ID・アプリ・モード"
                onChange={(event) => setCorrectionSourceFilter(event.target.value)}
              />
            </div>
            <div className="history-list correction-list">
              {visibleCorrections.length === 0 ? (
                <p className="settings-note">条件に一致する修正学習はありません。</p>
              ) : (
                visibleCorrections.map((item) => (
                  <article className={`history-card ${item.status === "undone" ? "is-error" : ""}`} key={item.id}>
                    <div className="history-meta">
                      <span>{formatHistoryTime(item.created_at)}</span>
                      <span>{item.mode === "polish" ? `Polish/${item.polish_preset}` : "Raw"}</span>
                      <span>{item.app_process || "全アプリ/不明"}</span>
                      <span>{item.status === "active" ? "有効" : "取り消し済み"}</span>
                      <span>{item.classification}</span>
                    </div>
                    <small>元の出力</small>
                    <p>{item.original_text}</p>
                    <small>修正後</small>
                    {editingCorrectionId === item.id ? (
                      <textarea
                        className="settings-textarea"
                        value={editingCorrectionText}
                        maxLength={20000}
                        onChange={(event) => setEditingCorrectionText(event.target.value)}
                      />
                    ) : (
                      <p>{item.corrected_text}</p>
                    )}
                    {correctionArtifactWarning(item) && (
                      <p className="settings-note" role="status">{correctionArtifactWarning(item)}</p>
                    )}
                    {editingCorrectionId === item.id ? (
                      <div className="correction-artifact-management">
                        {editingCorrectionArtifacts.map((artifact, index) => (
                          <div className="correction-artifact-editor" key={`manage-${index}`}>
                            <select
                              aria-label="学習タイプ"
                              value={artifact.type}
                              onChange={(event) =>
                                changeEditingArtifactType(
                                  index,
                                  event.target.value as CorrectionArtifact["type"],
                                  item
                                )
                              }
                            >
                              <option value="vocabulary">語彙</option>
                              <option value="replacement">置換</option>
                              {item.mode === "polish" && <option value="style_example">文体例</option>}
                              <option value="none">適用なし</option>
                            </select>
                            {artifact.type === "vocabulary" && (
                              <>
                                <input
                                  aria-label="語彙"
                                  value={artifact.value}
                                  maxLength={128}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      value: event.target.value,
                                    })
                                  }
                                />
                                <select
                                  aria-label="語彙の適用範囲"
                                  value={artifact.scope}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      scope: event.target.value as "global" | "app",
                                    })
                                  }
                                >
                                  <option value="global">全アプリ</option>
                                  <option value="app">アプリ別</option>
                                </select>
                              </>
                            )}
                            {artifact.type === "replacement" && (
                              <>
                                <input
                                  aria-label="置換元"
                                  value={artifact.from}
                                  maxLength={128}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      from: event.target.value,
                                    })
                                  }
                                />
                                <input
                                  aria-label="置換先"
                                  value={artifact.to}
                                  maxLength={2000}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      to: event.target.value,
                                    })
                                  }
                                />
                                <select
                                  aria-label="置換の適用範囲"
                                  value={artifact.scope}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      scope: event.target.value as "global" | "app",
                                    })
                                  }
                                >
                                  <option value="global">全アプリ</option>
                                  <option value="app">アプリ別</option>
                                </select>
                              </>
                            )}
                            {artifact.type === "style_example" && (
                              <>
                                <textarea
                                  aria-label="文体例の入力"
                                  value={artifact.input}
                                  maxLength={8000}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      input: event.target.value,
                                    })
                                  }
                                />
                                <textarea
                                  aria-label="文体例の出力"
                                  value={artifact.output}
                                  maxLength={8000}
                                  onChange={(event) =>
                                    updateEditingArtifact(index, {
                                      ...artifact,
                                      output: event.target.value,
                                    })
                                  }
                                />
                              </>
                            )}
                            {editingCorrectionArtifacts.length > 1 && (
                              <button
                                className="button secondary compact"
                                onClick={() =>
                                  setEditingCorrectionArtifacts((current) =>
                                    current.filter((_, itemIndex) => itemIndex !== index)
                                  )
                                }
                              >
                                候補を削除
                              </button>
                            )}
                          </div>
                        ))}
                        {editingCorrectionArtifacts.length < 16 &&
                          !editingCorrectionArtifacts.some((artifact) => artifact.type === "none") && (
                            <button
                              className="button secondary compact"
                              onClick={() =>
                                setEditingCorrectionArtifacts((current) => [
                                  ...current,
                                  { type: "vocabulary", value: "", scope: "global" },
                                ])
                              }
                            >
                              学習候補を追加
                            </button>
                          )}
                      </div>
                    ) : (
                      <div className="suggestion-chips">
                        {item.artifacts.map((artifact, index) => (
                          <span className="suggestion-chip" key={`${artifact.type}-${index}`}>
                            {artifact.type === "vocabulary"
                              ? `語彙/${artifact.scope}: ${artifact.value}`
                              : artifact.type === "replacement"
                                ? `置換/${artifact.scope}: ${artifact.from} → ${artifact.to}`
                                : artifact.type === "style_example"
                                  ? "Polish文体例/API送信あり"
                                  : "適用なし"}
                          </span>
                        ))}
                      </div>
                    )}
                    {editingCorrectionId === item.id && (() => {
                      const artifactError = validateCorrectionArtifacts(editingCorrectionArtifacts, {
                        appProcess: item.app_process,
                        mode: item.mode,
                      });
                      return artifactError ? <p className="field-error" role="alert">{artifactError}</p> : null;
                    })()}
                    <div className="history-actions">
                      {editingCorrectionId === item.id ? (
                        <>
                          <button
                            className="button secondary compact"
                            onClick={() => handleUpdateCorrection(item)}
                            disabled={dataBusy || Boolean(validateCorrectionArtifacts(editingCorrectionArtifacts, { appProcess: item.app_process, mode: item.mode }))}
                          >
                            保存
                          </button>
                          <button
                            className="button secondary compact"
                            onClick={() => {
                              setEditingCorrectionId(null);
                              setEditingCorrectionArtifacts([]);
                            }}
                          >
                            キャンセル
                          </button>
                        </>
                      ) : (
                        <button
                          className="button secondary compact"
                          onClick={() => {
                            setEditingCorrectionId(item.id);
                            setEditingCorrectionText(item.corrected_text);
                            setEditingCorrectionArtifacts(item.artifacts.map((artifact) => ({ ...artifact })));
                          }}
                        >
                          編集
                        </button>
                      )}
                      <button
                        className="button secondary compact"
                        onClick={() => handleCorrectionStatus(item.id, item.status === "active" ? "undone" : "active")}
                        disabled={dataBusy}
                      >
                        {item.status === "active" ? "取り消す" : "再有効化"}
                      </button>
                      <button className="button danger compact" onClick={() => handleDeleteCorrection(item.id)} disabled={dataBusy}>
                        削除
                      </button>
                    </div>
                  </article>
                ))
              )}
            </div>
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
              label="Rawハンズフリー"
              value={settings.hands_free_raw_hotkey}
              onChange={(binding) => updateHotkey("hands_free_raw_hotkey", binding)}
              onError={setHotkeyError}
            />
            <HotkeyField
              label="Polishハンズフリー"
              value={settings.hands_free_polish_hotkey}
              onChange={(binding) => updateHotkey("hands_free_polish_hotkey", binding)}
              onError={setHotkeyError}
            />
            <HotkeyField
              label="選択テキストから修正を学習"
              value={settings.learn_selected_hotkey}
              onChange={(binding) => updateHotkey("learn_selected_hotkey", binding)}
              onError={setHotkeyError}
            />
            <HotkeyField
              label="選択テキストを音声指示で編集"
              value={settings.voice_edit_selected_hotkey}
              onChange={(binding) => updateHotkey("voice_edit_selected_hotkey", binding)}
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

          <SettingsSection id="settings-app-profiles" title="アプリ別プロファイル">
            <p className="settings-note">
              プロセス名が完全一致する入力先だけに設定を上書きします。プロセス名だけの条件を推奨します。タイトルには文書名などが含まれる場合があるため、条件として採用した文字だけを保存し、入力欄本文は取得・表示・保存しません。
            </p>
            <div className="api-key-actions">
              <button className="button secondary compact" onClick={handleUseCurrentTarget} disabled={profileBusy}>
                直前の外部入力先から作成
              </button>
              <button className="button secondary compact" onClick={handlePreviewEffectiveAppProfile} disabled={profileBusy || Boolean(profileLoadError)}>
                現在の有効設定を確認
              </button>
              <button className="button danger compact" onClick={handleClearAppProfiles} disabled={profileBusy}>
                全消去・破損から復旧
              </button>
            </div>
            <p className="settings-note">KoeTypeを開く直前にフォーカスしていた外部アプリを入力先候補として使います。取得できる場合は現在の外部アプリを優先します。</p>
            {profileLoadError && (
              <p className="field-error" role="alert">
                読み込みに失敗しました。データは上書きしていません: {profileLoadError}
              </p>
            )}
            {effectiveAppProfile && (
              <div className="context-preview">
                <strong>{effectiveAppProfile.process_name || "対象プロセスを取得できませんでした"}</strong>
                <small>モード: {effectiveAppProfile.mode}（{effectiveSourceLabel(effectiveAppProfile.mode_source)}）</small>
                <small>Polish: {effectiveAppProfile.polish_preset}（{effectiveSourceLabel(effectiveAppProfile.polish_preset_source)}）</small>
                <small>言語: {effectiveAppProfile.language_mode}（{effectiveSourceLabel(effectiveAppProfile.language_mode_source)}）</small>
              </div>
            )}

            <div className="settings-list">
              {appProfiles.map((profile) => (
                <article className="context-preview" key={profile.id}>
                  <strong>{profile.name} {!profile.enabled && "（無効）"}</strong>
                  <small>{profile.process_name}{profile.title_condition ? ` / タイトルに「${profile.title_condition.pattern}」を含む` : " / タイトル条件なし"}</small>
                  <small>優先度: {profile.priority}</small>
                  <div className="api-key-actions">
                    <button className="button secondary compact" onClick={() => handleToggleAppProfile(profile)} disabled={profileBusy}>
                      {profile.enabled ? "無効にする" : "有効にする"}
                    </button>
                    <button className="button secondary compact" onClick={() => handleEditAppProfile(profile)} disabled={profileBusy}>編集</button>
                    <button className="button danger compact" onClick={() => handleDeleteAppProfile(profile.id)} disabled={profileBusy}>削除</button>
                  </div>
                </article>
              ))}
              {!profileLoadError && appProfiles.length === 0 && <p className="settings-note">登録済みプロファイルはありません。</p>}
            </div>

            <h4>{editingAppProfileId ? "プロファイルを編集" : "プロファイルを追加"}</h4>
            <FormField label="名前">
              <input maxLength={80} value={appProfileDraft.name} onChange={(event) => setAppProfileDraft((current) => ({ ...current, name: event.target.value }))} />
            </FormField>
            <FormField label="プロセス名（パスを貼っても保存時にファイル名へ正規化）">
              <input maxLength={260} value={appProfileDraft.process_name} onChange={(event) => setAppProfileDraft((current) => ({ ...current, process_name: event.target.value }))} />
            </FormField>
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={Boolean(appProfileDraft.title_condition)}
                onChange={(event) => setAppProfileDraft((current) => ({
                  ...current,
                  title_condition: event.target.checked ? { match_kind: "contains", pattern: "" } : null,
                }))}
              />
              <span>タイトルの部分一致条件を使う（大文字小文字を区別しない）</span>
            </label>
            {appProfileDraft.title_condition && (
              <>
                <FormField label="タイトルに含む文字">
                  <input
                    maxLength={200}
                    value={appProfileDraft.title_condition.pattern}
                    onChange={(event) => setAppProfileDraft((current) => ({ ...current, title_condition: { match_kind: "contains", pattern: event.target.value } }))}
                  />
                </FormField>
                {profileCurrentTitle && (
                  <button
                    className="button secondary compact"
                    onClick={() => setAppProfileDraft((current) => ({ ...current, title_condition: { match_kind: "contains", pattern: profileCurrentTitle.slice(0, 200) } }))}
                    disabled={profileBusy}
                  >
                    現在のタイトルを明示的に採用
                  </button>
                )}
                <p className="settings-note">現在のタイトルは一時プレビューです。このボタンで採用するか手入力して保存するまで永続化しません。文書名を含む可能性を確認してください。</p>
              </>
            )}
            <FormField label="優先度（同じ種類の条件では大きい値を優先）">
              <input type="number" min={-10000} max={10000} value={appProfileDraft.priority} onChange={(event) => setAppProfileDraft((current) => ({ ...current, priority: Number(event.target.value) || 0 }))} />
            </FormField>
            <FormField label="モード">
              <select value={appProfileDraft.overrides.mode ?? "inherit"} onChange={(event) => setAppProfileDraft((current) => ({ ...current, overrides: { ...current.overrides, mode: event.target.value === "inherit" ? null : event.target.value as "raw" | "polish" } }))}>
                <option value="inherit">継承</option><option value="raw">Raw</option><option value="polish">Polish</option>
              </select>
            </FormField>
            <FormField label="Polishプリセット">
              <select value={appProfileDraft.overrides.polish_preset ?? "inherit"} onChange={(event) => setAppProfileDraft((current) => ({ ...current, overrides: { ...current.overrides, polish_preset: event.target.value === "inherit" ? null : event.target.value as AppProfileInput["overrides"]["polish_preset"] } }))}>
                <option value="inherit">継承</option><option value="slack">Slack</option><option value="email">メール</option><option value="memo">メモ</option><option value="prompt">プロンプト</option><option value="technical">技術文書</option>
              </select>
            </FormField>
            <FormField label="言語">
              <select value={appProfileDraft.overrides.language_mode ?? "inherit"} onChange={(event) => setAppProfileDraft((current) => ({ ...current, overrides: { ...current.overrides, language_mode: event.target.value === "inherit" ? null : event.target.value as "auto" | "ja" | "en" } }))}>
                <option value="inherit">継承</option><option value="auto">自動</option><option value="ja">日本語</option><option value="en">英語</option>
              </select>
            </FormField>
            <label className="toggle-row">
              <input type="checkbox" checked={appProfileDraft.enabled} onChange={(event) => setAppProfileDraft((current) => ({ ...current, enabled: event.target.checked }))} />
              <span>保存後すぐ有効にする</span>
            </label>
            {profileWarnings.map((warning) => <p className="field-warning" key={warning}>{warning}</p>)}
            <div className="api-key-actions">
              <button className="button primary compact" onClick={handleSaveAppProfile} disabled={profileBusy || Boolean(profileLoadError)}>保存</button>
              <button className="button secondary compact" onClick={() => { setEditingAppProfileId(null); setProfileCurrentTitle(""); setProfileWarnings([]); setAppProfileDraft(emptyAppProfileDraft()); }} disabled={profileBusy}>下書きをリセット</button>
            </div>
            <p className="settings-note">録音開始時に一度だけ解決し、録音中にフォーカスや設定が変わってもそのセッションの値は変わりません。Raw/Polish専用ショートカットのモード指定はプロファイルより優先されます。</p>
          </SettingsSection>

          <SettingsSection id="settings-data-flow" title="データ処理経路（現在の有効設定）">
            {dataFlow ? (
              <div className="settings-list">
                <div className="context-preview">
                  <strong>端末内</strong>
                  <small>{dataFlow.capture}</small>
                </div>
                <div className="context-preview">
                  <strong>音声認識API: {dataFlow.asr.destination_host}</strong>
                  <small>
                    {dataFlow.asr.processing} / {dataFlow.asr.model}
                    {dataFlow.asr.fallback_model
                      ? ` → Batch ${dataFlow.asr.fallback_model}`
                      : ""}
                  </small>
                  <small>言語: {dataFlow.language}</small>
                  <small>送信され得る内容: {dataFlow.asr.sent_data.join("、")}</small>
                  <small>{dataFlow.external_retention}</small>
                </div>
                {dataFlow.polish && (
                  <div className="context-preview">
                    <strong>Polish API: {dataFlow.polish.destination_host}</strong>
                    <small>{dataFlow.polish.processing} / {dataFlow.polish.model}</small>
                    <small>送信され得る内容: {dataFlow.polish.sent_data.join("、")}</small>
                  </div>
                )}
                <div className="context-preview">
                  <strong>F9 選択音声編集API: {dataFlow.selected_voice_edit.destination_host}</strong>
                  <small>{dataFlow.selected_voice_edit.processing} / {dataFlow.selected_voice_edit.model}</small>
                  <small>送信される内容: {dataFlow.selected_voice_edit.sent_data.join("、")}</small>
                  <small>音声指示と編集案はpreview表示前にローカル平文履歴へ自動保存します。原選択文は履歴・Recoveryへ別保存しません。</small>
                </div>
                <div className="context-preview">
                  <strong>ローカル保存</strong>
                  <small>{dataFlow.local_storage.join("、")}</small>
                  <small>修正学習: {dataFlow.correction_learning_status}</small>
                </div>
              </div>
            ) : (
              <p className="settings-note">保存済み設定から処理経路を読み込んでいます。</p>
            )}
            <p className="settings-note">
              アプリ別プロファイルは、現在の外部アプリまたはKoeTypeを開く直前の外部入力先を基準に反映します。送信先は設定URLのホスト名だけを表示し、パス・クエリ・認証情報は表示しません。「端末内／クラウド」を切り替える機能ではありません。
            </p>
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
                checked={settings.show_live_transcript_in_floating_bar}
                disabled={!settings.show_floating_bar}
                onChange={(event) => handleLiveTranscriptToggle(event.target.checked)}
              />
              <span>録音中の文字起こしをフローティングバーに表示する</span>
            </label>
            {settings.show_live_transcript_in_floating_bar && (
              <p className="settings-note">
                この設定をONにすると、ASR Modelは {LIVE_TRANSCRIPT_MODEL} に切り替わります。
              </p>
            )}
            <p className="settings-note">
              変換結果は互換性の高い貼り付け方式で入力し、入力後もクリップボードに残ります。画像など既存の形式は復元しません。
            </p>
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
              <span>ログイン時にKoeTypeを起動する</span>
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
                        {item.operation_kind === "selected_voice_edit" && <span>選択音声編集</span>}
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
                          disabled={dataBusy || !item.final_text || item.operation_kind === "selected_voice_edit"}
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
                  const polishLabel = showRawText || item.operation_kind === "selected_voice_edit"
                    ? null
                    : polishStateLabel(item.polish_state);
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
                        {item.operation_kind === "selected_voice_edit" && <span>選択音声編集</span>}
                        <span>{item.status === "error" ? "失敗" : "成功"}</span>
                        {polishLabel && (
                          <span
                            className={`polish-result-chip ${
                              isPolishFallback(item.polish_state) ? "is-fallback" : ""
                            }`}
                            title={polishStateDetail(item.polish_state)}
                          >
                            {polishLabel}
                          </span>
                        )}
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
                            {showRawText ? "編集案" : item.operation_kind === "selected_voice_edit" ? "音声指示" : "Raw"}
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
                          disabled={dataBusy || item.operation_kind === "selected_voice_edit" || !(item.raw_text || item.final_text)}
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
              料金はローカル推定です。OpenAI公式価格を2026-07-31時点で確認し、ASRは分単価、Polishは推定トークン数で概算しています。実際の請求額はプロバイダ、モデル、トークン化、割引、リージョン設定に依存します。
            </p>
          </SettingsSection>
          </div>
        </div>
        </fieldset>

        <footer className="dialog-footer">
          <button className="button secondary" onClick={onClose}>
            キャンセル
          </button>
          <button
            className={`button primary ${saved ? "is-saved" : ""}`}
            onClick={handleSave}
            disabled={saving || initialLoading}
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
    <label className="form-field">
      <span>{label}</span>
      {children}
    </label>
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
    if (event.metaKey || event.key === "Meta") {
      onError("Windowsキーを含むショートカットには対応していません。");
      return;
    }
    if (["Control", "Alt", "Shift"].includes(event.key)) {
      onChange({
        ...value,
        ctrl: event.key === "Control" ? !value.ctrl : value.ctrl,
        alt: event.key === "Alt" ? !value.alt : value.alt,
        shift: event.key === "Shift" ? !value.shift : value.shift,
      });
      onError(null);
      return;
    }
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
