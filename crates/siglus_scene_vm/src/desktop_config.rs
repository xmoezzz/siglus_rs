//! Built-in configuration window, following Siglus cfg_wnd_config_base and cfg_wnd_func_*.
//! Gameexe controls tab visibility, channel visibility, labels, and game-specific switches.

use crate::formats::gameexe::GameexeConfig;
use crate::render::Renderer;
use crate::runtime::CommandContext;
use crate::runtime::forms::syscom;
use crate::runtime::globals::OriginalConfigRuntimeState;
use anyhow::{Context, Result};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use std::sync::Arc;
use std::time::Instant;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

fn configure_egui_default_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "siglus_default".to_string(),
        egui::FontData::from_static(crate::text_render::DEFAULT_FONT_BYTES).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "siglus_default".to_string());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "siglus_default".to_string());
    ctx.set_fonts(fonts);
}

pub enum DesktopConfigAction {
    Changed,
    Close,
}

pub struct DesktopConfigWindow {
    window: Arc<dyn Window>,
    window_id: WindowId,
    renderer: Renderer,
    egui_renderer: EguiRenderer,
    egui_ctx: egui::Context,
    start_time: Instant,
    input_events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    pointer_pos: egui::Pos2,
    pub dialog: ConfigDialog,
}

impl DesktopConfigWindow {
    pub fn new(elwt: &dyn ActiveEventLoop, dialog: ConfigDialog) -> Result<Self> {
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_title("環境設定")
                    .with_surface_size(LogicalSize::new(760.0, 570.0))
                    .with_min_surface_size(LogicalSize::new(640.0, 440.0)),
            )
            .context("create configuration window")?;
        let window: Arc<dyn Window> = Arc::from(window);
        crate::ime::enable_ime(
            window.as_ref(),
            LogicalPosition::new(0, 0).into(),
            LogicalSize::new(0, 0).into(),
        );
        let renderer =
            pollster::block_on(Renderer::new(window.clone())).context("config renderer init")?;
        let egui_renderer = EguiRenderer::new(
            &renderer.device,
            renderer.config.format,
            egui_wgpu::RendererOptions::default(),
        );
        let egui_ctx = egui::Context::default();
        configure_egui_default_font(&egui_ctx);
        egui_ctx.set_visuals(egui::Visuals::light());
        window.request_redraw();
        Ok(Self {
            window_id: window.id(),
            window,
            renderer,
            egui_renderer,
            egui_ctx,
            start_time: Instant::now(),
            input_events: Vec::new(),
            modifiers: egui::Modifiers::default(),
            pointer_pos: egui::Pos2::ZERO,
            dialog,
        })
    }
    pub fn window_id(&self) -> WindowId {
        self.window_id
    }
    pub fn into_dialog(self) -> ConfigDialog {
        // Dropping the renderer releases the surface's Arc, then the native
        // window closes even on backends that cannot hide windows (Wayland).
        self.dialog
    }
    pub fn handle_window_event(&mut self, event: WindowEvent) -> Option<DesktopConfigAction> {
        match event {
            WindowEvent::CloseRequested => return Some(DesktopConfigAction::Close),
            WindowEvent::SurfaceResized(size) => {
                self.renderer.resize(size.width.max(1), size.height.max(1));
                self.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = map_modifiers(modifiers.state());
            }
            WindowEvent::PointerMoved {
                position,
                primary: true,
                ..
            }
            | WindowEvent::PointerEntered {
                position,
                primary: true,
                ..
            } => {
                let logical = position.to_logical::<f64>(self.window.scale_factor());
                self.pointer_pos = egui::pos2(logical.x as f32, logical.y as f32);
                self.input_events
                    .push(egui::Event::PointerMoved(self.pointer_pos));
                self.window.request_redraw();
            }
            WindowEvent::PointerLeft { primary: true, .. } => {
                self.input_events.push(egui::Event::PointerGone);
                self.window.request_redraw();
            }
            WindowEvent::PointerButton {
                state,
                button,
                position,
                primary: true,
                ..
            } => {
                let button = button.mouse_button()?;
                let logical = position.to_logical::<f64>(self.window.scale_factor());
                self.pointer_pos = egui::pos2(logical.x as f32, logical.y as f32);
                if let Some(button) = map_pointer_button(button) {
                    self.input_events.push(egui::Event::PointerButton {
                        pos: self.pointer_pos,
                        button,
                        pressed: state == ElementState::Pressed,
                        modifiers: self.modifiers,
                    });
                    self.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => egui::vec2(x * 24.0, y * 24.0),
                    MouseScrollDelta::PixelDelta(pos) => egui::vec2(pos.x as f32, pos.y as f32),
                    _ => return None,
                };
                self.input_events.push(egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    modifiers: self.modifiers,
                });
                self.window.request_redraw();
            }
            WindowEvent::Ime(ime) => {
                let event = match ime {
                    Ime::Enabled => egui::ImeEvent::Enabled,
                    Ime::Preedit(text, _) => egui::ImeEvent::Preedit {
                        text,
                        active_range_chars: None,
                    },
                    Ime::Commit(text) => egui::ImeEvent::Commit(text),
                    Ime::Disabled => egui::ImeEvent::Disabled,
                    _ => return None,
                };
                self.input_events.push(egui::Event::Ime(event));
                self.window.request_redraw();
            }
            #[cfg(target_os = "windows")]
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: winit::keyboard::Key::Named(winit::keyboard::NamedKey::Process),
                        ..
                    },
                ..
            } => return None,
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(key) = map_key(code)
                {
                    self.input_events.push(egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: event.state == ElementState::Pressed,
                        repeat: event.repeat,
                        modifiers: self.modifiers,
                    });
                }
                if let KeyEvent {
                    state: ElementState::Pressed,
                    text: Some(text),
                    ..
                } = event
                    && !self.modifiers.command
                {
                    let text = text.to_string();
                    if !text.is_empty() && !text.chars().all(char::is_control) {
                        self.input_events.push(egui::Event::Text(text));
                    }
                }
                self.window.request_redraw();
            }
            WindowEvent::RedrawRequested => match self.render() {
                Ok(action) => return action,
                Err(err) => log::error!("desktop config render failed: {err:#}"),
            },
            _ => {}
        }
        None
    }

    fn render(&mut self) -> Result<Option<DesktopConfigAction>> {
        let size = self.window.surface_size();
        if size.width == 0 || size.height == 0 {
            return Ok(None);
        }
        let scale = self.window.scale_factor() as f32;
        self.egui_ctx.set_pixels_per_point(scale);
        let logical_w = size.width as f32 / scale.max(1.0);
        let logical_h = size.height as f32 / scale.max(1.0);
        let mut events = std::mem::take(&mut self.input_events);
        events.insert(0, egui::Event::ModifiersChanged(self.modifiers));
        let modifiers = self.modifiers;
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical_w, logical_h),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),

            events,
            ..Default::default()
        };

        let before = self.dialog.state.clone();
        let mut close = false;
        let egui_ctx = self.egui_ctx.clone();
        let output = egui_ctx.run_ui(raw_input, |ctx| {
            close = self.dialog.show(ctx);
        });
        let action = if close {
            Some(DesktopConfigAction::Close)
        } else if self.dialog.state != before {
            Some(DesktopConfigAction::Changed)
        } else {
            None
        };
        let screen_desc = ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: scale,
        };
        let paint_jobs = self.egui_ctx.tessellate(output.shapes, scale);
        for (id, delta) in &output.textures_delta.set {
            for delta in delta {
                self.egui_renderer.update_texture(
                    &self.renderer.device,
                    &self.renderer.queue,
                    *id,
                    delta,
                );
            }
        }
        let frame = match self.renderer.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.renderer
                    .resize(self.renderer.config.width, self.renderer.config.height);
                return Ok(action);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("Config surface validation failed")
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(action);
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("siglus_config_egui_encoder"),
                });
        self.egui_renderer.update_buffers(
            &self.renderer.device,
            &self.renderer.queue,
            &mut encoder,
            &paint_jobs,
            &screen_desc,
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("siglus_config_egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.965,
                            g: 0.970,
                            b: 0.980,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.egui_renderer
                .render(&mut pass.forget_lifetime(), &paint_jobs, &screen_desc);
        }
        self.renderer.queue.submit(Some(encoder.finish()));
        self.renderer.queue.present(frame);
        for &id in &output.textures_delta.free {
            self.egui_renderer.free_texture(&id);
        }
        Ok(action)
    }
}
fn map_pointer_button(button: MouseButton) -> Option<egui::PointerButton> {
    match button {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        _ => None,
    }
}

