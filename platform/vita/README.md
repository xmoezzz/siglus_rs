# Siglus PS Vita port

The Vita player uses the shared Rust Siglus VM, resource decoders, text code,
and Kira mixer. Its platform entry point uses VitaSDK bindings for input,
audio, and memory readings. The renderer
(`crates/siglus_scene_vm/src/render/vita`) draws everything with GXM: it
runs the desktop renderer's frame plan (`render_plan`, shared with
`render/mod.rs`) and the desktop's WGSL shaders ported to Cg. The shaders are
compiled to GXP ahead of time and embedded, so the VPK needs no runtime
shader compiler (`libshacccg.suprx`) or other extra module: only firmware
modules are imported. vita2d (the VitaSDK `libvita2d` package, linked
statically) still provides the display, context and shader patcher. No SDK or
third-party source is copied into this repository.

## Installing (players)

The player runs SiglusEngine games (`Scene.pck`) on a PS Vita with homebrew
enabled (HENkaku/h-encore or Ensō) and VitaShell. It is experimental.

1. Download `siglus-psvita.vpk` from the
   [releases page](https://github.com/xmoezzz/siglus_rs/releases) (the
   rolling `pre-release` has the newest build).
2. Install it with VitaShell: copy the VPK to the Vita (USB or FTP), select
   it and install. It appears as **Siglus Vita**.
