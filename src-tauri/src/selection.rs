use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::context::FocusedWindowTarget;

pub const MAX_SELECTED_TEXT_CHARS: usize = 20_000;

struct CaptureGate(AtomicBool);

impl CaptureGate {
    const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    fn try_acquire(&self) -> Result<CapturePermit<'_>, SelectionError> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| SelectionError::new(SelectionErrorCode::CaptureBusy))?;
        Ok(CapturePermit { gate: self })
    }
}

struct CapturePermit<'a> {
    gate: &'a CaptureGate,
}

impl Drop for CapturePermit<'_> {
    fn drop(&mut self) {
        self.gate.0.store(false, Ordering::Release);
    }
}

static CAPTURE_GATE: CaptureGate = CaptureGate::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMethod {
    UiaTextPattern,
    ClipboardFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionWarning {
    /// Ctrl+C後に別プロセスがclipboardを更新したため、元clipboardを復元しなかった。
    ExternalClipboardChangePreserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionCapture {
    pub target: FocusedWindowTarget,
    pub text: String,
    pub method: SelectionMethod,
    pub warning: Option<SelectionWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionErrorCode {
    TargetUnavailable,
    SelfTarget,
    FocusMismatch,
    PasswordProtected,
    PasswordStateUnavailable,
    UiaUnavailable,
    TextPatternFailure,
    EmptySelection,
    MultipleSelections,
    SelectionTooLarge,
    ModifierKeysHeld,
    ClipboardBusy,
    ClipboardFormatUnsupported,
    ClipboardUnchanged,
    ClipboardTextUnavailable,
    ClipboardRestoreFailed,
    ClipboardSequenceUnavailable,
    UipiBlocked,
    PlatformUnsupported,
    CaptureBusy,
    CaptureTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionError {
    pub code: SelectionErrorCode,
    /// 選択本文を含まない固定メッセージだけを保持する。
    pub message: String,
}

impl SelectionError {
    pub fn new(code: SelectionErrorCode) -> Self {
        Self {
            code,
            message: error_message(code).to_string(),
        }
    }
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SelectionError {}

fn error_message(code: SelectionErrorCode) -> &'static str {
    match code {
        SelectionErrorCode::TargetUnavailable => "選択テキストの取得対象を確認できませんでした。",
        SelectionErrorCode::SelfTarget => "KoeType自身からは選択テキストを取得できません。",
        SelectionErrorCode::FocusMismatch => "操作中に入力先のフォーカスが変わりました。",
        SelectionErrorCode::PasswordProtected => "パスワード入力欄からはテキストを取得できません。",
        SelectionErrorCode::PasswordStateUnavailable => {
            "入力欄の保護状態を確認できないため、テキストを取得しませんでした。"
        }
        SelectionErrorCode::UiaUnavailable => "UI Automationへ接続できませんでした。",
        SelectionErrorCode::TextPatternFailure => {
            "UI Automationから選択範囲を安全に取得できませんでした。"
        }
        SelectionErrorCode::EmptySelection => "選択されているテキストがありません。",
        SelectionErrorCode::MultipleSelections => "複数の選択範囲には対応していません。",
        SelectionErrorCode::SelectionTooLarge => "選択テキストが上限の20,000文字を超えています。",
        SelectionErrorCode::ModifierKeysHeld => {
            "修飾キーが押されたままのため、安全にコピーできませんでした。"
        }
        SelectionErrorCode::ClipboardBusy => "クリップボードを安全に確保できませんでした。",
        SelectionErrorCode::ClipboardFormatUnsupported => {
            "現在のクリップボード形式をすべて保全できないため、コピーを中止しました。"
        }
        SelectionErrorCode::ClipboardUnchanged => {
            "コピー操作でクリップボードが更新されませんでした。"
        }
        SelectionErrorCode::ClipboardTextUnavailable => {
            "コピー結果をUnicodeテキストとして取得できませんでした。"
        }
        SelectionErrorCode::ClipboardRestoreFailed => {
            "コピー前のクリップボードを完全に復元できませんでした。"
        }
        SelectionErrorCode::ClipboardSequenceUnavailable => {
            "クリップボードの更新状態を安全に確認できませんでした。"
        }
        SelectionErrorCode::UipiBlocked => {
            "権限差により対象アプリへコピー操作を送信できませんでした。"
        }
        SelectionErrorCode::PlatformUnsupported => "この環境では選択テキスト取得を利用できません。",
        SelectionErrorCode::CaptureBusy => "別の選択テキスト取得が進行中です。",
        SelectionErrorCode::CaptureTimeout => "選択テキストの取得が時間内に完了しませんでした。",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiaSelection {
    /// TextPatternを明示的に利用できない場合だけfallbackを許可する。
    TextPatternUnsupported,
    Ranges(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSelection {
    pub text: String,
    pub external_change_preserved: bool,
}

/// Win32/UIA所有権処理を状態機械から分離する境界。
/// 実装はエラーへ選択本文を含めてはならない。
pub trait SelectionBackend {
    fn snapshot_target(&mut self) -> Result<FocusedWindowTarget, SelectionError>;
    fn read_uia_selection(
        &mut self,
        target: &FocusedWindowTarget,
    ) -> Result<UiaSelection, SelectionError>;
    fn read_clipboard_fallback(
        &mut self,
        target: &FocusedWindowTarget,
    ) -> Result<ClipboardSelection, SelectionError>;
}

pub fn capture_with_backend<B: SelectionBackend>(
    backend: &mut B,
    self_process_id: u32,
) -> Result<SelectionCapture, SelectionError> {
    let target = backend.snapshot_target()?;
    if target.process_id == self_process_id {
        return Err(SelectionError::new(SelectionErrorCode::SelfTarget));
    }
    if target.hwnd == 0 || target.process_id == 0 {
        return Err(SelectionError::new(SelectionErrorCode::TargetUnavailable));
    }

    match backend.read_uia_selection(&target)? {
        UiaSelection::Ranges(ranges) => Ok(SelectionCapture {
            target,
            text: validate_ranges(ranges)?,
            method: SelectionMethod::UiaTextPattern,
            warning: None,
        }),
        UiaSelection::TextPatternUnsupported => {
            let fallback = backend.read_clipboard_fallback(&target)?;
            let text = validate_single_text(fallback.text)?;
            Ok(SelectionCapture {
                target,
                text,
                method: SelectionMethod::ClipboardFallback,
                warning: fallback
                    .external_change_preserved
                    .then_some(SelectionWarning::ExternalClipboardChangePreserved),
            })
        }
    }
}

fn validate_ranges(mut ranges: Vec<String>) -> Result<String, SelectionError> {
    match ranges.len() {
        0 => Err(SelectionError::new(SelectionErrorCode::EmptySelection)),
        1 => validate_single_text(ranges.pop().expect("single range missing")),
        _ => Err(SelectionError::new(SelectionErrorCode::MultipleSelections)),
    }
}

fn validate_single_text(text: String) -> Result<String, SelectionError> {
    if text.is_empty() {
        return Err(SelectionError::new(SelectionErrorCode::EmptySelection));
    }
    if text.chars().count() > MAX_SELECTED_TEXT_CHARS {
        return Err(SelectionError::new(SelectionErrorCode::SelectionTooLarge));
    }
    Ok(text)
}

fn ensure_scalar_limit(text: &str) -> Result<(), SelectionError> {
    if text.chars().count() > MAX_SELECTED_TEXT_CHARS {
        Err(SelectionError::new(SelectionErrorCode::SelectionTooLarge))
    } else {
        Ok(())
    }
}

const UIA_TEXT_REQUEST_UTF16_UNITS: usize = MAX_SELECTED_TEXT_CHARS * 2 + 1;
const CLIPBOARD_SCAN_UTF16_UNITS: usize = MAX_SELECTED_TEXT_CHARS * 2 + 2;

fn decode_uia_text_units(units: &[u16]) -> Result<String, SelectionError> {
    if units.len() >= UIA_TEXT_REQUEST_UTF16_UNITS {
        return Err(SelectionError::new(SelectionErrorCode::SelectionTooLarge));
    }
    let text = String::from_utf16(units)
        .map_err(|_| SelectionError::new(SelectionErrorCode::TextPatternFailure))?;
    ensure_scalar_limit(&text)?;
    Ok(text)
}

fn bounded_clipboard_unit_count(total_units: usize) -> usize {
    total_units.min(CLIPBOARD_SCAN_UTF16_UNITS)
}

fn decode_clipboard_units(units: &[u16]) -> Result<String, SelectionError> {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| SelectionError::new(SelectionErrorCode::SelectionTooLarge))?;
    let text = String::from_utf16(&units[..end])
        .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardTextUnavailable))?;
    ensure_scalar_limit(&text)?;
    Ok(text)
}

#[cfg(target_os = "windows")]
pub(crate) struct ClipboardOwner(windows::Win32::Foundation::HWND);

#[cfg(target_os = "windows")]
impl ClipboardOwner {
    pub(crate) fn create() -> Result<Self, SelectionError> {
        use windows::{
            core::w,
            Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
            },
        };
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("KoeTypeClipboardOwner"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                None,
                None,
            )
        }
        .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardBusy))?;
        Ok(Self(hwnd))
    }
}

#[cfg(target_os = "windows")]
impl Drop for ClipboardOwner {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0) };
    }
}

