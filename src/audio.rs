use crate::constants::{
    STREAM_AUDIO_BUFFER_SECONDS, STREAM_AUDIO_CHANNELS, STREAM_AUDIO_MAX_MIX_FRAMES_PER_UPDATE,
    STREAM_AUDIO_SAMPLE_RATE,
};
use bevy::prelude::*;
use ffmpeg::{
    ChannelLayout, codec, format, frame, media,
    software::resampling::context::Context as ResampleContext,
    util::format::{Sample, sample::Type as SampleType},
};
use ffmpeg_next as ffmpeg;
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Asset, TypePath, Clone)]
pub struct StreamAudioClip {
    samples: Arc<Vec<f32>>,
    channels: usize,
    sample_rate: u32,
}

#[derive(Event, Message, Clone)]
pub struct PlayStreamSound {
    pub clip: Handle<StreamAudioClip>,
    pub volume: f32,
    pub repeat: bool,
}

impl PlayStreamSound {
    pub fn once(clip: Handle<StreamAudioClip>) -> Self {
        Self {
            clip,
            volume: 1.0,
            repeat: false,
        }
    }

    pub fn looping(clip: Handle<StreamAudioClip>) -> Self {
        Self {
            clip,
            volume: 1.0,
            repeat: true,
        }
    }

    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }
}

#[derive(Resource, Default)]
pub(crate) struct StreamAudioMixer {
    voices: Vec<StreamVoice>,
    pending_frames: f64,
}

struct StreamVoice {
    clip: Handle<StreamAudioClip>,
    cursor_frames: usize,
    volume: f32,
    repeat: bool,
}

pub(crate) fn collect_stream_audio_events(
    mut events: MessageReader<PlayStreamSound>,
    mut mixer: ResMut<StreamAudioMixer>,
) {
    for event in events.read() {
        mixer.voices.push(StreamVoice {
            clip: event.clip.clone(),
            cursor_frames: 0,
            volume: event.volume.max(0.0),
            repeat: event.repeat,
        });
    }
}

pub(crate) fn mix_stream_audio(
    time: Res<Time>,
    clips: Res<Assets<StreamAudioClip>>,
    mut mixer: ResMut<StreamAudioMixer>,
    audio_target: Res<DirectStreamAudioTarget>,
) {
    if mixer.voices.is_empty() {
        mixer.pending_frames = 0.0;
        return;
    }

    mixer.pending_frames += time.delta_secs_f64() * STREAM_AUDIO_SAMPLE_RATE as f64;
    let frames_to_mix = mixer
        .pending_frames
        .floor()
        .min(STREAM_AUDIO_MAX_MIX_FRAMES_PER_UPDATE as f64) as usize;
    mixer.pending_frames -= frames_to_mix as f64;
    if frames_to_mix == 0 {
        return;
    }

    let mut mixed = vec![0.0; frames_to_mix * STREAM_AUDIO_CHANNELS];
    let mut active_voices = Vec::with_capacity(mixer.voices.len());

    for mut voice in mixer.voices.drain(..) {
        let Some(clip) = clips.get(&voice.clip) else {
            active_voices.push(voice);
            continue;
        };

        let total_frames = clip.stream_frame_count();
        if total_frames == 0 {
            continue;
        }

        for frame in 0..frames_to_mix {
            if voice.cursor_frames >= total_frames {
                if voice.repeat {
                    voice.cursor_frames = 0;
                } else {
                    break;
                }
            }

            let [left, right] = clip.stereo_frame(voice.cursor_frames);
            mixed[frame * 2] += left * voice.volume;
            mixed[frame * 2 + 1] += right * voice.volume;
            voice.cursor_frames += 1;
        }

        if voice.repeat || voice.cursor_frames < total_frames {
            active_voices.push(voice);
        }
    }

    mixer.voices = active_voices;
    audio_target.push_stereo_f32(&mixed);
}

impl StreamAudioClip {
    pub fn from_interleaved_f32(samples: Vec<f32>, channels: usize, sample_rate: u32) -> Self {
        Self {
            samples: Arc::new(samples.into_iter().map(normalize_stream_sample).collect()),
            channels: channels.max(1),
            sample_rate,
        }
    }

    pub fn from_mono_f32(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self::from_interleaved_f32(samples, 1, sample_rate)
    }

    pub fn from_wav_file(path: impl AsRef<Path>) -> Result<Self, String> {
        if let Ok(clip) = decode_audio_file_with_ffmpeg(path.as_ref()) {
            return Ok(clip);
        }

        let bytes = fs::read(path.as_ref())
            .map_err(|err| format!("could not read {}: {err}", path.as_ref().display()))?;
        decode_wav_clip(&bytes)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels
    }

