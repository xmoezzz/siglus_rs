//! Desktop Twitter dialog used by the Siglus Twitter subsystem.

#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::render::Renderer;
use crate::runtime::twitter::TwitterDialogRequest;

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

#[derive(Debug, Clone)]
pub enum DesktopTwitterAction {
    Authorize,
    CompleteAuthorize(String),
    Tweet(String),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogNotice {
    None,
    EmptyTweetConfirm,
    AuthSuccess,
    TweetSuccess,
    Error,
}

pub struct DesktopTwitterWindow {
    request: TwitterDialogRequest,
    // The GPU surface owns another Arc; dropping both destroys the native window.
    window: Arc<dyn Window>,
    window_id: WindowId,
    renderer: Renderer,
    egui_renderer: EguiRenderer,
    egui_ctx: egui::Context,
    preview: egui::TextureHandle,
    start_time: Instant,
    input_events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    pointer_pos: egui::Pos2,
    tweet_text: String,
    callback_text: String,
    authorized: bool,
    account_label: String,
    authorization_entry: bool,
    notice: DialogNotice,
    notice_text: String,
}

impl DesktopTwitterWindow {
    pub fn new(elwt: &dyn ActiveEventLoop, request: TwitterDialogRequest) -> Result<Self> {
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_title("Twitter")
                    .with_surface_size(LogicalSize::new(620.0, 520.0))
                    .with_min_surface_size(LogicalSize::new(520.0, 440.0)),
            )
            .context("create desktop Twitter window")?;
        let window: Arc<dyn Window> = Arc::from(window);
        crate::ime::enable_ime(
            window.as_ref(),
            LogicalPosition::new(0, 0).into(),
            LogicalSize::new(0, 0).into(),
        );
        let renderer =
            pollster::block_on(Renderer::new(window.clone())).context("Twitter renderer init")?;
        let egui_renderer = EguiRenderer::new(
            &renderer.device,
            renderer.config.format,
            egui_wgpu::RendererOptions::default(),
        );
        let egui_ctx = egui::Context::default();
        configure_egui_default_font(&egui_ctx);
        let preview_image = egui::ColorImage::from_rgba_unmultiplied(
            [
                request.image_width.max(1) as usize,
                request.image_height.max(1) as usize,
            ],
            &request.image_rgba,
        );
        let preview = egui_ctx.load_texture(
            "siglus_tweet_preview",
            preview_image,
            egui::TextureOptions::LINEAR,
        );
        let tweet_text = request.initial_text.clone();
        window.request_redraw();
        Ok(Self {
            request,
            window_id: window.id(),
            window,
            renderer,
            egui_renderer,
            egui_ctx,
            preview,
            start_time: Instant::now(),
            input_events: Vec::new(),
            modifiers: egui::Modifiers::default(),
            pointer_pos: egui::Pos2::ZERO,
            tweet_text,
            callback_text: String::new(),
            authorized: false,
            account_label: String::new(),
            authorization_entry: false,
            notice: DialogNotice::None,
            notice_text: String::new(),
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn image_path(&self) -> &std::path::Path {
        &self.request.image_path
    }

    pub fn set_account_state(&mut self, authorized: bool, user_name: &str, screen_name: &str) {
        self.authorized = authorized;
        self.account_label = if authorized {
            format!("{} (@{})", user_name, screen_name)
        } else {
            "投稿するには認証が必要です。".to_string()
        };
        self.window.request_redraw();
    }

    pub fn show_authorization_entry(&mut self) {
        self.authorization_entry = true;
        self.callback_text.clear();
        self.notice = DialogNotice::None;
        self.notice_text.clear();
        self.window.request_redraw();
    }

    pub fn authentication_succeeded(&mut self) {
        self.authorization_entry = false;
        self.callback_text.clear();
        self.notice = DialogNotice::AuthSuccess;
        self.notice_text = "認証に成功しました。".to_string();
        self.window.request_redraw();
    }

    pub fn tweet_succeeded(&mut self) {
        self.tweet_text.clear();
        self.notice = DialogNotice::TweetSuccess;
        self.notice_text = "投稿しました。".to_string();
        self.window.request_redraw();
    }

    pub fn show_error(&mut self, text: impl Into<String>) {
        self.notice = DialogNotice::Error;
        self.notice_text = text.into();
        self.window.request_redraw();
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) -> Option<DesktopTwitterAction> {
        match event {
            WindowEvent::CloseRequested => return Some(DesktopTwitterAction::Close),
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
                if self.modifiers.command {
                    match (event.state, event.physical_key) {
                        (ElementState::Pressed, PhysicalKey::Code(KeyCode::KeyC)) => {
                            self.input_events.push(egui::Event::Copy);
                        }
                        (ElementState::Pressed, PhysicalKey::Code(KeyCode::KeyX)) => {
                            self.input_events.push(egui::Event::Cut);
                        }
                        (ElementState::Pressed, PhysicalKey::Code(KeyCode::KeyV)) => {
                            match read_system_clipboard() {
                                Ok(text) if !text.is_empty() => {
                                    self.input_events.push(egui::Event::Paste(text));
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    log::debug!("desktop Twitter clipboard read failed: {err:#}");
                                }
                            }
                        }
                        _ => {}
                    }
                }
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
                Err(err) => log::error!("desktop Twitter render failed: {err:#}"),
            },
            _ => {}
        }
        None
    }

    fn render(&mut self) -> Result<Option<DesktopTwitterAction>> {
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

        let mut action = None;
        let preview_id = self.preview.id();
        let preview_w = self.request.image_width.max(1) as f32;
        let preview_h = self.request.image_height.max(1) as f32;
        let authorized = self.authorized;
        let account_label = self.account_label.clone();
        let egui_ctx = self.egui_ctx.clone();
        let output = egui_ctx.run_ui(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Twitter");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(&account_label);
                    if ui.button("認証").clicked() {
                        action = Some(DesktopTwitterAction::Authorize);
                    }
                });
                ui.add_space(8.0);

                let available_w = ui.available_width();
                let canvas_h = (ui.available_height() * 0.58).clamp(180.0, 300.0);
                let (canvas_rect, _) = ui.allocate_exact_size(
                    egui::vec2(available_w, canvas_h),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(canvas_rect, 0.0, egui::Color32::GRAY);
                let scale = (canvas_rect.width() / preview_w)
                    .min(canvas_rect.height() / preview_h)
                    .max(0.0001);
                let draw_size = egui::vec2(preview_w * scale, preview_h * scale);
                let image_rect = egui::Rect::from_center_size(canvas_rect.center(), draw_size);
                ui.put(
                    image_rect,
                    egui::Image::new((preview_id, draw_size)),
                );

                ui.add_space(8.0);
                ui.add_enabled(
                    authorized,
                    egui::TextEdit::multiline(&mut self.tweet_text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(authorized, egui::Button::new("Twitter へ投稿")).clicked() {
                        if self.tweet_text.is_empty() {
                            self.notice = DialogNotice::EmptyTweetConfirm;
                            self.notice_text = "メッセージが空です。投稿してよろしいですか？".to_string();
                        } else {
                            action = Some(DesktopTwitterAction::Tweet(self.tweet_text.clone()));
                        }
                    }
                    if ui.button("閉じる").clicked() {
                        action = Some(DesktopTwitterAction::Close);
                    }
                });
            });

