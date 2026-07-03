import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Checkmark20Regular,
  Dismiss20Regular,
  Mic20Regular,
  Play20Regular,
  Stop20Regular,
} from "@fluentui/react-icons";
import { type AppSettings, type RecordingState, formatHotkey } from "../types";

interface AudioDevice {
  id: string;
  name: string;
}

interface Props {
  settings: AppSettings;
  recordingState: RecordingState;
  lastText: string | null;
  onSettingsSaved: (settings: AppSettings) => void;
  onComplete: () => void;
  onClose: () => void;
}

const isTauri = "__TAURI_INTERNALS__" in window;

export function OnboardingPanel({
  settings,
  recordingState,
  lastText,
  onSettingsSaved,
  onComplete,
  onClose,
}: Props) {
  const [step, setStep] = useState(0);
  const [draftSettings, setDraftSettings] = useState(settings);
  const [apiKey, setApiKey] = useState("");
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [apiReady, setApiReady] = useState(Boolean(settings.has_api_key));
  const [testStarted, setTestStarted] = useState(false);
  const initialLastText = useRef(lastText);

  useEffect(() => {
    setDraftSettings(settings);
    setApiReady(Boolean(settings.has_api_key));
  }, [settings]);

  useEffect(() => {
    const loadDevices = async () => {
      try {
        const next = isTauri
          ? await invoke<AudioDevice[]>("list_audio_devices")
          : [{ id: "preview-default", name: "内蔵マイク" }];
        setDevices(next);
      } catch (loadError) {
        setError(`マイクデバイスを取得できませんでした: ${loadError}`);
      }
    };
    loadDevices();
  }, []);

  const testSucceeded = useMemo(
    () => testStarted && Boolean(lastText && lastText !== initialLastText.current),
    [lastText, testStarted]
  );

  const saveDraftSettings = async (nextSettings: AppSettings) => {
    const payload = { ...nextSettings, api_key: "" };
    if (isTauri) {
      await invoke("save_settings", { newSettings: payload });
    }
    setDraftSettings(nextSettings);
    onSettingsSaved(nextSettings);
  };

  const handleSaveApiKey = async () => {
    if (!apiReady && !apiKey.trim()) {
      setError("APIキーを入力してください。");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (apiKey.trim()) {
        const next = isTauri
          ? await invoke<AppSettings>("save_api_key", { apiKey: apiKey.trim() })
          : { ...draftSettings, has_api_key: true };
        setDraftSettings(next);
        onSettingsSaved(next);
        setApiKey("");
        setApiReady(true);
      }
      setStep(1);
    } catch (saveError) {
      setError(`APIキーを保存できませんでした: ${saveError}`);
    } finally {
      setBusy(false);
    }
  };

  const handleSaveDevice = async () => {
    setBusy(true);
    setError(null);
    try {
      await saveDraftSettings(draftSettings);
      setStep(2);
    } catch (saveError) {
      setError(`マイク設定を保存できませんでした: ${saveError}`);
    } finally {
      setBusy(false);
    }
  };

  const handleStartTest = async () => {
    setBusy(true);
    setError(null);
    try {
      initialLastText.current = lastText;
      setTestStarted(true);
      if (isTauri) {
        await invoke("start_recording_session");
      }
    } catch (startError) {
      setError(`テスト録音を開始できませんでした: ${startError}`);
    } finally {
      setBusy(false);
    }
  };

  const handleStopTest = async () => {
    setBusy(true);
    setError(null);
    try {
      if (isTauri) {
        await invoke("stop_recording_session");
      }
    } catch (stopError) {
      setError(`テスト録音を停止できませんでした: ${stopError}`);
    } finally {
      setBusy(false);
    }
  };

  const handleComplete = async () => {
    setBusy(true);
    setError(null);
    try {
      await saveDraftSettings({ ...draftSettings, onboarding_completed: true });
      onComplete();
    } catch (saveError) {
      setError(`セットアップ完了状態を保存できませんでした: ${saveError}`);
    } finally {
      setBusy(false);
    }
  };

  const steps = ["APIキー", "マイク", "ショートカット", "テスト録音"];

  return (
    <div className="modal-backdrop onboarding-backdrop">
      <section
        className="onboarding-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
      >
        <header className="dialog-header">
          <div>
            <Mic20Regular />
            <h2 id="onboarding-title">初回セットアップ</h2>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="セットアップを閉じる">
            <Dismiss20Regular />
          </button>
        </header>

        <div className="onboarding-steps" aria-label="セットアップ手順">
          {steps.map((label, index) => (
            <button
              key={label}
              className={index === step ? "is-active" : index < step ? "is-complete" : ""}
              onClick={() => setStep(index)}
              type="button"
            >
              {index < step ? <Checkmark20Regular /> : <span>{index + 1}</span>}
              {label}
            </button>
          ))}
        </div>

        {error && (
          <div className="dialog-error" role="alert">
            <span>{error}</span>
            <button className="icon-button" onClick={() => setError(null)} aria-label="エラーを閉じる">
              <Dismiss20Regular />
            </button>
          </div>
        )}

        <div className="onboarding-content">
          {step === 0 && (
            <div className="onboarding-pane">
              <h3>APIキー</h3>
              <p>OpenAI APIキーをWindows Credential Managerへ保存します。保存済みの場合はそのまま次へ進めます。</p>
              <input
                type="password"
                value={apiKey}
                placeholder={apiReady ? "保存済み（変更時のみ入力）" : "sk-..."}
                onChange={(event) => setApiKey(event.target.value)}
              />
              <button className="button primary" onClick={handleSaveApiKey} disabled={busy}>
                {apiReady && !apiKey.trim() ? "次へ" : "保存して次へ"}
              </button>
            </div>
          )}

          {step === 1 && (
            <div className="onboarding-pane">
              <h3>マイク</h3>
              <p>音声入力に使うマイクを選びます。迷う場合は既定の入力デバイスで進めてください。</p>
              <select
                value={draftSettings.device_id ?? ""}
                onChange={(event) =>
                  setDraftSettings((current) => ({
                    ...current,
                    device_id: event.target.value || null,
                  }))
                }
              >
                <option value="">既定の入力デバイス</option>
                {devices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name}
                  </option>
                ))}
              </select>
              <button className="button primary" onClick={handleSaveDevice} disabled={busy}>
                保存して次へ
              </button>
            </div>
          )}

          {step === 2 && (
            <div className="onboarding-pane">
              <h3>ショートカット</h3>
              <p>普段使うキーを確認します。変更は後から設定画面でできます。</p>
              <div className="onboarding-hotkeys">
                <span>押している間録音</span>
                <kbd>{formatHotkey(draftSettings.push_to_talk_hotkey)}</kbd>
                <span>ハンズフリー録音</span>
                <kbd>{formatHotkey(draftSettings.hands_free_hotkey)}</kbd>
                <span>モード切替</span>
                <kbd>{formatHotkey(draftSettings.toggle_mode_hotkey)}</kbd>
              </div>
              <button className="button primary" onClick={() => setStep(3)}>
                確認して次へ
              </button>
            </div>
          )}

          {step === 3 && (
            <div className="onboarding-pane">
              <h3>テスト録音</h3>
              <p>短く話して停止し、テキストが入力先へ注入されることを確認します。</p>
              <div className="onboarding-test-actions">
                <button
                  className="button secondary"
                  onClick={handleStartTest}
                  disabled={busy || recordingState !== "idle"}
                >
                  <Play20Regular />
                  録音開始
                </button>
                <button
                  className="button secondary"
                  onClick={handleStopTest}
                  disabled={busy || recordingState !== "recording"}
                >
                  <Stop20Regular />
                  録音停止
                </button>
              </div>
              <div className={`onboarding-test-result ${testSucceeded ? "is-success" : ""}`}>
                {testSucceeded ? "テスト入力を確認しました。" : "停止後、成功するとここに確認状態が出ます。"}
              </div>
              <button
                className="button primary"
                onClick={handleComplete}
                disabled={busy || (!testSucceeded && isTauri)}
              >
                セットアップ完了
              </button>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
