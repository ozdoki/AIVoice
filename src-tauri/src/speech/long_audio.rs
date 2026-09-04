use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::Context;

pub const OPENAI_SAFE_WAV_LIMIT: usize = 24_000_000;
const WAV_HEADER_SIZE: usize = 44;
const SILENCE_SCAN_FRAMES: usize = 48_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_bytes: usize,
    pub frame_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavPart {
    pub start_frame: usize,
    pub end_frame: usize,
}

pub fn read_pcm_wav_info(path: &Path) -> anyhow::Result<WavInfo> {
    let mut file = File::open(path).context("ASR audio file open failed")?;
    let mut header = [0u8; WAV_HEADER_SIZE];
    file.read_exact(&mut header)
        .context("ASR WAV header read failed")?;
    if &header[0..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
        || &header[12..16] != b"fmt "
        || &header[36..40] != b"data"
        || u16::from_le_bytes([header[20], header[21]]) != 1
        || u16::from_le_bytes([header[34], header[35]]) != 16
    {
        anyhow::bail!("unsupported WAV format for long transcription");
    }
    let channels = u16::from_le_bytes([header[22], header[23]]);
    let sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let frame_bytes = channels as usize * 2;
    if channels == 0
        || sample_rate == 0
        || frame_bytes == 0
        || u16::from_le_bytes([header[32], header[33]]) as usize != frame_bytes
        || u32::from_le_bytes([header[28], header[29], header[30], header[31]])
            != sample_rate.saturating_mul(frame_bytes as u32)
    {
        anyhow::bail!("invalid WAV format for long transcription");
    }
    let declared = u32::from_le_bytes([header[40], header[41], header[42], header[43]]) as usize;
    let available = file
        .metadata()?
        .len()
        .saturating_sub(WAV_HEADER_SIZE as u64) as usize;
    if declared != available {
        anyhow::bail!("incomplete PCM WAV data");
    }
    let data_bytes = declared;
    if data_bytes == 0 || data_bytes % frame_bytes != 0 {
        anyhow::bail!("invalid or empty PCM WAV data");
    }
    Ok(WavInfo {
        sample_rate,
        channels,
        frame_bytes,
        frame_count: data_bytes / frame_bytes,
    })
}

pub fn split_wav_parts(path: &Path, info: &WavInfo, limit: usize) -> anyhow::Result<Vec<WavPart>> {
    let max_data = limit
        .checked_sub(WAV_HEADER_SIZE)
        .filter(|bytes| *bytes >= info.frame_bytes)
        .context("WAV upload limit is too small")?;
    let max_frames = max_data / info.frame_bytes;
    if info.frame_count <= max_frames {
        return Ok(vec![WavPart {
            start_frame: 0,
            end_frame: info.frame_count,
        }]);
    }
    let mut file = File::open(path)?;
    let mut parts = Vec::new();
    let mut start = 0;
    while start < info.frame_count {
        let target = (start + max_frames).min(info.frame_count);
        let end = if target == info.frame_count {
            target
        } else {
            find_silence_boundary(&mut file, info, start, target)?
        };
        if end <= start {
            anyhow::bail!("invalid WAV split boundary");
        }
        parts.push(WavPart {
            start_frame: start,
            end_frame: end,
        });
        start = end;
    }
    Ok(parts)
}

fn find_silence_boundary(
    file: &mut File,
    info: &WavInfo,
    start: usize,
    target: usize,
) -> anyhow::Result<usize> {
    let scan_start = target.saturating_sub(SILENCE_SCAN_FRAMES).max(start + 1);
    let frames = target - scan_start;
    let mut bytes = vec![0u8; frames * info.frame_bytes];
    file.seek(SeekFrom::Start(
        (WAV_HEADER_SIZE + scan_start * info.frame_bytes) as u64,
    ))?;
    file.read_exact(&mut bytes)?;
    let quiet_run = (info.sample_rate as usize / 10).max(1).min(frames);
    let mut candidate = None;
    let mut consecutive_quiet = 0;
    for frame in 0..frames {
        let silent = (0..info.channels as usize).all(|channel| {
            let at = frame * info.frame_bytes + channel * 2;
            i16::from_le_bytes([bytes[at], bytes[at + 1]]).unsigned_abs() <= 400
        });
        if silent {
            consecutive_quiet += 1;
            if consecutive_quiet >= quiet_run {
                candidate = Some(scan_start + frame + 1);
            }
        } else {
            consecutive_quiet = 0;
        }
    }
    Ok(candidate.unwrap_or(target))
}

pub fn read_wav_part(path: &Path, info: &WavInfo, part: &WavPart) -> anyhow::Result<Vec<u8>> {
    let frames = part
        .end_frame
        .checked_sub(part.start_frame)
        .context("invalid WAV part")?;
    let data_size = frames
        .checked_mul(info.frame_bytes)
        .context("WAV part is too large")?;
    let mut output = wav_header(info.sample_rate, info.channels, data_size)?;
    output.resize(WAV_HEADER_SIZE + data_size, 0);
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(
        (WAV_HEADER_SIZE + part.start_frame * info.frame_bytes) as u64,
    ))?;
    file.read_exact(&mut output[WAV_HEADER_SIZE..])?;
    Ok(output)
}

fn wav_header(sample_rate: u32, channels: u16, data_size: usize) -> anyhow::Result<Vec<u8>> {
    let data_size = u32::try_from(data_size).context("WAV part exceeds RIFF limit")?;
    let mut bytes = Vec::with_capacity(WAV_HEADER_SIZE);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36u32.saturating_add(data_size)).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    Ok(bytes)
}

pub fn fingerprint_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

pub fn fingerprint_text(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::WavRecorder;

    fn fixture(samples: &[i16]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("long-audio-{}.wav", uuid::Uuid::new_v4()));
        let mut writer = WavRecorder::create(&path, 8_000, 1).unwrap();
        writer.write_i16_samples(samples).unwrap();
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn parts_are_capped_and_reconstruct_every_frame_without_silence() {
        let path = fixture(&(0..101).map(|n| n as i16 + 500).collect::<Vec<_>>());
        let info = read_pcm_wav_info(&path).unwrap();
        let parts = split_wav_parts(&path, &info, 64).unwrap();
        let mut rebuilt = Vec::new();
        for part in parts {
            let wav = read_wav_part(&path, &info, &part).unwrap();
            assert!(wav.len() <= 64);
            rebuilt.extend_from_slice(&wav[44..]);
        }
        let source = std::fs::read(&path).unwrap();
        assert_eq!(rebuilt, source[44..]);
    }

    #[test]
    fn silence_boundary_stays_in_range_and_cap_edge_is_single_part() {
        let edge_path = fixture(&[700; 10]);
        let edge_info = read_pcm_wav_info(&edge_path).unwrap();
        assert_eq!(
            split_wav_parts(&edge_path, &edge_info, 64).unwrap().len(),
            1
        );
        let path = fixture(&[700; 8_000]);
        let info = read_pcm_wav_info(&path).unwrap();
        let parts = split_wav_parts(&path, &info, 54).unwrap();
        assert!(parts[0].end_frame <= 5);
    }
}
