//! Desktop winit message box support.
//!
//! Desktop ports keep modal dialogs inside the winit event loop. Mobile ports use
//! the native callback path in `host.rs`.

#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::render::Renderer;
use crate::runtime::native_ui::{NativeMessageBoxRequest, NativeUiBackend};

fn configure_egui_default_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "siglus_default".to_string(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/default.ttf")).into(),
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

#[derive(Clone, Default)]
pub struct DesktopMessageBoxBridge {
    queue: Arc<Mutex<VecDeque<NativeMessageBoxRequest>>>,
}

impl DesktopMessageBoxBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn backend(&self) -> Arc<dyn NativeUiBackend> {
        Arc::new(DesktopMessageBoxBackend {
            queue: Arc::clone(&self.queue),
        })
    }

    pub fn pop_request(&self) -> Option<NativeMessageBoxRequest> {
        self.queue.lock().ok().and_then(|mut q| q.pop_front())
    }
}

struct DesktopMessageBoxBackend {
    queue: Arc<Mutex<VecDeque<NativeMessageBoxRequest>>>,
}

impl NativeUiBackend for DesktopMessageBoxBackend {
    fn show_system_messagebox(&self, request: NativeMessageBoxRequest) {
        if let Ok(mut q) = self.queue.lock() {
            q.push_back(request);
        } else {
            log::error!("desktop messagebox queue lock failed");
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ButtonRect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl ButtonRect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

pub struct DesktopMessageBoxWindow {
    request: NativeMessageBoxRequest,
    window: &'static Window,
    window_id: WindowId,
    renderer: Renderer,
    egui_renderer: EguiRenderer,
    egui_ctx: egui::Context,
    start_time: Instant,
    selected: usize,
    cursor_pos: Option<(f32, f32)>,
}

impl DesktopMessageBoxWindow {
    pub fn new(elwt: &ActiveEventLoop, request: NativeMessageBoxRequest) -> Result<Self> {
        let button_count = request.buttons.len().max(1) as f64;
        let width = (420.0f64).max(220.0 + button_count * 112.0);
        let height = 190.0f64;
        let title = if request.title.trim().is_empty() {
            "Siglus".to_string()
        } else {
            request.title.clone()
        };
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_title(title)
                    .with_inner_size(LogicalSize::new(width, height))
                    .with_min_inner_size(LogicalSize::new(360.0, 160.0))
                    .with_resizable(false),
            )
            .context("create desktop messagebox window")?;
        let window: &'static Window = Box::leak(Box::new(window));
        let renderer = pollster::block_on(Renderer::new(window)).context("messagebox renderer init")?;
        let egui_renderer = EguiRenderer::new(&renderer.device, renderer.config.format, None, 1);
        let egui_ctx = egui::Context::default();
        configure_egui_default_font(&egui_ctx);
        let selected = request.buttons.len().saturating_sub(1).min(1);
        window.request_redraw();
        Ok(Self {
            request,
            window_id: window.id(),
            window,
            renderer,
            egui_renderer,
            egui_ctx,
            start_time: Instant::now(),
            selected,
            cursor_pos: None,
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    pub fn request_id(&self) -> u64 {
        self.request.request_id
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn cancel_value(&self) -> i64 {
        self.request
            .buttons
            .last()
            .map(|button| button.value)
            .unwrap_or(0)
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) -> Option<i64> {
        match event {
            WindowEvent::CloseRequested => Some(self.cancel_value()),
            WindowEvent::Resized(size) => {
                self.renderer.resize(size.width.max(1), size.height.max(1));
                self.window.request_redraw();
                None
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = self.logical_pos(position);
                self.cursor_pos = Some(pos);
                if let Some(idx) = self.hit_test_button(pos.0, pos.1) {
                    self.selected = idx;
                }
                self.window.request_redraw();
                None
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let pos = self.cursor_pos?;
                let idx = self.hit_test_button(pos.0, pos.1)?;
                self.selected = idx;
                self.request.buttons.get(idx).map(|button| button.value)
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(code),
                        ..
                    },
                ..
            } => self.handle_key(code),
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.render() {
                    log::error!("desktop messagebox render failed: {err:#}");
                }
                None
            }
            _ => None,
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> Option<i64> {
        match code {
            KeyCode::Escape => Some(self.cancel_value()),
            KeyCode::Enter | KeyCode::Space => self
                .request
                .buttons
                .get(self.selected.min(self.request.buttons.len().saturating_sub(1)))
                .map(|button| button.value),
            KeyCode::ArrowLeft | KeyCode::ArrowUp => {
                let len = self.request.buttons.len();
                if len > 0 {
                    self.selected = if self.selected == 0 { len - 1 } else { self.selected - 1 };
                    self.window.request_redraw();
                }
                None
            }
            KeyCode::ArrowRight | KeyCode::ArrowDown | KeyCode::Tab => {
                let len = self.request.buttons.len();
                if len > 0 {
                    self.selected = (self.selected + 1) % len;
                    self.window.request_redraw();
                }
                None
            }
            _ => None,
        }
    }

    fn logical_pos(&self, position: PhysicalPosition<f64>) -> (f32, f32) {
        let p = position.to_logical::<f64>(self.window.scale_factor());
        (p.x as f32, p.y as f32)
    }

    fn button_rects_for_size(&self, logical_w: f32, logical_h: f32) -> Vec<ButtonRect> {
        let count = self.request.buttons.len().max(1);
        let button_w = 96.0f32;
        let button_h = 32.0f32;
        let gap = 12.0f32;
        let total_w = count as f32 * button_w + count.saturating_sub(1) as f32 * gap;
        let mut x = ((logical_w - total_w) * 0.5).max(16.0);
        let y = (logical_h - 52.0).max(104.0);
        let mut rects = Vec::with_capacity(count);
        for _ in 0..count {
            rects.push(ButtonRect {
                x0: x,
                y0: y,
                x1: x + button_w,
                y1: y + button_h,
            });
            x += button_w + gap;
        }
        rects
    }

    fn hit_test_button(&self, x: f32, y: f32) -> Option<usize> {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor() as f32;
        let logical_w = size.width as f32 / scale.max(1.0);
        let logical_h = size.height as f32 / scale.max(1.0);
        self.button_rects_for_size(logical_w, logical_h)
            .into_iter()
            .position(|rect| rect.contains(x, y))
    }

    fn render(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let scale = self.window.scale_factor() as f32;
        self.egui_ctx.set_pixels_per_point(scale);
        let logical_w = size.width as f32 / scale.max(1.0);
        let logical_h = size.height as f32 / scale.max(1.0);
        let button_rects = self.button_rects_for_size(logical_w, logical_h);
        let message = self.request.message.clone();
        let title = self.request.title.clone();
        let buttons = self.request.buttons.clone();
        let selected = self.selected;
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical_w, logical_h),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            ..Default::default()
        };
        let output = self.egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::default()
                        .fill(egui::Color32::from_rgb(246, 247, 250))
                        .inner_margin(egui::Margin::same(18.0)),
                )
                .show(ctx, |ui| {
                    let full = ui.max_rect();
                    let painter = ui.painter();
                    let icon_rect = egui::Rect::from_min_size(
                        egui::pos2(full.left() + 4.0, full.top() + 12.0),
                        egui::vec2(36.0, 36.0),
                    );
                    painter.circle_filled(
                        icon_rect.center(),
                        18.0,
                        egui::Color32::from_rgb(66, 133, 244),
                    );
                    painter.text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "?",
                        egui::FontId::proportional(24.0),
                        egui::Color32::WHITE,
                    );

                    let text_x = icon_rect.right() + 16.0;
                    let title_text = if title.trim().is_empty() { "Siglus" } else { title.as_str() };
                    painter.text(
                        egui::pos2(text_x, full.top() + 8.0),
                        egui::Align2::LEFT_TOP,
                        title_text,
                        egui::FontId::proportional(16.0),
                        egui::Color32::from_rgb(35, 38, 42),
                    );
                    painter.text(
                        egui::pos2(text_x, full.top() + 42.0),
                        egui::Align2::LEFT_TOP,
                        message,
                        egui::FontId::proportional(18.0),
                        egui::Color32::from_rgb(20, 22, 25),
                    );

                    for (idx, rect) in button_rects.iter().enumerate() {
                        let r = egui::Rect::from_min_max(
                            egui::pos2(rect.x0, rect.y0),
                            egui::pos2(rect.x1, rect.y1),
                        );
                        let is_selected = idx == selected;
                        let fill = if is_selected {
                            egui::Color32::from_rgb(43, 107, 235)
                        } else {
                            egui::Color32::from_rgb(255, 255, 255)
                        };
                        let stroke = if is_selected {
                            egui::Stroke::new(1.5, egui::Color32::from_rgb(28, 86, 210))
                        } else {
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(166, 174, 186))
                        };
                        painter.rect_filled(r, egui::Rounding::same(4.0), fill);
                        painter.line_segment([r.left_top(), r.right_top()], stroke);
                        painter.line_segment([r.right_top(), r.right_bottom()], stroke);
                        painter.line_segment([r.right_bottom(), r.left_bottom()], stroke);
                        painter.line_segment([r.left_bottom(), r.left_top()], stroke);
                        let label = buttons
                            .get(idx)
                            .map(|button| button.label.as_str())
                            .unwrap_or("OK");
                        let text_color = if is_selected {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_rgb(25, 27, 30)
                        };
                        painter.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(15.0),
                            text_color,
                        );
                    }
                });
        });

        let screen_desc = ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: scale,
        };
        let paint_jobs = self.egui_ctx.tessellate(output.shapes, scale);
        for (id, delta) in &output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.renderer.device, &self.renderer.queue, *id, delta);
        }

        let frame = match self.renderer.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.renderer.resize(self.renderer.config.width, self.renderer.config.height);
                return Ok(());
            }
            Err(wgpu::SurfaceError::OutOfMemory) => anyhow::bail!("messagebox surface out of memory"),
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus_messagebox_egui_encoder"),
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
                label: Some("siglus_messagebox_egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
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
            self.egui_renderer.render(&mut pass, &paint_jobs, &screen_desc);
        }
        self.renderer.queue.submit(Some(encoder.finish()));
        frame.present();
        for id in output.textures_delta.free {
            self.egui_renderer.free_texture(&id);
        }
        Ok(())
    }
}
