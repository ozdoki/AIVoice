use std::{ptr::null_mut, slice};
use anyhow::bail;
use tokio::sync::watch;
use windows::{
    core::GUID,
    Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        Media::Audio::{
            eCapture, eConsole, IAudioCaptureClient, IAudioClient3, IMMDeviceEnumerator,
            MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            WAVEFORMATEXTENSIBLE,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
                COINIT_MULTITHREADED,
            },
            Threading::{CreateEventW, WaitForSingleObject},
        },
    },
};

use super::types::{AudioInput, CapturedAudio};

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

// {00000001-0000-0010-8000-00AA00389B71}
const SUBTYPE_PCM: GUID = GUID::from_values(
    0x0000_0001,
    0x0000,
    0x0010,
    [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
);

// {00000003-0000-0010-8000-00AA00389B71}
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_values(
    0x0000_0003,
    0x0000,
    0x0010,
    [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
);

pub struct WasapiInput;

impl AudioInput for WasapiInput {
    fn capture_blocking(&self, stop_rx: watch::Receiver<bool>) -> anyhow::Result<CapturedAudio> {
        unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
        let result = capture_inner(stop_rx);
        unsafe { CoUninitialize(); }
        result
    }
}

struct CaptureFormat {
    sample_rate: u32,
    channels: u16,
    is_float: bool,
}

fn capture_inner(stop_rx: watch::Receiver<bool>) -> anyhow::Result<CapturedAudio> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        let client: IAudioClient3 = device.Activate(CLSCTX_ALL, None)?;

        let mix = client.GetMixFormat()?;
        let fmt = read_format(&*mix)?;

        let mut default_period = 0u32;
        let mut fundamental = 0u32;
        let mut min_period = 0u32;
        let mut max_period = 0u32;
        client.GetSharedModeEnginePeriod(
            mix,
            &mut default_period,
            &mut fundamental,
            &mut min_period,
            &mut max_period,
        )?;
        client.InitializeSharedAudioStream(
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            default_period,
            mix,
            None,
        )?;
        CoTaskMemFree(Some(mix.cast()));

        let event = CreateEventW(None, false, false, None)?;
        client.SetEventHandle(event)?;
        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        let mut samples: Vec<f32> = Vec::new();

        loop {
            if *stop_rx.borrow() {
                break;
            }
            let wait_result = WaitForSingleObject(event, 50);
            if wait_result != WAIT_OBJECT_0 {
                continue;
            }
            loop {
                let packet_frames = capture.GetNextPacketSize()?;
                if packet_frames == 0 {
                    break;
                }
                let mut data: *mut u8 = null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let sample_count = frames as usize * fmt.channels as usize;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    samples.resize(samples.len() + sample_count, 0.0f32);
                } else if fmt.is_float {
                    let src = slice::from_raw_parts(data as *const f32, sample_count);
                    samples.extend_from_slice(src);
                } else {
                    let src = slice::from_raw_parts(data as *const i16, sample_count);
                    samples.extend(src.iter().map(|&v| v as f32 / 32768.0));
                }
                capture.ReleaseBuffer(frames)?;
            }
        }

        client.Stop().ok();
        CloseHandle(event).ok();

        Ok(CapturedAudio {
            samples,
            sample_rate: fmt.sample_rate,
            channels: fmt.channels,
        })
    }
}

unsafe fn read_format(
    wf: &windows::Win32::Media::Audio::WAVEFORMATEX,
) -> anyhow::Result<CaptureFormat> {
    let is_float = match wf.wFormatTag {
        WAVE_FORMAT_IEEE_FLOAT => true,
        WAVE_FORMAT_PCM => false,
        WAVE_FORMAT_EXTENSIBLE => {
            let ext_ptr = wf as *const _ as *const WAVEFORMATEXTENSIBLE;
            // WAVEFORMATEXTENSIBLE は packed struct なので read_unaligned を使う
            let sub_format = std::ptr::read_unaligned(std::ptr::addr_of!((*ext_ptr).SubFormat));
            if sub_format == SUBTYPE_IEEE_FLOAT {
                true
            } else if sub_format == SUBTYPE_PCM {
                false
            } else {
                bail!("unsupported WASAPI sub format: {:?}", sub_format);
            }
        }
        other => bail!("unsupported WASAPI format tag: {other}"),
    };
    Ok(CaptureFormat {
        sample_rate: wf.nSamplesPerSec,
        channels: wf.nChannels,
        is_float,
    })
}
