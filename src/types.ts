export type Mode = "raw" | "polish";
export type RecordingState = "idle" | "recording" | "processing";

export interface HotkeyBinding {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
}

export interface AppSettings {
  api_base_url: string;
  api_key: string;
  has_api_key?: boolean;
  api_model: string;
  polish_model: string;
  mode: Mode;
  device_id: string | null;
  custom_polish_instructions: string;
  deep_context_enabled: boolean;
  show_floating_bar: boolean;
  launch_at_login: boolean;
  push_to_talk_hotkey: HotkeyBinding;
  hands_free_hotkey: HotkeyBinding;
  toggle_mode_hotkey: HotkeyBinding;
}

export interface HistoryEntry {
  id: string;
  raw_text: string;
  final_text: string;
  mode: Mode;
  duration_ms: number;
  created_at: number;
  error: string | null;
  status: "success" | "error";
}

export interface UsageDaySummary {
  day: string;
  sessions: number;
  words: number;
  characters: number;
  audio_seconds: number;
  asr_cost_usd: number;
  polish_cost_usd: number;
  models: string[];
}

export interface FocusedAppContext {
  process_name: string;
  window_title: string;
}

export const defaultSettings: AppSettings = {
  api_base_url: "https://api.openai.com/v1",
  api_key: "",
  api_model: "whisper-1",
  polish_model: "gpt-4o-mini",
  mode: "raw",
  device_id: null,
  custom_polish_instructions: "",
  deep_context_enabled: false,
  show_floating_bar: true,
  launch_at_login: false,
  push_to_talk_hotkey: { ctrl: true, alt: false, shift: true, key: "F4" },
  hands_free_hotkey: { ctrl: true, alt: false, shift: true, key: "F6" },
  toggle_mode_hotkey: { ctrl: true, alt: false, shift: true, key: "F5" },
};

export function hotkeyParts(binding: HotkeyBinding): string[] {
  return [
    binding.ctrl ? "Ctrl" : null,
    binding.alt ? "Alt" : null,
    binding.shift ? "Shift" : null,
    binding.key,
  ].filter((part): part is string => Boolean(part));
}

export function formatHotkey(binding: HotkeyBinding): string {
  return hotkeyParts(binding).join(" + ");
}
