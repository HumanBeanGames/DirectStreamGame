use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
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
    pub audience: CustomHostPanelAudience,
}

#[derive(Clone, Resource)]
pub struct CustomHostBranding {
    pub page_title: String,
    pub header_title: String,
}

impl Default for CustomHostBranding {
    fn default() -> Self {
        Self {
            page_title: "Direct Stream Game".to_owned(),
            header_title: "Direct Stream Game custom palette stream".to_owned(),
        }
    }
}

impl CustomHostBranding {
    pub fn new(page_title: impl Into<String>, header_title: impl Into<String>) -> Self {
        Self {
            page_title: page_title.into(),
            header_title: header_title.into(),
        }
    }
}

#[derive(Clone, Resource)]
pub struct CustomHostLayout {
    pub max_player_width_px: Option<u32>,
    pub prefer_larger_player: bool,
    pub minimizable_player: bool,
    pub start_player_minimized: bool,
}

impl Default for CustomHostLayout {
    fn default() -> Self {
        Self {
            max_player_width_px: None,
            prefer_larger_player: false,
            minimizable_player: false,
            start_player_minimized: false,
        }
    }
}

impl CustomHostLayout {
    pub fn with_max_player_width(mut self, width_px: u32) -> Self {
        self.max_player_width_px = Some(width_px);
        self
    }

    pub fn prefer_larger_player(mut self) -> Self {
        self.prefer_larger_player = true;
        self
    }

    pub fn minimizable_player(mut self) -> Self {
        self.minimizable_player = true;
        self
    }

    pub fn start_player_minimized(mut self) -> Self {
        self.minimizable_player = true;
        self.start_player_minimized = true;
        self
    }
}

impl CustomHostPanel {
    pub fn for_viewer_identity(mut self, identity: impl Into<String>) -> Self {
        self.audience = CustomHostPanelAudience::ViewerIdentity(identity.into());
        self
    }

