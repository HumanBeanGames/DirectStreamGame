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
    pub anchor: CustomHostPanelAnchor,
    pub order: i32,
    pub size_hint: Option<CustomHostPanelSize>,
    pub style_hint: Option<CustomHostPanelStyle>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum CustomHostPanelRegion {
    LeftOfStream,
    RightOfStream,
    BelowStream,
    AboveStream,
    #[default]
    SidePanelDefault,
}

impl CustomHostPanelRegion {
    pub fn anchor(self) -> CustomHostPanelAnchor {
        match self {
            CustomHostPanelRegion::LeftOfStream => CustomHostPanelAnchor::LeftOfStream,
            CustomHostPanelRegion::RightOfStream => CustomHostPanelAnchor::RightOfStream,
            CustomHostPanelRegion::BelowStream => CustomHostPanelAnchor::BelowStream,
            CustomHostPanelRegion::AboveStream => CustomHostPanelAnchor::AboveStream,
            CustomHostPanelRegion::SidePanelDefault => CustomHostPanelAnchor::RightOfStream,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CustomHostPanelAnchor {
    LeftOfStream,
    #[default]
    RightOfStream,
    AboveStream,
    BelowStream,
    OverlayTopLeft,
    OverlayTopRight,
    OverlayBottomLeft,
    OverlayBottomRight,
    NamedRegion(String),
}

impl CustomHostPanelAnchor {
    pub(crate) fn as_json_str(&self) -> String {
        match self {
            CustomHostPanelAnchor::LeftOfStream => "LeftOfStream".to_owned(),
            CustomHostPanelAnchor::RightOfStream => "RightOfStream".to_owned(),
            CustomHostPanelAnchor::AboveStream => "AboveStream".to_owned(),
            CustomHostPanelAnchor::BelowStream => "BelowStream".to_owned(),
            CustomHostPanelAnchor::OverlayTopLeft => "OverlayTopLeft".to_owned(),
            CustomHostPanelAnchor::OverlayTopRight => "OverlayTopRight".to_owned(),
            CustomHostPanelAnchor::OverlayBottomLeft => "OverlayBottomLeft".to_owned(),
            CustomHostPanelAnchor::OverlayBottomRight => "OverlayBottomRight".to_owned(),
            CustomHostPanelAnchor::NamedRegion(region) => format!("NamedRegion:{region}"),
        }
    }

    fn sort_key(&self) -> (u8, String) {
        match self {
            CustomHostPanelAnchor::LeftOfStream => (0, String::new()),
            CustomHostPanelAnchor::RightOfStream => (1, String::new()),
            CustomHostPanelAnchor::AboveStream => (2, String::new()),
            CustomHostPanelAnchor::BelowStream => (3, String::new()),
            CustomHostPanelAnchor::OverlayTopLeft => (4, String::new()),
            CustomHostPanelAnchor::OverlayTopRight => (5, String::new()),
            CustomHostPanelAnchor::OverlayBottomLeft => (6, String::new()),
            CustomHostPanelAnchor::OverlayBottomRight => (7, String::new()),
            CustomHostPanelAnchor::NamedRegion(region) => (8, region.clone()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomHostPanelSize {
    pub min_width_px: Option<u32>,
    pub max_width_px: Option<u32>,
    pub min_height_px: Option<u32>,
    pub max_height_px: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomHostPanelStyle {
    pub css_class: Option<String>,
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
            anchor: CustomHostPanelAnchor::RightOfStream,
            order: 0,
            size_hint: None,
            style_hint: None,
        });
    }

    pub fn publish_text_in_region(
        &self,
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        region: CustomHostPanelRegion,
    ) {
        self.publish(CustomHostPanel {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            revision: 0,
            anchor: region.anchor(),
            order: 0,
            size_hint: None,
            style_hint: None,
        });
    }

    pub fn publish_text_at(
        &self,
        id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        anchor: CustomHostPanelAnchor,
        order: i32,
    ) {
        self.publish(CustomHostPanel {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            revision: 0,
            anchor,
            order,
            size_hint: None,
            style_hint: None,
        });
    }

    pub fn clear(&self, id: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.panels.remove(id);
            state.next_revision = state.next_revision.wrapping_add(1);
        }
    }

    pub fn clear_region(&self, anchor: CustomHostPanelAnchor) {
        if let Ok(mut state) = self.state.lock() {
            state.panels.retain(|_, panel| panel.anchor != anchor);
            state.next_revision = state.next_revision.wrapping_add(1);
        }
    }

    pub fn snapshot(&self) -> Vec<CustomHostPanel> {
        let mut panels: Vec<CustomHostPanel> = self
            .state
            .lock()
            .map(|state| state.panels.values().cloned().collect())
            .unwrap_or_default();
        panels.sort_by_key(|panel| (panel.anchor.sort_key(), panel.order, panel.id.clone()));
        panels
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
