# Switch platform port

The Switch frontend reuses the existing `siglus_scene_vm` host, Scene VM,
resource loader, script runtime, image manager, and audio engine.  libnx owns
lifecycle/input/audio in `runtime/source/main.c`; the artifact contains no
`winit`, `wgpu`, or desktop-window dependency.

The renderer (`crates/siglus_scene_vm/src/render/horizon`) is the desktop
renderer with deko3d in place of wgpu: the frame is planned by the shared
`render_plan`, drawn in the same passes (direct frames, overlay ping-pong,
wipe composite, page wipes, E-mote targets with stencil masks, meshes and the
shadow map), with render targets of the logical size and textures of full
size with mip chains. The shaders are the desktop's WGSL
(`crates/siglus_scene_vm/src/render/shaders`), translated to GLSL at build
time by `shaderc/` (naga 0.20, the version wgpu uses) and compiled by
devkitPro's `uam` into the RomFS. The deko3d calls live in
`runtime/source/gpu.c`.

`./platform/switch/build_switch.sh` produces:

`platform/switch/runtime/siglus_switch.nro`

## Deploying GameData

Copy the NRO to `sdmc:/switch/siglus_rs/siglus_switch.nro` and place the
unmodified game directory at `sdmc:/switch/siglus_rs/game/`. The runtime passes
that directory to the existing `SiglusHostConfig`, so standard engine resource
lookup remains responsible for locating the game's `Gameexe` data, `Scene.pck`,
archives, movies, and audio assets.

Encrypted games also need their valid `key.toml` in that directory. Automatic
resource key recovery is disabled on Switch because of its startup memory
cost. Start the game once with the desktop player to generate the file, then
copy it with the game assets. A missing or invalid key causes startup to fail
before the main loop.

Startup and runtime failures open the native Switch error dialog. Missing
encryption keys are reported before scene decoding, with the game directory
and instructions for generating `key.toml` on a PC.

For a self-contained NRO, use:

```sh
./platform/switch/package_game_nro.sh /path/to/game /path/to/game.nro
```

The script copies the supplied game directory into a temporary RomFS staging
area, builds the normal Switch runtime and embeds the copied assets in the
output NRO. The original game directory is never modified. At startup the
runtime prefers `romfs:/game` and only falls back to the SD-card path above
when no embedded `Scene.pck` exists. Full game packages larger than 4 GiB need
an exFAT-formatted SD card because FAT32 cannot hold the resulting NRO.

Some emulators cannot mount a multi-gigabyte NRO RomFS: their storage adapter
uses a signed 32-bit buffer range even though the NRO/RomFS format uses
64-bit offsets. For that case, make an SD-card deployment bundle instead:

```sh
./platform/switch/package_game_nro.sh --sdmc /path/to/game /path/to/siglus_rs
```

It produces `siglus_switch.nro` and a complete `game/` directory under the
specified `siglus_rs` directory. Copy its contents to
`sdmc:/switch/siglus_rs/`. This uses the same engine and native renderer; only
the asset storage location changes.

## Toolchain

Install devkitA64, libnx, deko3d, and switch-tools (`/opt/devkitpro`), or use
the Docker image, which CI also builds in (`ghcr.io/xmoezzz/siglus_rs/switch-build`,
tagged with the Dockerfile's hash):

```sh
docker build --platform linux/amd64 -t siglus-switch-build platform/switch/docker
docker run --rm --platform linux/amd64 -v "$PWD:/src" -w /src siglus-switch-build \
    make -C platform/switch/runtime
```

## Layout

- `runtime/` — deployable libnx/deko3d frontend linked to the existing Rust engine (`source/gpu.c`: the deko3d layer).
- `shaderc/` — translates the desktop WGSL to GLSL for `uam`.
- `docker/` — the build image.
- `build_switch.sh` — packages `runtime/siglus_switch.nro`.
- `package_game_nro.sh` — embeds a chosen game directory in a standalone NRO.
- `rust/aarch64-switch.json` — the Rust target (`os = "horizon"`, `env = "newlib"`). The standard library is built with `-Zbuild-std`, so engine crates and their dependencies compile unmodified from crates.io.
- `patches/` — crates that must be patched for the pinned nightly; see `patches/README.md`.
- `ROADMAP.md` — historical staged-port notes.
