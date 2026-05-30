use crate::{
    audio::DirectStreamAudioTarget,
    chat::LocalChatHub,
    config::{AppConfig, effective_custom_batch_size},
    frames::{IndexedFrame, RawFrame, RawFrameSenders},
    gpu_palette::{
        GpuPalettePipeline, PaletteMaterial, make_stream_source_image,
        retarget_custom_host_pipeline,
    },
    palette::{
        PaletteBias, SharedPaletteBias, load_palette_config_runtime, load_prebaked_lookup_runtime,
    },
    public_types::{DirectStreamState, DirectStreamTarget},
    scene::StreamReadback,
    stats::SharedStats,
};
use bevy::{
    camera::RenderTarget, input::keyboard::KeyboardInput, prelude::*, ui::RelativeCursorPosition,
};
use crossbeam_channel::Sender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

#[derive(Resource)]
pub(crate) struct StreamControl {
    pub(crate) custom_width: String,
    pub(crate) custom_height: String,
    pub(crate) custom_fps: String,
    pub(crate) palette_bias: PaletteBias,
    pub(crate) prebaked_palette: bool,
    pub(crate) focused_input: Option<StreamControlInput>,
    pub(crate) status: String,
    preview_sender: Option<Sender<RawFrame>>,
    custom_sender: Option<Sender<IndexedFrame>>,
    custom_stream_state: CustomStreamState,
    shared_palette_bias: SharedPaletteBias,
}

impl StreamControl {
    pub(crate) fn new(
        config: &AppConfig,
        preview_sender: Option<Sender<RawFrame>>,
        custom_sender: Option<Sender<IndexedFrame>>,
        custom_stream_state: CustomStreamState,
        shared_palette_bias: SharedPaletteBias,
    ) -> Self {
        let palette_bias = shared_palette_bias.get();
        Self {
            custom_width: config.stream_width.to_string(),
            custom_height: config.stream_height.to_string(),
            custom_fps: config.stream_fps.to_string(),
            palette_bias,
            prebaked_palette: config.prebaked_palette,
            focused_input: None,
            status: "Ready".to_owned(),
            preview_sender,
            custom_sender,
            custom_stream_state,
            shared_palette_bias,
        }
    }

    pub(crate) fn is_streaming(&self) -> bool {
        self.custom_stream_state.is_active()
    }

    fn start(
        &mut self,
        senders: &mut RawFrameSenders,
        stats: &SharedStats,
        images: &mut Assets<Image>,
        palette_materials: &mut Assets<PaletteMaterial>,
        target: &mut DirectStreamTarget,
        direct_stream_state: &mut DirectStreamState,
        readback: &mut StreamReadback,
        gpu_palette: Option<&mut GpuPalettePipeline>,
        camera_targets: &mut Query<&mut RenderTarget>,
        quad_transforms: &mut Query<&mut Transform>,
        config: &AppConfig,
    ) {
        if self.is_streaming() {
            self.status = "Already streaming".to_owned();
            return;
        }

        let Some(custom_sender) = self.custom_sender.clone() else {
            self.status = "Custom host unavailable".to_owned();
            return;
        };
        let Ok((width, height, fps)) = self.custom_dimensions() else {
            self.status = "Use an 8-aligned square size 64-256 and fps 1-60".to_owned();
            return;
        };
        let Some(gpu_palette) = gpu_palette else {
            self.status = "GPU palette pipeline unavailable".to_owned();
            return;
        };

        let batch_size = effective_custom_batch_size(config.custom_host_batch_size, fps);
        let palette_config = load_palette_config_runtime(&config.palette_config_path);
        let palette_lookup = self
            .prebaked_palette
            .then(|| load_prebaked_lookup_runtime(&config.palette_config_path, &palette_config))
            .flatten();
        let image = images.add(make_stream_source_image(width, height));

        if let Ok(mut camera_target) = camera_targets.get_mut(target.camera) {
            *camera_target = RenderTarget::Image(image.clone().into());
        } else {
            self.status = "Could not retarget stream camera".to_owned();
            return;
        }

        if retarget_custom_host_pipeline(
            gpu_palette,
            images,
            palette_materials,
            camera_targets,
            quad_transforms,
            width,
            height,
            image.clone(),
            palette_lookup.as_ref(),
            target,
            batch_size,
        )
        .is_err()
        {
            self.status = "Could not retarget GPU output pipeline".to_owned();
            return;
        }

        target.image = image;
        target.width = width;
        target.height = height;
        target.fps = fps;
        direct_stream_state.active = true;
        direct_stream_state.width = width;
        direct_stream_state.height = height;
        direct_stream_state.fps = fps;
        readback.images = gpu_palette.output_images.clone();
        readback.batch_size = batch_size;
        readback.next_readback_entity = 0;
        readback.batch_started_at = None;
        readback.batch_in_progress = false;
        readback.frame_due = false;
        readback.textures_rendered_in_batch = 0;
        readback.frame_waiting_for_render = None;
        readback.rendered_batch_frames.clear();
        readback.rendered_batch_frames.reserve(batch_size);
        readback.frame_interval = std::time::Duration::from_secs_f64(1.0 / fps as f64);
        readback.frame_accumulator = std::time::Duration::ZERO;
        readback.pending_requests.clear();

        senders.preview = None;
        senders.custom = Some(custom_sender);
        self.custom_stream_state.set_fps(fps);
        self.custom_stream_state.set_active(true);
        self.status = "Custom host streaming".to_owned();
        stats.with_mut(|stats| stats.reset_custom_session());
    }

