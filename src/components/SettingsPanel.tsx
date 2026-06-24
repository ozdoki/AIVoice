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
  type FocusedAppContext,
  type HistoryEntry,
  type HotkeyBinding,
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
  onSaved: (settings: AppSettings) => void;
}

const isTauri = "__TAURI_INTERNALS__" in window;

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

export function SettingsPanel({ onClose, onSaved }: Props) {
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
  const [dictionaryDraft, setDictionaryDraft] = useState("");
  const [dictionaryLimit, setDictionaryLimit] = useState(800);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [usage, setUsage] = useState<UsageDaySummary[]>([]);
  const [focusedContext, setFocusedContext] = useState<FocusedAppContext | null>(null);
  const [dataBusy, setDataBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
        : ["gpt-4o-mini", "gpt-4.1-mini", "whisper-1"];
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
          },
        ]);
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
      const [words, historyItems, usageItems, limit] = await Promise.all([
        invoke<string[]>("get_dictionary"),
        invoke<HistoryEntry[]>("get_history"),
        invoke<UsageDaySummary[]>("get_usage_summary"),
        invoke<number>("dictionary_limit"),
      ]);
      setDictionaryWords(words);
      setHistory(historyItems);
      setUsage(usageItems);
      setDictionaryLimit(limit);
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
    setDataBusy(true);
    setError(null);
    try {
      const next = isTauri
        ? await invoke<string[]>("add_dictionary_word", { word })
        : [...new Set([...dictionaryWords, word])].sort();
      setDictionaryWords(next);
      setDictionaryDraft("");
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
    } catch (removeError) {
      setError(`辞書から削除できませんでした: ${removeError}`);
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
    } catch (deleteError) {
      setError(`履歴を削除できませんでした: ${deleteError}`);
    } finally {
      setDataBusy(false);
    }
  };

  const handleClearHistory = async () => {
    setDataBusy(true);
    setError(null);
    try {
      if (isTauri) {
        await invoke("clear_history");
      }
      setHistory([]);
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

  const formatCost = (value: number) => `$${value.toFixed(4)}`;

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

        <div className="dialog-body settings-body-with-nav">
          <nav className="settings-side-nav" aria-label="設定カテゴリ">
            {[
              ["settings-api", "API"],
              ["settings-models", "モデル"],
              ["settings-audio", "オーディオ"],
              ["settings-hotkeys", "ショートカット"],
              ["settings-custom", "カスタム指示"],
              ["settings-dictionary", "辞書"],
              ["settings-context", "コンテキスト"],
              ["settings-general", "一般"],
              ["settings-history", "履歴"],
              ["settings-usage", "ステータス"],
            ].map(([id, label]) => (
              <button
                key={id}
                type="button"
                onClick={() => document.getElementById(id)?.scrollIntoView({ block: "start" })}
              >
                {label}
              </button>
            ))}
          </nav>
          <div className="settings-pane">
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
              Polish モードのときだけ使用します。本文だけを出力する制約は固定で維持されます。
            </p>
          </SettingsSection>

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
            <p className="settings-note">
              辞書は最大 {dictionaryLimit} 語までです。ASRの補助プロンプトとPolishの固有名詞候補として使います。
            </p>
          </SettingsSection>

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
              取得対象は前面アプリ名とウィンドウタイトルのみです。入力欄本文は読み取りません。取得したコンテキストは保存せず、文字起こし/Polishリクエスト時だけ使用します。
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
            <div className="history-list">
              {history.length === 0 ? (
                <p className="settings-note">履歴はまだありません。</p>
              ) : (
                history.slice(0, 20).map((item) => (
                  <article className="history-card" key={item.id}>
                    <div className="history-meta">
                      <span>{formatHistoryTime(item.created_at)}</span>
                      <span>{item.mode === "polish" ? "Polish" : "Raw"}</span>
                      <span>{Math.round(item.duration_ms / 1000)}秒</span>
                    </div>
                    <p>{item.final_text || item.error || "テキストなし"}</p>
                    <div className="history-actions">
                      <button
                        className="button secondary compact"
                        onClick={() => handleCopyHistory(item.final_text)}
                        disabled={!item.final_text}
                      >
                        コピー
                      </button>
                      <button
                        className="button secondary compact"
                        onClick={() => handleInjectHistory(item.final_text)}
                        disabled={!item.final_text}
                      >
                        再注入
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
                ))
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
