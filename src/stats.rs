use bevy::prelude::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Component)]
pub(crate) struct StatsText;

#[derive(Default)]
pub(crate) struct StreamStats {
    pub(crate) frames_captured: u64,
    pub(crate) frames_read: u64,
    pub(crate) frames_encoded: u64,
    pub(crate) frames_dropped: u64,
    pub(crate) preview_frames_dropped: u64,
    pub(crate) twitch_frames_dropped: u64,
    pub(crate) twitch_frames_sent: u64,
    pub(crate) twitch_video_packets: u64,
    pub(crate) twitch_audio_packets: u64,
    pub(crate) twitch_bytes_sent: u64,
    pub(crate) twitch_started_at: Option<Instant>,
    pub(crate) twitch_byte_samples: VecDeque<(Instant, u64)>,
    pub(crate) twitch_kbps: f64,
    pub(crate) twitch_errors: u64,
    pub(crate) twitch_stage: &'static str,
    pub(crate) twitch_last_error: String,
    pub(crate) stream_clients: u32,
    pub(crate) preview_requests: u64,
    pub(crate) latest_frame_bytes: usize,
}

#[derive(Clone, Resource)]
pub(crate) struct SharedStats(pub(crate) Arc<Mutex<StreamStats>>);

impl SharedStats {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(StreamStats::default())))
    }

    pub(crate) fn with_mut(&self, update: impl FnOnce(&mut StreamStats)) {
        if let Ok(mut stats) = self.0.lock() {
            update(&mut stats);
        }
    }

    pub(crate) fn set_twitch_stage(&self, stage: &'static str) {
        self.with_mut(|stats| stats.twitch_stage = stage);
    }

    pub(crate) fn set_twitch_error(&self, err: impl Into<String>) {
        self.with_mut(|stats| stats.twitch_last_error = err.into());
    }
}

impl StreamStats {
    pub(crate) fn reset_twitch_session(&mut self) {
        self.frames_captured = 0;
        self.frames_read = 0;
        self.frames_dropped = 0;
        self.twitch_frames_dropped = 0;
        self.twitch_frames_sent = 0;
        self.twitch_video_packets = 0;
        self.twitch_audio_packets = 0;
        self.twitch_bytes_sent = 0;
        self.twitch_started_at = Some(Instant::now());
        self.twitch_byte_samples.clear();
        self.twitch_kbps = 0.0;
        self.twitch_errors = 0;
        self.twitch_last_error.clear();
    }

    pub(crate) fn record_twitch_packet_bytes(&mut self, bytes: u64) {
        let now = Instant::now();
        self.twitch_bytes_sent += bytes;
        self.twitch_started_at.get_or_insert(now);
        self.twitch_byte_samples.push_back((now, bytes));
        self.refresh_twitch_kbps(now);
    }

    pub(crate) fn refresh_twitch_kbps(&mut self, now: Instant) {
        let window = Duration::from_secs(10);
        while self
            .twitch_byte_samples
            .front()
            .is_some_and(|(sample_time, _)| now.duration_since(*sample_time) > window)
        {
            self.twitch_byte_samples.pop_front();
        }

        let Some((oldest, _)) = self.twitch_byte_samples.front().copied() else {
            self.twitch_kbps = 0.0;
            return;
        };

        let elapsed = now.duration_since(oldest).as_secs_f64();
        if elapsed <= 0.0 {
            self.twitch_kbps = 0.0;
            return;
        }

        let bytes: u64 = self
            .twitch_byte_samples
            .iter()
            .map(|(_, bytes)| *bytes)
            .sum();
        self.twitch_kbps = bytes as f64 * 8.0 / elapsed / 1000.0;
    }

    pub(crate) fn stop_twitch_session(&mut self) {
        self.twitch_kbps = 0.0;
        self.twitch_byte_samples.clear();
        self.twitch_started_at = None;
    }
}
