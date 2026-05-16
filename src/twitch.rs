use crate::{
    audio::DirectStreamAudioTarget,
    constants::{STREAM_FPS, STREAM_HEIGHT, STREAM_WIDTH, TWITCH_VIDEO_BITRATE},
    frames::{RawFrame, RawFrameHub, copy_bgra_into_frame},
    stats::SharedStats,
};
use ffmpeg::{
    ChannelLayout, Dictionary, Packet, Rational, codec, encoder, format, frame,
    software::scaling::{context::Context as ScaleContext, flag::Flags as ScaleFlags},
    util::format::{Pixel, Sample, sample::Type as SampleType},
};
use ffmpeg_next as ffmpeg;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) struct TwitchStreamHandle {
    stop_requested: Arc<AtomicBool>,
}

impl TwitchStreamHandle {
    pub(crate) fn stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn start_twitch_sink(
    frame_hub: RawFrameHub,
    twitch_url: String,
    stats: SharedStats,
    audio_target: DirectStreamAudioTarget,
) -> TwitchStreamHandle {
    let stop_requested = Arc::new(AtomicBool::new(false));
    let thread_stop_requested = stop_requested.clone();

    thread::spawn(move || {
        stats.set_twitch_stage("opening");
        let mut sink = match TwitchRtmpSink::new(
            &twitch_url,
            STREAM_WIDTH,
            STREAM_HEIGHT,
            STREAM_FPS,
            audio_target,
        ) {
            Ok(sink) => sink,
            Err(err) => {
                eprintln!("Could not start Twitch RTMP sink: {err}");
                stats.set_twitch_error(err.to_string());
                stats.with_mut(|stats| stats.twitch_errors += 1);
                return;
            }
        };
        stats.set_twitch_stage("waiting for first frame");

        let Some(mut latest_frame) = frame_hub.wait_for_first_frame() else {
            stats.set_twitch_stage("no frames");
            return;
        };
        let frame_interval = Duration::from_secs_f64(1.0 / STREAM_FPS as f64);
        let mut next_tick = Instant::now();

        while !thread_stop_requested.load(Ordering::Relaxed) {
            if let Some(frame) = frame_hub.latest_frame() {
                latest_frame = frame;
            }

            stats.with_mut(|stats| stats.frames_read += 1);
            if let Err(err) = sink.push_video_frame(&latest_frame, &stats) {
                eprintln!("Twitch RTMP write failed: {err}");
                stats.set_twitch_error(err.to_string());
                stats.with_mut(|stats| stats.twitch_errors += 1);
                break;
            }
            stats.with_mut(|stats| stats.twitch_frames_sent += 1);
            stats.set_twitch_stage("clock wait");

            next_tick += frame_interval;
            let now = Instant::now();
            if next_tick > now {
                thread::sleep(next_tick - now);
            } else {
                next_tick = now;
            }
        }

        stats.set_twitch_stage("finishing");
        let _ = sink.finish();
        stats.with_mut(|stats| {
            stats.stop_twitch_session();
            stats.twitch_stage = "finished";
        });
    });

    TwitchStreamHandle { stop_requested }
}

struct TwitchRtmpSink {
    output: format::context::Output,
    video_encoder: encoder::Video,
    video_scaler: ScaleContext,
    video_stream_index: usize,
    video_encoder_time_base: Rational,
    video_stream_time_base: Rational,
    video_frame_index: i64,
    audio_encoder: encoder::Audio,
    audio_stream_index: usize,
    audio_encoder_time_base: Rational,
    audio_stream_time_base: Rational,
    audio_pts: i64,
    audio_frame_size: usize,
    audio_format: Sample,
    audio_rate: u32,
    audio_layout: ChannelLayout,
    audio_source: TwitchAudioSource,
    width: u32,
    height: u32,
}

enum TwitchAudioSource {
    Target(DirectStreamAudioTarget),
}

impl TwitchRtmpSink {
    fn new(
        url: &str,
        width: u32,
        height: u32,
        fps: u32,
        audio_target: DirectStreamAudioTarget,
    ) -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;

        let mut output = format::output_as(url, "flv")?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);

        let video_codec = encoder::find_by_name("libopenh264")
            .or_else(|| encoder::find_by_name("h264_mf"))
            .ok_or(ffmpeg::Error::EncoderNotFound)?;
        let video_codec_name = video_codec.name();
        let video_pixel_format = if video_codec_name == "h264_mf" {
            Pixel::NV12
        } else {
            Pixel::YUV420P
        };
        eprintln!("Using H.264 encoder: {video_codec_name}");

        let mut video_encoder = codec::context::Context::new_with_codec(video_codec)
            .encoder()
            .video()?;
        video_encoder.set_width(width);
        video_encoder.set_height(height);
        video_encoder.set_format(video_pixel_format);
        video_encoder.set_time_base(Rational(1, fps as i32));
        video_encoder.set_frame_rate(Some(Rational(fps as i32, 1)));
        video_encoder.set_bit_rate(TWITCH_VIDEO_BITRATE);
        video_encoder.set_gop(fps * 2);
        video_encoder.set_max_b_frames(0);
        if global_header {
            video_encoder.set_flags(codec::Flags::GLOBAL_HEADER);
        }