    fn stop(
        &mut self,
        senders: &mut RawFrameSenders,
        stats: &SharedStats,
        audio_target: &DirectStreamAudioTarget,
        direct_stream_state: &mut DirectStreamState,
        readback: &mut StreamReadback,
    ) {
        if !self.is_streaming() {
            self.status = "Not streaming".to_owned();
            return;
        }

        self.custom_stream_state.set_active(false);
        direct_stream_state.active = false;
        senders.custom = None;
        if self.preview_sender.is_some() {
            senders.preview = self.preview_sender.clone();
        }
        readback.pending_requests.clear();
        readback.batch_started_at = None;
        readback.batch_in_progress = false;
        readback.frame_due = false;
        readback.frame_accumulator = std::time::Duration::ZERO;
        readback.textures_rendered_in_batch = 0;
        readback.frame_waiting_for_render = None;
        readback.rendered_batch_frames.clear();
        audio_target.clear();
        self.status = "Custom host stopped".to_owned();
        stats.with_mut(|stats| {
            stats.custom_stage = "stopped";
            stats.custom_audio_packets_sent = 0;
            stats.custom_audio_bytes_sent = 0;
        });
    }

    fn open_custom_host(&mut self) {
        match open_url("http://127.0.0.1:8080") {
            Ok(()) => self.status = "Opened custom host preview".to_owned(),
            Err(err) => self.status = format!("Could not open custom host: {err}"),
        }
    }

    fn custom_dimensions(&self) -> Result<(u32, u32, u32), ()> {
        let width = self.custom_width.trim().parse::<u32>().map_err(|_| ())?;
        let height = self.custom_height.trim().parse::<u32>().map_err(|_| ())?;
        let fps = self.custom_fps.trim().parse::<u32>().map_err(|_| ())?;

        if width != height
            || !(64..=256).contains(&width)
            || !(64..=256).contains(&height)
            || width % 8 != 0
            || !(1..=60).contains(&fps)
        {
            return Err(());
        }

        Ok((width, height, fps))
    }

