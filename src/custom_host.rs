use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct CustomHostPanel {
    pub id: String,
    pub title: String,
    pub body: String,
    pub revision: u64,
}

#[derive(Clone, Resource, Default)]
pub struct CustomHostPanelHub {
    state: Arc<Mutex<CustomHostPanelState>>,
}

#[derive(Default)]
struct CustomHostPanelState {
    panels: BTreeMap<String, CustomHostPanel>,
    next_revision: u64,
}

impl CustomHostPanelHub {
    pub fn publish(&self, mut panel: CustomHostPanel) {
        if panel.id.trim().is_empty() {
            return;
        }

        if let Ok(mut state) = self.state.lock() {
            state.next_revision = state.next_revision.wrapping_add(1);
            panel.revision = state.next_revision;
            state.panels.insert(panel.id.clone(), panel);
        }
    }

    pub fn publish_text(
        &self,
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) {
        self.publish(CustomHostPanel {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            revision: 0,
        });
    }

    pub fn clear(&self, id: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.panels.remove(id);
            state.next_revision = state.next_revision.wrapping_add(1);
        }
    }

    pub fn snapshot(&self) -> Vec<CustomHostPanel> {
        self.state
            .lock()
            .map(|state| state.panels.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[derive(Message, Clone)]
pub struct StreamPointerClick {
    pub identity: String,
    pub display_name: String,
    pub x: u32,
    pub y: u32,
    pub normalized_x: f32,
    pub normalized_y: f32,
}

#[derive(Clone, Resource, Default)]
pub(crate) struct StreamPointerClickHub {
    clicks: Arc<Mutex<Vec<StreamPointerClick>>>,
}

impl StreamPointerClickHub {
    pub(crate) fn submit(&self, click: StreamPointerClick) {
        if let Ok(mut clicks) = self.clicks.lock() {
            clicks.push(click);
        }
    }

    fn drain(&self) -> Vec<StreamPointerClick> {
        self.clicks
            .lock()
            .map(|mut clicks| clicks.drain(..).collect())
            .unwrap_or_default()
    }
}

pub(crate) fn poll_stream_pointer_clicks(
    hub: Option<Res<StreamPointerClickHub>>,
    mut writer: MessageWriter<StreamPointerClick>,
) {
    let Some(hub) = hub else {
        return;
    };

    for click in hub.drain() {
        writer.write(click);
    }
}
