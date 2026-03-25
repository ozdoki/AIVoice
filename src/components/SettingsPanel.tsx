import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

interface AppSettings {
  api_base_url: string;
  api_key: string;
  api_model: string;
}

interface Props {
  onClose: () => void;
}

export function SettingsPanel({ onClose }: Props) {
  const [settings, setSettings] = useState<AppSettings>({
    api_base_url: "https://api.openai.com/v1",
    api_key: "",
    api_model: "whisper-1",
  });
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    invoke<AppSettings>("get_settings").then(setSettings).catch(console.error);
  }, []);

  const handleSave = async () => {
    setSaving(true);
    setSaved(false);
    try {
      await invoke("save_settings", { newSettings: settings });
      setSaved(true);
      setTimeout(onClose, 800);
    } catch (e) {
      console.error(e);
    } finally {
      setSaving(false);
    }
  };

  const field = (
    label: string,
    key: keyof AppSettings,
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

        {field("API Base URL", "api_base_url")}
        {field("API Key", "api_key", "password")}
        {field("Model", "api_model")}

        <p style={{ fontSize: "0.72rem", color: "#666", marginBottom: "1rem" }}>
          ホットキー: F4 = 録音開始/停止　F5 = Raw / Polish 切替
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
