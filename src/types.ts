export type Mode = "raw" | "polish";
export type RecordingState = "idle" | "recording" | "processing";
export type SessionPhase =
  | "idle"
  | "recording"
  | "transcribing"
  | "polishing"
  | "injecting"
  | "completed"
  | "cancelled"
  | "failed";
export type PolishPreset = "slack" | "email" | "memo" | "prompt" | "technical";
export type LanguageMode = "auto" | "ja" | "en";
export type CorrectionLearningMode = "off" | "ask";
export type OperationKind = "dictation" | "selected_voice_edit";
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
  language_mode: LanguageMode;
  polish_model: string;
  mode: Mode;
  device_id: string | null;
  polish_preset: PolishPreset;
  custom_polish_instructions: string;
  deep_context_enabled: boolean;
  show_floating_bar: boolean;
  show_live_transcript_in_floating_bar: boolean;
  correction_learning_mode: CorrectionLearningMode;
  correction_learning_multi_diff_enabled: boolean;
  launch_at_login: boolean;
  onboarding_completed: boolean;
  push_to_talk_hotkey: HotkeyBinding;
  hands_free_raw_hotkey: HotkeyBinding;
  hands_free_polish_hotkey: HotkeyBinding;
  learn_selected_hotkey: HotkeyBinding;
  voice_edit_selected_hotkey: HotkeyBinding;
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
  polish_preset: string;
  app_process: string;
  operation_kind: OperationKind;
}

export interface AppProfileOverrides {
  mode: Mode | null;
  polish_preset: PolishPreset | null;
  language_mode: LanguageMode | null;
}

export interface AppProfileInput {
  enabled: boolean;
  name: string;
  process_name: string;
  title_condition: { match_kind: "contains"; pattern: string } | null;
  priority: number;
  overrides: AppProfileOverrides;
}

export interface AppProfile extends AppProfileInput {
  id: string;
  created_at: number;
  updated_at: number;
}

export interface EffectiveSource {
  kind: "profile" | "suggestion" | "global" | "hotkey_override";
  profile_id: string | null;
  profile_name: string | null;
}

export interface EffectiveAppProfile {
  process_name: string;
  mode: Mode;
  mode_source: EffectiveSource;
  polish_preset: PolishPreset;
  polish_preset_source: EffectiveSource;
  language_mode: LanguageMode;
  language_mode_source: EffectiveSource;
  matched_profile_ids: string[];
}

export interface ProfileMutationResult {
  profile: AppProfile;
  warnings: string[];
}

export type CorrectionStatus = "active" | "undone";
export type CorrectionClassification = "minor" | "substantial" | "meaning_change_suspected";
export type LearningScope = "global" | "app";
export type ComparisonMode = "full" | "explicit_range";
export type CorrectionCandidateStatus = "eligible" | "needs_review" | "unsupported";
export type CorrectionCandidateReasonCode =
  | "none"
  | "short_source"
  | "ambiguous_source"
  | "overlapping_candidate"
  | "conflicting_destination"
  | "pure_insertion"
  | "pure_deletion"
  | "meaning_change_suspected"
  | "manual_revalidation_required"
  | "existing_rule_conflict"
  | "candidate_limit_exceeded"
  | "diff_budget_exceeded"
  | "range_mismatch";
export type CorrectionCandidatePersistenceState = "new" | "persisted_verified" | "persisted_unverified";
export type CorrectionArtifact =
  | { type: "vocabulary"; value: string; scope: LearningScope }
  | { type: "replacement"; from: string; to: string; scope: LearningScope }
  | { type: "style_example"; input: string; output: string }
  | { type: "none" };

export interface CorrectionRecord {
  id: string;
  source_history_id: string;
  raw_text: string;
  original_text: string;
  corrected_text: string;
  mode: Mode;
  polish_preset: string;
  app_process: string;
  classification: CorrectionClassification;
  status: CorrectionStatus;
  artifacts: CorrectionArtifact[];
  created_at: number;
  updated_at: number;
}

export interface CorrectionTextRange {
  /** Grapheme-cluster offsets, end-exclusive. */
  start: number;
  end: number;
}

export interface CorrectionCandidate {
  id: string;
  artifact: CorrectionArtifact;
  source_range: CorrectionTextRange;
  corrected_range: CorrectionTextRange;
  occurrence_count: number;
  context_before: string;
  context_after: string;
  status: CorrectionCandidateStatus;
  reason_code: CorrectionCandidateReasonCode;
  reason: string;
  origin: "automatic" | "manual";
  persistence_state: CorrectionCandidatePersistenceState;
  warnings: string[];
}