fn map_modifiers(state: ModifiersState) -> egui::Modifiers {
    egui::Modifiers {
        alt: state.alt_key(),
        ctrl: state.control_key(),
        shift: state.shift_key(),
        mac_cmd: cfg!(target_os = "macos") && state.meta_key(),
        command: if cfg!(target_os = "macos") {
            state.meta_key()
        } else {
            state.control_key()
        },
    }
}

fn map_key(code: KeyCode) -> Option<egui::Key> {
    use KeyCode::*;
    Some(match code {
        ArrowDown => egui::Key::ArrowDown,
        ArrowLeft => egui::Key::ArrowLeft,
        ArrowRight => egui::Key::ArrowRight,
        ArrowUp => egui::Key::ArrowUp,
        Escape => egui::Key::Escape,
        Tab => egui::Key::Tab,
        Backspace => egui::Key::Backspace,
        Enter | NumpadEnter => egui::Key::Enter,
        Space => egui::Key::Space,
        Insert => egui::Key::Insert,
        Delete => egui::Key::Delete,
        Home => egui::Key::Home,
        End => egui::Key::End,
        PageUp => egui::Key::PageUp,
        PageDown => egui::Key::PageDown,
        KeyA => egui::Key::A,
        KeyC => egui::Key::C,
        KeyV => egui::Key::V,
        KeyX => egui::Key::X,
        KeyY => egui::Key::Y,
        KeyZ => egui::Key::Z,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Screen,
    Volume,
    Message,
    Background,
    Voice,
    Auto,
    Jitan,
    Other,
    System,
}

impl Tab {
    const ALL: [Self; 9] = [
        Self::Screen,
        Self::Volume,
        Self::Message,
        Self::Background,
        Self::Voice,
        Self::Auto,
        Self::Jitan,
        Self::Other,
        Self::System,
    ];
    fn definition(self) -> (&'static str, &'static str) {
        match self {
            Self::Screen => ("SCREEN", "画面"),
            Self::Volume => ("VOLUME", "音量"),
            Self::Message => ("MESSAGE", "文章"),
            Self::Background => ("MWNDBK", "背景色"),
            Self::Voice => ("KOE", "音声"),
            Self::Auto => ("AUTOMODE", "オートモード"),
            Self::Jitan => ("JITAN", "時短再生"),
            Self::Other => ("ELSE", "その他"),
            Self::System => ("SYSTEM", "システム"),
        }
    }
}

pub struct ConfigDialog {
    pub state: OriginalConfigRuntimeState,
    defaults: OriginalConfigRuntimeState,
    gameexe: GameexeConfig,
    tabs: Vec<Tab>,
    tab: Tab,
    voices: Vec<(usize, String)>,
    remember_tab: bool,
}

impl ConfigDialog {
    pub fn remember_tab_from(&mut self, previous: &Self) {
        if self.remember_tab && self.tabs.contains(&previous.tab) {
            self.tab = previous.tab;
        }
    }

    pub fn new(ctx: &CommandContext) -> Self {
        use crate::runtime::forms::codes::syscom_op::*;
        let gameexe = ctx.tables.gameexe.clone().unwrap_or_default();
        let tabs: Vec<_> = Tab::ALL
            .into_iter()
            .filter(|t| {
                gameexe
                    .get_i64(&format!("DIALOG_TAB_EXIST.{}", t.definition().0))
                    .unwrap_or(1)
                    != 0
            })
            .collect();
        let requested = match ctx.globals.syscom.last_menu_call {
            CALL_CONFIG_WINDOW_MODE_MENU => Tab::Screen,
            CALL_CONFIG_VOLUME_MENU | CALL_CONFIG_BGMFADE_MENU => Tab::Volume,
            CALL_CONFIG_FONT_MENU | CALL_CONFIG_MESSAGE_SPEED_MENU => Tab::Message,
            CALL_CONFIG_FILTER_COLOR_MENU => Tab::Background,
            CALL_CONFIG_KOEMODE_MENU | CALL_CONFIG_CHARAKOE_MENU => Tab::Voice,
            CALL_CONFIG_AUTO_MODE_MENU => Tab::Auto,
            CALL_CONFIG_JITAN_MENU => Tab::Jitan,
            CALL_CONFIG_SYSTEM_MENU | CALL_CONFIG_MOVIE_MENU => Tab::System,
            _ => Tab::Screen,
        };
        let tab = if tabs.contains(&requested) {
            requested
        } else {
            tabs.first().copied().unwrap_or(Tab::Screen)
        };
        let state = syscom::config_state_for_save(ctx);
        let voices = (0..state.chrkoe.len())
            .filter_map(|i| {
                let e = gameexe.get_indexed_entry("CHRKOE", i)?;
                let name = e.item_unquoted(0)?.to_owned();
                if name.is_empty() {
                    return None;
                }
                let label = if syscom::config_chrkoe_name_visible(ctx, i) {
                    name
                } else {
                    gameexe
                        .get_unquoted("CHRKOE.NOT_LOOK_NAME_STR")
                        .unwrap_or("？？？")
                        .to_owned()
                };
                Some((i, label))
            })
            .collect();
        Self {
            state,
            defaults: syscom::original_config_defaults(ctx),
            gameexe,
            tabs,
            tab,
            voices,
            remember_tab: ctx.globals.syscom.last_menu_call == CALL_CONFIG_MENU,
        }
    }

    fn show(&mut self, ctx: &mut egui::Ui) -> bool {
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Panel::bottom("config_footer").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(self.gameexe.get_unquoted("GAMEVERSION").unwrap_or(""));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("閉じる").clicked() {
                        close = true;
                    }
                    if ui.button("全て初期状態に戻す").clicked() {
                        for tab in Tab::ALL {
                            self.reset_tab(tab);
                        }
                    }
                });
            });
            ui.add_space(8.0);
        });
        egui::Panel::top("config_tabs").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for &tab in &self.tabs {
                    ui.selectable_value(&mut self.tab, tab, tab.definition().1);
                }
            });
            ui.add_space(6.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 12.0);
            ui.spacing_mut().slider_width = 270.0;
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.tabs.is_empty() {
                    return;
                }
                match self.tab {
                    Tab::Screen => self.screen(ui),
                    Tab::Volume => self.volume(ui),
                    Tab::Message => self.message(ui),
                    Tab::Background => self.background(ui),
                    Tab::Voice => self.voice(ui),
                    Tab::Auto => self.auto(ui),
                    Tab::Jitan => self.jitan(ui),
                    Tab::Other => self.other(ui),
                    Tab::System => self.system(ui),
                }
                ui.separator();
                if ui.button("初期状態に戻す").clicked() {
                    self.reset_tab(self.tab);
                }
            });
        });
        close
    }

    fn screen(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label("画面モード");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.state.screen_size_mode, 1, "フルスクリーン");
                ui.radio_value(&mut self.state.screen_size_mode, 0, "ウィンドウ");
            });
        });
        ui.add_enabled_ui(self.state.screen_size_mode == 0, |ui| {
            ui.group(|ui| {
                ui.label("ウィンドウサイズ");
                ui.add(
                    egui::Slider::new(&mut self.state.screen_size_scale.0, 50..=200).suffix(" %"),
                );
                self.state.screen_size_scale.1 = self.state.screen_size_scale.0;
                ui.horizontal(|ui| {
                    for size in [50, 75, 100, 125, 150, 200] {
                        if ui.button(format!("{size}%")).clicked() {
                            self.state.screen_size_scale = (size, size);
                        }
                    }
                });
            });
        });
        ui.checkbox(
            &mut self.state.mouse_cursor_hide_onoff,
            "自動でマウスカーソルを隠す",
        );
        ui.add_enabled(
            self.state.mouse_cursor_hide_onoff,
            egui::Slider::new(&mut self.state.mouse_cursor_hide_time, 0..=10000).suffix(" ms"),
        );
    }

    fn volume(&mut self, ui: &mut egui::Ui) {
        ui.label("音量");
        volume_row(
            ui,
            "全体",
            &mut self.state.all_sound_user_volume,
            &mut self.state.play_all_sound_check,
        );
        for (i, (key, label)) in [
            ("BGM", "ＢＧＭ"),
            ("KOE", "音声"),
            ("PCM", "効果音"),
            ("SE", "システム音"),
            ("MOVIE", "ムービー"),
        ]
        .into_iter()
        .enumerate()
        {
            if self
                .gameexe
                .get_i64(&format!("DIALOG_EXIST.{key}"))
                .unwrap_or(1)
                != 0
            {
                volume_row(
                    ui,
                    label,
                    &mut self.state.sound_user_volume[i],
                    &mut self.state.play_sound_check[i],
                );
            }
        }
        if self.gameexe.get_i64("DIALOG_STYLE.VOLUME").unwrap_or(0) != 1 {
            ui.separator();
            ui.checkbox(
                &mut self.state.bgmfade_use_check,
                "音声再生時にＢＧＭの音量を下げる",
            );
            ui.add_enabled(
                self.state.bgmfade_use_check,
                egui::Slider::new(&mut self.state.bgmfade_volume, 0..=255).text("ＢＧＭ音量"),
            );
        }
    }

    fn message(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label("フォント");
            egui::ComboBox::from_id_salt("font_name")
                .selected_text(&self.state.font_name)
                .show_ui(ui, |ui| {
                    let mut names = vec!["ＭＳ ゴシック", "ＭＳ 明朝", "メイリオ"];
                    if !names.contains(&self.defaults.font_name.as_str()) {
                        names.push(&self.defaults.font_name);
                    }
                    for name in names {
                        ui.selectable_value(&mut self.state.font_name, name.to_owned(), name);
                    }
                });
        });
        ui.group(|ui| {
            ui.label("文字速度");
            // The original slider increases left-to-right while stored delay decreases.
            let mut speed = 100 - self.state.message_speed;
            if ui
                .add_enabled(
                    !self.state.message_speed_nowait,
                    egui::Slider::new(&mut speed, 0..=100).text("遅い ← → 速い"),
                )
                .changed()
            {
                self.state.message_speed = 100 - speed;
            }
            ui.checkbox(&mut self.state.message_speed_nowait, "一瞬で表示する");
        });
    }

    fn background(&mut self, ui: &mut egui::Ui) {
        ui.label("メッセージウィンドウ背景色");
        let mut rgba = [
            ((self.state.filter_color_argb >> 16) & 255) as i64,
            ((self.state.filter_color_argb >> 8) & 255) as i64,
            (self.state.filter_color_argb & 255) as i64,
            ((self.state.filter_color_argb >> 24) & 255) as i64,
        ];
        for (v, label) in rgba.iter_mut().zip(["赤", "緑", "青", "不透明度"]) {
            ui.add(egui::Slider::new(v, 0..=255).text(label));
        }
        self.state.filter_color_argb = ((rgba[3] as u32) << 24)
            | ((rgba[0] as u32) << 16)
            | ((rgba[1] as u32) << 8)
            | rgba[2] as u32;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(320.0, 70.0), egui::Sense::hover());
        ui.painter().rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(
                rgba[0] as u8,
                rgba[1] as u8,
                rgba[2] as u8,
                rgba[3] as u8,
            ),
        );
    }

    fn voice(&mut self, ui: &mut egui::Ui) {
        let style = self.gameexe.get_i64("DIALOG_STYLE.KOE").unwrap_or(0);
        if style != 2 {
            ui.label("音声モード");
            for (i, label) in [
                "音声あり（文章あり）",
                "音声なし（文章のみ）",
                "音声あり（文章なし）",
            ]
            .into_iter()
            .enumerate()
            {
                ui.radio_value(&mut self.state.koe_mode, i as i64, label);
            }
        }
        if style != 1 {
            ui.separator();
            ui.label("キャラクター音声");
            for (i, name) in &self.voices {
                if let Some(voice) = self.state.chrkoe.get_mut(*i) {
                    ui.push_id(i, |ui| {
                        volume_row(ui, name, &mut voice.volume, &mut voice.onoff);
                    });
                }
            }
        }
    }

    fn auto(&mut self, ui: &mut egui::Ui) {
        ui.label("オートモード");
        ui.add(
            egui::Slider::new(&mut self.state.auto_mode_moji_wait, 0..=500)
                .suffix(" ms")
                .text("１文字あたりの待ち時間"),
        );
        ui.add(
            egui::Slider::new(&mut self.state.auto_mode_min_wait, 0..=10000)
                .suffix(" ms")
                .text("最小待ち時間"),
        );
    }

    fn jitan(&mut self, ui: &mut egui::Ui) {
        ui.label("時短再生");
        ui.add(
            egui::Slider::new(&mut self.state.jitan_speed, 100..=300)
                .step_by(25.0)
                .suffix(" %"),
        );
        ui.checkbox(
            &mut self.state.jitan_normal_onoff,
            "文章を普通に読み進めている時に使用する",
        );
        ui.checkbox(
            &mut self.state.jitan_auto_mode_onoff,
            "オートモード中に使用する",
        );
        ui.checkbox(
            &mut self.state.jitan_msgbk_onoff,
            "声のリプレイ時の再生速度を変更する",
        );
    }

    fn other(&mut self, ui: &mut egui::Ui) {
        configured_checkbox(
            ui,
            &self.gameexe,
            "MESSAGE_CHRCOLOR",
            true,
            "文章を色分けする。",
            &mut self.state.message_chrcolor_flag,
        );
        for i in 0..4 {
            configured_checkbox(
                ui,
                &self.gameexe,
                &format!("OBJECT_DISP.{i:03}"),
                i < 2,
                &format!("オブジェクト表示{i}番を表示する。"),
                &mut self.state.object_disp_flag[i],
            );
            configured_checkbox(
                ui,
                &self.gameexe,
                &format!("GLOBAL_EXTRA_SWITCH.{i:03}"),
                i < 2,
                &format!("グローバル汎用スイッチ{i}番を使用する。"),
                &mut self.state.global_extra_switch_flag[i],
            );
            let key = format!("DIALOG.GLOBAL_EXTRA_MODE.{i:03}");
            if self.gameexe.get_i64(&format!("{key}.EXIST")).unwrap_or(1) == 0 {
                continue;
            }
            ui.group(|ui| {
                ui.label(
                    self.gameexe
                        .get_unquoted(&format!("{key}.STR"))
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("グローバル汎用モード{i}番")),
                );
                let count = self
                    .gameexe
                    .get_i64(&format!("{key}.ITEM_CNT"))
                    .unwrap_or(if i < 2 { 3 } else { 1 })
                    .clamp(0, 32);
                for j in 0..count {
                    let label = self
                        .gameexe
                        .get_unquoted(&format!("{key}.ITEM.{j:03}.STR"))
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("モード{j}"));
                    ui.radio_value(&mut self.state.global_extra_mode_flag[i], j, label);
                }
            });
        }
    }

    fn system(&mut self, ui: &mut egui::Ui) {
        let c = &mut self.state;
        for (key, label, value) in [
            (
                "SLEEP",
                "本プログラムの動作を遅くして、他のプログラムがスムーズに動作するようにする。",
                &mut c.sleep_flag,
            ),
            (
                "NO_WIPE_ANIME",
                "画面暗転効果のアニメを無効にする。",
                &mut c.no_wipe_anime_flag,
            ),
            (
                "SKIP_WIPE_ANIME",
                "画面暗転効果をマウスクリックで飛ばす。",
                &mut c.skip_wipe_anime_flag,
            ),
            (
                "NO_MWND_ANIME",
                "メッセージウィンドウの開閉時のアニメを無効にする。",
                &mut c.no_mwnd_anime_flag,
            ),
            (
                "WHEEL_NEXT_MESSAGE",
                "マウスのホイールボタンの下回しで文章を読み進める。",
                &mut c.wheel_next_message_flag,
            ),
            (
                "KOE_DONT_STOP",
                "声の再生中に次の文章に進んでも再生を続ける。",
                &mut c.koe_dont_stop_flag,
            ),
            (
                "SKIP_UNREAD_MESSAGE",
                "未読の文章も早送りできるようにする。",
                &mut c.skip_unread_message_flag,
            ),
        ] {
            configured_checkbox(ui, &self.gameexe, key, true, label, value);
        }
        // SiglusEngine 1.1.141.x adds these two fixed system-config checkboxes.
        // They are persisted as the two one-byte fields appended by config.sav v1.4.
        ui.checkbox(
            &mut c.joypad_swap_ab,
            "コントローラーの A / B ボタンを入れ替える。",
        );
        ui.checkbox(
            &mut c.joypad_swap_xy,
            "コントローラーの X / Y ボタンを入れ替える。",
        );
    }

    fn reset_tab(&mut self, tab: Tab) {
        let c = &mut self.state;
        let d = &self.defaults;
        // Reset only controls owned by the page; preserve paths, display inventory and other tabs.
        match tab {
            Tab::Screen => {
                c.screen_size_mode = d.screen_size_mode;
                c.screen_size_scale = d.screen_size_scale;
                c.mouse_cursor_hide_onoff = d.mouse_cursor_hide_onoff;
                c.mouse_cursor_hide_time = d.mouse_cursor_hide_time;
            }
            Tab::Volume => {
                c.all_sound_user_volume = d.all_sound_user_volume;
                c.sound_user_volume = d.sound_user_volume;
                c.play_all_sound_check = d.play_all_sound_check;
                c.play_sound_check = d.play_sound_check;
                c.bgmfade_use_check = d.bgmfade_use_check;
                c.bgmfade_volume = d.bgmfade_volume;
            }
            Tab::Message => {
                c.font_name = d.font_name.clone();
                c.font_futoku = d.font_futoku;
                c.font_shadow = d.font_shadow;
                c.message_speed = d.message_speed;
                c.message_speed_nowait = d.message_speed_nowait;
            }
            Tab::Background => c.filter_color_argb = d.filter_color_argb,
            Tab::Voice => {
                c.koe_mode = d.koe_mode;
                c.chrkoe = d.chrkoe.clone();
            }
            Tab::Auto => {
                c.auto_mode_moji_wait = d.auto_mode_moji_wait;
                c.auto_mode_min_wait = d.auto_mode_min_wait;
            }
            Tab::Jitan => {
                c.jitan_normal_onoff = d.jitan_normal_onoff;
                c.jitan_auto_mode_onoff = d.jitan_auto_mode_onoff;
                c.jitan_msgbk_onoff = d.jitan_msgbk_onoff;
                c.jitan_speed = d.jitan_speed;
            }
            Tab::Other => {
                c.message_chrcolor_flag = d.message_chrcolor_flag;
                c.object_disp_flag = d.object_disp_flag.clone();
                c.global_extra_switch_flag = d.global_extra_switch_flag.clone();
                c.global_extra_mode_flag = d.global_extra_mode_flag.clone();
            }
            Tab::System => {
                c.sleep_flag = d.sleep_flag;
                c.no_wipe_anime_flag = d.no_wipe_anime_flag;
                c.skip_wipe_anime_flag = d.skip_wipe_anime_flag;
                c.no_mwnd_anime_flag = d.no_mwnd_anime_flag;
                c.wheel_next_message_flag = d.wheel_next_message_flag;
                c.koe_dont_stop_flag = d.koe_dont_stop_flag;
                c.skip_unread_message_flag = d.skip_unread_message_flag;
                c.joypad_swap_ab = d.joypad_swap_ab;
                c.joypad_swap_xy = d.joypad_swap_xy;
            }
        }
    }
}