#[cfg(target_os = "windows")]
struct OpenClipboardGuard;

#[cfg(target_os = "windows")]
impl OpenClipboardGuard {
    fn open(owner: &ClipboardOwner) -> Result<Self, SelectionError> {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            if unsafe { windows::Win32::System::DataExchange::OpenClipboard(owner.0) }.is_ok() {
                return Ok(Self);
            }
            if Instant::now() >= deadline {
                return Err(SelectionError::new(SelectionErrorCode::ClipboardBusy));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::System::DataExchange::CloseClipboard() };
    }
}

#[cfg(target_os = "windows")]
struct DuplicatedClipboardFormat {
    format: u32,
    handle: windows::Win32::Foundation::HANDLE,
    retry_handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(target_os = "windows")]
impl Drop for DuplicatedClipboardFormat {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
        if !self.handle.is_invalid() {
            let _ = unsafe { GlobalFree(HGLOBAL(self.handle.0)) };
        }
        if !self.retry_handle.is_invalid() {
            let _ = unsafe { GlobalFree(HGLOBAL(self.retry_handle.0)) };
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct ClipboardSnapshot {
    formats: Vec<DuplicatedClipboardFormat>,
}

#[cfg(target_os = "windows")]
impl ClipboardSnapshot {
    pub(crate) fn capture(owner: &ClipboardOwner) -> Result<Self, SelectionError> {
        let _open = OpenClipboardGuard::open(owner)?;
        Self::capture_open()
    }

    fn capture_open() -> Result<Self, SelectionError> {
        use windows::Win32::{
            Foundation::{GetLastError, SetLastError, WIN32_ERROR},
            System::{
                DataExchange::{EnumClipboardFormats, GetClipboardData},
                Memory::GMEM_MOVEABLE,
                Ole::{OleDuplicateData, CLIPBOARD_FORMAT},
            },
        };
        let mut formats = Vec::new();
        let mut current = 0_u32;
        loop {
            unsafe { SetLastError(WIN32_ERROR(0)) };
            let next = unsafe { EnumClipboardFormats(current) };
            if next == 0 {
                if unsafe { GetLastError() } != WIN32_ERROR(0) {
                    return Err(SelectionError::new(
                        SelectionErrorCode::ClipboardFormatUnsupported,
                    ));
                }
                break;
            }
            if !clipboard_format_uses_hglobal(next) {
                return Err(SelectionError::new(
                    SelectionErrorCode::ClipboardFormatUnsupported,
                ));
            }
            let source = unsafe { GetClipboardData(next) }
                .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardFormatUnsupported))?;
            let duplicate =
                unsafe { OleDuplicateData(source, CLIPBOARD_FORMAT(next as u16), GMEM_MOVEABLE) };
            if duplicate.is_invalid() {
                return Err(SelectionError::new(
                    SelectionErrorCode::ClipboardFormatUnsupported,
                ));
            }
            let retry_handle =
                unsafe { OleDuplicateData(source, CLIPBOARD_FORMAT(next as u16), GMEM_MOVEABLE) };
            if retry_handle.is_invalid() {
                let _ = unsafe {
                    windows::Win32::Foundation::GlobalFree(windows::Win32::Foundation::HGLOBAL(
                        duplicate.0,
                    ))
                };
                return Err(SelectionError::new(
                    SelectionErrorCode::ClipboardFormatUnsupported,
                ));
            }
            formats.push(DuplicatedClipboardFormat {
                format: next,
                handle: duplicate,
                retry_handle,
            });
            current = next;
        }
        Ok(Self { formats })
    }