        let mut video_options = Dictionary::new();
        if video_codec_name == "h264_mf" {
            video_options.set("rate_control", "cbr");
        } else {
            video_options.set("preset", "veryfast");
            video_options.set("tune", "zerolatency");
        }
        let video_encoder = video_encoder.open_as_with(video_codec, video_options)?;
        let video_stream_index;
        {
            let mut video_stream = output.add_stream(video_codec)?;
            video_stream.set_time_base(Rational(1, fps as i32));
            video_stream.set_parameters(&video_encoder);
            video_stream_index = video_stream.index();
        }

        let audio_codec = encoder::find(codec::Id::AAC).ok_or(ffmpeg::Error::EncoderNotFound)?;
        let audio_codec_info = audio_codec.audio()?;
        let audio_format = audio_codec_info
            .formats()
            .and_then(|mut formats| {
                formats.find(|format| *format == Sample::F32(SampleType::Planar))
            })
            .unwrap_or_else(|| {
                audio_codec_info
                    .formats()
                    .and_then(|mut formats| formats.next())
                    .unwrap_or(Sample::F32(SampleType::Planar))
            });
        let audio_rate = 48_000;
        let audio_layout = ChannelLayout::STEREO;
        let mut audio_encoder = codec::context::Context::new_with_codec(audio_codec)
            .encoder()
            .audio()?;
        audio_encoder.set_rate(audio_rate as i32);
        audio_encoder.set_channel_layout(audio_layout);
        audio_encoder.set_format(audio_format);
        audio_encoder.set_bit_rate(96_000);
        audio_encoder.set_time_base(Rational(1, audio_rate as i32));
        if global_header {
            audio_encoder.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let audio_encoder = audio_encoder.open_as(audio_codec)?;
        let audio_frame_size = audio_encoder.frame_size().max(1024) as usize;
        let audio_stream_index;
        {
            let mut audio_stream = output.add_stream(audio_codec)?;
            audio_stream.set_time_base(Rational(1, audio_rate as i32));
            audio_stream.set_parameters(&audio_encoder);
            audio_stream_index = audio_stream.index();
        }

        output.write_header()?;
        let video_stream_time_base = output
            .stream(video_stream_index)
            .map(|stream| stream.time_base())
            .unwrap_or(Rational(1, fps as i32));
        let audio_stream_time_base = output
            .stream(audio_stream_index)
            .map(|stream| stream.time_base())
            .unwrap_or(Rational(1, audio_rate as i32));
        let audio_source = TwitchAudioSource::Target(audio_target);

        Ok(Self {
            output,
            video_encoder,
            video_scaler: ScaleContext::get(
                Pixel::BGRA,
                width,
                height,
                video_pixel_format,
                width,
                height,
                ScaleFlags::FAST_BILINEAR,
            )?,
            video_stream_index,
            video_encoder_time_base: Rational(1, fps as i32),
            video_stream_time_base,
            video_frame_index: 0,
            audio_encoder,
            audio_stream_index,
            audio_encoder_time_base: Rational(1, audio_rate as i32),
            audio_stream_time_base,
            audio_pts: 0,
            audio_frame_size,
            audio_format,
            audio_rate,
            audio_layout,
            audio_source,
            width,
            height,
        })
    }

    fn push_video_frame(
        &mut self,
        raw: &Arc<RawFrame>,
        stats: &SharedStats,
    ) -> Result<(), ffmpeg::Error> {
        if raw.width != self.width || raw.height != self.height {
            return Err(ffmpeg::Error::InvalidData);
        }

        stats.set_twitch_stage("audio");
        let audio_target_pts =
            (self.video_frame_index + 1) * self.audio_rate as i64 / STREAM_FPS as i64;
        self.write_silent_audio_until(audio_target_pts, stats)?;

        stats.set_twitch_stage("copy video");
        let mut input = frame::Video::new(Pixel::BGRA, raw.width, raw.height);
        copy_bgra_into_frame(&raw.bgra, &mut input, raw.width, raw.height);

        stats.set_twitch_stage("scale video");
        let mut converted = frame::Video::new(self.video_encoder.format(), raw.width, raw.height);
        self.video_scaler.run(&input, &mut converted)?;
        converted.set_pts(Some(self.video_frame_index));
        self.video_frame_index += 1;

        stats.set_twitch_stage("send video frame");
        self.video_encoder.send_frame(&converted)?;
        stats.set_twitch_stage("receive video packets");
        self.receive_video_packets(stats)
    }

