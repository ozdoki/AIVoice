use std::{
    cmp::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    context::FocusedWindowTarget,
    corrections::{self, ComparisonMode, CorrectionArtifact, CorrectionStatus, CorrectionStore},
    local_data::{HistoryEntry, HistoryStatus, OperationKind},
    selection::SelectionWarning,
    settings::CorrectionLearningMode,
    state::{Mode, RecordingState},
};

pub const CANDIDATE_WINDOW_SECS: u64 = 30 * 60;
pub const PENDING_EXPIRY_SECS: u64 = 10 * 60;
pub const MAX_CANDIDATES: usize = 5;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SelectedCorrectionCandidate {
    pub id: String,
    pub final_text: String,
    pub mode: Mode,
    pub polish_preset: String,
    pub app_process: String,
    pub created_at: u64,
    pub same_app: bool,
    pub source_display_text: String,
    pub source_display_fingerprint: String,
    pub existing_records: Vec<ExistingCorrectionSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExistingCorrectionSummary {
    pub id: String,
    pub status: CorrectionStatus,
    pub updated_at: u64,
    pub corrected_excerpt: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrepareSelectedCorrectionResult {
    pub selected_text: String,
    pub candidates: Vec<SelectedCorrectionCandidate>,
    pub token: String,
    pub warning: Option<SelectionWarning>,
    pub multi_diff_enabled: bool,
}

pub struct PendingSelectedLearning {
    token: String,
    selected_text: Option<String>,
    candidates: Vec<SelectedCorrectionCandidate>,
    created_at: u64,
    target: FocusedWindowTarget,
    expired: bool,
    confirmed_preview: Option<ConfirmedSelectedPreview>,
    multi_diff_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedSelectedPreview {
    pub history_id: String,
    pub comparison_mode: ComparisonMode,
    pub source_start_utf16: Option<usize>,
    pub source_end_utf16: Option<usize>,
    pub source_display_fingerprint: String,
    pub preview_fingerprint: String,
    pub idempotency_key: String,
    pub target_record_id: Option<String>,
    pub record_edit_fingerprint: Option<String>,
    pub source_records_fingerprint: String,
    pub persisted_unverified_artifacts: Vec<CorrectionArtifact>,
    pub persisted_artifacts: Vec<CorrectionArtifact>,
}

#[derive(Clone)]
pub struct ResolvedPendingSelection {
    pub selected_text: String,
    pub candidate: SelectedCorrectionCandidate,
    pub multi_diff_enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectedCorrectionOperation {
    Create,
    Update,
    Delete,
    None,
}

impl SelectedCorrectionOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SelectedLearningReplay {
    pub token: String,
    pub idempotency_key: String,
    pub preview_fingerprint: String,
    pub create_request_digest: String,
    pub record_id: String,
    pub focus_warning: Option<String>,
    pub created_at: u64,
}

pub fn check_replay(
    cache: &mut Vec<SelectedLearningReplay>,
    token: &str,
    idempotency_key: &str,
    preview_fingerprint: &str,
    request_digest: &str,
    now: u64,
) -> Result<Option<(String, Option<String>)>, String> {
    cache.retain(|entry| now.saturating_sub(entry.created_at) <= PENDING_EXPIRY_SECS);
    if let Some(entry) = cache
        .iter()
        .find(|entry| entry.idempotency_key == idempotency_key)
    {
        if entry.token == token
            && entry.preview_fingerprint == preview_fingerprint
            && entry.create_request_digest == request_digest
        {
            return Ok(Some((entry.record_id.clone(), entry.focus_warning.clone())));
        }
        return Err(
            "同じ冪等性キーの保存内容が一致しません。再プレビューしてください。".to_string(),
        );
    }
    if cache.iter().any(|entry| entry.token == token) {
        return Err("この確認操作は別の保存要求ですでに完了しています。".to_string());
    }
    Ok(None)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_token() -> String {
    format!("selected-{}", uuid::Uuid::new_v4())
}

pub fn new_idempotency_key() -> String {
    format!("preview-{}", uuid::Uuid::new_v4())
}

fn process_basename(value: &str) -> &str {
    value
        .rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or("")
}

fn same_app(left: &str, right: &str) -> bool {
    let left = process_basename(left);
    let right = process_basename(right);
    !left.is_empty() && !right.is_empty() && left.eq_ignore_ascii_case(right)
}

pub fn ensure_learning_enabled(mode: CorrectionLearningMode) -> Result<(), String> {
    if mode == CorrectionLearningMode::Ask {
        Ok(())
    } else {
        Err("修正学習はオフです。設定で「保存前に確認」を選択してください。".to_string())
    }
}

pub fn ensure_session_idle(
    recording_state: &RecordingState,
    has_session: bool,
) -> Result<(), String> {
    if matches!(recording_state, RecordingState::Idle) && !has_session {
        Ok(())
    } else {
        Err("録音中または処理中は、選択テキストから修正を学習できません。".to_string())
    }
}

pub fn select_candidates(
    history: &[HistoryEntry],
    store: &CorrectionStore,
    focused_process: &str,
    now: u64,
    multi_diff_enabled: bool,
) -> Vec<SelectedCorrectionCandidate> {
    let mut candidates = history
        .iter()
        .filter(|item| {
            item.status == HistoryStatus::Success
                && item.operation_kind == OperationKind::Dictation
                && !item.final_text.trim().is_empty()
                && item.created_at <= now
                && now - item.created_at <= CANDIDATE_WINDOW_SECS
                && (multi_diff_enabled
                    || !store
                        .items
                        .iter()
                        .any(|record| record.source_history_id == item.id))
        })
        .map(|item| SelectedCorrectionCandidate {
            id: item.id.clone(),
            final_text: item.final_text.clone(),
            mode: item.mode.clone(),
            polish_preset: item.polish_preset.clone(),
            app_process: item.app_process.clone(),
            created_at: item.created_at,
            same_app: same_app(&item.app_process, focused_process),
            source_display_text: corrections::source_display_text(&item.final_text),
            source_display_fingerprint: corrections::source_display_fingerprint(&item.final_text),
            existing_records: store
                .items
                .iter()
                .filter(|record| record.source_history_id == item.id)
                .map(|record| ExistingCorrectionSummary {
                    id: record.id.clone(),
                    status: record.status,
                    updated_at: record.updated_at,
                    corrected_excerpt: record.corrected_text.chars().take(80).collect(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| match right.same_app.cmp(&left.same_app) {
        Ordering::Equal => match right.created_at.cmp(&left.created_at) {
            Ordering::Equal => left.id.cmp(&right.id),
            ordering => ordering,
        },
        ordering => ordering,
    });
    candidates.truncate(MAX_CANDIDATES);
    candidates
}

pub fn replace_pending(
    pending: &mut Option<PendingSelectedLearning>,
    selected_text: String,
    candidates: Vec<SelectedCorrectionCandidate>,
    now: u64,
    target: FocusedWindowTarget,
    warning: Option<SelectionWarning>,
    multi_diff_enabled: bool,
) -> PrepareSelectedCorrectionResult {
    let token = new_token();
    *pending = Some(PendingSelectedLearning {
        token: token.clone(),
        selected_text: Some(selected_text.clone()),
        candidates: candidates.clone(),
        created_at: now,
        target,
        expired: false,
        confirmed_preview: None,
        multi_diff_enabled,
    });
    PrepareSelectedCorrectionResult {
        selected_text,
        candidates,
        token,
        warning,
        multi_diff_enabled,
    }
}

pub fn all_candidates_match_selected(
    candidates: &[SelectedCorrectionCandidate],
    selected_text: &str,
) -> bool {
    !candidates.is_empty()
        && candidates
            .iter()
            .all(|candidate| candidate.final_text == selected_text)
}

pub fn resolve_pending(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
    history_id: &str,
    now: u64,
) -> Result<ResolvedPendingSelection, String> {
    let Some(current) = pending.as_mut() else {
        return Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string());
    };
    if current.token != token {
        return Err("選択テキストの学習確認トークンが一致しません。".to_string());
    }
    if current.expired || now.saturating_sub(current.created_at) > PENDING_EXPIRY_SECS {
        // 本文は保持し続けず、明示cancel時のfocus復帰に必要なtargetとtokenだけを残す。
        current.selected_text = None;
        current.candidates.clear();
        current.confirmed_preview = None;
        current.expired = true;
        return Err("選択テキストの学習確認は10分で期限切れになりました。".to_string());
    }
    let candidate = current
        .candidates
        .iter()
        .find(|candidate| candidate.id == history_id)
        .cloned()
        .ok_or_else(|| "選択した履歴候補はこの確認操作に含まれていません。".to_string())?;
    Ok(ResolvedPendingSelection {
        selected_text: current
            .selected_text
            .clone()
            .ok_or_else(|| "選択テキストの学習確認は10分で期限切れになりました。".to_string())?,
        candidate,
        multi_diff_enabled: current.multi_diff_enabled,
    })
}

pub fn confirm_preview(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
    history_id: &str,
    confirmed: ConfirmedSelectedPreview,
) -> Result<(), String> {
    let Some(current) = pending.as_mut() else {
        return Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string());
    };
    if current.token != token {
        return Err("選択テキストの学習確認トークンが一致しません。".to_string());
    }
    if current.expired {
        return Err("選択テキストの学習確認は期限切れになりました。".to_string());
    }
    if !current
        .candidates
        .iter()
        .any(|candidate| candidate.id == history_id)
    {
        return Err("選択した履歴候補はこの確認操作に含まれていません。".to_string());
    }
    if confirmed.history_id != history_id {
        return Err("確認済みプレビューの履歴が一致しません。".to_string());
    }
    current.confirmed_preview = Some(confirmed);
    Ok(())
}

pub fn begin_preview(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
    history_id: &str,
) -> Result<(), String> {
    let Some(current) = pending.as_mut() else {
        return Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string());
    };
    if current.token != token {
        return Err("選択テキストの学習確認トークンが一致しません。".to_string());
    }
    if current.expired {
        return Err("選択テキストの学習確認は期限切れになりました。".to_string());
    }
    if !current
        .candidates
        .iter()
        .any(|candidate| candidate.id == history_id)
    {
        return Err("選択した履歴候補はこの確認操作に含まれていません。".to_string());
    }
    current.confirmed_preview = None;
    Ok(())
}

pub fn require_confirmed_preview(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
    history_id: &str,
    comparison_mode: ComparisonMode,
    source_start_utf16: Option<usize>,
    source_end_utf16: Option<usize>,
    source_display_fingerprint: &str,
    preview_fingerprint: &str,
    idempotency_key: &str,
    target_record_id: Option<&str>,
) -> Result<ConfirmedSelectedPreview, String> {
    let Some(current) = pending.as_mut() else {
        return Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string());
    };
    if current.token != token {
        return Err("選択テキストの学習確認トークンが一致しません。".to_string());
    }
    if current.expired {
        current.confirmed_preview = None;
        return Err("選択テキストの学習確認は期限切れになりました。".to_string());
    }
    let Some(confirmed) = current.confirmed_preview.as_ref() else {
        return Err("保存前に選択範囲のプレビューを確認してください。".to_string());
    };
    if confirmed.history_id != history_id
        || confirmed.comparison_mode != comparison_mode
        || confirmed.source_start_utf16 != source_start_utf16
        || confirmed.source_end_utf16 != source_end_utf16
        || confirmed.source_display_fingerprint != source_display_fingerprint
        || confirmed.preview_fingerprint != preview_fingerprint
        || confirmed.idempotency_key != idempotency_key
        || confirmed.target_record_id.as_deref() != target_record_id
    {
        return Err(
            "選択範囲が確認済みプレビューと一致しません。もう一度プレビューしてください。"
                .to_string(),
        );
    }
    Ok(confirmed.clone())
}

pub fn cancel_pending(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
) -> Result<FocusedWindowTarget, String> {
    match pending.as_ref() {
        Some(current) if current.token == token => {
            let target = current.target.clone();
            *pending = None;
            Ok(target)
        }
        _ => Err("選択テキストの学習確認は期限切れかキャンセル済みです。".to_string()),
    }
}

pub fn consume_pending(
    pending: &mut Option<PendingSelectedLearning>,
    token: &str,
) -> Option<FocusedWindowTarget> {
    if pending
        .as_ref()
        .is_some_and(|current| current.token == token)
    {
        pending.take().map(|current| current.target)
    } else {
        None
    }
}

pub fn candidate_matches_history(
    candidate: &SelectedCorrectionCandidate,
    entry: &HistoryEntry,
) -> bool {
    candidate.id == entry.id
        && candidate.final_text == entry.final_text
        && candidate.mode == entry.mode
        && candidate.polish_preset == entry.polish_preset
        && candidate.app_process == entry.app_process
        && candidate.created_at == entry.created_at
        && entry.status == HistoryStatus::Success
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        corrections::{CorrectionRecord, CorrectionStatus},
        polish::PolishState,
    };

    fn history(id: &str, created_at: u64, app: &str, status: HistoryStatus) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            raw_text: format!("raw-{id}"),
            final_text: format!("final-{id}"),
            mode: Mode::Raw,
            duration_ms: 1,
            created_at,
            error: None,
            status,
            polish_state: PolishState::NotRequested,
            pinned: false,
            polish_preset: "memo".into(),
            app_process: app.into(),
            operation_kind: OperationKind::Dictation,
        }
    }

    fn learned(history_id: &str) -> CorrectionRecord {
        CorrectionRecord {
            id: format!("corr-{history_id}"),
            source_history_id: history_id.into(),
            raw_text: "raw".into(),
            original_text: "before".into(),
            corrected_text: "after".into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            classification: crate::corrections::CorrectionClassification::Minor,
            status: CorrectionStatus::Active,
            artifacts: vec![crate::corrections::CorrectionArtifact::None],
            created_at: 1,
            updated_at: 1,
        }
    }

    fn target() -> FocusedWindowTarget {
        FocusedWindowTarget {
            hwnd: 1,
            process_id: 2,
            process_name: "app.exe".into(),
            window_title: String::new(),
        }
    }

    #[test]
    fn candidates_filter_age_status_future_and_include_learned_history_for_reedit() {
        let now = 10_000;
        let items = vec![
            history(
                "fresh",
                now - CANDIDATE_WINDOW_SECS,
                "app.exe",
                HistoryStatus::Success,
            ),
            history(
                "old",
                now - CANDIDATE_WINDOW_SECS - 1,
                "app.exe",
                HistoryStatus::Success,
            ),
            history("future", now + 1, "app.exe", HistoryStatus::Success),
            history("error", now, "app.exe", HistoryStatus::Error),
            history("learned", now, "app.exe", HistoryStatus::Success),
        ];
        let store = CorrectionStore {
            items: vec![learned("learned")],
            ..Default::default()
        };
        let result = select_candidates(&items, &store, "app.exe", now, true);
        assert_eq!(
            result
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["learned", "fresh"]
        );

        let mut undone = learned("fresh");
        undone.status = CorrectionStatus::Undone;
        let reedit = select_candidates(
            &[history("fresh", now, "app.exe", HistoryStatus::Success)],
            &CorrectionStore {
                items: vec![undone],
                ..Default::default()
            },
            "app.exe",
            now,
            true,
        );
        assert_eq!(reedit.len(), 1);
        assert_eq!(
            reedit[0].existing_records[0].status,
            CorrectionStatus::Undone
        );
        assert!(select_candidates(
            &[history("fresh", now, "app.exe", HistoryStatus::Success)],
            &CorrectionStore {
                items: vec![learned("fresh")],
                ..Default::default()
            },
            "app.exe",
            now,
            false,
        )
        .is_empty());
    }

    #[test]
    fn candidates_rank_same_basename_then_newest_then_id_and_cap_five() {
        let now = 10_000;
        let items = vec![
            history("z", now - 1, "other.exe", HistoryStatus::Success),
            history("b", now - 2, r"C:\Apps\NOTEPAD.EXE", HistoryStatus::Success),
            history("a", now - 2, "notepad.exe", HistoryStatus::Success),
            history("c", now - 3, "notepad.exe", HistoryStatus::Success),
            history("d", now - 4, "notepad.exe", HistoryStatus::Success),
            history("e", now - 5, "notepad.exe", HistoryStatus::Success),
            history("f", now - 6, "notepad.exe", HistoryStatus::Success),
        ];
        let result = select_candidates(
            &items,
            &CorrectionStore::default(),
            "notepad.exe",
            now,
            true,
        );
        assert_eq!(
            result
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d", "e"]
        );
        assert!(result.iter().all(|item| item.same_app));
    }

    #[test]
    fn ask_off_expiry_tamper_cancel_and_replacement_are_enforced() {
        assert!(ensure_learning_enabled(CorrectionLearningMode::Off).is_err());
        assert!(ensure_learning_enabled(CorrectionLearningMode::Ask).is_ok());
        let candidate = SelectedCorrectionCandidate {
            id: "h1".into(),
            final_text: "original".into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            created_at: 1,
            same_app: true,
            source_display_text: "original".into(),
            source_display_fingerprint: corrections::source_display_fingerprint("original"),
            existing_records: Vec::new(),
        };
        let mut pending = None;
        let first = replace_pending(
            &mut pending,
            "selected".into(),
            vec![candidate.clone()],
            100,
            target(),
            None,
            true,
        );
        assert!(resolve_pending(&mut pending, "tampered", "h1", 100).is_err());
        assert!(resolve_pending(
            &mut pending,
            "tampered",
            "h1",
            100 + PENDING_EXPIRY_SECS + 1
        )
        .is_err());
        assert!(!pending.as_ref().unwrap().expired);
        assert!(pending.as_ref().unwrap().selected_text.is_some());
        assert!(resolve_pending(&mut pending, &first.token, "tampered", 100).is_err());
        let second = replace_pending(
            &mut pending,
            "new".into(),
            vec![candidate],
            200,
            target(),
            None,
            true,
        );
        assert!(resolve_pending(&mut pending, &first.token, "h1", 200).is_err());
        assert!(resolve_pending(
            &mut pending,
            &second.token,
            "h1",
            200 + PENDING_EXPIRY_SECS + 1
        )
        .is_err());
        assert!(pending.is_some());
        let expired = pending.as_ref().unwrap();
        assert!(expired.expired);
        assert!(expired.selected_text.is_none());
        assert!(expired.candidates.is_empty());
        assert_eq!(
            cancel_pending(&mut pending, &second.token).unwrap(),
            target()
        );
        assert!(pending.is_none());
        let third = replace_pending(
            &mut pending,
            "new".into(),
            Vec::new(),
            300,
            target(),
            None,
            true,
        );
        cancel_pending(&mut pending, &third.token).unwrap();
        assert!(pending.is_none());
    }

    #[test]
    fn preview_range_binding_requires_same_token_history_and_utf16_range() {
        let candidate = SelectedCorrectionCandidate {
            id: "h1".into(),
            final_text: "foo X foo".into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            created_at: 1,
            same_app: true,
            source_display_text: "foo X foo".into(),
            source_display_fingerprint: corrections::source_display_fingerprint("foo X foo"),
            existing_records: Vec::new(),
        };
        let mut pending = None;
        let prepared = replace_pending(
            &mut pending,
            "bar".into(),
            vec![candidate],
            now_secs(),
            target(),
            None,
            true,
        );
        let confirmed = ConfirmedSelectedPreview {
            history_id: "h1".into(),
            comparison_mode: ComparisonMode::ExplicitRange,
            source_start_utf16: Some(0),
            source_end_utf16: Some(3),
            source_display_fingerprint: "source-fp".into(),
            preview_fingerprint: "preview-fp".into(),
            idempotency_key: "idempotency".into(),
            target_record_id: None,
            record_edit_fingerprint: None,
            source_records_fingerprint: "records-fp".into(),
            persisted_unverified_artifacts: Vec::new(),
            persisted_artifacts: Vec::new(),
        };
        let require = |pending: &mut Option<PendingSelectedLearning>,
                       token: &str,
                       history: &str,
                       start,
                       end| {
            require_confirmed_preview(
                pending,
                token,
                history,
                ComparisonMode::ExplicitRange,
                Some(start),
                Some(end),
                "source-fp",
                "preview-fp",
                "idempotency",
                None,
            )
        };
        assert!(require(&mut pending, &prepared.token, "h1", 0, 3).is_err());
        confirm_preview(&mut pending, &prepared.token, "h1", confirmed).unwrap();
        assert!(require(&mut pending, &prepared.token, "h1", 6, 9).is_err());
        assert!(require(&mut pending, &prepared.token, "other", 0, 3).is_err());
        assert!(require(&mut pending, "wrong", "h1", 0, 3).is_err());
        assert!(require(&mut pending, &prepared.token, "h1", 0, 3).is_ok());

        begin_preview(&mut pending, &prepared.token, "h1").unwrap();
        confirm_preview(
            &mut pending,
            &prepared.token,
            "h1",
            ConfirmedSelectedPreview {
                history_id: "h1".into(),
                comparison_mode: ComparisonMode::ExplicitRange,
                source_start_utf16: Some(0),
                source_end_utf16: Some(3),
                source_display_fingerprint: "source-fp".into(),
                preview_fingerprint: "preview-fp".into(),
                idempotency_key: "idempotency".into(),
                target_record_id: Some("record-a".into()),
                record_edit_fingerprint: Some("edit-a".into()),
                source_records_fingerprint: "records-fp".into(),
                persisted_unverified_artifacts: Vec::new(),
                persisted_artifacts: Vec::new(),
            },
        )
        .unwrap();
        assert!(require_confirmed_preview(
            &mut pending,
            &prepared.token,
            "h1",
            ComparisonMode::ExplicitRange,
            Some(0),
            Some(3),
            "source-fp",
            "preview-fp",
            "idempotency",
            Some("record-b"),
        )
        .is_err());
    }

    #[test]
    fn recording_processing_and_active_session_are_rejected() {
        assert!(ensure_session_idle(&RecordingState::Idle, false).is_ok());
        assert!(ensure_session_idle(&RecordingState::Idle, true).is_err());
        assert!(ensure_session_idle(&RecordingState::Recording, false).is_err());
        assert!(ensure_session_idle(&RecordingState::Processing, false).is_err());
    }

    #[test]
    fn unchanged_is_rejected_only_when_every_candidate_matches() {
        let candidate = |id: &str, text: &str| SelectedCorrectionCandidate {
            id: id.into(),
            final_text: text.into(),
            mode: Mode::Raw,
            polish_preset: "memo".into(),
            app_process: "app.exe".into(),
            created_at: 1,
            same_app: true,
            source_display_text: text.into(),
            source_display_fingerprint: corrections::source_display_fingerprint(text),
            existing_records: Vec::new(),
        };
        assert!(all_candidates_match_selected(
            &[candidate("a", "same")],
            "same"
        ));
        assert!(!all_candidates_match_selected(
            &[candidate("a", "same"), candidate("b", "different")],
            "same"
        ));
        assert!(!all_candidates_match_selected(&[], "same"));
    }

    #[test]
    fn replay_cache_accepts_exact_retry_and_rejects_mismatch_or_reused_token() {
        let mut cache = vec![SelectedLearningReplay {
            token: "token".into(),
            idempotency_key: "key".into(),
            preview_fingerprint: "preview".into(),
            create_request_digest: "digest".into(),
            record_id: "record".into(),
            focus_warning: Some("warning".into()),
            created_at: 100,
        }];
        assert_eq!(
            check_replay(&mut cache, "token", "key", "preview", "digest", 101).unwrap(),
            Some(("record".into(), Some("warning".into())))
        );
        assert!(check_replay(&mut cache, "token", "key", "preview", "changed", 101).is_err());
        assert!(check_replay(&mut cache, "token", "other", "preview", "digest", 101).is_err());
        assert_eq!(
            check_replay(
                &mut cache,
                "fresh",
                "fresh-key",
                "preview",
                "digest",
                100 + PENDING_EXPIRY_SECS + 1,
            )
            .unwrap(),
            None
        );
    }
}
