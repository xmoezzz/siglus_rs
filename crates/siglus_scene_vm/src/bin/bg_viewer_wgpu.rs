use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

use siglus_scene_vm::assets::g00;
use siglus_scene_vm::image_manager::ImageManager;
use siglus_scene_vm::layer::LayerManager;
use siglus_scene_vm::resource::find_bg_image;
use siglus_scene_vm::render::Renderer;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Game root directory (contains g00/ and bg/).
    #[arg(long)]
    project_dir: PathBuf,

    /// BG name (without extension), e.g. bg_001
    #[arg(long)]
    bg: String,
}

#[derive(Debug, Clone)]
struct BgSource {
    path: PathBuf,
    frame_count: usize,
    cur: usize,
}

impl BgSource {
    fn load(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let frame_count = if ext == "g00" {
            let bytes = std::fs::read(path).with_context(|| format!("read {:?}", path))?;
            let dec = g00::decode_g00(&bytes).with_context(|| format!("decode {:?}", path))?;
            dec.frames.len().max(1)
        } else {
            1
        };

        Ok(Self {
            path: path.to_path_buf(),
            frame_count,
            cur: 0,
        })
    }

    fn next(&mut self) {
        self.cur = (self.cur + 1) % self.frame_count.max(1);
    }

    fn prev(&mut self) {
        let n = self.frame_count.max(1);
        self.cur = (self.cur + n - 1) % n;
    }

    fn reload(&mut self) -> Result<()> {
        let re = BgSource::load(&self.path)?;
        self.frame_count = re.frame_count;
        self.cur = self.cur.min(self.frame_count.saturating_sub(1));
        Ok(())
    }

    fn current_frame(&self) -> usize {
        self.cur
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    let (path, _ty) = find_bg_image(&args.project_dir, &args.bg)
        .with_context(|| format!("find bg {}", args.bg))?;

    let mut bg = BgSource::load(&path).with_context(|| format!("load bg {:?}", path))?;

    let event_loop = EventLoop::new().context("create EventLoop")?;
    let window = WindowBuilder::new()
        .with_title(format!("Siglus BG Viewer - {}", args.bg))
        .build(&event_loop)
        .context("create window")?;

    // Avoid self-referential Window+Surface storage by leaking the window.
    let window: &'static winit::window::Window = Box::leak(Box::new(window));

    let mut renderer = pollster::block_on(Renderer::new(window)).context("init renderer")?;

    let mut images = ImageManager::new(args.project_dir.clone());
    let mut layers = LayerManager::default();

    let img_id = images.load_file(&path, bg.current_frame()).with_context(|| "load initial frame")?;
    layers.set_bg_image(img_id);

    window.request_redraw();

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    elwt.exit();
                }
                WindowEvent::Resized(size) => {
                    renderer.resize(size.width, size.height);
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    let sprites = layers.build_render_list();
                    if let Err(e) = renderer.render_sprites(&images, &sprites) {
                        log::error!("render error: {:#}", e);
                    }
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if let Some(action) = map_key(event) {
                        match action {
                            Action::Quit => elwt.exit(),
                            Action::Prev => {
                                bg.prev();
                                if let Ok(id) = images.load_file(&path, bg.current_frame()) {
                                    layers.set_bg_image(id);
                                    window.request_redraw();
                                }
                            }
                            Action::Next => {
                                bg.next();
                                if let Ok(id) = images.load_file(&path, bg.current_frame()) {
                                    layers.set_bg_image(id);
                                    window.request_redraw();
                                }
                            }
                            Action::Reload => {
                                match bg.reload() {
                                    Ok(()) => {
                                        if let Ok(id) = images.load_file(&path, bg.current_frame()) {
                                            layers.set_bg_image(id);
                                            window.request_redraw();
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("reload failed: {:#}", e);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::AboutToWait => {}
            _ => {}
        }
    })?;

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Prev,
    Next,
    Reload,
    Quit,
}

fn map_key(ev: KeyEvent) -> Option<Action> {
    if ev.state != ElementState::Pressed {
        return None;
    }
    match ev.physical_key {
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(Action::Prev),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(Action::Next),
        PhysicalKey::Code(KeyCode::KeyR) => Some(Action::Reload),
        PhysicalKey::Code(KeyCode::Escape) => Some(Action::Quit),
        _ => None,
    }
}
