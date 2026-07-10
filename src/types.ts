export type Mode = "raw" | "polish";
export type RecordingState = "idle" | "recording" | "processing";
export type SessionPhase =
  | "idle"
  | "recording"
  | "transcribing"
  | "polishing"
  | "injecting"
  | "completed"
  | "failed";
export type PolishPreset = "slack" | "email" | "memo" | "prompt" | "technical";
export type PolishState =
  | "unknown"
  | "not_requested"
  | "applied_changed"
  | "applied_unchanged"
  | "fallback_not_configured"
  | "fallback_request_error"
  | "fallback_invalid_response"
  | "fallback_empty_response";

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
  polish_preset: PolishPreset;
  custom_polish_instructions: string;
  deep_context_enabled: boolean;
  show_floating_bar: boolean;
  show_live_transcript_in_floating_bar: boolean;
  launch_at_login: boolean;
  onboarding_completed: boolean;
  push_to_talk_hotkey: HotkeyBinding;
  hands_free_raw_hotkey: HotkeyBinding;
  hands_free_polish_hotkey: HotkeyBinding;
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
  polish_state: PolishState;
  pinned: boolean;
}

export function polishStateLabel(state: PolishState | null | undefined): string | null {
  switch (state) {
    case "applied_changed":
      return "Polish適用";
    case "applied_unchanged":
      return "Polish・変更なし";
    case "fallback_not_configured":
    case "fallback_request_error":
    case "fallback_invalid_response":
    case "fallback_empty_response":
      return "Rawフォールバック";
    default:
      return null;
  }
}

export function polishStateDetail(state: PolishState | null | undefined): string | undefined {
  switch (state) {
    case "applied_changed":
      return "Polish APIが本文を整形しました。";
    case "applied_unchanged":
      return "Polish APIは成功しましたが、モデルは元の本文と同じ内容を返しました。";
    case "fallback_not_configured":
      return "Polish APIの設定が不足していたため、Raw本文を使用しました。";
    case "fallback_request_error":
      return "Polish APIへのリクエストに失敗したため、Raw本文を使用しました。";
    case "fallback_invalid_response":
      return "Polish APIの応答を読み取れなかったため、Raw本文を使用しました。";
    case "fallback_empty_response":
      return "Polish APIが空の本文を返したため、Raw本文を使用しました。";
    default:
      return undefined;
  }
}

export function isPolishFallback(state: PolishState | null | undefined): boolean {
  return Boolean(state?.startsWith("fallback_"));
}

export interface DictionarySuggestion {
  word: string;
  count: number;
}

export interface SnippetEntry {
  id: string;
  cue: string;
  text: string;
  created_at: number;
}

export type RecoveryStatus =
  | "recording"
  | "captured"
  | "transcribing"
  | "text_ready"
  | "completed"
  | "failed"
  | "orphaned";

export interface RecoverySessionSummary {
  id: string;
  created_at: number;
  updated_at: number;
  mode: Mode;
  status: RecoveryStatus;
  duration_ms: number;
  raw_text: string;
  final_text: string;
  error: string | null;
  has_audio: boolean;
  can_retry: boolean;
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
  api_model: "gpt-realtime-whisper",
  polish_model: "gpt-4o-mini",
  mode: "raw",
  device_id: null,
  polish_preset: "memo",
  custom_polish_instructions: "",
  deep_context_enabled: false,
  show_floating_bar: true,
  show_live_transcript_in_floating_bar: false,
  launch_at_login: false,
  onboarding_completed: false,
  push_to_talk_hotkey: { ctrl: true, alt: false, shift: true, key: "F4" },
  hands_free_raw_hotkey: { ctrl: true, alt: false, shift: true, key: "F6" },
  hands_free_polish_hotkey: { ctrl: true, alt: false, shift: true, key: "F7" },
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