    pub(crate) fn set_palette_bias_slider(&mut self, slider: PaletteBiasSlider, value: f32) {
        if self.prebaked_palette {
            return;
        }

        let value = value.clamp(0.0, 1.0);
        let mut values = [
            self.palette_bias.lightness,
            self.palette_bias.chroma,
            self.palette_bias.hue,
        ];
        let changed_index = slider.index();
        values[changed_index] = value;
        let remaining = 1.0 - value;
        let other_indices: Vec<usize> = (0..3).filter(|index| *index != changed_index).collect();
        let other_total: f32 = other_indices.iter().map(|index| values[*index]).sum();

        if other_total <= f32::EPSILON {
            let split = remaining / other_indices.len() as f32;
            for index in other_indices {
                values[index] = split;
            }
        } else {
            for index in other_indices {
                values[index] = values[index] / other_total * remaining;
            }
        }

        let total = values.iter().sum::<f32>();
        values[2] = (values[2] + 1.0 - total).clamp(0.0, 1.0);
        self.palette_bias = PaletteBias {
            lightness: values[0],
            chroma: values[1],
            hue: values[2],
        };
        self.shared_palette_bias.set(self.palette_bias);
    }
}

#[derive(Clone, Resource)]
pub(crate) struct CustomStreamState {
    active: Arc<AtomicBool>,
    fps: Arc<AtomicU32>,
}

impl CustomStreamState {
    pub(crate) fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            fps: Arc::new(AtomicU32::new(1)),
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    pub(crate) fn fps(&self) -> u32 {
        self.fps.load(Ordering::Relaxed).max(1)
    }

    fn set_fps(&self, fps: u32) {
        self.fps.store(fps.max(1), Ordering::Relaxed);
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteBiasSlider {
    Lightness,
    Chroma,
    Hue,
}

impl PaletteBiasSlider {
    fn index(self) -> usize {
        match self {
            Self::Lightness => 0,
            Self::Chroma => 1,
            Self::Hue => 2,
        }
    }
}

#[derive(Component)]
pub(crate) struct CustomWidthInputBox;
#[derive(Component)]
pub(crate) struct CustomHeightInputBox;
#[derive(Component)]
pub(crate) struct CustomFpsInputBox;
#[derive(Component)]
pub(crate) struct CustomWidthInputText;
#[derive(Component)]
pub(crate) struct CustomHeightInputText;
#[derive(Component)]
pub(crate) struct CustomFpsInputText;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaletteBiasSliderValueText(pub(crate) PaletteBiasSlider);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaletteBiasSliderFill(pub(crate) PaletteBiasSlider);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamControlInput {
    CustomWidth,
    CustomHeight,
    CustomFps,
}

#[derive(Component)]
pub(crate) struct StreamControlStatusText;
#[derive(Component)]
pub(crate) struct StartStreamButton;
#[derive(Component)]
pub(crate) struct StopStreamButton;
#[derive(Component)]
pub(crate) struct OpenStreamButton;
#[derive(Component)]
pub(crate) struct PurgeChatButton;

pub(crate) fn handle_stream_key_typing(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut key_events: MessageReader<KeyboardInput>,
    mut control: ResMut<StreamControl>,
) {
    let Some(focused_input) = control.focused_input else {
        return;
    };

    if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        && keyboard.just_pressed(KeyCode::KeyV)
    {
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => {
                push_focused_text(&mut control, focused_input, text.trim());
                control.status = "Pasted".to_owned();
            }
            Err(err) => control.status = format!("Clipboard unavailable: {err}"),
        }
    }

    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        match event.key_code {
            KeyCode::Backspace => {
                pop_focused_text(&mut control, focused_input);
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                control.focused_input = None;
            }
            _ => {
                if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
                    continue;
                }
                if let Some(text) = &event.text {
                    push_focused_text(&mut control, focused_input, text);
                }
            }
        }
    }
}