    /// Copy fallback用。既知のcopy sequenceとの一致だけを要求する。
    fn restore_known_copy(
        &mut self,
        owner: &ClipboardOwner,
        expected_sequence: u32,
    ) -> Result<bool, SelectionError> {
        self.restore_with_policy(owner, expected_sequence)
    }

    fn restore_with_policy(
        &mut self,
        owner: &ClipboardOwner,
        expected_sequence: u32,
    ) -> Result<bool, SelectionError> {
        use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
        let _open = OpenClipboardGuard::open(owner)?;
        validate_clipboard_sequence(expected_sequence)?;
        let current_sequence =
            validate_clipboard_sequence(unsafe { GetClipboardSequenceNumber() })?;
        let sequence_matches = current_sequence == expected_sequence;
        if !sequence_matches {
            return Ok(false);
        }
        if self.restore_open(false).is_ok() {
            return Ok(true);
        }
        // 1組目が途中まで移譲された場合も、独立した2組目から一度だけ再構築する。
        self.restore_open(true)
            .map(|()| true)
            .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardRestoreFailed))
    }

    fn restore_open(&mut self, retry: bool) -> Result<(), SelectionError> {
        use windows::Win32::{
            Foundation::HANDLE,
            System::DataExchange::{EmptyClipboard, SetClipboardData},
        };
        unsafe { EmptyClipboard() }
            .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardRestoreFailed))?;
        for item in &mut self.formats {
            let handle = if retry {
                item.retry_handle
            } else {
                item.handle
            };
            unsafe { SetClipboardData(item.format, handle) }
                .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardRestoreFailed))?;
            if retry {
                item.retry_handle = HANDLE::default();
            } else {
                item.handle = HANDLE::default();
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub(crate) struct ClipboardPasteTransaction {
    owner: ClipboardOwner,
    result_sequence: u32,
}

#[cfg(target_os = "windows")]
impl ClipboardPasteTransaction {
    pub(crate) fn begin(text: &str) -> Result<Self, SelectionError> {
        use windows::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber};

        let owner = ClipboardOwner::create()?;
        let _open = OpenClipboardGuard::open(&owner)?;
        if let Err(error) = set_unicode_clipboard_open(text) {
            return Err(error);
        }
        let result_sequence =
            match validate_clipboard_sequence(unsafe { GetClipboardSequenceNumber() }) {
                Ok(sequence) => sequence,
                Err(error) => {
                    return Err(error);
                }
            };
        if unsafe { GetClipboardOwner() }.ok() != Some(owner.0) {
            return Err(SelectionError::new(
                SelectionErrorCode::ClipboardSequenceUnavailable,
            ));
        }
        drop(_open);
        Ok(Self {
            owner,
            result_sequence,
        })
    }

    /// 貼り付け直前に、KoeTypeが設定した本文がまだ利用可能か確認する。
    /// sequenceが変わった場合だけclipboardを開いたまま本文とsequenceを読み、
    /// 同一本文かつ安定している場合に限って復元の基準sequenceを更新する。
    pub(crate) fn preflight(&mut self, expected_text: &str) -> ClipboardPreflight {
        let current_sequence = match clipboard_sequence() {
            Ok(sequence) => sequence,
            Err(_) => {
                log_clipboard_preflight(false, false, false, false);
                return ClipboardPreflight::Unavailable;
            }
        };
        if clipboard_result_is_current(self.result_sequence, current_sequence) {
            log_clipboard_preflight(false, true, true, true);
            return ClipboardPreflight::Current;
        }

        match read_current_unicode_clipboard(&self.owner) {
            Ok(read) => apply_changed_clipboard_preflight(
                &mut self.result_sequence,
                expected_text,
                Ok(ClipboardPreflightRead {
                    text: &read.text,
                    sequence: read.sequence,
                    owner_matches: read.owner_matches,
                }),
            ),
            Err(error) => apply_changed_clipboard_preflight(
                &mut self.result_sequence,
                expected_text,
                Err(error),
            ),
        }
    }
}

#[cfg(target_os = "windows")]
fn set_unicode_clipboard_open(text: &str) -> Result<(), SelectionError> {
    use windows::Win32::{
        Foundation::{HANDLE, HGLOBAL},
        System::{
            DataExchange::{EmptyClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            Ole::CF_UNICODETEXT,
        },
    };

    let units = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let bytes = units
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| SelectionError::new(SelectionErrorCode::ClipboardFormatUnsupported))?;
    let global = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
        .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardFormatUnsupported))?;
    let pointer = unsafe { GlobalLock(global) } as *mut u16;
    if pointer.is_null() {
        let _ = unsafe { windows::Win32::Foundation::GlobalFree(global) };
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardFormatUnsupported,
        ));
    }
    unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len()) };
    let _ = unsafe { GlobalUnlock(global) };
    if unsafe { EmptyClipboard() }.is_err() {
        let _ = unsafe { windows::Win32::Foundation::GlobalFree(HGLOBAL(global.0)) };
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardRestoreFailed,
        ));
    }
    if unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(global.0)) }.is_err() {
        let _ = unsafe { windows::Win32::Foundation::GlobalFree(HGLOBAL(global.0)) };
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardRestoreFailed,
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn clipboard_result_is_current(result_sequence: u32, current_sequence: u32) -> bool {
    result_sequence == current_sequence
}

fn clipboard_text_matches_expected(expected_text: &str, current_text: &str) -> bool {
    expected_text == current_text
}

struct ClipboardPreflightRead<'a> {
    text: &'a str,
    sequence: u32,
    owner_matches: bool,
}

