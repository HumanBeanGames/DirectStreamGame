use crate::{
    constants::{STREAM_HEIGHT, STREAM_WIDTH},
    frames::{RawFrame, RawFrameSenders},
    scene::StreamReadback,
    stream_control::StreamControl,
};
use bevy::{
    prelude::*,
    render::gpu_readback::{Readback, ReadbackComplete},
};
use crossbeam_channel::Sender;

pub(crate) fn request_stream_readback(
    mut commands: Commands,
    time: Res<Time>,
    senders: Res<RawFrameSenders>,
    stream_control: Res<StreamControl>,
    mut readback: ResMut<StreamReadback>,
) {
    if !stream_control.should_capture() {
        return;
    }

    let preview_full = senders.preview.as_ref().is_some_and(Sender::is_full);
    if readback.in_flight || preview_full {
        return;
    }

    readback.timer.tick(time.delta());
    if !readback.timer.just_finished() {
        return;
    }

    readback.in_flight = true;
    commands
        .spawn(Readback::texture(readback.image.clone()))
        .observe(queue_readback_frame);
}

fn queue_readback_frame(
    event: On<ReadbackComplete>,
    mut commands: Commands,
    senders: Res<RawFrameSenders>,
    mut readback: ResMut<StreamReadback>,
) {
    readback.in_flight = false;
    commands.entity(event.entity).despawn();

    let preview_full = senders.preview.as_ref().is_some_and(Sender::is_full);
    if preview_full {
        senders.stats.with_mut(|stats| {
            stats.frames_dropped += 1;
            stats.preview_frames_dropped += 1;
        });
        return;
    }

    let row_bytes = STREAM_WIDTH as usize * 4;
    let aligned_row_bytes =
        bevy::render::renderer::RenderDevice::align_copy_bytes_per_row(row_bytes);

    let bgra = if row_bytes == aligned_row_bytes {
        event.data.clone()
    } else {
        event
            .data
            .chunks(aligned_row_bytes)
            .take(STREAM_HEIGHT as usize)
            .flat_map(|row| row[..row_bytes.min(row.len())].iter().copied())
            .collect()
    };
    senders.stats.with_mut(|stats| stats.frames_captured += 1);

    if let Some(preview) = &senders.preview
        && preview
            .try_send(RawFrame {
                bgra: bgra.clone(),
                width: STREAM_WIDTH,
                height: STREAM_HEIGHT,
            })
            .is_err()
    {
        senders.stats.with_mut(|stats| {
            stats.frames_dropped += 1;
            stats.preview_frames_dropped += 1;
        });
    }

    if let Some(twitch) = &senders.twitch {
        twitch.publish(RawFrame {
            bgra,
            width: STREAM_WIDTH,
            height: STREAM_HEIGHT,
        });
    }
}