    fn write_silent_audio_until(
        &mut self,
        target_pts: i64,
        stats: &SharedStats,
    ) -> Result<(), ffmpeg::Error> {
        while self.audio_pts < target_pts {
            stats.set_twitch_stage("make audio frame");
            let mut audio =
                frame::Audio::new(self.audio_format, self.audio_frame_size, self.audio_layout);
            audio.set_rate(self.audio_rate);
            audio.set_channel_layout(self.audio_layout);
            audio.set_pts(Some(self.audio_pts));
            match &self.audio_source {
                TwitchAudioSource::Target(target) => {
                    fill_audio_frame_from_target(&mut audio, target, self.audio_frame_size);
                }
            }
            audio.set_pts(Some(self.audio_pts));
            self.audio_pts += self.audio_frame_size as i64;

            stats.set_twitch_stage("send audio frame");
            self.audio_encoder.send_frame(&audio)?;
            stats.set_twitch_stage("receive audio packets");
            self.receive_audio_packets(stats)?;
        }

        Ok(())
    }

    fn receive_video_packets(&mut self, stats: &SharedStats) -> Result<(), ffmpeg::Error> {
        let mut packet = Packet::empty();
        while self.video_encoder.receive_packet(&mut packet).is_ok() {
            let bytes = packet.size().max(0) as u64;
            packet.set_stream(self.video_stream_index);
            packet.rescale_ts(self.video_encoder_time_base, self.video_stream_time_base);
            packet.write_interleaved(&mut self.output)?;
            stats.with_mut(|stats| {
                stats.twitch_video_packets += 1;
                stats.record_twitch_packet_bytes(bytes);
            });
            packet = Packet::empty();
        }
        Ok(())
    }

    fn receive_audio_packets(&mut self, stats: &SharedStats) -> Result<(), ffmpeg::Error> {
        let mut packet = Packet::empty();
        while self.audio_encoder.receive_packet(&mut packet).is_ok() {
            let bytes = packet.size().max(0) as u64;
            packet.set_stream(self.audio_stream_index);
            packet.rescale_ts(self.audio_encoder_time_base, self.audio_stream_time_base);
            packet.write_interleaved(&mut self.output)?;
            stats.with_mut(|stats| {
                stats.twitch_audio_packets += 1;
                stats.record_twitch_packet_bytes(bytes);
            });
            packet = Packet::empty();
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), ffmpeg::Error> {
        self.video_encoder.send_eof()?;
        self.receive_video_packets(&SharedStats::new())?;
        self.audio_encoder.send_eof()?;
        self.receive_audio_packets(&SharedStats::new())?;
        self.output.write_trailer()
    }
}

fn clear_audio_samples(audio: &mut frame::Audio) {
    match audio.format() {
        Sample::F32(_) => {
            for plane in 0..audio.planes() {
                audio.plane_mut::<f32>(plane).fill(0.0);
            }
        }
        Sample::F64(_) => {
            for plane in 0..audio.planes() {
                audio.plane_mut::<f64>(plane).fill(0.0);
            }
        }
        Sample::I16(_) => {
            for plane in 0..audio.planes() {
                audio.plane_mut::<i16>(plane).fill(0);
            }
        }
        Sample::I32(_) => {
            for plane in 0..audio.planes() {
                audio.plane_mut::<i32>(plane).fill(0);
            }
        }
        Sample::I64(_) | Sample::U8(_) | Sample::None => {
            for plane in 0..audio.planes() {
                audio.data_mut(plane).fill(0);
            }
        }
    }
}

fn fill_audio_frame_from_target(
    audio: &mut frame::Audio,
    target: &DirectStreamAudioTarget,
    frames: usize,
) {
    clear_audio_samples(audio);
    let samples = target.take_stereo_f32(frames);

    match audio.format() {
        Sample::F32(SampleType::Planar) if audio.planes() >= 2 => {
            {
                let left = audio.plane_mut::<f32>(0);
                for index in 0..frames.min(left.len()) {
                    left[index] = samples[index * 2];
                }
            }
            {
                let right = audio.plane_mut::<f32>(1);
                for index in 0..frames.min(right.len()) {
                    right[index] = samples[index * 2 + 1];
                }
            }
        }
        Sample::F32(SampleType::Packed) if audio.planes() >= 1 => {
            let plane = audio.plane_mut::<f32>(0);
            for (index, sample) in samples.iter().take(plane.len()).enumerate() {
                plane[index] = *sample;
            }
        }
        _ => {}
    }

    sanitize_audio_samples(audio);
}

fn sanitize_audio_samples(audio: &mut frame::Audio) {
    match audio.format() {
        Sample::F32(_) => {
            for plane in 0..audio.planes() {
                for sample in audio.plane_mut::<f32>(plane) {
                    if !sample.is_finite() {
                        *sample = 0.0;
                    }
                }
            }
        }
        Sample::F64(_) => {
            for plane in 0..audio.planes() {
                for sample in audio.plane_mut::<f64>(plane) {
                    if !sample.is_finite() {
                        *sample = 0.0;
                    }
                }
            }
        }
        _ => {}
    }
}
