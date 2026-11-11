//! Desktop implementation of the original Chihaya benchmark information dialog.
//!
//! `eng_chihaya.cpp::C_info_dialog` is a modal, resizable window with a read-only
//! multi-line text area, Copy-to-Clipboard, and Close.  Keep it separate from the
//! generic Siglus message box because the benchmark depends on the modal call
//! boundary and exposes the complete result text for copying.

#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::desktop_twitter::write_system_clipboard;
use crate::render::Renderer;
use crate::runtime::native_ui::NativeChihayaBenchDialogRequest;

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

    fn egui_rect(self) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(self.x0, self.y0), egui::pos2(self.x1, self.y1))
    }
}

pub struct DesktopChihayaBenchWindow {
    request: NativeChihayaBenchDialogRequest,
    window: Arc<dyn Window>,
    window_id: WindowId,
    renderer: Renderer,
    egui_renderer: EguiRenderer,
    egui_ctx: egui::Context,
    start_time: Instant,
    cursor_pos: Option<(f32, f32)>,
    selected: usize,
    scroll_line: usize,
    notice: Option<String>,
}

impl DesktopChihayaBenchWindow {
    pub fn new(
        elwt: &dyn ActiveEventLoop,
        request: NativeChihayaBenchDialogRequest,
    ) -> Result<Self> {
        let title = if request.title.trim().is_empty() {
            "Siglus".to_string()
        } else {
            request.title.clone()
        };
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_title(title)
                    .with_surface_size(LogicalSize::new(680.0, 460.0))
                    .with_min_surface_size(LogicalSize::new(440.0, 300.0))
                    .with_resizable(true),
            )
            .context("create Chihaya benchmark information window")?;
        let window: Arc<dyn Window> = Arc::from(window);
        let renderer = pollster::block_on(Renderer::new(window.clone()))
            .context("Chihaya benchmark dialog renderer init")?;
        let egui_renderer = EguiRenderer::new(
            &renderer.device,
            renderer.config.format,
            egui_wgpu::RendererOptions::default(),
        );
        let egui_ctx = egui::Context::default();
        configure_egui_default_font(&egui_ctx);
        window.request_redraw();
        Ok(Self {
            request,
            window_id: window.id(),
            window,
            renderer,
            egui_renderer,
            egui_ctx,
            start_time: Instant::now(),
            cursor_pos: None,
            selected: 1, // original dialog gives Close the default-button role.
            scroll_line: 0,
            notice: None,
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window_id
    }

    pub fn request_id(&self) -> u64 {
        self.request.request_id
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) -> Option<i64> {
        match event {
            WindowEvent::CloseRequested => Some(0),
            WindowEvent::SurfaceResized(size) => {
                self.renderer.resize(size.width.max(1), size.height.max(1));
                self.clamp_scroll();
                self.window.request_redraw();
                None
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
                let pos = self.logical_pos(position);
                self.cursor_pos = Some(pos);
                if self.notice.is_none()
                    && let Some(index) = self.hit_test_button(pos.0, pos.1)
                {
                    self.selected = index;
                }
                self.window.request_redraw();
                None
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.notice.is_none() {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => (-y * 3.0).round() as i32,
                        MouseScrollDelta::PixelDelta(p) => (-p.y / 22.0).round() as i32,
                        _ => 0,
                    };
                    self.adjust_scroll(lines);
                }
                None
            }
            WindowEvent::PointerButton {
                state: ElementState::Released,
                button,
                position,
                primary: true,
                ..
            } if button.clone().mouse_button() == Some(MouseButton::Left) => {
                self.cursor_pos = Some(self.logical_pos(position));
                if self.notice.is_some() {
                    self.notice = None;
                    self.window.request_redraw();
                    return None;
                }
                let (x, y) = self.cursor_pos?;
                match self.hit_test_button(x, y) {
                    Some(0) => {
                        self.copy_to_clipboard();
                        None
                    }
                    Some(1) => Some(0),
                    _ => None,
                }
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
                    log::error!("Chihaya benchmark dialog render failed: {err:#}");
                }
                None
            }
            _ => None,
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> Option<i64> {
        if self.notice.is_some() {
            if matches!(code, KeyCode::Escape | KeyCode::Enter | KeyCode::Space) {
                self.notice = None;
                self.window.request_redraw();
            }
            return None;
        }
        match code {
            KeyCode::Escape => Some(0),
            KeyCode::Enter | KeyCode::Space => {
                if self.selected == 0 {
                    self.copy_to_clipboard();
                    None
                } else {
                    Some(0)
                }
            }
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::Tab => {
                self.selected ^= 1;
                self.window.request_redraw();
                None
            }
            KeyCode::ArrowUp => {
                self.adjust_scroll(-1);
                None
            }
            KeyCode::ArrowDown => {
                self.adjust_scroll(1);
                None
            }
            KeyCode::PageUp => {
                self.adjust_scroll(-(self.visible_line_count() as i32));
                None
            }
            KeyCode::PageDown => {
                self.adjust_scroll(self.visible_line_count() as i32);
                None
            }
            KeyCode::Home => {
                self.scroll_line = 0;
                self.window.request_redraw();
                None
            }
            KeyCode::End => {
                self.scroll_line = self.request.text.lines().count();
                self.clamp_scroll();
                self.window.request_redraw();
                None
            }
            _ => None,
        }
    }

    fn copy_to_clipboard(&mut self) {
        self.notice = Some(match write_system_clipboard(&self.request.text) {
            Ok(()) => "情報をクリップボードにコピーしました。\nメモ帳などのエディタで「貼り付け」または「ペースト」で情報を貼り付けることができます。".to_string(),
            Err(err) => {
                log::error!("Chihaya benchmark clipboard copy failed: {err:#}");
                format!("クリップボードへのコピーに失敗しました。\n{err:#}")
            }
        });
        self.window.request_redraw();
    }

    fn logical_pos(&self, position: PhysicalPosition<f64>) -> (f32, f32) {
        let p = position.to_logical::<f64>(self.window.scale_factor());
        (p.x as f32, p.y as f32)
    }

    fn logical_size(&self) -> (f32, f32) {
        let size = self.window.surface_size();
        let scale = self.window.scale_factor() as f32;
        (
            size.width as f32 / scale.max(1.0),
            size.height as f32 / scale.max(1.0),
        )
    }

    fn button_rects(&self) -> [ButtonRect; 2] {
        let (w, h) = self.logical_size();
        let y0 = (h - 52.0).max(220.0);
        [
            ButtonRect {
                x0: 18.0,
                y0,
                x1: 226.0,
                y1: y0 + 34.0,
            },
            ButtonRect {
                x0: (w - 116.0).max(250.0),
                y0,
                x1: (w - 18.0).max(348.0),
                y1: y0 + 34.0,
            },
        ]
    }

    fn hit_test_button(&self, x: f32, y: f32) -> Option<usize> {
        self.button_rects()
            .into_iter()
            .position(|rect| rect.contains(x, y))
    }

    fn visible_line_count(&self) -> usize {
        let (_, h) = self.logical_size();
        (((h - 112.0).max(80.0) / 20.0).floor() as usize).max(1)
    }

    fn max_scroll(&self) -> usize {
        self.request
            .text
            .lines()
            .count()
            .saturating_sub(self.visible_line_count())
    }

    fn clamp_scroll(&mut self) {
        self.scroll_line = self.scroll_line.min(self.max_scroll());
    }

    fn adjust_scroll(&mut self, delta: i32) {
        if delta < 0 {
            self.scroll_line = self
                .scroll_line
                .saturating_sub(delta.unsigned_abs() as usize);
        } else {
            self.scroll_line = self.scroll_line.saturating_add(delta as usize);
        }
        self.clamp_scroll();
        self.window.request_redraw();
    }

    fn render(&mut self) -> Result<()> {
        let size = self.window.surface_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.clamp_scroll();
        let scale = self.window.scale_factor() as f32;
        self.egui_ctx.set_pixels_per_point(scale);
        let logical_w = size.width as f32 / scale.max(1.0);
        let logical_h = size.height as f32 / scale.max(1.0);
        let rects = self.button_rects();
        let selected = self.selected;
        let visible_lines = self.visible_line_count();
        let start = self.scroll_line;
        let body = self
            .request
            .text
            .lines()
            .skip(start)
            .take(visible_lines)
            .collect::<Vec<_>>()
            .join("\n");
        let line_count = self.request.text.lines().count();
        let notice = self.notice.clone();

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical_w, logical_h),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            ..Default::default()
        };
        let output = self.egui_ctx.run_ui(raw_input, |ctx| {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::default()
                        .fill(egui::Color32::from_rgb(246, 247, 250))
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ctx, |ui| {
                    let full = ui.max_rect();
                    let body_rect = egui::Rect::from_min_max(
                        egui::pos2(full.left() + 6.0, full.top() + 6.0),
                        egui::pos2(full.right() - 6.0, rects[0].y0 - 12.0),
                    );
                    let painter = ui.painter();
                    painter.rect_filled(
                        body_rect,
                        egui::CornerRadius::same(2),
                        egui::Color32::WHITE,
                    );
                    let border = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(150, 154, 160));
                    painter.line_segment([body_rect.left_top(), body_rect.right_top()], border);
                    painter.line_segment([body_rect.right_top(), body_rect.right_bottom()], border);
                    painter
                        .line_segment([body_rect.right_bottom(), body_rect.left_bottom()], border);
                    painter.line_segment([body_rect.left_bottom(), body_rect.left_top()], border);
                    let body_painter = painter.with_clip_rect(body_rect.shrink(2.0));
                    body_painter.text(
                        body_rect.left_top() + egui::vec2(8.0, 8.0),
                        egui::Align2::LEFT_TOP,
                        &body,
                        egui::FontId::monospace(15.0),
                        egui::Color32::from_rgb(20, 22, 25),
                    );
                    if line_count > visible_lines {
                        painter.text(
                            egui::pos2(body_rect.right() - 8.0, body_rect.bottom() - 6.0),
                            egui::Align2::RIGHT_BOTTOM,
                            format!("{} / {}", start + 1, line_count),
                            egui::FontId::proportional(11.0),
                            egui::Color32::from_rgb(100, 104, 110),
                        );
                    }

                    for (idx, rect) in rects.iter().enumerate() {
                        let r = rect.egui_rect();
                        let active = idx == selected;
                        let fill = if active {
                            egui::Color32::from_rgb(43, 107, 235)
                        } else {
                            egui::Color32::WHITE
                        };
                        let stroke = if active {
                            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(28, 86, 210))
                        } else {
                            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(150, 154, 160))
                        };
                        painter.rect_filled(r, egui::CornerRadius::same(3), fill);
                        painter.line_segment([r.left_top(), r.right_top()], stroke);
                        painter.line_segment([r.right_top(), r.right_bottom()], stroke);
                        painter.line_segment([r.right_bottom(), r.left_bottom()], stroke);
                        painter.line_segment([r.left_bottom(), r.left_top()], stroke);
                        painter.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            if idx == 0 {
                                "クリップボードにコピー"
                            } else {
                                "閉じる"
                            },
                            egui::FontId::proportional(14.0),
                            if active {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::BLACK
                            },
                        );
                    }

                    if let Some(notice) = notice.as_deref() {
                        let popup = egui::Rect::from_center_size(
                            full.center(),
                            egui::vec2((logical_w - 80.0).clamp(300.0, 480.0), 120.0),
                        );
                        painter.rect_filled(
                            popup,
                            egui::CornerRadius::same(5),
                            egui::Color32::WHITE,
                        );
                        let stroke =
                            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(120, 124, 132));
                        painter.line_segment([popup.left_top(), popup.right_top()], stroke);
                        painter.line_segment([popup.right_top(), popup.right_bottom()], stroke);
                        painter.line_segment([popup.right_bottom(), popup.left_bottom()], stroke);
                        painter.line_segment([popup.left_bottom(), popup.left_top()], stroke);
                        painter.text(
                            egui::pos2(popup.center().x, popup.top() + 34.0),
                            egui::Align2::CENTER_TOP,
                            notice,
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_rgb(25, 27, 30),
                        );
                        painter.text(
                            egui::pos2(popup.center().x, popup.bottom() - 18.0),
                            egui::Align2::CENTER_BOTTOM,
                            "OK",
                            egui::FontId::proportional(13.0),
                            egui::Color32::from_rgb(43, 107, 235),
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
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("Chihaya benchmark dialog surface validation failed")
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("siglus_chihaya_bench_dialog_encoder"),
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
                label: Some("siglus_chihaya_bench_dialog_pass"),
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
        Ok(())
    }
}