fn apply_changed_clipboard_preflight(
    result_sequence: &mut u32,
    expected_text: &str,
    read: Result<ClipboardPreflightRead<'_>, SelectionError>,
) -> ClipboardPreflight {
    let Ok(read) = read else {
        log_clipboard_preflight(true, false, false, false);
        return ClipboardPreflight::Unavailable;
    };
    let equivalent = clipboard_text_matches_expected(expected_text, read.text);
    log_clipboard_preflight(true, read.owner_matches, equivalent, true);
    if equivalent {
        *result_sequence = read.sequence;
        ClipboardPreflight::Equivalent
    } else {
        ClipboardPreflight::Different
    }
}

fn log_clipboard_preflight(
    sequence_changed: bool,
    owner_matches: bool,
    equivalent_text: bool,
    available: bool,
) {
    tracing::debug!(
        sequence_changed,
        owner_matches,
        equivalent_text,
        available,
        current_branch = !sequence_changed && available,
        equivalent_branch = sequence_changed && available && equivalent_text,
        different_branch = sequence_changed && available && !equivalent_text,
        unavailable_branch = !available,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardPreflight {
    Current,
    Equivalent,
    Different,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackCleanupState {
    PartialSend,
    ClipboardTimeout,
    FocusMismatch,
    KnownCopy { sequence: u32, target_matches: bool },
}

trait ClipboardRestorer {
    fn restore_known(&mut self, expected_sequence: u32) -> Result<bool, SelectionError>;
}

#[cfg(target_os = "windows")]
struct SnapshotRestorer<'a> {
    snapshot: &'a mut ClipboardSnapshot,
    owner: &'a ClipboardOwner,
}

#[cfg(target_os = "windows")]
impl ClipboardRestorer for SnapshotRestorer<'_> {
    fn restore_known(&mut self, expected_sequence: u32) -> Result<bool, SelectionError> {
        self.snapshot
            .restore_known_copy(self.owner, expected_sequence)
    }
}

/// Some(true/false)はrestoreを試行した結果、Noneは外部更新保護のため未試行。
fn apply_fallback_cleanup<R: ClipboardRestorer>(
    state: FallbackCleanupState,
    restorer: &mut R,
) -> Result<Option<bool>, SelectionError> {
    match state {
        FallbackCleanupState::KnownCopy {
            sequence,
            target_matches: true,
        } => restorer.restore_known(sequence).map(Some),
        FallbackCleanupState::PartialSend
        | FallbackCleanupState::ClipboardTimeout
        | FallbackCleanupState::FocusMismatch
        | FallbackCleanupState::KnownCopy {
            target_matches: false,
            ..
        } => Ok(None),
    }
}

#[cfg(target_os = "windows")]
fn clipboard_format_uses_hglobal(format: u32) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x0080;
    const CF_DSP_LAST: u32 = 0x008f;
    const CF_PRIVATE_FIRST: u32 = 0x0200;
    const CF_PRIVATE_LAST: u32 = 0x02ff;
    const CF_GDIOBJ_FIRST: u32 = 0x0300;
    const CF_GDIOBJ_LAST: u32 = 0x03ff;
    !matches!(
        format,
        CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | CF_ENHMETAFILE
    ) && !(CF_OWNERDISPLAY..=CF_DSP_LAST).contains(&format)
        && !(CF_PRIVATE_FIRST..=CF_PRIVATE_LAST).contains(&format)
        && !(CF_GDIOBJ_FIRST..=CF_GDIOBJ_LAST).contains(&format)
}

pub(crate) fn validate_clipboard_sequence(sequence: u32) -> Result<u32, SelectionError> {
    if sequence == 0 {
        Err(SelectionError::new(
            SelectionErrorCode::ClipboardSequenceUnavailable,
        ))
    } else {
        Ok(sequence)
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn clipboard_sequence() -> Result<u32, SelectionError> {
    validate_clipboard_sequence(unsafe {
        windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
    })
}

#[cfg(target_os = "windows")]
fn wait_for_modifier_release() -> Result<(), SelectionError> {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let released = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .into_iter()
            .all(|key| unsafe { GetAsyncKeyState(key.0 as i32) } >= 0);
        if released {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(SelectionError::new(SelectionErrorCode::ModifierKeysHeld));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(target_os = "windows")]
fn send_ctrl_c() -> Result<(), SelectionError> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL,
    };
    fn key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: if key_up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    let c_key = VIRTUAL_KEY(b'C' as u16);
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(c_key, false),
        key_input(c_key, true),
        key_input(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
    if sent != inputs.len() {
        // 部分送信でも修飾キーを押下状態に残さないよう、keyupをbest effortで送る。
        let releases = [key_input(c_key, true), key_input(VK_CONTROL, true)];
        let _ = unsafe { SendInput(&releases, std::mem::size_of::<INPUT>() as i32) };
        return Err(SelectionError::new(SelectionErrorCode::UipiBlocked));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn wait_for_clipboard_change(previous: u32) -> Result<u32, SelectionError> {
    use std::time::{Duration, Instant};
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        let current = validate_clipboard_sequence(unsafe { GetClipboardSequenceNumber() })?;
        if current != previous {
            return Ok(current);
        }
        if Instant::now() >= deadline {
            return Err(SelectionError::new(SelectionErrorCode::ClipboardUnchanged));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(target_os = "windows")]
fn read_unicode_clipboard(
    owner: &ClipboardOwner,
    expected_sequence: u32,
) -> Result<String, SelectionError> {
    let read = read_current_unicode_clipboard(owner)?;
    if read.sequence != expected_sequence {
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardTextUnavailable,
        ));
    }
    Ok(read.text)
}

#[cfg(target_os = "windows")]
struct CurrentUnicodeClipboard {
    text: String,
    sequence: u32,
    owner_matches: bool,
}

/// clipboardを開いたままCF_UNICODETEXTと前後のsequenceを確認する。
/// 本文や変換結果は呼び出し元の比較以外に出力しない。
#[cfg(target_os = "windows")]
fn read_current_unicode_clipboard(
    owner: &ClipboardOwner,
) -> Result<CurrentUnicodeClipboard, SelectionError> {
    use windows::Win32::{
        Foundation::HGLOBAL,
        System::{
            DataExchange::{GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
    };
    let _open = OpenClipboardGuard::open(owner)?;
    let sequence = validate_clipboard_sequence(unsafe { GetClipboardSequenceNumber() })?;
    let owner_matches = unsafe { GetClipboardOwner() }.ok() == Some(owner.0);
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }
        .map_err(|_| SelectionError::new(SelectionErrorCode::ClipboardTextUnavailable))?;
    let global = HGLOBAL(handle.0);
    let bytes = unsafe { GlobalSize(global) };
    if bytes < std::mem::size_of::<u16>() {
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardTextUnavailable,
        ));
    }
    let pointer = unsafe { GlobalLock(global) } as *const u16;
    if pointer.is_null() {
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardTextUnavailable,
        ));
    }
    let unit_count = bounded_clipboard_unit_count(bytes / std::mem::size_of::<u16>());
    let units = unsafe { std::slice::from_raw_parts(pointer, unit_count) };
    let result = decode_clipboard_units(units);
    let _ = unsafe { GlobalUnlock(global) };
    let text = result?;
    if validate_clipboard_sequence(unsafe { GetClipboardSequenceNumber() })? != sequence {
        return Err(SelectionError::new(
            SelectionErrorCode::ClipboardTextUnavailable,
        ));
    }
    Ok(CurrentUnicodeClipboard {
        text,
        sequence,
        owner_matches,
    })
}