export interface CorrectionPreview {
  source_history_id: string;
  original_text: string;
  corrected_text: string;
  app_process: string;
  mode: Mode;
  comparison_mode: ComparisonMode;
  source_display_text: string;
  source_display_fingerprint: string;
  preview_fingerprint: string;
  idempotency_key: string;
  target_record_id: string | null;
  record_edit_fingerprint: string | null;
  source_records_fingerprint: string;
  persisted_unverified_artifacts: CorrectionArtifact[];
  persisted_artifacts: CorrectionArtifact[];
  target_record_corrected_text: string | null;
  classification: CorrectionClassification;
  candidates: CorrectionCandidate[];
  warnings: string[];
  total_candidates: number;
  omitted_candidates: number;
  default_artifacts: CorrectionArtifact[];
  source_range?: CorrectionTextRange | null;
  corrected_excerpt_range?: CorrectionTextRange | null;
  original_excerpt?: string | null;
  corrected_excerpt?: string | null;
}

export interface SelectedCorrectionCandidate {
  id: string;
  final_text: string;
  mode: Mode;
  polish_preset: string;
  app_process: string;
  created_at: number;
  same_app: boolean;
  source_display_text: string;
  source_display_fingerprint: string;
  existing_records: ExistingCorrectionSummary[];
}

export interface ExistingCorrectionSummary {
  id: string;
  status: CorrectionStatus;
  updated_at: number;
  corrected_excerpt: string;
}

export type SelectedCorrectionOperation = "create" | "update" | "delete" | "none";

export interface CreateSelectedCorrectionResult {
  record_id: string;
  replayed: boolean;
  focus_warning: string | null;
}

export interface VocabularyCandidateAssociation {
  artifact_index: number;
  candidate_id: string;
}

export interface PrepareSelectedCorrectionResult {
  selected_text: string;
  candidates: SelectedCorrectionCandidate[];
  token: string;
  warning: "external_clipboard_change_preserved" | null;
  multi_diff_enabled: boolean;
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
  operation_kind: OperationKind;
  injection_warning: string | null;
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

export interface ExternalApiFlow {
  sendable: boolean;
  destination_host: string;
  model: string;
  fallback_model: string | null;
  processing: string;
  sent_data: string[];
}

export interface DataProcessingSummary {
  capture: string;
  asr: ExternalApiFlow;
  polish: ExternalApiFlow | null;
  selected_voice_edit: ExternalApiFlow;
  language: string;
  local_storage: string[];
  external_retention: string;
  correction_learning_enabled: boolean;
  correction_learning_status: string;
}

export interface SelectedVoiceEditPreview {
  token: string;
  original_text: string;
  instruction: string;
  proposal: string;
  app_process: string;
  replace_available: boolean;
  selection_warning: "external_clipboard_change_preserved" | null;
  history_id: string | null;
}

export type SelectedVoiceEditToggleResult =
  | { status: "recording"; warning: string | null }
  | { status: "preview"; preview: SelectedVoiceEditPreview };

export interface SelectedVoiceEditReplaceResult {
  replaced: boolean;
  code: string;
  message: string;
  partial: boolean;
}

export const defaultSettings: AppSettings = {
  api_base_url: "https://api.openai.com/v1",
  api_key: "",
  api_model: "gpt-transcribe",
  language_mode: "auto",
  polish_model: "gpt-5.6-terra",
  mode: "raw",
  device_id: null,
  polish_preset: "memo",
  custom_polish_instructions: "",
  deep_context_enabled: false,
  show_floating_bar: true,
  show_live_transcript_in_floating_bar: false,
  correction_learning_mode: "off",
  correction_learning_multi_diff_enabled: false,
  launch_at_login: false,
  onboarding_completed: false,
  push_to_talk_hotkey: { ctrl: true, alt: false, shift: true, key: "F4" },
  hands_free_raw_hotkey: { ctrl: true, alt: false, shift: true, key: "F6" },
  hands_free_polish_hotkey: { ctrl: true, alt: false, shift: true, key: "F7" },
  learn_selected_hotkey: { ctrl: true, alt: false, shift: true, key: "F8" },
  voice_edit_selected_hotkey: { ctrl: true, alt: false, shift: true, key: "F9" },
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