    pub fn for_viewer_name(mut self, name: impl Into<String>) -> Self {
        self.audience = CustomHostPanelAudience::ViewerName(name.into());
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CustomHostPanelAudience {
    #[default]
    All,
    ViewerIdentity(String),
    ViewerName(String),
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
    pub hide_header: bool,
    pub body_white_space: Option<PanelWhiteSpace>,
}

impl CustomHostPanelStyle {
    pub fn headerless() -> Self {
        Self {
            hide_header: true,
            ..Default::default()
        }
    }

    pub fn with_css_class(mut self, css_class: impl Into<String>) -> Self {
        self.css_class = Some(css_class.into());
        self
    }

    pub fn with_body_white_space(mut self, body_white_space: PanelWhiteSpace) -> Self {
        self.body_white_space = Some(body_white_space);
        self
    }

    pub fn no_wrap(mut self) -> Self {
        self.body_white_space = Some(PanelWhiteSpace::NoWrap);
        self
    }

    pub fn pre_wrap(mut self) -> Self {
        self.body_white_space = Some(PanelWhiteSpace::PreWrap);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelWhiteSpace {
    PreWrap,
    NoWrap,
}

impl PanelWhiteSpace {
    pub(crate) fn as_json_str(self) -> &'static str {
        match self {
            PanelWhiteSpace::PreWrap => "PreWrap",
            PanelWhiteSpace::NoWrap => "NoWrap",
        }
    }
}

#[derive(Clone)]
pub struct CustomHostOverlayElement {
    pub id: String,
    pub audience: CustomHostPanelAudience,
    pub x: f32,
    pub y: f32,
    pub coordinate_space: OverlayCoordinateSpace,
    pub kind: OverlayElementKind,
    pub order: i32,
    pub style: OverlayElementStyle,
    pub ttl_ms: Option<u64>,
}

impl CustomHostOverlayElement {
    pub fn for_viewer_identity(mut self, identity: impl Into<String>) -> Self {
        self.audience = CustomHostPanelAudience::ViewerIdentity(identity.into());
        self
    }

    pub fn for_viewer_name(mut self, name: impl Into<String>) -> Self {
        self.audience = CustomHostPanelAudience::ViewerName(name.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayCoordinateSpace {
    StreamPixels,
    #[default]
    NormalizedStream,
}

impl OverlayCoordinateSpace {
    pub(crate) fn as_json_str(self) -> &'static str {
        match self {
            OverlayCoordinateSpace::StreamPixels => "StreamPixels",
            OverlayCoordinateSpace::NormalizedStream => "NormalizedStream",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverlayElementKind {
    Circle {
        radius: f32,
    },
    Flag {
        width: f32,
        height: f32,
    },
    Text {
        text: String,
    },
    Sprite {
        image_id: String,
        width: f32,
        height: f32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayElementStyle {
    pub stroke_color: Option<String>,
    pub fill_color: Option<String>,
    pub text_color: Option<String>,
    pub line_width: f32,
    pub font_px: f32,
    pub css_class: Option<String>,
}

impl Default for OverlayElementStyle {
    fn default() -> Self {
        Self {
            stroke_color: Some("#ff3b30".to_owned()),
            fill_color: None,
            text_color: Some("#ffffff".to_owned()),
            line_width: 2.0,
            font_px: 12.0,
            css_class: None,
        }
    }
}

#[derive(Clone, Resource, Default)]
pub struct CustomHostPanelHub {
    state: Arc<Mutex<CustomHostPanelState>>,
}

#[derive(Clone, Resource, Default)]
pub struct CustomHostOverlayHub {
    state: Arc<Mutex<CustomHostOverlayState>>,
}

#[derive(Default)]
struct CustomHostPanelState {
    panels: BTreeMap<CustomHostPanelKey, CustomHostPanel>,
    next_revision: u64,
}

#[derive(Default)]
struct CustomHostOverlayState {
    overlays: BTreeMap<CustomHostPanelKey, CustomHostOverlayEntry>,
}

#[derive(Clone)]
struct CustomHostOverlayEntry {
    element: CustomHostOverlayElement,
    created_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CustomHostPanelKey {
    audience: CustomHostPanelKeyAudience,
    id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CustomHostPanelKeyAudience {
    All,
    ViewerIdentity(String),
    ViewerName(String),
}

impl CustomHostPanelHub {
    pub fn publish(&self, mut panel: CustomHostPanel) {
        if panel.id.trim().is_empty() {
            return;
        }

        if let Ok(mut state) = self.state.lock() {
            state.next_revision = state.next_revision.wrapping_add(1);
            panel.revision = state.next_revision;
            state
                .panels
                .insert(CustomHostPanelKey::for_panel(&panel), panel);
        }
    }

    pub fn publish_for_viewer_identity(&self, panel: CustomHostPanel, identity: impl Into<String>) {
        self.publish(panel.for_viewer_identity(identity));
    }

    pub fn publish_for_viewer_name(&self, panel: CustomHostPanel, name: impl Into<String>) {
        self.publish(panel.for_viewer_name(name));
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
            audience: CustomHostPanelAudience::All,
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
            audience: CustomHostPanelAudience::All,
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
            audience: CustomHostPanelAudience::All,
        });
    }

    pub fn clear(&self, id: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.panels.retain(|key, _| key.id != id);
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

    pub fn snapshot_for_viewer(
        &self,
        viewer_identity: Option<&str>,
        viewer_name: Option<&str>,
    ) -> Vec<CustomHostPanel> {
        let mut panels: Vec<CustomHostPanel> = self
            .state
            .lock()
            .map(|state| {
                state
                    .panels
                    .values()
                    .filter(|panel| panel_matches_audience(panel, viewer_identity, viewer_name))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        panels.sort_by_key(|panel| (panel.anchor.sort_key(), panel.order, panel.id.clone()));
        panels
    }
}

impl CustomHostOverlayHub {
    pub fn publish(&self, element: CustomHostOverlayElement) {
        if element.id.trim().is_empty() {
            return;
        }

        if let Ok(mut state) = self.state.lock() {
            purge_expired_overlays_locked(&mut state, current_time_millis());
            state.overlays.insert(
                CustomHostPanelKey::for_overlay(&element),
                CustomHostOverlayEntry {
                    element,
                    created_at_ms: current_time_millis(),
                },
            );
        }
    }

    pub fn publish_overlay_for_viewer_identity(
        &self,
        element: CustomHostOverlayElement,
        identity: impl Into<String>,
    ) {
        self.publish(element.for_viewer_identity(identity));
    }

    pub fn publish_overlay_for_viewer_name(
        &self,
        element: CustomHostOverlayElement,
        name: impl Into<String>,
    ) {
        self.publish(element.for_viewer_name(name));
    }

    pub fn clear_overlay(&self, id: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.overlays.retain(|key, _| key.id != id);
        }
    }

    pub fn clear_overlay_for_viewer_identity(&self, id: &str, identity: impl Into<String>) {
        self.clear_overlay_for_audience(
            id,
            CustomHostPanelAudience::ViewerIdentity(identity.into()),
        );
    }

    pub fn clear_overlay_for_viewer_name(&self, id: &str, name: impl Into<String>) {
        self.clear_overlay_for_audience(id, CustomHostPanelAudience::ViewerName(name.into()));
    }

    fn clear_overlay_for_audience(&self, id: &str, audience: CustomHostPanelAudience) {
        let key = CustomHostPanelKey {
            audience: CustomHostPanelKeyAudience::from_audience(&audience),
            id: id.to_owned(),
        };
        if let Ok(mut state) = self.state.lock() {
            state.overlays.remove(&key);
        }
    }

    pub fn snapshot_for_viewer(
        &self,
        viewer_identity: Option<&str>,
        viewer_name: Option<&str>,
    ) -> Vec<CustomHostOverlayElement> {
        let now_ms = current_time_millis();
        let mut overlays: Vec<CustomHostOverlayElement> = if let Ok(mut state) = self.state.lock() {
            purge_expired_overlays_locked(&mut state, now_ms);
            state
                .overlays
                .values()
                .filter(|entry| {
                    overlay_matches_audience(&entry.element, viewer_identity, viewer_name)
                })
                .map(|entry| entry.element.clone())
                .collect()
        } else {
            Vec::new()
        };
        overlays.sort_by_key(|element| (element.order, element.id.clone()));
        overlays
    }
}

impl CustomHostPanelKey {
    fn for_panel(panel: &CustomHostPanel) -> Self {
        Self {
            audience: CustomHostPanelKeyAudience::from_audience(&panel.audience),
            id: panel.id.clone(),
        }
    }

    fn for_overlay(element: &CustomHostOverlayElement) -> Self {
        Self {
            audience: CustomHostPanelKeyAudience::from_audience(&element.audience),
            id: element.id.clone(),
        }
    }
}

impl CustomHostPanelKeyAudience {
    fn from_audience(audience: &CustomHostPanelAudience) -> Self {
        match audience {
            CustomHostPanelAudience::All => Self::All,
            CustomHostPanelAudience::ViewerIdentity(identity) => {
                Self::ViewerIdentity(identity.clone())
            }
            CustomHostPanelAudience::ViewerName(name) => {
                Self::ViewerName(name.to_ascii_lowercase())
            }
        }
    }
}

fn panel_matches_audience(
    panel: &CustomHostPanel,
    viewer_identity: Option<&str>,
    viewer_name: Option<&str>,
) -> bool {
    match &panel.audience {
        CustomHostPanelAudience::All => true,
        CustomHostPanelAudience::ViewerIdentity(identity) => {
            viewer_identity == Some(identity.as_str())
        }
        CustomHostPanelAudience::ViewerName(name) => viewer_name
            .map(|viewer_name| viewer_name.eq_ignore_ascii_case(name))
            .unwrap_or(false),
    }
}

fn overlay_matches_audience(
    element: &CustomHostOverlayElement,
    viewer_identity: Option<&str>,
    viewer_name: Option<&str>,
) -> bool {
    audience_matches(&element.audience, viewer_identity, viewer_name)
}

fn audience_matches(
    audience: &CustomHostPanelAudience,
    viewer_identity: Option<&str>,
    viewer_name: Option<&str>,
) -> bool {
    match audience {
        CustomHostPanelAudience::All => true,
        CustomHostPanelAudience::ViewerIdentity(identity) => {
            viewer_identity == Some(identity.as_str())
        }
        CustomHostPanelAudience::ViewerName(name) => viewer_name
            .map(|viewer_name| viewer_name.eq_ignore_ascii_case(name))
            .unwrap_or(false),
    }
}

fn purge_expired_overlays_locked(state: &mut CustomHostOverlayState, now_ms: u64) {
    state.overlays.retain(|_, entry| {
        entry
            .element
            .ttl_ms
            .is_none_or(|ttl| now_ms < entry.created_at_ms.saturating_add(ttl))
    });
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[derive(Message, Clone)]
pub struct StreamPointerClick {
    pub identity: String,
    pub display_name: String,
    pub client_x: f32,
    pub client_y: f32,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_panel(id: &str, body: &str) -> CustomHostPanel {
        CustomHostPanel {
            id: id.to_owned(),
            title: id.to_owned(),
            body: body.to_owned(),
            revision: 0,
            anchor: CustomHostPanelAnchor::LeftOfStream,
            order: 0,
            size_hint: None,
            style_hint: None,
            audience: CustomHostPanelAudience::All,
        }
    }

    #[test]
    fn viewer_panels_with_same_id_do_not_collide() {
        let hub = CustomHostPanelHub::default();
        hub.publish_for_viewer_identity(test_panel("selected-town", "viewer-a"), "viewer-a");
        hub.publish_for_viewer_identity(test_panel("selected-town", "viewer-b"), "viewer-b");
        hub.publish(test_panel("shared", "global"));

        let viewer_a = hub.snapshot_for_viewer(Some("viewer-a"), Some("A"));
        let viewer_b = hub.snapshot_for_viewer(Some("viewer-b"), Some("B"));

        assert!(viewer_a.iter().any(|panel| panel.body == "viewer-a"));
        assert!(!viewer_a.iter().any(|panel| panel.body == "viewer-b"));
        assert!(viewer_b.iter().any(|panel| panel.body == "viewer-b"));
        assert!(!viewer_b.iter().any(|panel| panel.body == "viewer-a"));
        assert!(viewer_a.iter().any(|panel| panel.body == "global"));
        assert!(viewer_b.iter().any(|panel| panel.body == "global"));
    }

    fn test_overlay(id: &str, x: f32) -> CustomHostOverlayElement {
        CustomHostOverlayElement {
            id: id.to_owned(),
            audience: CustomHostPanelAudience::All,
            x,
            y: 0.5,
            coordinate_space: OverlayCoordinateSpace::NormalizedStream,
            kind: OverlayElementKind::Circle { radius: 4.0 },
            order: 0,
            style: OverlayElementStyle::default(),
            ttl_ms: None,
        }
    }

    #[test]
    fn viewer_overlays_with_same_id_do_not_collide() {
        let hub = CustomHostOverlayHub::default();
        hub.publish_overlay_for_viewer_identity(test_overlay("selected-town", 0.25), "viewer-a");
        hub.publish_overlay_for_viewer_identity(test_overlay("selected-town", 0.75), "viewer-b");

        let viewer_a = hub.snapshot_for_viewer(Some("viewer-a"), Some("A"));
        let viewer_b = hub.snapshot_for_viewer(Some("viewer-b"), Some("B"));

        assert_eq!(viewer_a.len(), 1);
        assert_eq!(viewer_b.len(), 1);
        assert_eq!(viewer_a[0].x, 0.25);
        assert_eq!(viewer_b[0].x, 0.75);
    }
}