3. Copy the game's own files from your PC install into
   `ux0:data/siglus_rs/game/`: `Scene.pck`, `Gameexe.dat` and the asset
   folders (`g00`, `bgm`, `koe`, `wav`, `mov`, `dat`, `gan`, ... as the game
   has them), plus `key.toml` if you made one for the game (see
   [the main README](../../README.md#the-keytoml-configuration-file); without
   it the engine finds the key itself). `SiglusEngine.exe` and manuals are
   not needed. The game's desktop `savedata` is not used.
4. Start **Siglus Vita**.

Saves go to `ux0:data/siglus_rs/game/savedata/`; the log is
`ux0:data/siglus_rs/vita-player.log` (include it when reporting a problem).
One game is installed at a time: to switch, replace the files in `game/`
(keep each game's `savedata/` if you want to keep its progress).

For encrypted games without `key.toml`, a native Vita dialog explains that
on-device key recovery can take several minutes. Choose OK to continue or
Cancel to exit, then generate the key with the desktop player and copy it
into `ux0:data/siglus_rs/game/`. Startup and runtime errors also open a native
message dialog instead of immediately closing the player.

Games are shown fitted into the 960×544 screen (up to 1920×1080). Touch the
screen to click; Cross confirms, Circle cancels, the D-pad moves between
choices.

## Build

Install VitaSDK, `cargo-vita`, Rust nightly, and the VitaSDK `libvita2d`
package (`vdpm install libvita2d`).
From this repository:

```sh
cd platform/vita/player
export VITASDK=/usr/local/vitasdk
cargo +nightly vita build vpk -- --release
```

The VPK is written to
`platform/vita/player/target/armv7-sony-vita-newlibeabihf/release/siglus_vita_player.vpk`.

The same environment is a Docker image, `platform/vita/docker/Dockerfile`
(VitaSDK with libvita2d, the pinned nightly and cargo-vita). CI publishes it
as `ghcr.io/xmoezzz/siglus_rs/vita-build`, tagged with the Dockerfile's hash,
and builds the release VPK in it; the image is rebuilt only when the
Dockerfile changes. Locally:

```sh
docker build -t siglus-vita-build platform/vita/docker
docker run --rm -v "$PWD:/src" -w /src/platform/vita/player \
    siglus-vita-build cargo vita build vpk -- --release
```
Install it on a Vita homebrew environment and place your own game files under
`ux0:data/siglus_rs/game/`, including `Scene.pck`, `Gameexe.dat`, and any
required `key.toml`. Saves and logs use `ux0:data/siglus_rs/`. The player log
is `ux0:data/siglus_rs/vita-player.log`.

For Vita3K, install its firmware font package before judging text rendering.

The standalone `bootstrap/` probe can be built in the same way. It checks
display and storage access without loading the engine or game assets.

## Input and memory

Cross confirms, Circle cancels, the D-pad navigates, and the front touch panel
maps to the letterboxed game viewport. The player accepts game logical sizes
up to 1920×1080 (GameData) and presents at 960×544.

vita2d keeps its display buffers in CDRAM. Each texture is its own memory
block: 256 KiB or larger in CDRAM, smaller ones (text, icons) in uncached user
memory, so they do not each occupy a 256 KiB CDRAM unit. Images above 720p
are uploaded at half size (a 1080p screen is shown at 960×540); tone curves,
being look-up tables, stay exact. The texture cache evicts old entries at
72 MiB; a replaced texture is freed four frames later, because Vita3K's
renderer can still read it after `sceGxmFinish`. vita2d does not initialize
a new texture's render-target and depth fields, which `vita2d_free_texture`
frees when non-zero; the renderer clears them, or a free could release an
unrelated memory block (the newlib heap included).

The passes follow the desktop renderer. Ordinary frames are drawn straight to
the display. Wipes and overlay blending go through four render targets of the
on-screen size (scene A/B, wipe A/B): the wipe compositor, page wipes and the
overlay backdrop read them. E-mote objects are composed into their own
targets with stencil masks, and 3D meshes use the mesh and shadow-map
programs. Every sprite effect (tone curve, mask, the wipe effects, light,
fog, every blend mode) is a shader; nothing is composited on the CPU. Frame
captures (`CAPTURE`, save thumbnails) read a finished render target back.
Vita3K keeps GPU surfaces on the host, so captures come out black there; on a
Vita they hold the frame.

## Shaders

The Cg sources are `crates/siglus_scene_vm/src/render/vita/shaders/*.cg`
(`*_v` vertex, `*_f` fragment, `*.cgh` included), ported one to one from the
WGSL in `render/shaders/`; change both together. The
compiled programs in `shaders/gxp/` are embedded in the player. After
changing a shader, run

```sh
platform/vita/shaderc/compile-shaders.sh
```

It builds `platform/vita/shaderc` (title SIGSHADR1), runs it in Vita3K, which
compiles every shader with Sony's compiler (`libshacccg.suprx`, placed in
Vita3K's `ur0:data`), and copies the GXPs back into the tree. The script
needs the Vita3K window visible. vitaShaRK's wrapper fails in Vita3K, so
shaderc calls `sceShaccCgCompileProgram` directly.

The RewriteHF `Scene.pck` used for bring-up is about 11 MiB compressed and
43 MiB rebuilt. The engine no longer rebuilds it: the decrypted, still
compressed pack stays in memory and a scene is decompressed when it is first
used, shared while any stream holds it (`ScenePck::load_lazy`). Fonts are
resolved once per requested face, and a font file is read once per process
and shared by every face and name that uses it (GameData requests one 22 MiB
TTC under many names; each request kept its own copy until the heap ran
out). The embedded default font is parsed in place from the executable.
Plain OGG sound effects of 256 KiB or more stream instead of being decoded
whole (a long ambience loop decoded to 15 MiB). `siglus_scene_vm/examples/memory_probe.rs` runs a scene headlessly
with a counting allocator to measure live and peak heap use and to trace
large allocations (`MEMORY_PROBE_BIG=bytes`). On a RewriteHF route it went
from about 105 MiB live to about 55 MiB with these changes.
Audio output uses 512 stereo frames
and a 128 KiB worker stack. The newlib heap is 320 MiB, reserved at startup
(extended memory mode). With 256 MiB about 100 MiB of user memory stayed
unused beside it while GameData's 1080p title menu ran the heap out. If an
allocation still fails, the player log records its size, the heap figures and
the last VM status before the abort. Vita movie streams
keep one presented MPEG/OMV frame and one queued frame. MPEG, OMV, and WMV convert
directly to no more than 960×544 RGBA pixels; for the RewriteHF 1280×720
opening this reduces each frame from 3.69 MiB to 2.07 MiB. OMV loop-head
frame caching is disabled on Vita; indexed seeking handles loop restarts.
An OMV frame is converted straight from the Theora decoder's planes. The
decoder keeps three reference frames, as libtheora's does (it had six), and
no copy of the output frame; a 1920×1440 4:4:4 RGBA OMV frame is 8.6 MiB, and
GameData's title menu plays four such streams at once.
Movie streams that a scene has not polled for 120 engine frames release their
decoder and frame queue. (A wall-clock limit evicted the movie playing when one
frame stalled, and the restart then sat on its first frame.) A restarted MPEG
stream seeks to the GOP before its time, handing the decoder the file's only
sequence header first, and without the audio track takes the duration from the
last video timestamp. Movies decoded smaller than their size (the 960×544 cap)
are drawn stretched to the video's own size, so object movies keep their
position. A movie's texture is rewritten in place for each new frame instead of
being freed and reallocated. `siglus_scene_vm/examples/movie_probe.rs` plays a
movie on a simulated clock (`MOVIE_PROBE_START=ms` to start mid-movie). NWA music is decoded while it plays from the file (one
compressed unit in memory); decoding a whole track up front needed up to
~100 MiB for RewriteHF's longest BGM and ran the heap out of memory at the
title screen.

Global data (flags such as "opening seen", read text, CG and BGM tables) is
written a few seconds after it changes, since a Vita app is normally closed
without an exit the engine sees. RewriteHF only lets the opening be skipped
after it has been watched once, which it records in a global flag.

The player is built without LLVM's SLP vectorizer (`profile.*.rustflags` in
`player/Cargo.toml`): it combined 64-bit saturating arithmetic into NEON
`vqadd.s64`/`vqsub.s64`, which Vita3K cannot execute, breaking input hit
tests and the VM's frame bookkeeping in the emulator.
Not all image, audio, mesh, and text caches have a measured total cap yet.
The periodic log counts the rebuilt pack, live decoded RGBA images, GPU
textures, and movie frames/PCM separately so growth can be tracked during
long play sessions.

For emulator diagnosis only, creating
`ux0:data/siglus_rs/disable-audio` skips Vita audio-port initialization.
Normal Vita builds start audio by default. When output is disabled, the player
still advances Kira's mixer with silent 16 ms buffers so movie audio remains
a working clock. Remove this file to test audio.

`ux0:data/siglus_rs/smoke-taps` scripts touches for emulator runs whose
window cannot deliver clicks: one `frame x y` per line, in game coordinates.
The log records the VM state every 120 frames.

`ux0:data/siglus_rs/start-scene` holds a scene name (for example
`010_プロローグ0701`) to boot into instead of the game's start scene.

`ux0:data/siglus_rs/dump-frames` lists frame numbers (one per line) to save
as `dump/frame-N.png` (on a Vita; Vita3K's buffers read back black).

## Validation status

The player links and packages with VitaSDK and `cargo-vita`. The vitaGL build
ran on Vita3K with RewriteHF: it parsed `Gameexe.dat`, initialized the VM,
played the Key logo and the roughly 100-second `op00.mpg` opening, and reached
the title screen once BGM streaming was in place; entering a route then ran the
160 MiB heap out of memory, which led to the memory work above. With the
vita2d renderer, RewriteHF's opening, title and the start of the story run at
55–60 fps on Vita3K. With the GXM renderer, GameData's prologue (with
its tone-curved backgrounds, which the vita2d renderer composited on the CPU
at over a second a frame) runs at 50–60 fps on Vita3K, render time a few
milliseconds. Its picture cannot be read back from Vita3K, so the output
still needs checking on screen. GameData (1920×1080) reaches its title menu and prologue
with audio on Vita3K; its particle frame actions (`$$fa_particle`, thousands
of interpreted object operations per frame) still drop the prologue to 17–30
fps, the VM's object-operation path being about 30 times slower than on a
desktop CPU. Earlier Vita3K builds crashed in `sceAudioOutOpenPort`; the
`disable-audio` marker remains for such emulators. No physical Vita run or
complete playthrough has been verified. The port remains
experimental; see [ROADMAP.md](ROADMAP.md).
