import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

interface AppSettings {
  api_base_url: string;
  api_key: string;
  api_model: string;
  polish_model: string;
  device_id: string | null;
}

interface AudioDevice {
  id: string;
  name: string;
}

interface Props {
  onClose: () => void;
}

export function SettingsPanel({ onClose }: Props) {
  const [settings, setSettings] = useState<AppSettings>({
    api_base_url: "https://api.openai.com/v1",
    api_key: "",
    api_model: "whisper-1",
    polish_model: "gpt-4o-mini",
    device_id: null,
  });
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<AppSettings>("get_settings").then(setSettings).catch((e) => setError(String(e)));
    invoke<AudioDevice[]>("list_audio_devices")
      .then(setDevices)
      .catch((e) => setError(`マイクデバイス取得失敗: ${e}`));
  }, []);

  const handleSave = async () => {
    setSaving(true);
    setSaved(false);
    setError(null);
    try {
      await invoke("save_settings", { newSettings: settings });
      setSaved(true);
      setTimeout(onClose, 800);
    } catch (e) {
      setError(`保存失敗: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  type StringKey = { [K in keyof AppSettings]: AppSettings[K] extends string ? K : never }[keyof AppSettings];
  const field = (
    label: string,
    key: StringKey,
    type: "text" | "password" = "text"
  ) => (
    <div style={{ marginBottom: "1rem" }}>
      <label style={{ display: "block", fontSize: "0.8rem", color: "#aaa", marginBottom: "0.3rem" }}>
        {label}
      </label>
      <input
        type={type}
        value={settings[key]}
        onChange={(e) => setSettings((s) => ({ ...s, [key]: e.target.value }))}
        style={{
          width: "100%",
          background: "#1a1a1a",
          border: "1px solid #444",
          borderRadius: "6px",
          padding: "0.4rem 0.6rem",
          color: "#f0f0f0",
          fontSize: "0.9rem",
        }}
      />
    </div>
  );

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.7)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 100,
      }}
    >
      <div
        style={{
          background: "#252525",
          borderRadius: "12px",
          padding: "1.5rem",
          width: "320px",
          border: "1px solid #444",
        }}
      >
        <h2 style={{ fontSize: "1rem", marginBottom: "1.2rem" }}>設定</h2>

        {error && (
          <div
            style={{
              background: "#5a1a1a",
              border: "1px solid #a33",
              borderRadius: "6px",
              padding: "0.5rem 0.7rem",
              marginBottom: "1rem",
              fontSize: "0.8rem",
              color: "#f88",
              display: "flex",
              justifyContent: "space-between",
              alignItems: "flex-start",
              gap: "0.5rem",
            }}
          >
            <span>{error}</span>
            <button
              onClick={() => setError(null)}
              style={{
                background: "transparent",
                border: "none",
                color: "#f88",
                cursor: "pointer",
                padding: 0,
                fontSize: "0.9rem",
                lineHeight: 1,
                flexShrink: 0,
              }}
            >
              ✕
            </button>
          </div>
        )}

        {field("API Base URL", "api_base_url")}
        {field("API Key", "api_key", "password")}
        {field("ASR Model", "api_model")}
        {field("Polish Model", "polish_model")}

        <div style={{ marginBottom: "1rem" }}>
          <label style={{ display: "block", fontSize: "0.8rem", color: "#aaa", marginBottom: "0.3rem" }}>
            マイクデバイス
          </label>
          <select
            value={settings.device_id ?? ""}
            onChange={(e) => setSettings((s) => ({ ...s, device_id: e.target.value || null }))}
            style={{
              width: "100%",
              background: "#1a1a1a",
              border: "1px solid #444",
              borderRadius: "6px",
              padding: "0.4rem 0.6rem",
              color: "#f0f0f0",
              fontSize: "0.9rem",
            }}
          >
            <option value="">デフォルト</option>
            {devices.map((d) => (
              <option key={d.id} value={d.id}>{d.name}</option>
            ))}
          </select>
        </div>

        <p style={{ fontSize: "0.72rem", color: "#666", marginBottom: "1rem" }}>
          ホットキー: Ctrl+Shift+F4 = 録音開始/停止　Ctrl+Shift+F5 = Raw / Polish 切替
        </p>

        <div style={{ display: "flex", gap: "0.5rem", justifyContent: "flex-end" }}>
          <button
            onClick={onClose}
            style={{
              padding: "0.4rem 1rem",
              background: "transparent",
              border: "1px solid #555",
              borderRadius: "6px",
              color: "#aaa",
              cursor: "pointer",
            }}
          >
            キャンセル
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            style={{
              padding: "0.4rem 1rem",
              background: saved ? "#2a6" : "#36a",
              border: "none",
              borderRadius: "6px",
              color: "#fff",
              cursor: saving ? "not-allowed" : "pointer",
            }}
          >
            {saved ? "保存済み" : saving ? "保存中..." : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
