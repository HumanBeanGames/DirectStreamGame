use bevy::prelude::*;

pub struct DirectStreamFrame<'a> {
    bgra: &'a mut [u8],
    width: u32,
    height: u32,
}

impl<'a> DirectStreamFrame<'a> {
    pub(crate) fn new(bgra: &'a mut [u8], width: u32, height: u32) -> Self {
        Self {
            bgra,
            width,
            height,
        }
    }

    pub fn bgra(&self) -> &[u8] {
        self.bgra
    }

    pub fn bgra_mut(&mut self) -> &mut [u8] {
        self.bgra
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn row_bytes(&self) -> usize {
        self.width as usize * 4
    }
}

pub type DirectStreamFrameProcessor =
    Box<dyn for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static>;

#[derive(Resource, Default)]
pub struct DirectStreamFrameProcessors {
    processors: Vec<DirectStreamFrameProcessor>,
}

impl DirectStreamFrameProcessors {
    pub fn add(
        &mut self,
        processor: impl for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static,
    ) {
        self.processors.push(Box::new(processor));
    }

    pub(crate) fn process(&mut self, bgra: &mut [u8], width: u32, height: u32) {
        for processor in &mut self.processors {
            processor(DirectStreamFrame::new(bgra, width, height));
        }
    }
}

pub trait DirectStreamFrameAppExt {
    fn add_direct_stream_frame_processor(
        &mut self,
        processor: impl for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static,
    ) -> &mut Self;
}

impl DirectStreamFrameAppExt for App {
    fn add_direct_stream_frame_processor(
        &mut self,
        processor: impl for<'a> FnMut(DirectStreamFrame<'a>) + Send + Sync + 'static,
    ) -> &mut Self {
        if !self
            .world()
            .contains_resource::<DirectStreamFrameProcessors>()
        {
            self.insert_resource(DirectStreamFrameProcessors::default());
        }

        self.world_mut()
            .resource_mut::<DirectStreamFrameProcessors>()
            .add(processor);
        self
    }
}

#[derive(Clone, Resource)]
pub struct DirectStreamTarget {
    pub camera: Entity,
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectStreamSet {
    Setup,
}