#[cfg(target_os = "windows")]
fn read_clipboard_fallback_windows(
    target: &FocusedWindowTarget,
) -> Result<ClipboardSelection, SelectionError> {
    wait_for_modifier_release()?;
    let owner = ClipboardOwner::create()?;
    let mut snapshot = ClipboardSnapshot::capture(&owner)?;
    let sequence_before = clipboard_sequence()?;
    if !foreground_matches(target) {
        return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
    }
    if let Err(send_error) = send_ctrl_c() {
        // partial SendInput後のclipboard更新主体は識別不能。外部更新を上書きしない。
        let mut restorer = SnapshotRestorer {
            snapshot: &mut snapshot,
            owner: &owner,
        };
        let _ = apply_fallback_cleanup(FallbackCleanupState::PartialSend, &mut restorer)?;
        return Err(send_error);
    }
    let copy_sequence = match wait_for_clipboard_change(sequence_before) {
        Ok(sequence) => sequence,
        Err(wait_error) => {
            // timeout後のclipboard更新主体は識別不能。外部更新を上書きしない。
            let mut restorer = SnapshotRestorer {
                snapshot: &mut snapshot,
                owner: &owner,
            };
            let _ = apply_fallback_cleanup(FallbackCleanupState::ClipboardTimeout, &mut restorer)?;
            return Err(wait_error);
        }
    };
    if !foreground_matches(target) {
        let mut restorer = SnapshotRestorer {
            snapshot: &mut snapshot,
            owner: &owner,
        };
        let _ = apply_fallback_cleanup(FallbackCleanupState::FocusMismatch, &mut restorer)?;
        return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
    }
    let text = match read_unicode_clipboard(&owner, copy_sequence) {
        Ok(text) => text,
        Err(error) => {
            let mut restorer = SnapshotRestorer {
                snapshot: &mut snapshot,
                owner: &owner,
            };
            let cleanup = apply_fallback_cleanup(
                FallbackCleanupState::KnownCopy {
                    sequence: copy_sequence,
                    target_matches: foreground_matches(target),
                },
                &mut restorer,
            );
            return match cleanup {
                Ok(_) => Err(error),
                Err(_) => Err(SelectionError::new(
                    SelectionErrorCode::ClipboardRestoreFailed,
                )),
            };
        }
    };
    if !foreground_matches(target) {
        let mut restorer = SnapshotRestorer {
            snapshot: &mut snapshot,
            owner: &owner,
        };
        let _ = apply_fallback_cleanup(FallbackCleanupState::FocusMismatch, &mut restorer)?;
        return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
    }
    let mut restorer = SnapshotRestorer {
        snapshot: &mut snapshot,
        owner: &owner,
    };
    let restored = apply_fallback_cleanup(
        FallbackCleanupState::KnownCopy {
            sequence: copy_sequence,
            target_matches: true,
        },
        &mut restorer,
    )?
    .expect("known matching copy must attempt restore");
    Ok(ClipboardSelection {
        text,
        external_change_preserved: !restored,
    })
}

#[cfg(target_os = "windows")]
struct WindowsSelectionBackend {
    target: Option<FocusedWindowTarget>,
}

#[cfg(target_os = "windows")]
impl SelectionBackend for WindowsSelectionBackend {
    fn snapshot_target(&mut self) -> Result<FocusedWindowTarget, SelectionError> {
        self.target
            .take()
            .ok_or_else(|| SelectionError::new(SelectionErrorCode::TargetUnavailable))
    }

    fn read_uia_selection(
        &mut self,
        target: &FocusedWindowTarget,
    ) -> Result<UiaSelection, SelectionError> {
        read_uia_selection_windows(target)
    }

    fn read_clipboard_fallback(
        &mut self,
        target: &FocusedWindowTarget,
    ) -> Result<ClipboardSelection, SelectionError> {
        read_clipboard_fallback_windows(target)
    }
}

#[cfg(target_os = "windows")]
fn foreground_matches(target: &FocusedWindowTarget) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0 as isize != target.hwnd {
            return false;
        }
        let mut process_id = 0_u32;
        GetWindowThreadProcessId(foreground, Some(&mut process_id));
        process_id == target.process_id
    }
}