pub(crate) fn handle_stream_input_box_interactions(
    mut control: ResMut<StreamControl>,
    mut input_boxes: ParamSet<(
        Query<
            (&Interaction, &mut BackgroundColor),
            (Changed<Interaction>, With<CustomWidthInputBox>),
        >,
        Query<
            (&Interaction, &mut BackgroundColor),
            (Changed<Interaction>, With<CustomHeightInputBox>),
        >,
        Query<
            (&Interaction, &mut BackgroundColor),
            (Changed<Interaction>, With<CustomFpsInputBox>),
        >,
    )>,
) {
    handle_input_box_interactions(
        &mut control,
        &mut input_boxes.p0(),
        StreamControlInput::CustomWidth,
    );
    handle_input_box_interactions(
        &mut control,
        &mut input_boxes.p1(),
        StreamControlInput::CustomHeight,
    );
    handle_input_box_interactions(
        &mut control,
        &mut input_boxes.p2(),
        StreamControlInput::CustomFps,
    );
}

pub(crate) fn handle_stream_start_interactions(
    mut control: ResMut<StreamControl>,
    mut senders: ResMut<RawFrameSenders>,
    stats: Res<SharedStats>,
    mut readback: Option<ResMut<StreamReadback>>,
    mut images: ResMut<Assets<Image>>,
    mut palette_materials: ResMut<Assets<PaletteMaterial>>,
    mut target: ResMut<DirectStreamTarget>,
    mut direct_stream_state: ResMut<DirectStreamState>,
    mut gpu_palette: Option<ResMut<GpuPalettePipeline>>,
    mut camera_targets: Query<&mut RenderTarget>,
    mut quad_transforms: Query<&mut Transform>,
    config: Res<AppConfig>,
    mut start_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<StartStreamButton>),
    >,
) {
    for (interaction, mut color) in &mut start_buttons {
        if *interaction == Interaction::Pressed {
            if let Some(readback) = readback.as_deref_mut() {
                let gpu_palette = gpu_palette.as_deref_mut();
                control.start(
                    &mut senders,
                    &stats,
                    &mut images,
                    &mut palette_materials,
                    &mut target,
                    &mut direct_stream_state,
                    readback,
                    gpu_palette,
                    &mut camera_targets,
                    &mut quad_transforms,
                    &config,
                );
            }
        }
        *color = button_color(*interaction, Color::srgb(0.05, 0.20, 0.13));
    }
}

pub(crate) fn handle_stream_stop_interactions(
    mut control: ResMut<StreamControl>,
    mut senders: ResMut<RawFrameSenders>,
    stats: Res<SharedStats>,
    audio_target: Res<DirectStreamAudioTarget>,
    mut direct_stream_state: ResMut<DirectStreamState>,
    mut readback: Option<ResMut<StreamReadback>>,
    mut stop_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<StopStreamButton>),
    >,
) {
    for (interaction, mut color) in &mut stop_buttons {
        if *interaction == Interaction::Pressed {
            if let Some(readback) = readback.as_deref_mut() {
                control.stop(
                    &mut senders,
                    &stats,
                    &audio_target,
                    &mut direct_stream_state,
                    readback,
                );
            }
        }
        *color = button_color(*interaction, Color::srgb(0.21, 0.06, 0.07));
    }
}

pub(crate) fn handle_stream_misc_button_interactions(
    mut control: ResMut<StreamControl>,
    local_chat: Option<Res<LocalChatHub>>,
    mut buttons: ParamSet<(
        Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<OpenStreamButton>)>,
        Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<PurgeChatButton>)>,
    )>,
) {
    for (interaction, mut color) in &mut buttons.p0() {
        if *interaction == Interaction::Pressed {
            control.open_custom_host();
        }
        *color = button_color(*interaction, Color::srgb(0.07, 0.10, 0.19));
    }

    for (interaction, mut color) in &mut buttons.p1() {
        if *interaction == Interaction::Pressed {
            if let Some(chat) = &local_chat {
                chat.purge();
                control.status = "Purged local chat".to_owned();
            }
        }
        *color = button_color(*interaction, Color::srgb(0.17, 0.10, 0.04));
    }
}

