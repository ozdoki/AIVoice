use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::{local_data::OperationKind, state::Mode};

const RECOVERY_DIR: &str = "recovery";
const META_FILE: &str = "meta.json";
const AUDIO_FILE: &str = "audio.wav";
static META_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn meta_lock() -> std::sync::MutexGuard<'static, ()> {
    META_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    Recording,
    Captured,
    Transcribing,
    TextReady,
    Completed,
    Failed,
    Orphaned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySessionMeta {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub mode: Mode,
    #[serde(default)]
    pub polish_preset: String,
    /// 録音開始時の実行ファイル名。ウィンドウタイトルは保存しない。
    #[serde(default)]
    pub app_process: String,
    pub trigger: String,
    pub status: RecoveryStatus,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub frame_count: Option<usize>,
    pub duration_ms: u64,
    pub raw_text: String,
    pub final_text: String,
    pub error: Option<String>,
    pub history_id: Option<String>,
    #[serde(default)]
    pub operation_kind: OperationKind,
    /// 長尺ASRの再開用チェックポイント。旧セッションには存在しない。
    #[serde(default)]
    pub long_audio: Option<LongAudioCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongAudioCheckpoint {
    pub source_fingerprint: String,
    pub request_fingerprint: String,
    pub total_parts: usize,
    /// `Some("")` は無音区間の成功を表す。`None` は未完了。
    pub completed_parts: Vec<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoverySessionSummary {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub mode: Mode,
    pub status: RecoveryStatus,
    pub duration_ms: u64,
    pub raw_text: String,
    pub final_text: String,
    pub error: Option<String>,
    pub has_audio: bool,
    pub can_retry: bool,
    pub operation_kind: OperationKind,
    pub injection_warning: Option<String>,
    pub transcription_progress: Option<RecoveryProgress>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryProgress {
    pub current: usize,
    pub total: usize,
}

#[derive(Debug, Clone)]
pub struct RecoverySessionRuntime {
    pub id: String,
    pub audio_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_count: usize,
    pub duration_ms: u64,
}

pub struct WavRecorder {
    file: File,
    data_size: u32,
    sample_rate: u32,
    channels: u16,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn recovery_root(app: &AppHandle) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join(RECOVERY_DIR))
}

fn validate_recovery_id(id: &str) -> anyhow::Result<()> {
    let mut parts = id.split('-');
    let valid = parts.next() == Some("rec")
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && parts.next().is_none();
    if !valid
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || Path::new(id).is_absolute()
    {
        anyhow::bail!("invalid recovery session id");
    }
    Ok(())
}

fn session_dir(app: &AppHandle, id: &str) -> anyhow::Result<PathBuf> {
    validate_recovery_id(id)?;
    let root = recovery_root(app)?;
    let dir = root.join(id);
    if dir.parent() != Some(root.as_path()) {
        anyhow::bail!("recovery session path escaped its root");
    }
    Ok(dir)
}

pub fn audio_path(app: &AppHandle, id: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(app, id)?.join(AUDIO_FILE))
}

fn meta_path(app: &AppHandle, id: &str) -> anyhow::Result<PathBuf> {
    Ok(session_dir(app, id)?.join(META_FILE))
}

fn save_meta_path(path: &Path, meta: &RecoverySessionMeta) -> anyhow::Result<()> {
    if !path.parent().is_some_and(Path::exists) {
        anyhow::bail!("recovery session directory no longer exists");
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(meta)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn load_meta(app: &AppHandle, id: &str) -> anyhow::Result<RecoverySessionMeta> {
    let path = meta_path(app, id)?;
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

pub fn update_meta<F>(app: &AppHandle, id: &str, update: F) -> anyhow::Result<RecoverySessionMeta>
where
    F: FnOnce(&mut RecoverySessionMeta),
{
    let _guard = meta_lock();
    let mut meta = load_meta(app, id)?;
    update(&mut meta);
    meta.updated_at = now_secs();
    save_meta_path(&meta_path(app, id)?, &meta)?;
    Ok(meta)
}

pub fn create_session(
    app: &AppHandle,
    mode: Mode,
    trigger: String,
    polish_preset: String,
    app_process: String,
) -> anyhow::Result<RecoverySessionRuntime> {
    create_session_with_operation(
        app,
        mode,
        trigger,
        polish_preset,
        app_process,
        OperationKind::Dictation,
    )
}

pub fn create_session_with_operation(
    app: &AppHandle,
    mode: Mode,
    trigger: String,
    polish_preset: String,
    app_process: String,
    operation_kind: OperationKind,
) -> anyhow::Result<RecoverySessionRuntime> {
    let root = recovery_root(app)?;
    fs::create_dir_all(&root)?;
    let id = format!("rec-{}-{}", now_millis(), std::process::id());
    let dir = root.join(&id);
    fs::create_dir_all(&dir)?;
    let meta = RecoverySessionMeta {
        id: id.clone(),
        created_at: now_secs(),
        updated_at: now_secs(),
        mode,
        polish_preset,
        app_process,
        trigger,
        status: RecoveryStatus::Recording,
        sample_rate: None,
        channels: None,
        frame_count: None,
        duration_ms: 0,
        raw_text: String::new(),
        final_text: String::new(),
        error: None,
        history_id: None,
        operation_kind,
        long_audio: None,
    };
    save_meta_path(&dir.join(META_FILE), &meta)?;
    Ok(RecoverySessionRuntime {
        id,
        audio_path: dir.join(AUDIO_FILE),
    })
}

pub fn load_long_audio_checkpoint(
    app: &AppHandle,
    id: &str,
    source_fingerprint: &str,
    request_fingerprint: &str,
    total_parts: usize,
) -> anyhow::Result<Vec<Option<String>>> {
    let meta = load_meta(app, id)?;
    Ok(reusable_long_audio_parts(
        meta.long_audio.as_ref(),
        source_fingerprint,
        request_fingerprint,
        total_parts,
    ))
}

fn reusable_long_audio_parts(
    checkpoint: Option<&LongAudioCheckpoint>,
    source_fingerprint: &str,
    request_fingerprint: &str,
    total_parts: usize,
) -> Vec<Option<String>> {
    let Some(checkpoint) = checkpoint else {
        return vec![None; total_parts];
    };
    if checkpoint.source_fingerprint != source_fingerprint
        || checkpoint.request_fingerprint != request_fingerprint
        || checkpoint.total_parts != total_parts
        || checkpoint.completed_parts.len() != total_parts
    {
        return vec![None; total_parts];
    }
    checkpoint.completed_parts.clone()
}

pub fn save_long_audio_part(
    app: &AppHandle,
    id: &str,
    source_fingerprint: String,
    request_fingerprint: String,
    total_parts: usize,
    index: usize,
    text: String,
) -> anyhow::Result<()> {
    if index >= total_parts {
        anyhow::bail!("long audio part index is out of range");
    }
    update_meta(app, id, |meta| {
        let checkpoint = meta.long_audio.get_or_insert_with(|| LongAudioCheckpoint {
            source_fingerprint: source_fingerprint.clone(),
            request_fingerprint: request_fingerprint.clone(),
            total_parts,
            completed_parts: vec![None; total_parts],
        });
        if checkpoint.source_fingerprint != source_fingerprint
            || checkpoint.request_fingerprint != request_fingerprint
            || checkpoint.total_parts != total_parts
            || checkpoint.completed_parts.len() != total_parts
        {
            *checkpoint = LongAudioCheckpoint {
                source_fingerprint,
                request_fingerprint,
                total_parts,
                completed_parts: vec![None; total_parts],
            };
        }
        checkpoint.completed_parts[index] = Some(text);
    })?;
    Ok(())
}

pub fn mark_captured(
    app: &AppHandle,
    id: &str,
    sample_rate: u32,
    channels: u16,
    frame_count: usize,
    duration_ms: u64,
) -> anyhow::Result<RecoverySessionMeta> {
    update_meta(app, id, |meta| {
        meta.status = RecoveryStatus::Captured;
        meta.sample_rate = Some(sample_rate);
        meta.channels = Some(channels);
        meta.frame_count = Some(frame_count);
        meta.duration_ms = duration_ms;
        meta.error = None;
    })
}

pub fn mark_transcribing(app: &AppHandle, id: &str) -> anyhow::Result<RecoverySessionMeta> {
    update_meta(app, id, |meta| {
        meta.status = RecoveryStatus::Transcribing;
        meta.error = None;
    })
}

pub fn mark_text_ready(
    app: &AppHandle,
    id: &str,
    raw_text: String,
    final_text: String,
) -> anyhow::Result<RecoverySessionMeta> {
    let meta = update_meta(app, id, |meta| {
        meta.status = RecoveryStatus::TextReady;
        meta.raw_text = raw_text;
        meta.final_text = final_text;
        meta.error = None;
    })?;
    Ok(meta)
}

pub fn claim_retry(
    app: &AppHandle,
    id: &str,
    info: WavInfo,
) -> anyhow::Result<RecoverySessionMeta> {
    let _guard = meta_lock();
    let mut meta = load_meta(app, id)?;
    if matches!(meta.status, RecoveryStatus::Transcribing) {
        anyhow::bail!("recovery retry is already running");
    }
    meta.status = RecoveryStatus::Transcribing;
    meta.sample_rate = Some(info.sample_rate);
    meta.channels = Some(info.channels);
    meta.frame_count = Some(info.frame_count);
    meta.duration_ms = info.duration_ms;
    meta.error = None;
    meta.updated_at = now_secs();
    save_meta_path(&meta_path(app, id)?, &meta)?;
    Ok(meta)
}

/// ASR全文は後処理より先に保全する。未完了の最終テキストとは区別する。
pub fn save_raw_text(
    app: &AppHandle,
    id: &str,
    raw_text: String,
) -> anyhow::Result<RecoverySessionMeta> {
    update_meta(app, id, |meta| apply_raw_text(meta, raw_text))
}

fn apply_raw_text(meta: &mut RecoverySessionMeta, raw_text: String) {
    meta.raw_text = raw_text;
    // 後処理前に落ちても、旧世代の完成文と新しいraw本文を組み合わせない。
    meta.final_text.clear();
    meta.error = None;
}

pub fn mark_failed(
    app: &AppHandle,
    id: &str,
    error: String,
) -> anyhow::Result<RecoverySessionMeta> {
    update_meta(app, id, |meta| {
        meta.status = RecoveryStatus::Failed;
        meta.error = Some(error);
    })
}

pub fn mark_completed(
    app: &AppHandle,
    id: &str,
    history_id: Option<String>,
) -> anyhow::Result<RecoverySessionMeta> {
    update_meta(app, id, |meta| {
        meta.status = RecoveryStatus::Completed;
        meta.history_id = history_id;
        meta.error = None;
    })
}

pub fn delete_session(app: &AppHandle, id: &str) -> anyhow::Result<()> {
    let _guard = meta_lock();
    let dir = session_dir(app, id)?;
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

pub fn cleanup_completed_session(app: &AppHandle, id: &str) -> anyhow::Result<()> {
    let mark_error = mark_completed(app, id, None).err();
    match delete_session(app, id) {
        Ok(()) => Ok(()),
        Err(delete_error) => match mark_error {
            Some(mark_error) => anyhow::bail!(
                "recovery metadata could not be hidden ({mark_error}) and its directory could not be deleted ({delete_error})"
            ),
            None => Err(delete_error),
        },
    }
}

pub fn summarize(app: &AppHandle, id: &str) -> anyhow::Result<RecoverySessionSummary> {
    let meta = load_meta(app, id)?;
    let audio = audio_path(app, id)?;
    Ok(summary_from_meta(&meta, audio.exists()))
}

pub fn list_sessions(
    app: &AppHandle,
    active_id: Option<&str>,
) -> anyhow::Result<Vec<RecoverySessionSummary>> {
    let _guard = meta_lock();
    let root = recovery_root(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if validate_recovery_id(&id).is_err() {
            continue;
        }
        let meta_file = entry.path().join(META_FILE);
        if !meta_file.exists() {
            continue;
        }

        let mut meta: RecoverySessionMeta = serde_json::from_str(&fs::read_to_string(&meta_file)?)?;
        if meta.id != id {
            continue;
        }
        if active_id == Some(id.as_str()) {
            summaries.push(summary_from_meta(
                &meta,
                entry.path().join(AUDIO_FILE).exists(),
            ));
            continue;
        }
        let wav = entry.path().join(AUDIO_FILE);
        if wav.exists() {
            if let Ok(info) = repair_wav_header(&wav) {
                meta.sample_rate.get_or_insert(info.sample_rate);
                meta.channels.get_or_insert(info.channels);
                meta.frame_count.get_or_insert(info.frame_count);
                if meta.duration_ms == 0 {
                    meta.duration_ms = info.duration_ms;
                }
            }
        }
        if matches!(
            meta.status,
            RecoveryStatus::Recording | RecoveryStatus::Transcribing
        ) {
            meta.status = RecoveryStatus::Orphaned;
            meta.error
                .get_or_insert_with(|| "前回の録音または処理が途中で終了しました。".to_string());
        }
        meta.updated_at = now_secs();
        save_meta_path(&meta_file, &meta)?;

        if !matches!(meta.status, RecoveryStatus::Completed) {
            summaries.push(summary_from_meta(&meta, wav.exists()));
        }
    }
    summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(summaries)
}

fn summary_from_meta(meta: &RecoverySessionMeta, has_audio: bool) -> RecoverySessionSummary {
    RecoverySessionSummary {
        id: meta.id.clone(),
        created_at: meta.created_at,
        updated_at: meta.updated_at,
        mode: meta.mode.clone(),
        status: meta.status.clone(),
        duration_ms: meta.duration_ms,
        raw_text: meta.raw_text.clone(),
        final_text: meta.final_text.clone(),
        error: meta.error.clone(),
        has_audio,
        can_retry: meta.operation_kind == OperationKind::Dictation
            && has_audio
            && meta.sample_rate.is_some()
            && meta.channels.is_some(),
        operation_kind: meta.operation_kind,
        injection_warning: None,
        transcription_progress: meta.long_audio.as_ref().map(|checkpoint| RecoveryProgress {
            current: checkpoint
                .completed_parts
                .iter()
                .filter(|text| text.is_some())
                .count(),
            total: checkpoint.total_parts,
        }),
    }
}

fn write_wav_header(
    file: &mut File,
    sample_rate: u32,
    channels: u16,
    data_size: u32,
) -> anyhow::Result<()> {
    let file_size = 36u32.saturating_add(data_size);
    file.seek(SeekFrom::Start(0))?;
    file.write_all(b"RIFF")?;
    file.write_all(&file_size.to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate * channels as u32 * 2).to_le_bytes())?;
    file.write_all(&(channels * 2).to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;
    Ok(())
}

impl WavRecorder {
    pub fn create(path: &Path, sample_rate: u32, channels: u16) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)?;
        write_wav_header(&mut file, sample_rate, channels, 0)?;
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            file,
            data_size: 0,
            sample_rate,
            channels,
        })
    }

    pub fn write_f32_samples(&mut self, samples: &[f32]) -> anyhow::Result<()> {
        for sample in samples {
            let value = (*sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            self.file.write_all(&value.to_le_bytes())?;
            self.data_size = self.data_size.saturating_add(2);
        }
        Ok(())
    }

    pub fn write_i16_samples(&mut self, samples: &[i16]) -> anyhow::Result<()> {
        for sample in samples {
            self.file.write_all(&sample.to_le_bytes())?;
            self.data_size = self.data_size.saturating_add(2);
        }
        Ok(())
    }

    pub fn write_silence(&mut self, sample_count: usize) -> anyhow::Result<()> {
        let bytes = vec![0u8; sample_count.saturating_mul(2)];
        self.file.write_all(&bytes)?;
        self.data_size = self.data_size.saturating_add(bytes.len() as u32);
        Ok(())
    }

    pub fn finalize(mut self) -> anyhow::Result<WavInfo> {
        write_wav_header(
            &mut self.file,
            self.sample_rate,
            self.channels,
            self.data_size,
        )?;
        self.file.flush()?;
        let frame_count = self.data_size as usize / (self.channels.max(1) as usize * 2);
        Ok(WavInfo {
            sample_rate: self.sample_rate,
            channels: self.channels,
            frame_count,
            duration_ms: duration_ms(frame_count, self.sample_rate),
        })
    }
}

fn duration_ms(frame_count: usize, sample_rate: u32) -> u64 {
    ((frame_count as u128 * 1000) / sample_rate.max(1) as u128) as u64
}

pub fn read_wav_info(path: &Path) -> anyhow::Result<WavInfo> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 44];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" || &header[36..40] != b"data" {
        anyhow::bail!("unsupported wav file");
    }
    let channels = u16::from_le_bytes([header[22], header[23]]);
    let sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let declared_data_size = u32::from_le_bytes([header[40], header[41], header[42], header[43]]);
    let actual_data_size = file.metadata()?.len().saturating_sub(44) as u32;
    let data_size = if declared_data_size == 0 {
        actual_data_size
    } else {
        declared_data_size.min(actual_data_size)
    };
    let frame_count = data_size as usize / (channels.max(1) as usize * 2);
    Ok(WavInfo {
        sample_rate,
        channels,
        frame_count,
        duration_ms: duration_ms(frame_count, sample_rate),
    })
}