#[cfg(target_os = "windows")]
fn read_uia_selection_windows(
    target: &FocusedWindowTarget,
) -> Result<UiaSelection, SelectionError> {
    use windows::Win32::{
        Foundation::HWND,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
            UIA_E_NOTSUPPORTED,
        },
    };

    if !foreground_matches(target) {
        return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
    }
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| SelectionError::new(SelectionErrorCode::UiaUnavailable))?;
        let target_element = automation
            .ElementFromHandle(HWND(target.hwnd as *mut _))
            .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?;
        let focused = automation
            .GetFocusedElement()
            .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?;
        let focused_pid = focused
            .CurrentProcessId()
            .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?;
        if focused_pid != target.process_id as i32 {
            return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
        }
        if !focused
            .CurrentHasKeyboardFocus()
            .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?
            .as_bool()
        {
            return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
        }
        ensure_not_password(&automation, &focused, &target_element)?;

        let pattern =
            match focused.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                Ok(pattern) => pattern,
                Err(error) if error.code().0 as u32 == UIA_E_NOTSUPPORTED => {
                    return Ok(UiaSelection::TextPatternUnsupported)
                }
                Err(_) => return Err(SelectionError::new(SelectionErrorCode::TextPatternFailure)),
            };
        let selection = pattern
            .GetSelection()
            .map_err(|_| SelectionError::new(SelectionErrorCode::TextPatternFailure))?;
        let count = selection
            .Length()
            .map_err(|_| SelectionError::new(SelectionErrorCode::TextPatternFailure))?;
        let mut ranges = Vec::with_capacity(count.max(0) as usize);
        for index in 0..count {
            let range = selection
                .GetElement(index)
                .map_err(|_| SelectionError::new(SelectionErrorCode::TextPatternFailure))?;
            let text = range
                .GetText(UIA_TEXT_REQUEST_UTF16_UNITS as i32)
                .map_err(|_| SelectionError::new(SelectionErrorCode::TextPatternFailure))?;
            ranges.push(decode_uia_text_units(text.as_wide())?);
        }
        if !foreground_matches(target) {
            return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
        }
        let focused_after = automation
            .GetFocusedElement()
            .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?;
        if focused_after
            .CurrentProcessId()
            .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?
            != target.process_id as i32
            || !focused_after
                .CurrentHasKeyboardFocus()
                .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?
                .as_bool()
            || !automation
                .CompareElements(&focused, &focused_after)
                .map_err(|_| SelectionError::new(SelectionErrorCode::FocusMismatch))?
                .as_bool()
        {
            return Err(SelectionError::new(SelectionErrorCode::FocusMismatch));
        }
        Ok(UiaSelection::Ranges(ranges))
    }
}

#[cfg(target_os = "windows")]
unsafe fn ensure_not_password(
    automation: &windows::Win32::UI::Accessibility::IUIAutomation,
    focused: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    target_element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Result<(), SelectionError> {
    const MAX_ANCESTORS: usize = 64;
    let walker = automation
        .ControlViewWalker()
        .map_err(|_| SelectionError::new(SelectionErrorCode::PasswordStateUnavailable))?;
    let mut current = focused.clone();
    for _ in 0..MAX_ANCESTORS {
        if current
            .CurrentIsPassword()
            .map_err(|_| SelectionError::new(SelectionErrorCode::PasswordStateUnavailable))?
            .as_bool()
        {
            return Err(SelectionError::new(SelectionErrorCode::PasswordProtected));
        }
        if automation
            .CompareElements(&current, target_element)
            .map_err(|_| SelectionError::new(SelectionErrorCode::PasswordStateUnavailable))?
            .as_bool()
        {
            return Ok(());
        }
        current = walker
            .GetParentElement(&current)
            .map_err(|_| SelectionError::new(SelectionErrorCode::PasswordStateUnavailable))?;
    }
    Err(SelectionError::new(
        SelectionErrorCode::PasswordStateUnavailable,
    ))
}

#[cfg(target_os = "windows")]
struct ComApartment;

#[cfg(target_os = "windows")]
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }
}

#[cfg(target_os = "windows")]
fn initialize_mta() -> Result<ComApartment, SelectionError> {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result.is_err() {
        Err(SelectionError::new(SelectionErrorCode::UiaUnavailable))
    } else {
        Ok(ComApartment)
    }
}