    fn stream_frame_count(&self) -> usize {
        self.frame_count() * STREAM_AUDIO_SAMPLE_RATE as usize / self.sample_rate.max(1) as usize
    }

    fn stereo_frame(&self, frame: usize) -> [f32; 2] {
        let source_frame = if self.sample_rate == STREAM_AUDIO_SAMPLE_RATE {
            frame
        } else {
            frame * self.sample_rate as usize / STREAM_AUDIO_SAMPLE_RATE as usize
        };
        let base = source_frame.saturating_mul(self.channels);
        if base >= self.samples.len() {
            return [0.0, 0.0];
        }

        match self.channels {
            1 => {
                let sample = self.samples[base];
                [sample, sample]
            }
            _ => [
                self.samples[base],
                self.samples.get(base + 1).copied().unwrap_or(0.0),
            ],
        }
    }
}

fn decode_audio_file_with_ffmpeg(path: &Path) -> Result<StreamAudioClip, String> {
    ffmpeg::init().map_err(|err| err.to_string())?;

    let mut input = format::input(path).map_err(|err| err.to_string())?;
    let stream = input
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| "file does not contain an audio stream".to_owned())?;
    let stream_index = stream.index();
    let context = codec::context::Context::from_parameters(stream.parameters())
        .map_err(|err| err.to_string())?;
    let mut decoder = context.decoder().audio().map_err(|err| err.to_string())?;
    decoder
        .set_parameters(stream.parameters())
        .map_err(|err| err.to_string())?;

    let source_layout = if decoder.channel_layout().is_empty() {
        ChannelLayout::default(decoder.channels() as i32)
    } else {
        decoder.channel_layout()
    };
    let sample_rate = decoder.rate();
    let mut resampler = ResampleContext::get(
        decoder.format(),
        source_layout,
        sample_rate,
        Sample::F32(SampleType::Planar),
        ChannelLayout::STEREO,
        sample_rate,
    )
    .map_err(|err| err.to_string())?;

    let mut samples = Vec::new();
    for (stream, packet) in input.packets() {
        if stream.index() == stream_index {
            decoder
                .send_packet(&packet)
                .map_err(|err| err.to_string())?;
            receive_resampled_audio(&mut decoder, &mut resampler, &mut samples)?;
        }
    }

    decoder.send_eof().map_err(|err| err.to_string())?;
    receive_resampled_audio(&mut decoder, &mut resampler, &mut samples)?;

    if samples.is_empty() {
        return Err("decoded audio stream had no samples".to_owned());
    }

    Ok(StreamAudioClip::from_interleaved_f32(
        samples,
        STREAM_AUDIO_CHANNELS,
        sample_rate,
    ))
}

fn receive_resampled_audio(
    decoder: &mut codec::decoder::Audio,
    resampler: &mut ResampleContext,
    samples: &mut Vec<f32>,
) -> Result<(), String> {
    let mut decoded = frame::Audio::empty();
    while decoder.receive_frame(&mut decoded).is_ok() {
        let mut converted = frame::Audio::empty();
        resampler
            .run(&decoded, &mut converted)
            .map_err(|err| err.to_string())?;
        append_planar_stereo_f32(&converted, samples)?;
    }

    Ok(())
}

fn append_planar_stereo_f32(frame: &frame::Audio, samples: &mut Vec<f32>) -> Result<(), String> {
    if frame.format() != Sample::F32(SampleType::Planar) || frame.planes() < 2 {
        return Err("resampler did not produce planar stereo f32 audio".to_owned());
    }

    let left = frame.plane::<f32>(0);
    let right = frame.plane::<f32>(1);
    for index in 0..frame.samples().min(left.len()).min(right.len()) {
        samples.push(left[index]);
        samples.push(right[index]);
    }

    Ok(())
}

fn decode_wav_clip(bytes: &[u8]) -> Result<StreamAudioClip, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_owned());
    }

    let mut cursor = 12;
    let mut format = None;
    let mut data = None;

    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_size = read_u32_le(bytes, cursor + 4)? as usize;
        let chunk_start = cursor + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| "WAV chunk size overflow".to_owned())?;
        if chunk_end > bytes.len() {
            return Err("WAV chunk extends past end of file".to_owned());
        }

        match chunk_id {
            b"fmt " => {
                if chunk_size < 16 {
                    return Err("WAV fmt chunk is too small".to_owned());
                }
                format = Some(WavFormat {
                    audio_format: read_u16_le(bytes, chunk_start)?,
                    channels: read_u16_le(bytes, chunk_start + 2)? as usize,
                    sample_rate: read_u32_le(bytes, chunk_start + 4)?,
                    bits_per_sample: read_u16_le(bytes, chunk_start + 14)?,
                });
            }
            b"data" => data = Some(&bytes[chunk_start..chunk_end]),
            _ => {}
        }

        cursor = chunk_end + (chunk_size % 2);
    }

    let format = format.ok_or_else(|| "WAV file is missing fmt chunk".to_owned())?;
    let data = data.ok_or_else(|| "WAV file is missing data chunk".to_owned())?;
    if format.channels == 0 {
        return Err("WAV file has zero channels".to_owned());
    }

    let samples = decode_wav_samples(data, format)?;
    Ok(StreamAudioClip::from_interleaved_f32(
        samples,
        format.channels,
        format.sample_rate,
    ))
}