            if self.authorization_entry {
                egui::Window::new("Twitter 認証")
                    .collapsible(false)
                    .resizable(true)
                    .default_width(500.0)
                    .show(ctx, |ui| {
                        ui.label("ブラウザで認証後、callback URL または oauth_verifier を貼り付けてください。");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.callback_text)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("クリップボードから貼り付け").clicked() {
                                match read_system_clipboard() {
                                    Ok(text) => self.callback_text = text.trim().to_string(),
                                    Err(err) => {
                                        self.notice = DialogNotice::Error;
                                        self.notice_text = format!("クリップボードを読み取れませんでした: {err:#}");
                                    }
                                }
                            }
                            if ui.button("認証を完了").clicked() {
                                action = Some(DesktopTwitterAction::CompleteAuthorize(
                                    self.callback_text.clone(),
                                ));
                            }
                            if ui.button("閉じる").clicked() {
                                self.authorization_entry = false;
                            }
                        });
                    });
            }

            match self.notice {
                DialogNotice::EmptyTweetConfirm => {
                    egui::Window::new("Twitter")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                        .show(ctx, |ui| {
                            ui.label(self.notice_text.clone());
                            ui.horizontal(|ui| {
                                if ui.button("OK").clicked() {
                                    self.notice = DialogNotice::None;
                                    action = Some(DesktopTwitterAction::Tweet(String::new()));
                                }
                                if ui.button("キャンセル").clicked() {
                                    self.notice = DialogNotice::None;
                                }
                            });
                        });
                }
                DialogNotice::AuthSuccess | DialogNotice::Error => {
                    egui::Window::new("Twitter")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                        .show(ctx, |ui| {
                            ui.label(self.notice_text.clone());
                            if ui.button("OK").clicked() {
                                self.notice = DialogNotice::None;
                            }
                        });
                }
                DialogNotice::TweetSuccess => {
                    egui::Window::new("Twitter")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                        .show(ctx, |ui| {
                            ui.label(self.notice_text.clone());
                            if ui.button("OK").clicked() {
                                action = Some(DesktopTwitterAction::Close);
                            }
                        });
                }
                DialogNotice::None => {}
            }
        });

        for command in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                if let Err(err) = write_system_clipboard(text) {
                    log::debug!("desktop Twitter clipboard write failed: {err:#}");
                }
            }
        }

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
                anyhow::bail!("Twitter surface validation failed")
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
                    label: Some("siglus_twitter_egui_encoder"),
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
                label: Some("siglus_twitter_egui_pass"),
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