pub(crate) fn handle_palette_bias_sliders(
    mut control: ResMut<StreamControl>,
    mut sliders: Query<
        (
            &Interaction,
            &RelativeCursorPosition,
            &PaletteBiasSlider,
            &mut BackgroundColor,
        ),
        Changed<Interaction>,
    >,
) {
    for (interaction, cursor, slider, mut color) in &mut sliders {
        if *interaction == Interaction::Pressed && !control.prebaked_palette {
            let value = cursor
                .normalized
                .map(|position| position.x.clamp(0.0, 1.0))
                .unwrap_or(0.0);
            control.set_palette_bias_slider(*slider, value);
        }
        *color = button_color(*interaction, Color::srgb(0.08, 0.10, 0.14));
    }
}

pub(crate) fn update_stream_control_ui(
    control: Res<StreamControl>,
    mut texts: ParamSet<(
        Query<&mut Text, With<StreamControlStatusText>>,
        Query<&mut Text, With<CustomWidthInputText>>,
        Query<&mut Text, With<CustomHeightInputText>>,
        Query<&mut Text, With<CustomFpsInputText>>,
        Query<(&PaletteBiasSliderValueText, &mut Text)>,
    )>,
    mut slider_fills: Query<(&PaletteBiasSliderFill, &mut Node)>,
) {
    if !control.is_changed() {
        return;
    }

    if let Ok(mut text) = texts.p0().single_mut() {
        **text = format!(
            "stream control: {} - {}",
            if control.is_streaming() {
                "streaming"
            } else {
                "idle"
            },
            control.status
        );
    }
    if let Ok(mut text) = texts.p1().single_mut() {
        **text = control.custom_width.clone();
    }
    if let Ok(mut text) = texts.p2().single_mut() {
        **text = control.custom_height.clone();
    }
    if let Ok(mut text) = texts.p3().single_mut() {
        **text = control.custom_fps.clone();
    }

    for (marker, mut text) in &mut texts.p4() {
        **text = format!("{:.2}", slider_value(control.palette_bias, marker.0));
    }
    for (marker, mut node) in &mut slider_fills {
        node.width = percent((slider_value(control.palette_bias, marker.0) * 100.0) as f64);
    }
}

fn handle_input_box_interactions<T: Component>(
    control: &mut StreamControl,
    query: &mut Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<T>)>,
    input: StreamControlInput,
) {
    for (interaction, mut color) in query {
        if *interaction == Interaction::Pressed {
            control.focused_input = Some(input);
        }
        *color = button_color(*interaction, Color::srgb(0.045, 0.055, 0.07));
    }
}

fn push_focused_text(control: &mut StreamControl, focused_input: StreamControlInput, text: &str) {
    for ch in text.chars().filter(|ch| ch.is_ascii_digit()) {
        match focused_input {
            StreamControlInput::CustomWidth => control.custom_width.push(ch),
            StreamControlInput::CustomHeight => control.custom_height.push(ch),
            StreamControlInput::CustomFps => control.custom_fps.push(ch),
        }
    }
}

fn pop_focused_text(control: &mut StreamControl, focused_input: StreamControlInput) {
    match focused_input {
        StreamControlInput::CustomWidth => {
            control.custom_width.pop();
        }
        StreamControlInput::CustomHeight => {
            control.custom_height.pop();
        }
        StreamControlInput::CustomFps => {
            control.custom_fps.pop();
        }
    }
}

fn slider_value(bias: PaletteBias, slider: PaletteBiasSlider) -> f32 {
    match slider {
        PaletteBiasSlider::Lightness => bias.lightness,
        PaletteBiasSlider::Chroma => bias.chroma,
        PaletteBiasSlider::Hue => bias.hue,
    }
}

fn button_color(interaction: Interaction, base: Color) -> BackgroundColor {
    match interaction {
        Interaction::Pressed => BackgroundColor(Color::srgb(0.24, 0.32, 0.46)),
        Interaction::Hovered => BackgroundColor(Color::srgb(0.13, 0.17, 0.24)),
        Interaction::None => BackgroundColor(base),
    }
}

fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map(|_| ())
}
