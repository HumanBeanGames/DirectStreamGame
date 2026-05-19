use crate::stats::SharedStats;
use bevy::prelude::*;
use crossbeam_channel::Sender;
use ffmpeg_next::frame;
use std::sync::{Arc, Condvar, Mutex};

type DirectStreamFrameProcessor = Box<dyn for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync>;

#[derive(Clone, Resource)]
pub(crate) struct EncodedFrameHub {
    inner: Arc<(Mutex<LatestEncodedFrame>, Condvar)>,
}

#[derive(Default)]
struct LatestEncodedFrame {
    sequence: u64,
    jpeg: Option<Arc<Vec<u8>>>,
}

impl EncodedFrameHub {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(LatestEncodedFrame::default()), Condvar::new())),
        }
    }

    pub(crate) fn publish(&self, jpeg: Vec<u8>) {
        let (lock, ready) = &*self.inner;
        if let Ok(mut latest) = lock.lock() {
            latest.sequence += 1;
            latest.jpeg = Some(Arc::new(jpeg));
            ready.notify_all();
        }
    }

    pub(crate) fn wait_for_frame_after(&self, last_sequence: u64) -> Option<(u64, Arc<Vec<u8>>)> {
        let (lock, ready) = &*self.inner;
        let mut latest = lock.lock().ok()?;

        while latest.sequence <= last_sequence || latest.jpeg.is_none() {
            latest = ready.wait(latest).ok()?;
        }

        Some((latest.sequence, latest.jpeg.as_ref()?.clone()))
    }
}

#[derive(Clone, Resource)]
pub(crate) struct RawFrameSenders {
    pub(crate) preview: Option<Sender<RawFrame>>,
    pub(crate) custom: Option<Sender<RawFrame>>,
    pub(crate) twitch: Option<RawFrameHub>,
    pub(crate) stats: SharedStats,
}

#[derive(Clone)]
pub(crate) struct RawFrame {
    pub(crate) bgra: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone)]
pub(crate) struct RawFrameHub {
    inner: Arc<(Mutex<LatestRawFrame>, Condvar)>,
}

#[derive(Resource, Default)]
pub(crate) struct DirectStreamFrameProcessors {
    processors: Vec<DirectStreamFrameProcessor>,
}

impl DirectStreamFrameProcessors {
    pub(crate) fn register<F>(&mut self, processor: F)
    where
        F: for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static,
    {
        self.processors.push(Box::new(processor));
    }

    pub(crate) fn process(&mut self, frame: DirectStreamFrame<'_>) {
        let mut frame = frame;
        for processor in &mut self.processors {
            processor(frame.reborrow());
        }
    }
}

#[derive(Default)]
struct LatestRawFrame {
    frame: Option<Arc<RawFrame>>,
}

impl RawFrameHub {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(LatestRawFrame::default()), Condvar::new())),
        }
    }

    pub(crate) fn publish(&self, frame: RawFrame) {
        let (lock, ready) = &*self.inner;
        if let Ok(mut latest) = lock.lock() {
            latest.frame = Some(Arc::new(frame));
            ready.notify_all();
        }
    }

    pub(crate) fn wait_for_first_frame(&self) -> Option<Arc<RawFrame>> {
        let (lock, ready) = &*self.inner;
        let mut latest = lock.lock().ok()?;

        while latest.frame.is_none() {
            latest = ready.wait(latest).ok()?;
        }

        latest.frame.clone()
    }

    pub(crate) fn latest_frame(&self) -> Option<Arc<RawFrame>> {
        let (lock, _) = &*self.inner;
        lock.lock().ok()?.frame.clone()
    }
}

pub struct DirectStreamFrame<'a> {
    bgra: &'a mut [u8],
    width: u32,
    height: u32,
    row_bytes: usize,
}

impl<'a> DirectStreamFrame<'a> {
    pub fn new(bgra: &'a mut [u8], width: u32, height: u32, row_bytes: usize) -> Self {
        Self {
            bgra,
            width,
            height,
            row_bytes,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn row_bytes(&self) -> usize {
        self.row_bytes
    }

    pub fn bgra_mut(&mut self) -> &mut [u8] {
        self.bgra
    }

    pub fn reborrow(&mut self) -> DirectStreamFrame<'_> {
        DirectStreamFrame {
            bgra: self.bgra,
            width: self.width,
            height: self.height,
            row_bytes: self.row_bytes,
        }
    }
}

pub trait DirectStreamFrameAppExt {
    fn add_direct_stream_frame_processor<F>(&mut self, processor: F) -> &mut Self
    where
        F: for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static;
}

impl DirectStreamFrameAppExt for bevy::prelude::App {
    fn add_direct_stream_frame_processor<F>(&mut self, processor: F) -> &mut Self
    where
        F: for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static,
    {
        self.init_resource::<DirectStreamFrameProcessors>();
        self.world_mut()
            .resource_mut::<DirectStreamFrameProcessors>()
            .register(processor);
        self
    }
}

pub(crate) fn copy_bgra_into_frame(
    source: &[u8],
    destination: &mut frame::Video,
    width: u32,
    height: u32,
) {
    let source_row_bytes = width as usize * 4;
    let destination_stride = destination.stride(0);
    let destination_data = destination.data_mut(0);

    for row in 0..height as usize {
        let source_start = row * source_row_bytes;
        let destination_start = row * destination_stride;
        destination_data[destination_start..destination_start + source_row_bytes]
            .copy_from_slice(&source[source_start..source_start + source_row_bytes]);
    }
}