#[cfg(target_os = "windows")]
pub async fn capture_selected_text() -> Result<SelectionCapture, SelectionError> {
    use std::time::Duration;

    let permit = CAPTURE_GATE.try_acquire()?;
    let target = crate::context::focused_window_target()
        .ok_or_else(|| SelectionError::new(SelectionErrorCode::TargetUnavailable))?;
    let mut task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _apartment = initialize_mta()?;
        let mut backend = WindowsSelectionBackend {
            target: Some(target),
        };
        capture_with_backend(&mut backend, std::process::id())
    });
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(joined) => {
            joined.map_err(|_| SelectionError::new(SelectionErrorCode::UiaUnavailable))?
        }
        Err(_) => Err(SelectionError::new(SelectionErrorCode::CaptureTimeout)),
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn capture_selected_text() -> Result<SelectionCapture, SelectionError> {
    Err(SelectionError::new(SelectionErrorCode::PlatformUnsupported))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBackend {
        target: Result<FocusedWindowTarget, SelectionError>,
        uia: Result<UiaSelection, SelectionError>,
        clipboard: Result<ClipboardSelection, SelectionError>,
        uia_calls: usize,
        clipboard_calls: usize,
    }

    impl SelectionBackend for FakeBackend {
        fn snapshot_target(&mut self) -> Result<FocusedWindowTarget, SelectionError> {
            self.target.clone()
        }

        fn read_uia_selection(
            &mut self,
            _target: &FocusedWindowTarget,
        ) -> Result<UiaSelection, SelectionError> {
            self.uia_calls += 1;
            self.uia.clone()
        }

        fn read_clipboard_fallback(
            &mut self,
            _target: &FocusedWindowTarget,
        ) -> Result<ClipboardSelection, SelectionError> {
            self.clipboard_calls += 1;
            self.clipboard.clone()
        }
    }

    fn target(pid: u32) -> FocusedWindowTarget {
        FocusedWindowTarget {
            hwnd: 10,
            process_id: pid,
            process_name: "notepad.exe".into(),
            window_title: String::new(),
        }
    }

    fn fake(uia: Result<UiaSelection, SelectionError>) -> FakeBackend {
        FakeBackend {
            target: Ok(target(42)),
            uia,
            clipboard: Ok(ClipboardSelection {
                text: "fallback".into(),
                external_change_preserved: false,
            }),
            uia_calls: 0,
            clipboard_calls: 0,
        }
    }

    #[test]
    fn uia_single_selection_succeeds_without_clipboard() {
        let mut backend = fake(Ok(UiaSelection::Ranges(vec!["selected".into()])));
        let result = capture_with_backend(&mut backend, 999).unwrap();
        assert_eq!(result.text, "selected");
        assert_eq!(result.method, SelectionMethod::UiaTextPattern);
        assert_eq!(backend.clipboard_calls, 0);
    }

    #[test]
    fn only_explicit_text_pattern_unsupported_uses_fallback() {
        let mut backend = fake(Ok(UiaSelection::TextPatternUnsupported));
        let result = capture_with_backend(&mut backend, 999).unwrap();
        assert_eq!(result.method, SelectionMethod::ClipboardFallback);
        assert_eq!(backend.clipboard_calls, 1);

        for code in [
            SelectionErrorCode::PasswordProtected,
            SelectionErrorCode::PasswordStateUnavailable,
            SelectionErrorCode::FocusMismatch,
            SelectionErrorCode::TextPatternFailure,
            SelectionErrorCode::UiaUnavailable,
        ] {
            let mut backend = fake(Err(SelectionError::new(code)));
            assert_eq!(
                capture_with_backend(&mut backend, 999).unwrap_err().code,
                code
            );
            assert_eq!(backend.clipboard_calls, 0);
        }
    }

    #[test]
    fn target_missing_invalid_and_self_are_rejected_before_reads() {
        let mut missing = fake(Ok(UiaSelection::Ranges(vec!["x".into()])));
        missing.target = Err(SelectionError::new(SelectionErrorCode::TargetUnavailable));
        assert_eq!(
            capture_with_backend(&mut missing, 999).unwrap_err().code,
            SelectionErrorCode::TargetUnavailable
        );

        let mut invalid = fake(Ok(UiaSelection::Ranges(vec!["x".into()])));
        invalid.target = Ok(target(0));
        assert_eq!(
            capture_with_backend(&mut invalid, 999).unwrap_err().code,
            SelectionErrorCode::TargetUnavailable
        );

        let mut own = fake(Ok(UiaSelection::Ranges(vec!["x".into()])));
        own.target = Ok(target(999));
        assert_eq!(
            capture_with_backend(&mut own, 999).unwrap_err().code,
            SelectionErrorCode::SelfTarget
        );
        assert_eq!(own.uia_calls, 0);
    }

    #[test]
    fn empty_multiple_and_oversized_are_distinct() {
        let cases = [
            (vec![], SelectionErrorCode::EmptySelection),
            (vec![String::new()], SelectionErrorCode::EmptySelection),
            (
                vec!["a".into(), "b".into()],
                SelectionErrorCode::MultipleSelections,
            ),
            (
                vec!["あ".repeat(MAX_SELECTED_TEXT_CHARS + 1)],
                SelectionErrorCode::SelectionTooLarge,
            ),
        ];
        for (ranges, expected) in cases {
            let mut backend = fake(Ok(UiaSelection::Ranges(ranges)));
            assert_eq!(
                capture_with_backend(&mut backend, 999).unwrap_err().code,
                expected
            );
            assert_eq!(backend.clipboard_calls, 0);
        }
    }

    #[test]
    fn fallback_validates_text_and_surfaces_external_change_warning() {
        let mut backend = fake(Ok(UiaSelection::TextPatternUnsupported));
        backend.clipboard = Ok(ClipboardSelection {
            text: "copied".into(),
            external_change_preserved: true,
        });
        let result = capture_with_backend(&mut backend, 999).unwrap();
        assert_eq!(
            result.warning,
            Some(SelectionWarning::ExternalClipboardChangePreserved)
        );

        backend.clipboard = Ok(ClipboardSelection {
            text: String::new(),
            external_change_preserved: false,
        });
        assert_eq!(
            capture_with_backend(&mut backend, 999).unwrap_err().code,
            SelectionErrorCode::EmptySelection
        );
    }

    #[test]
    fn serialized_errors_never_include_selected_text() {
        let secret = "private selected text";
        let mut backend = fake(Ok(UiaSelection::Ranges(vec![
            secret.repeat(MAX_SELECTED_TEXT_CHARS + 1)
        ])));
        let error = capture_with_backend(&mut backend, 999).unwrap_err();
        let serialized = serde_json::to_string(&error).unwrap();
        assert!(!serialized.contains(secret));
    }

    #[test]
    fn zero_clipboard_sequence_is_never_accepted() {
        for _checkpoint in ["before", "copy", "restore"] {
            assert_eq!(
                validate_clipboard_sequence(0).unwrap_err().code,
                SelectionErrorCode::ClipboardSequenceUnavailable
            );
        }
        assert_eq!(validate_clipboard_sequence(1).unwrap(), 1);
    }

    #[test]
    fn paste_preflight_accepts_the_current_result_sequence() {
        assert!(clipboard_result_is_current(42, 42));
        assert!(!clipboard_result_is_current(42, 43));
    }

    #[test]
    fn paste_preflight_requires_exact_unicode_and_newline_match() {
        let expected = "一行目\r\n😀 二行目。日本語の長い本文をそのまま比較します。";
        assert!(clipboard_text_matches_expected(expected, expected));
        assert!(!clipboard_text_matches_expected(
            expected,
            "一行目\n😀 二行目。日本語の長い本文をそのまま比較します。"
        ));
        assert!(!clipboard_text_matches_expected(
            expected,
            "一行目\r\n😀 二行目。日本語の長い本文をそのまま比較します。 "
        ));
    }

    #[test]
    fn changed_preflight_read_result_refreshes_baseline_and_preserves_owner_guard() {
        let expected = "一行目\r\n😀 二行目。テスト用の長い日本語本文です。";
        let different = "一行目\r\n😀 二行目。異なる本文です。";
        let mut baseline = 10;

        assert_eq!(
            apply_changed_clipboard_preflight(
                &mut baseline,
                expected,
                Ok(ClipboardPreflightRead {
                    text: expected,
                    sequence: 11,
                    owner_matches: true,
                })
            ),
            ClipboardPreflight::Equivalent
        );
        assert_eq!(baseline, 11);
        assert_eq!(
            apply_changed_clipboard_preflight(
                &mut baseline,
                expected,
                Ok(ClipboardPreflightRead {
                    text: different,
                    sequence: 12,
                    owner_matches: false,
                })
            ),
            ClipboardPreflight::Different
        );
        assert_eq!(baseline, 11);
        assert_eq!(
            apply_changed_clipboard_preflight(
                &mut baseline,
                expected,
                Err(SelectionError::new(
                    SelectionErrorCode::ClipboardTextUnavailable
                ))
            ),
            ClipboardPreflight::Unavailable
        );
        assert_eq!(baseline, 11);
    }

    #[test]
    fn capture_gate_is_single_flight_and_releases_on_drop() {
        let gate = CaptureGate::new();
        let permit = gate.try_acquire().unwrap();
        let busy = match gate.try_acquire() {
            Ok(_) => panic!("second capture unexpectedly acquired"),
            Err(error) => error,
        };
        assert_eq!(busy.code, SelectionErrorCode::CaptureBusy);
        drop(permit);
        assert!(gate.try_acquire().is_ok());
    }

    #[tokio::test]
    async fn timed_out_blocking_worker_holds_permit_until_it_exits() {
        let gate: &'static CaptureGate = Box::leak(Box::new(CaptureGate::new()));
        let permit = gate.try_acquire().unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();

        let mut worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _ = started_tx.send(());
            release_rx.recv().unwrap();
        });
        started_rx.await.unwrap();

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut worker)
                .await
                .is_err()
        );
        let busy = match gate.try_acquire() {
            Ok(_) => panic!("timed-out worker unexpectedly released capture permit"),
            Err(error) => error,
        };
        assert_eq!(busy.code, SelectionErrorCode::CaptureBusy);

        release_tx.send(()).unwrap();
        worker.await.unwrap();
        assert!(gate.try_acquire().is_ok());
    }

    struct FakeRestorer {
        calls: usize,
        result: Result<bool, SelectionError>,
    }

    impl ClipboardRestorer for FakeRestorer {
        fn restore_known(&mut self, _expected_sequence: u32) -> Result<bool, SelectionError> {
            self.calls += 1;
            self.result.clone()
        }
    }

    #[test]
    fn fallback_cleanup_restores_only_a_known_matching_copy() {
        for state in [
            FallbackCleanupState::PartialSend,
            FallbackCleanupState::ClipboardTimeout,
            FallbackCleanupState::FocusMismatch,
            FallbackCleanupState::KnownCopy {
                sequence: 7,
                target_matches: false,
            },
        ] {
            let mut restorer = FakeRestorer {
                calls: 0,
                result: Ok(true),
            };
            assert_eq!(apply_fallback_cleanup(state, &mut restorer).unwrap(), None);
            assert_eq!(restorer.calls, 0);
        }

        let mut restorer = FakeRestorer {
            calls: 0,
            result: Ok(true),
        };
        assert_eq!(
            apply_fallback_cleanup(
                FallbackCleanupState::KnownCopy {
                    sequence: 7,
                    target_matches: true,
                },
                &mut restorer,
            )
            .unwrap(),
            Some(true)
        );
        assert_eq!(restorer.calls, 1);
    }

    #[test]
    fn uia_utf16_limit_handles_non_bmp_scalars_without_truncation() {
        let exact = "😀".repeat(MAX_SELECTED_TEXT_CHARS);
        let exact_units = exact.encode_utf16().collect::<Vec<_>>();
        assert_eq!(exact_units.len(), MAX_SELECTED_TEXT_CHARS * 2);
        assert_eq!(decode_uia_text_units(&exact_units).unwrap(), exact);

        let oversized = "😀".repeat(MAX_SELECTED_TEXT_CHARS + 1);
        let oversized_units = oversized.encode_utf16().collect::<Vec<_>>();
        assert_eq!(
            decode_uia_text_units(&oversized_units).unwrap_err().code,
            SelectionErrorCode::SelectionTooLarge
        );
    }

    #[test]
    fn clipboard_decode_scans_a_bounded_prefix_and_requires_nul() {
        assert_eq!(
            bounded_clipboard_unit_count(usize::MAX),
            CLIPBOARD_SCAN_UTF16_UNITS
        );
        let no_nul = vec![b'a' as u16; CLIPBOARD_SCAN_UTF16_UNITS];
        assert_eq!(
            decode_clipboard_units(&no_nul).unwrap_err().code,
            SelectionErrorCode::SelectionTooLarge
        );
        let valid = "😀ok\0".encode_utf16().collect::<Vec<_>>();
        assert_eq!(decode_clipboard_units(&valid).unwrap(), "😀ok");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn non_hglobal_clipboard_formats_are_rejected_before_duplication() {
        for format in [2, 3, 9, 14, 0x80, 0x81, 0x8f, 0x200, 0x2ff, 0x300, 0x3ff] {
            assert!(!clipboard_format_uses_hglobal(format));
        }
        for format in [1, 7, 8, 13, 15, 16, 17, 0xc000] {
            assert!(clipboard_format_uses_hglobal(format));
        }
    }
}
