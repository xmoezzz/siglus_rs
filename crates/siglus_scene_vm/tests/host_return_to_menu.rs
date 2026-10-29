//! Return to the menu after the script has ended, using generated public scenes only.
#![cfg(any(target_os = "windows", target_os = "linux"))]

#[path = "support/scene_fixture.rs"]
mod scene_fixture;

use scene_fixture::{Code, gameexe, scene_chunk, scene_pack};
use siglus_scene_vm::{
    host::{SiglusHost, SiglusHostConfig},
    render::Renderer,
    runtime::globals::{SyscomPendingProc, SyscomPendingProcKind},
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

struct Project {
    base: PathBuf,
}

impl Project {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!(
            "siglus-public-host-return-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let project = Self { base };
        fs::create_dir_all(project.game()).unwrap();
        let start = Code::default().line(1).eof();
        let menu = Code::default().line(7).eof();
        fs::write(
            project.game().join("Scene.pck"),
            scene_pack(&[
                ("start", scene_chunk(&start, &[])),
                ("menu", scene_chunk(&menu, &[])),
            ]),
        )
        .unwrap();
        fs::write(project.game().join("Gameexe.dat"), gameexe(1280, "menu")).unwrap();
        project
    }

    fn game(&self) -> PathBuf {
        self.base.join("game")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Steps until the menu scene has run to its end and the proc stack is empty.
fn step_until_menu_ended(host: &mut SiglusHost) {
    assert!(
        (0..240).any(|_| host.step(0).unwrap()),
        "menu scene must end"
    );
    let vm = host.vm_mut();
    assert_eq!(vm.current_scene_name(), Some("menu"));
    assert_eq!(vm.current_line_no(), 7);
    assert!(vm.is_halted());
}

fn run(window: Arc<dyn Window>) {
    let project = Project::new();
    let renderer = pollster::block_on(Renderer::new(window)).unwrap();
    let mut config = SiglusHostConfig::new(project.game());
    config.width = Some(1280);
    config.height = Some(720);
    let mut host = SiglusHost::new_with_renderer_sync(config, renderer).unwrap();

    // The start scene ends and returns to the menu; the menu scene then ends
    // too, leaving the host idle with an empty proc stack.
    step_until_menu_ended(&mut host);
    for _ in 0..8 {
        assert!(host.step(0).unwrap());
    }

    // A return to menu requested while idle, as a native menu button does.
    host.vm_mut().ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
        kind: SyscomPendingProcKind::ReturnToMenu,
        warning: false,
        se_play: false,
        fade_out: false,
        leave_msgbk: false,
        save_id: 0,
    });
    // The host must restart the menu scene and then run it to its end again.
    let mut restarted = false;
    let finished = (0..240).any(|_| {
        let finished = host.step(0).unwrap();
        restarted |= !host.vm_mut().is_halted();
        finished && restarted
    });
    assert!(restarted, "idle host ignored the queued return to menu");
    assert!(finished, "restarted menu scene must end");
    assert_eq!(host.vm_mut().current_scene_name(), Some("menu"));
    assert_eq!(host.vm_mut().current_line_no(), 7);
}

#[test]
#[ignore = "requires a desktop session and GPU"]
fn return_to_menu_queued_after_the_script_ended_restarts_the_menu() {
    #[cfg(target_os = "linux")]
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    #[cfg(target_os = "windows")]
    use winit::platform::windows::EventLoopBuilderExtWindows;

    struct Probe(Arc<AtomicBool>);
    impl ApplicationHandler for Probe {
        fn can_create_surfaces(&mut self, elwt: &dyn ActiveEventLoop) {
            let window = elwt
                .create_window(
                    WindowAttributes::default()
                        .with_visible(false)
                        .with_surface_size(PhysicalSize::new(1280, 720)),
                )
                .unwrap();
            run(Arc::from(window));
            self.0.store(true, Ordering::Relaxed);
            elwt.exit();
        }

        fn window_event(&mut self, _: &dyn ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }

    let ran = Arc::new(AtomicBool::new(false));
    EventLoop::builder()
        .with_any_thread(true)
        .build()
        .unwrap()
        .run_app(Probe(ran.clone()))
        .unwrap();
    assert!(ran.load(Ordering::Relaxed));
}