fn read_command_stdout(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("run {program} for clipboard access"))?;
    if !output.status.success() {
        anyhow::bail!("{program} exited with {}", output.status);
    }
    String::from_utf8(output.stdout).context("clipboard text is not valid UTF-8")
}

#[cfg(target_os = "macos")]
fn read_system_clipboard() -> Result<String> {
    read_command_stdout("pbpaste", &[])
}

#[cfg(target_os = "windows")]
fn read_system_clipboard() -> Result<String> {
    read_command_stdout(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Get-Clipboard -Raw",
        ],
    )
}

#[cfg(target_os = "linux")]
fn read_system_clipboard() -> Result<String> {
    let candidates: &[(&str, &[&str])] = &[
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
    ];
    let mut errors = Vec::new();
    for (program, args) in candidates {
        match read_command_stdout(program, args) {
            Ok(text) => return Ok(text),
            Err(err) => errors.push(format!("{program}: {err:#}")),
        }
    }
    anyhow::bail!(
        "no usable Wayland/X11 clipboard reader was found ({})",
        errors.join("; ")
    )
}

fn write_command_stdin(program: &str, args: &[&str], text: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("run {program} for clipboard access"))?;
    child
        .stdin
        .as_mut()
        .context("clipboard writer stdin is unavailable")?
        .write_all(text.as_bytes())
        .context("write clipboard text")?;
    let output = child
        .wait_with_output()
        .context("wait for clipboard writer")?;
    if !output.status.success() {
        anyhow::bail!(
            "{program} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn write_system_clipboard(text: &str) -> Result<()> {
    write_command_stdin("pbcopy", &[], text)
}

#[cfg(target_os = "windows")]
pub(crate) fn write_system_clipboard(text: &str) -> Result<()> {
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Set-Clipboard -Value $env:SIGLUS_CLIPBOARD_TEXT",
        ])
        .env("SIGLUS_CLIPBOARD_TEXT", text)
        .status()
        .context("run powershell.exe for clipboard access")?;
    if !status.success() {
        anyhow::bail!("powershell.exe exited with {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn write_system_clipboard(text: &str) -> Result<()> {
    let candidates: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard", "-i"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    let mut errors = Vec::new();
    for (program, args) in candidates {
        match write_command_stdin(program, args, text) {
            Ok(()) => return Ok(()),
            Err(err) => errors.push(format!("{program}: {err:#}")),
        }
    }
    anyhow::bail!(
        "no usable Wayland/X11 clipboard writer was found ({})",
        errors.join("; ")
    )
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