pub fn repair_wav_header(path: &Path) -> anyhow::Result<WavInfo> {
    let info = read_wav_info(path)?;
    let actual_data_size = fs::metadata(path)?.len().saturating_sub(44) as u32;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    write_wav_header(&mut file, info.sample_rate, info.channels, actual_data_size)?;
    file.flush()?;
    read_wav_info(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_id_validator_rejects_every_path_shape() {
        assert!(validate_recovery_id("rec-123-456").is_ok());
        for id in [
            "../outside",
            "rec-1/../outside",
            "rec-1\\outside",
            "C:\\outside",
            "/tmp/outside",
            "rec-x-1",
            "rec-1-2-extra",
            "",
        ] {
            assert!(validate_recovery_id(id).is_err(), "accepted {id:?}");
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("recovery-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn wav_recorder_writes_header_and_duration() {
        let dir = test_dir("wav");
        let path = dir.join("audio.wav");
        let mut writer = WavRecorder::create(&path, 16_000, 1).unwrap();
        writer.write_i16_samples(&[0, 100, -100, 200]).unwrap();
        let info = writer.finalize().unwrap();
        assert_eq!(info.sample_rate, 16_000);
        assert_eq!(info.channels, 1);
        assert_eq!(info.frame_count, 4);
        assert_eq!(read_wav_info(&path).unwrap().frame_count, 4);
    }

    #[test]
    fn repair_wav_header_uses_actual_data_size() {
        let dir = test_dir("repair");
        let path = dir.join("audio.wav");
        {
            let mut file = File::create(&path).unwrap();
            write_wav_header(&mut file, 8_000, 1, 0).unwrap();
            file.seek(SeekFrom::End(0)).unwrap();
            file.write_all(&0i16.to_le_bytes()).unwrap();
            file.write_all(&1i16.to_le_bytes()).unwrap();
        }
        let before = read_wav_info(&path).unwrap();
        assert_eq!(before.frame_count, 2);
        let after = repair_wav_header(&path).unwrap();
        assert_eq!(after.frame_count, 2);
    }

    #[test]
    fn recovery_meta_legacy_defaults_and_current_context_roundtrip() {
        let legacy = serde_json::json!({
            "id": "rec-legacy",
            "created_at": 1,
            "updated_at": 1,
            "mode": "raw",
            "trigger": "manual",
            "status": "captured",
            "sample_rate": 16000,
            "channels": 1,
            "frame_count": 10,
            "duration_ms": 1,
            "raw_text": "",
            "final_text": "",
            "error": null,
            "history_id": null
        });
        let restored: RecoverySessionMeta = serde_json::from_value(legacy).unwrap();
        assert!(restored.polish_preset.is_empty());
        assert!(restored.app_process.is_empty());
        assert_eq!(restored.operation_kind, OperationKind::Dictation);

        let mut current = restored;
        current.polish_preset = "slack".to_string();
        current.app_process = "slack.exe".to_string();
        let roundtrip: RecoverySessionMeta =
            serde_json::from_value(serde_json::to_value(current).unwrap()).unwrap();
        assert_eq!(roundtrip.polish_preset, "slack");
        assert_eq!(roundtrip.app_process, "slack.exe");
    }

    #[test]
    fn long_audio_reuse_keeps_empty_success_but_rejects_changed_conditions() {
        let checkpoint = LongAudioCheckpoint {
            source_fingerprint: "source-a".into(),
            request_fingerprint: "request-a".into(),
            total_parts: 2,
            completed_parts: vec![Some(String::new()), None],
        };
        assert_eq!(
            reusable_long_audio_parts(Some(&checkpoint), "source-a", "request-a", 2),
            vec![Some(String::new()), None]
        );
        assert_eq!(
            reusable_long_audio_parts(Some(&checkpoint), "source-b", "request-a", 2),
            vec![None, None]
        );
        assert_eq!(
            reusable_long_audio_parts(Some(&checkpoint), "source-a", "request-b", 2),
            vec![None, None]
        );
    }

    #[test]
    fn saving_new_raw_text_invalidates_the_previous_final_text() {
        let mut meta: RecoverySessionMeta = serde_json::from_value(serde_json::json!({
            "id": "rec-1-2", "created_at": 1, "updated_at": 1, "mode": "raw",
            "trigger": "manual", "status": "text_ready", "sample_rate": 16000,
            "channels": 1, "frame_count": 1, "duration_ms": 1,
            "raw_text": "old raw", "final_text": "old final", "error": null,
            "history_id": null
        }))
        .unwrap();
        apply_raw_text(&mut meta, "new raw".into());
        assert_eq!(meta.raw_text, "new raw");
        assert!(meta.final_text.is_empty());
        assert_eq!(meta.status, RecoveryStatus::TextReady);
        meta.final_text = "new final".into();
        assert_eq!(
            (meta.raw_text, meta.final_text),
            ("new raw".into(), "new final".into())
        );
    }
}
