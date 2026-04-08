#![cfg(feature = "wgpu-winit")]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes};

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
    project_dir: Option<PathBuf>,

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

#[derive(Debug, Clone, Copy)]
enum Action {
    Quit,
    Prev,
    Next,
    Reload,
}

fn map_key(event: KeyEvent) -> Option<Action> {
    if event.state != ElementState::Pressed {
        return None;
    }
    match event.physical_key {
        PhysicalKey::Code(KeyCode::Escape) => Some(Action::Quit),
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(Action::Prev),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(Action::Next),
        PhysicalKey::Code(KeyCode::KeyR) => Some(Action::Reload),
        _ => None,
    }
}

struct App {
    args: Args,
    project_dir: PathBuf,
    bg: BgSource,
    window: Option<&'static Window>,
    renderer: Option<Renderer>,
    images: ImageManager,
    layers: LayerManager,
}

impl App {
    fn new(args: Args, project_dir: PathBuf, bg: BgSource) -> Result<Self> {
        Ok(Self {
            args,
            project_dir: project_dir.clone(),
            bg,
            window: None,
            renderer: None,
            images: ImageManager::new(project_dir),
            layers: LayerManager::default(),
        })
    }

    fn load_current(&mut self) {
        if let Ok(id) = self.images.load_file(&self.bg.path, self.bg.current_frame()) {
            self.layers.set_bg_image(id);
        }
    }

    fn redraw(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let sprites = self.layers.render_list();
        if let Err(e) = renderer.render_sprites(&self.images, &sprites) {
            eprintln!("render error: {e:#}");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, elwt: &ActiveEventLoop) {
        let size = LogicalSize::new(1280.0, 720.0);
        let window = elwt
            .create_window(WindowAttributes::default().with_inner_size(size).with_title(format!("Siglus BG Viewer - {}", self.args.bg)))
            .expect("create window");
        let window: &'static Window = Box::leak(Box::new(window));

        let renderer = pollster::block_on(Renderer::new(window)).expect("init renderer");

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.load_current();

        window.request_redraw();
    }

    fn window_event(&mut self, elwt: &ActiveEventLoop, _id: winit::window::WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => elwt.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(action) = map_key(event) {
                    match action {
                        Action::Quit => elwt.exit(),
                        Action::Prev => {
                            self.bg.prev();
                            self.load_current();
                        }
                        Action::Next => {
                            self.bg.next();
                            self.load_current();
                        }
                        Action::Reload => {
                            if let Err(e) = self.bg.reload() {
                                eprintln!("reload failed: {e:#}");
                            } else {
                                self.load_current();
                            }
                        }
                    }
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let project_dir = args
        .project_dir
        .clone()
        .unwrap_or(siglus_scene_vm::app_path::resolve_app_base_path()?);

    let (path, _ty) = find_bg_image(&project_dir, &args.bg)
        .with_context(|| format!("find bg {}", args.bg))?;

    let bg = BgSource::load(&path).with_context(|| format!("load bg {:?}", path))?;

    let el = EventLoop::new()?;
    let mut app = App::new(args, project_dir, bg)?;
    el.run_app(&mut app)?;
    Ok(())
}
