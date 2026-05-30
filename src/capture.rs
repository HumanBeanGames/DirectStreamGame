use crate::{
    frames::{
        DirectStreamFrame, DirectStreamFrameProcessors, IndexedFrame, RawFrame, RawFrameSenders,
    },
    public_types::DirectStreamTarget,
    scene::StreamReadback,
    stream_control::StreamControl,
};
use bevy::{
    prelude::*,
    render::gpu_readback::{Readback, ReadbackComplete},
};
use crossbeam_channel::Sender;
use std::time::Instant;

pub(crate) fn request_stream_readback(
    _time: Res<Time>,
    _senders: Res<RawFrameSenders>,
    _stream_control: Res<StreamControl>,
    _readback: Option<ResMut<StreamReadback>>,
) {
    // Readback requests are now issued by cycle_camera_render_targets
    // when all textures in the batch have been rendered to
}

pub(crate) fn queue_readback_frame(
    event: On<ReadbackComplete>,
    mut commands: Commands,
    senders: Res<RawFrameSenders>,
    target: Res<DirectStreamTarget>,
    mut processors: ResMut<DirectStreamFrameProcessors>,
    mut readback: ResMut<StreamReadback>,
) {
    let callback_started = Instant::now();
    let Some(pending) = readback.pending_requests.remove(&event.entity) else {
        commands.entity(event.entity).remove::<Readback>();
        senders
            .stats
            .with_mut(|stats| stats.custom_pending_readbacks = readback.pending_requests.len());
        return;
    };
    senders
        .stats
        .with_mut(|stats| stats.custom_pending_readbacks = readback.pending_requests.len());
    let captured_at = pending.captured_at;
    commands.entity(event.entity).remove::<Readback>();

    let preview_full = senders.preview.as_ref().is_some_and(Sender::is_full);
    let custom_full = senders.custom.as_ref().is_some_and(Sender::is_full);
    if preview_full || custom_full {
        senders.stats.with_mut(|stats| {
            stats.frames_dropped += 1;
            if preview_full {
                stats.preview_frames_dropped += 1;
            }
            if custom_full {
                stats.custom_frames_dropped += 1;
                stats.custom_queue_full_drops += 1;
            }
        });
        finish_readback_batch_if_complete(&mut readback, &senders);
        return;
    }

    if target.output_is_indexed {
        senders.stats.with_mut(|stats| {
            stats.record_custom_readback_wait(
                callback_started
                    .duration_since(pending.requested_at)
                    .as_secs_f64()
                    * 1000.0,
            );
        });

        let row_bytes = target.width as usize;
        let aligned_row_bytes =
            bevy::render::renderer::RenderDevice::align_copy_bytes_per_row(row_bytes);
        let indices = if row_bytes == aligned_row_bytes {
            event.data.clone()
        } else {
            event
                .data
                .chunks(aligned_row_bytes)
                .take(target.height as usize)
                .flat_map(|row| row[..row_bytes.min(row.len())].iter().copied())
                .collect()
        };

        senders.stats.with_mut(|stats| stats.frames_captured += 1);

        if let Some(custom) = &senders.custom
            && custom
                .try_send(IndexedFrame {
                    indices,
                    width: target.width,
                    height: target.height,
                    captured_at,
                })
                .is_err()
        {
            senders.stats.with_mut(|stats| {
                stats.frames_dropped += 1;
                stats.custom_frames_dropped += 1;
                stats.custom_queue_full_drops += 1;
            });
        }
        senders.stats.with_mut(|stats| {
            stats.record_custom_readback_cpu(callback_started.elapsed().as_secs_f64() * 1000.0);
        });
        finish_readback_batch_if_complete(&mut readback, &senders);
        return;
    }

    let row_bytes = target.width as usize * 4;
    let aligned_row_bytes =
        bevy::render::renderer::RenderDevice::align_copy_bytes_per_row(row_bytes);

    let mut bgra = if row_bytes == aligned_row_bytes {
        event.data.clone()
    } else {
        event
            .data
            .chunks(aligned_row_bytes)
            .take(target.height as usize)
            .flat_map(|row| row[..row_bytes.min(row.len())].iter().copied())
            .collect()
    };

    processors.process(DirectStreamFrame::new(
        bgra.as_mut_slice(),
        target.width,
        target.height,
        row_bytes,
    ));

    senders.stats.with_mut(|stats| stats.frames_captured += 1);

    if let Some(preview) = &senders.preview
        && preview
            .try_send(RawFrame {
                bgra: bgra.clone(),
                width: target.width,
                height: target.height,
            })
            .is_err()
    {
        senders.stats.with_mut(|stats| {
            stats.frames_dropped += 1;
            stats.preview_frames_dropped += 1;
        });
    }
}

fn finish_readback_batch_if_complete(readback: &mut StreamReadback, senders: &RawFrameSenders) {
    if !readback.pending_requests.is_empty() {
        senders.stats.with_mut(|stats| {
            stats.custom_pending_readbacks = readback.pending_requests.len();
            stats.custom_batch_buffered_frames = readback.rendered_batch_frames.len();
        });
        return;
    }

    if let Some(batch_start) = readback.batch_started_at.take() {
        let batch_latency_ms = batch_start.elapsed().as_secs_f64() * 1000.0;
        senders.stats.with_mut(|stats| {
            stats.custom_batch_size = readback.batch_size;
            stats.custom_batch_latency_ms = batch_latency_ms;
            stats.custom_pending_readbacks = 0;
            stats.custom_batch_buffered_frames = readback.rendered_batch_frames.len();
        });
    }
    readback.batch_in_progress = false;
}
