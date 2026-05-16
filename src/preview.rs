use crate::{
    constants::{STREAM_FPS, STREAM_HEIGHT, STREAM_WIDTH},
    frames::{EncodedFrameHub, RawFrame, copy_bgra_into_frame},
    stats::SharedStats,
};
use crossbeam_channel::Receiver;
use ffmpeg::{
    Packet, Rational, codec, encoder, frame,
    software::scaling::{context::Context as ScaleContext, flag::Flags as ScaleFlags},
    util::format::Pixel,
};
use ffmpeg_next as ffmpeg;
use std::thread;

pub(crate) fn start_preview_encoder(
    receiver: Receiver<RawFrame>,
    frame_hub: EncodedFrameHub,
    stats: SharedStats,
) {
    thread::spawn(move || {
        let mut encoder = match TwitchLikeJpegEncoder::new(STREAM_WIDTH, STREAM_HEIGHT, STREAM_FPS)
        {
            Ok(encoder) => encoder,
            Err(err) => {
                eprintln!("FFmpeg native initialization failed: {err}");
                return;
            }
        };

        for raw_frame in receiver {
            stats.with_mut(|stats| stats.frames_read += 1);
            match encoder.encode(&raw_frame) {
                Ok(frames) => {
                    for jpeg in frames {
                        let bytes = jpeg.len();
                        frame_hub.publish(jpeg);
                        stats.with_mut(|stats| {
                            stats.frames_encoded += 1;
                            stats.latest_frame_bytes = bytes;
                        });
                    }
                }
                Err(err) => eprintln!("FFmpeg JPEG encode failed: {err}"),
            }
        }
    });
}

struct TwitchLikeJpegEncoder {
    encoder: encoder::Video,
    scaler: ScaleContext,
    frame_index: i64,
    width: u32,
    height: u32,
}

impl TwitchLikeJpegEncoder {
    fn new(width: u32, height: u32, fps: u32) -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;

        let codec = encoder::find(codec::Id::MJPEG).ok_or(ffmpeg::Error::EncoderNotFound)?;
        let mut encoder = codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;

        encoder.set_width(width);
        encoder.set_height(height);
        encoder.set_format(Pixel::YUVJ420P);
        encoder.set_time_base(Rational(1, fps as i32));
        encoder.set_frame_rate(Some(Rational(fps as i32, 1)));
        encoder.set_bit_rate(1_500_000);

        let encoder = encoder.open_as(codec)?;
        let scaler = ScaleContext::get(
            Pixel::BGRA,
            width,
            height,
            Pixel::YUVJ420P,
            width,
            height,
            ScaleFlags::BILINEAR,
        )?;

        Ok(Self {
            encoder,
            scaler,
            frame_index: 0,
            width,
            height,
        })
    }

    fn encode(&mut self, raw: &RawFrame) -> Result<Vec<Vec<u8>>, ffmpeg::Error> {
        if raw.width != self.width || raw.height != self.height {
            return Err(ffmpeg::Error::InvalidData);
        }

        let mut input = frame::Video::new(Pixel::BGRA, raw.width, raw.height);
        copy_bgra_into_frame(&raw.bgra, &mut input, raw.width, raw.height);

        let mut converted = frame::Video::new(Pixel::YUVJ420P, raw.width, raw.height);
        self.scaler.run(&input, &mut converted)?;
        converted.set_pts(Some(self.frame_index));
        self.frame_index += 1;

        self.encoder.send_frame(&converted)?;
        self.receive_packets()
    }

    fn receive_packets(&mut self) -> Result<Vec<Vec<u8>>, ffmpeg::Error> {
        let mut packets = Vec::new();
        let mut packet = Packet::empty();

        while self.encoder.receive_packet(&mut packet).is_ok() {
            if let Some(data) = packet.data() {
                packets.push(data.to_vec());
            }
            packet = Packet::empty();
        }

        Ok(packets)
    }
}