fn volume_row(ui: &mut egui::Ui, label: &str, volume: &mut i64, enabled: &mut bool) {
    ui.horizontal(|ui| {
        ui.add_sized([90.0, 20.0], egui::Label::new(label));
        ui.add_enabled(*enabled, egui::Slider::new(volume, 0..=255));
        ui.checkbox(enabled, "再生");
    });
}

fn configured_checkbox(
    ui: &mut egui::Ui,
    gameexe: &GameexeConfig,
    key: &str,
    default_exist: bool,
    default_label: &str,
    value: &mut bool,
) {
    if gameexe
        .get_i64(&format!("DIALOG.{key}.EXIST"))
        .unwrap_or(default_exist as i64)
        != 0
    {
        ui.checkbox(
            value,
            gameexe
                .get_unquoted(&format!("DIALOG.{key}.STR"))
                .unwrap_or(default_label),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::forms::codes::syscom_op::*;

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a desktop session and GPU"]
    fn native_config_close_releases_window_and_surface() {
        use winit::application::ApplicationHandler;
        use winit::event_loop::EventLoop;
        use winit::platform::wayland::EventLoopBuilderExtWayland;

        struct Probe;
        impl ApplicationHandler for Probe {
            fn can_create_surfaces(&mut self, elwt: &dyn ActiveEventLoop) {
                let ctx =
                    CommandContext::new(std::env::temp_dir().join("siglus-config-close-test"));
                let mut dialog = ConfigDialog::new(&ctx);
                for _ in 0..2 {
                    let mut window = DesktopConfigWindow::new(elwt, dialog).unwrap();
                    let weak = Arc::downgrade(&window.window);
                    window.render().unwrap();
                    assert!(matches!(
                        window.handle_window_event(WindowEvent::CloseRequested),
                        Some(DesktopConfigAction::Close)
                    ));
                    dialog = window.into_dialog();
                    assert!(weak.upgrade().is_none(), "native window or surface leaked");
                }
                elwt.exit();
            }

            fn window_event(&mut self, _: &dyn ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        }

        let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
        event_loop.run_app(Probe).unwrap();
    }

    #[test]
    fn recreated_dialog_remembers_tab_without_reusing_stale_settings() {
        let mut ctx = CommandContext::new(std::env::temp_dir().join("siglus-config-reopen-test"));
        ctx.globals.syscom.last_menu_call = CALL_CONFIG_MENU;
        let mut previous = ConfigDialog::new(&ctx);
        previous.tab = Tab::Volume;
        ctx.globals
            .syscom
            .config_int
            .insert(GET_WINDOW_MODE_SIZE, 75);
        let mut reopened = ConfigDialog::new(&ctx);
        reopened.remember_tab_from(&previous);
        assert_eq!(reopened.tab, Tab::Volume);
        assert_eq!(reopened.state.screen_size_scale, (75, 75));

        ctx.globals.syscom.last_menu_call = CALL_CONFIG_WINDOW_MODE_MENU;
        let mut requested = ConfigDialog::new(&ctx);
        requested.remember_tab_from(&previous);
        assert_eq!(requested.tab, Tab::Screen);

        ctx.globals.syscom.last_menu_call = CALL_CONFIG_FONT_MENU;
        let mut font = ConfigDialog::new(&ctx);
        font.remember_tab_from(&previous);
        assert_eq!(font.tab, Tab::Message);

        ctx.globals.syscom.last_menu_call = CALL_CONFIG_MENU;
        ctx.tables.gameexe = Some(GameexeConfig::from_text("#DIALOG_TAB_EXIST.VOLUME=0"));
        let mut hidden = ConfigDialog::new(&ctx);
        hidden.remember_tab_from(&previous);
        assert_eq!(hidden.tab, Tab::Screen);
    }

    #[test]
    fn game_definitions_control_tabs_voice_names_and_reset_scope() {
        let mut ctx = CommandContext::new(std::env::temp_dir().join("siglus-config-ui-test"));
        ctx.tables.gameexe = Some(GameexeConfig::from_text(
            r#"
#DIALOG_TAB_EXIST.ELSE=0
#DIALOG_TAB_EXIST.ELSE=1
#DIALOG_TAB_EXIST.JITAN=0
#CONFIG.VOLUME.BGM=165
#CONFIG.GLOBAL_EXTRA_SWITCH.000.ONOFF=0
#CHRKOE.000="First",1,"First",1,235,(0)
#CHRKOE.001="Hidden",2,"Alias",1,210,(1)
#CHRKOE.NOT_LOOK_NAME_STR="???"
"#,
        ));
        syscom::apply_config_dialog_state(&mut ctx, OriginalConfigRuntimeState::default());
        // Script commands can have newer values than original_config.
        ctx.globals.syscom.config_int.insert(GET_BGM_VOLUME, 77);
        ctx.globals.syscom.last_menu_call = CALL_CONFIG_VOLUME_MENU;
        let mut dialog = ConfigDialog::new(&ctx);
        assert_eq!(dialog.tab, Tab::Volume);
        assert!(dialog.tabs.contains(&Tab::Other));
        assert!(!dialog.tabs.contains(&Tab::Jitan));
        assert_eq!(dialog.voices, vec![(0, "First".into()), (1, "???".into())]);
        assert_eq!(dialog.state.sound_user_volume[0], 77);
        dialog.state.editor_path = "keep this path".into();
        dialog.state.message_speed = 42;
        dialog.reset_tab(Tab::Volume);
        assert_eq!(dialog.state.sound_user_volume[0], 165);
        assert_eq!(dialog.state.message_speed, 42);
        assert_eq!(dialog.state.editor_path, "keep this path");
        dialog.reset_tab(Tab::Other);
        assert!(!dialog.state.global_extra_switch_flag[0]);
        syscom::reveal_config_voice_name(&mut ctx, "Alias");
        assert_eq!(ConfigDialog::new(&ctx).voices[1].1, "Hidden");

        // Exercise layout for each page without a GPU or game assets.
        let egui = egui::Context::default();
        for tab in Tab::ALL {
            dialog.tab = tab;
            let mut output = egui.run_ui(egui::RawInput::default(), |ctx| {
                dialog.show(ctx);
            });
            // This layout-only test has no renderer to consume texture updates.
            output.textures_delta.clear();
        }
    }
}
