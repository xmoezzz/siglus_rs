# Nintendo Switch Port Roadmap

Status: planned. PR1 lands the roadmap and toolchain scaffold only; no engine code has been ported yet.

## Platform reality check

The constraints below drive every decision in this roadmap and differ from the desktop/Android/iOS targets:

- **GPU API.** Switch homebrew reaches the Tegra GPU through the `gpu` service, which exposes **EGL + OpenGL ES 1.1/2.0 only**. Vulkan exists on retail but is not available to homebrew, so **wgpu cannot target Switch**. The port needs a second renderer backend (GLES2) beside the existing wgpu one.
- **Rust target.** There is no official Rust Switch target. Builds use devkitA64 (aarch64-none-elf toolchain) plus a custom target JSON and libnx bindings (switch2-rs / libnx-rs). `std` is available over newlib, so a **full `no_std` rewrite is not required**; only startup, panic handler, and global allocator glue are `no_std`-style, and that already exists as the `main` shim pattern.
- **Memory.** Switch has 4 GB total (~3.3 GB usable by homebrew). The engine's measured footprint on Apple Silicon is ~760 MB of which ~517 MB is GPU texture memory, so the port must ship with an explicit GPU texture budget from day one.
- **Shaders.** `render/mod.rs` embeds 32 WGSL entry points, plus `mipmap.wgsl` (compute) and the mpeg2 blit shader. GLES2 has **no compute shaders**, so the GPU mipmap generator needs a CPU fallback (or a two-pass blit), and the WGSL→GLSL-ES pipeline is effectively a rewrite of the shader flow.

## Staged PRs

| PR | Scope | Exit criterion |
|----|-------|----------------|
| 1 (this) | Roadmap + scaffold: target JSON, toolchain docs, build stub | Docs merge; no engine changes |
| 2 | Renderer backend abstraction: `SiglusHost` stops naming the wgpu `Renderer` type; `RenderFrame` becomes the backend-neutral IR; wgpu becomes a feature-gated backend | Desktop behaviour byte-identical; `cargo check` with `--no-default-features` |
| 3 | Toolchain integration: devkitA64 build script, libnx bindings crate (`siglus_switch_sys`), target JSON, cargo config | `cargo check -p siglus_scene_vm --target aarch64-switch` green for vm/runtime/assets modules |
| 4 | EGL/GLES2 backend: window, framebuffer, textured-quad pipeline, blend modes, scissor clip; **WGSL→GLSL ES 1.00 shader pipeline via naga** (build-time translation of the 32 entry points); CPU mipmap fallback | Chihaya Rolling WE renders on hardware |
| 5 | Input (`hid`), audio (kira backend over SDL2 audio or `aud:u`/cpia), filesystem + savedata | Title screen interactive; BGM plays |
| 6 | Text rendering (font atlas on GLES2), button/choice layer, HUD | A Rewrite-trial scene is completable |
| 7 | Validation: Chihaya benchmark score + rank on Switch, GPU texture budget enforcement, g00 sprite stress | Roadmap issue closes; movie codecs stay deferred |

## Risks

- naga's GLSL ES 1.00 backend coverage of the WGSL features actually used (dynamic loops, separate alpha blend) — audit in PR4 first commit.
- GLES2 uniform/attribute limits vs the current bind-group-per-frame layout.
- Audio latency through cpia; fallback is SDL2_mixer-style callback mixing.
- wasm Emote (`wasm32` runtime) JIT is unavailable; interpreter-only.