#[derive(Clone, Copy)]
struct WavFormat {
    audio_format: u16,
    channels: usize,
    sample_rate: u32,
    bits_per_sample: u16,
}

fn decode_wav_samples(data: &[u8], format: WavFormat) -> Result<Vec<f32>, String> {
    let bytes_per_sample = (format.bits_per_sample as usize)
        .checked_div(8)
        .filter(|bytes| *bytes > 0)
        .ok_or_else(|| "WAV bits per sample must be byte-aligned".to_owned())?;
    if data.len() % bytes_per_sample != 0 {
        return Err("WAV data chunk is not sample-aligned".to_owned());
    }

    data.chunks_exact(bytes_per_sample)
        .map(|sample| decode_wav_sample(sample, format))
        .collect()
}

fn decode_wav_sample(sample: &[u8], format: WavFormat) -> Result<f32, String> {
    match (format.audio_format, format.bits_per_sample) {
        (1, 8) => Ok((sample[0] as f32 - 128.0) / 128.0),
        (1, 16) => Ok(i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0),
        (1, 24) => {
            let sign = if sample[2] & 0x80 == 0 { 0x00 } else { 0xff };
            let value = i32::from_le_bytes([sample[0], sample[1], sample[2], sign]);
            Ok(value as f32 / 8_388_608.0)
        }
        (1, 32) => Ok(
            i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f32
                / 2_147_483_648.0,
        ),
        (3, 32) => Ok(f32::from_le_bytes([
            sample[0], sample[1], sample[2], sample[3],
        ])),
        _ => Err(format!(
            "unsupported WAV format: format {}, {} bits",
            format.audio_format, format.bits_per_sample
        )),
    }
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let data = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "unexpected end of WAV data".to_owned())?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let data = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "unexpected end of WAV data".to_owned())?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

#[derive(Clone, Resource)]
pub struct DirectStreamAudioTarget {
    inner: Arc<Mutex<DirectStreamAudioBuffer>>,
}

struct DirectStreamAudioBuffer {
    interleaved_stereo_f32: VecDeque<f32>,
    max_samples: usize,
}

impl DirectStreamAudioTarget {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DirectStreamAudioBuffer {
                interleaved_stereo_f32: VecDeque::new(),
                max_samples: STREAM_AUDIO_SAMPLE_RATE as usize
                    * STREAM_AUDIO_CHANNELS
                    * STREAM_AUDIO_BUFFER_SECONDS,
            })),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        STREAM_AUDIO_SAMPLE_RATE
    }

    pub fn channels(&self) -> usize {
        STREAM_AUDIO_CHANNELS
    }

    pub fn push_stereo_f32(&self, interleaved_stereo: &[f32]) {
        if let Ok(mut buffer) = self.inner.lock() {
            for sample in interleaved_stereo {
                if buffer.interleaved_stereo_f32.len() >= buffer.max_samples {
                    buffer.interleaved_stereo_f32.pop_front();
                }
                buffer
                    .interleaved_stereo_f32
                    .push_back(normalize_stream_sample(*sample));
            }
        }
    }

    pub fn push_mono_f32(&self, mono: &[f32]) {
        if let Ok(mut buffer) = self.inner.lock() {
            for sample in mono {
                let sample = normalize_stream_sample(*sample);
                for channel_sample in [sample, sample] {
                    if buffer.interleaved_stereo_f32.len() >= buffer.max_samples {
                        buffer.interleaved_stereo_f32.pop_front();
                    }
                    buffer.interleaved_stereo_f32.push_back(channel_sample);
                }
            }
        }
    }

    pub(crate) fn take_stereo_f32(&self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * STREAM_AUDIO_CHANNELS];
        if let Ok(mut buffer) = self.inner.lock() {
            for sample in &mut output {
                if let Some(next) = buffer.interleaved_stereo_f32.pop_front() {
                    *sample = next;
                } else {
                    break;
                }
            }
        }
        output
    }
}

pub(crate) fn normalize_stream_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

impl Default for DirectStreamAudioTarget {
    fn default() -> Self {
        Self::new()
    }
}
