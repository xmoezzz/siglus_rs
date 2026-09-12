//! Siglus OBJECT Emote host adapter.
//!
//! Eluna owns PSB parsing, timeline/physics state and native draw-list recovery.
//! This module only adapts the original Siglus object contract around it.

use std::collections::HashMap;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use std::cell::Cell;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, bail, Context, Result};
use eluna::{
    EmotePlayerControl, EmoteStaticScene, EmoteTextureSource, TimelinePlayMode,
};

mod player;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
static NEXT_RENDER_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
thread_local! {
    static NEXT_RENDER_ID: Cell<u64> = Cell::new(1);
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn next_render_id() -> u64 {
    NEXT_RENDER_ID.fetch_add(1, Ordering::Relaxed)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn next_render_id() -> u64 {
    NEXT_RENDER_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1).max(1));
        id
    })
}

#[derive(Debug, Clone)]
pub struct EmoteDecodedTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct EmoteHitSurface {
    version: u64,
    width: u32,
    height: u32,
    alpha: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct EmoteRenderPacket {
    pub render_id: u64,
    pub version: u64,
    pub width: u32,
    pub height: u32,
    pub rep_x: f32,
    pub rep_y: f32,
    pub alpha_readback: bool,
    pub scene: Arc<EmoteStaticScene>,
    pub textures: Arc<HashMap<u32, EmoteDecodedTexture>>,
    hit_surface: Arc<RwLock<Option<EmoteHitSurface>>>,
}

impl EmoteRenderPacket {
    pub fn alpha_hit_test(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        let Ok(surface) = self.hit_surface.read() else {
            return false;
        };
        let Some(surface) = surface.as_ref() else {
            return false;
        };
        if surface.version != self.version
            || surface.width != self.width
            || surface.height != self.height
        {
            return false;
        }
        let x = x as u32;
        let y = y as u32;
        if x >= surface.width || y >= surface.height {
            return false;
        }
        surface
            .alpha
            .get((y as usize) * (surface.width as usize) + x as usize)
            .copied()
            .unwrap_or(0)
            != 0
    }

    pub(crate) fn has_current_hit_surface(&self) -> bool {
        let Ok(surface) = self.hit_surface.read() else {
            return false;
        };
        surface.as_ref().is_some_and(|surface| {
            surface.version == self.version
                && surface.width == self.width
                && surface.height == self.height
        })
    }

    pub(crate) fn publish_hit_alpha(&self, alpha: Vec<u8>) {
        if alpha.len() != self.width as usize * self.height as usize {
            return;
        }
        let Ok(mut surface) = self.hit_surface.write() else {
            return;
        };
        if surface
            .as_ref()
            .is_some_and(|current| current.version > self.version)
        {
            return;
        }
        *surface = Some(EmoteHitSurface {
            version: self.version,
            width: self.width,
            height: self.height,
            alpha,
        });
    }
}

#[derive(Debug, Clone)]
pub struct SiglusEmoteRuntime {
    runtime: player::Player,
    decoded_textures: Arc<HashMap<u32, EmoteDecodedTexture>>,
    render_id: u64,
    version: u64,
    hit_surface: Arc<RwLock<Option<EmoteHitSurface>>>,
}

impl SiglusEmoteRuntime {
    pub fn from_psb_bytes(data: &[u8], key: Option<u32>) -> Result<Self> {
        Self::from_psb_sources(&[data], key)
    }

    pub fn from_psb_sources(sources: &[&[u8]], key: Option<u32>) -> Result<Self> {
        let runtime = player::Player::from_sources(sources, key)
            .context("Eluna failed to create Emote runtime")?;
        let spec = runtime.schema().spec.as_deref();

        let mut decoded = HashMap::new();
        for source in runtime.schema().textures.values() {
            let bytes = runtime
                .texture_bytes(source.resource_index)
                .ok_or_else(|| anyhow!("missing Emote texture resource {}", source.resource_index))?;
            let tex = decode_texture_source(source, bytes, spec).with_context(|| {
                format!(
                    "failed to decode Emote texture {} resource {}",
                    source.name, source.resource_index
                )
            })?;
            decoded.insert(source.resource_index, tex);
        }

        Ok(Self {
            runtime,
            decoded_textures: Arc::new(decoded),
            render_id: next_render_id(),
            version: 1,
            hit_surface: Arc::new(RwLock::new(None)),
        })
    }

    pub fn clone_for_object(&self) -> Self {
        let mut cloned = self.clone();
        // C_elm_object::copy clones the player but creates a fresh render target.
        cloned.render_id = next_render_id();
        cloned.version = cloned.version.wrapping_add(1).max(1);
        cloned.hit_surface = Arc::new(RwLock::new(None));
        cloned
    }

    pub fn progress_ms(&mut self, ms: i32) -> Result<()> {
        if ms <= 0 {
            return Ok(());
        }
        // Original `EmoteUpdate`: player->Progress(ms * 60 / 1000). Do not use
        // Eluna's RAF-capped helper here; Siglus does not clamp this delta.
        self.runtime
            .progress_ticks(ms as f32 * 60.0 / 1000.0)
            .context("Eluna Emote Progress failed")?;
        self.bump_version();
        Ok(())
    }

    pub fn set_face_talk(&mut self, value: f32) -> Result<()> {
        self.runtime.inner.set_variable_immediate("face_talk", value);
        self.runtime.rebuild_scene(0.0)
            .context("Eluna SetVariable(face_talk) failed")?;
        self.bump_version();
        Ok(())
    }

    pub fn play_timeline(&mut self, name: &str, option: i64) -> Result<()> {
        self.runtime
            .play_timeline(name, TimelinePlayMode::from_flags(option as u32))
            .with_context(|| format!("Eluna PlayTimeline({name:?}, {option}) failed"))?;
        self.bump_version();
        Ok(())
    }

    /// Mirrors the no-argument IEmotePlayer::StopTimeline overload used by
    /// Siglus: Eluna represents it as an empty timeline name.
    pub fn stop_all_timelines(&mut self) -> Result<()> {
        self.runtime.inner.stop_timeline("");
        self.runtime.rebuild_scene(0.0)
            .context("Eluna StopTimeline() failed")?;
        self.bump_version();
        Ok(())
    }

    pub fn stop_timeline(&mut self, name: &str) -> Result<()> {
        self.runtime.inner.stop_timeline(name);
        self.runtime.rebuild_scene(0.0)
            .with_context(|| format!("Eluna StopTimeline({name:?}) failed"))?;
        self.bump_version();
        Ok(())
    }

    pub fn is_animating(&self) -> bool {
        self.runtime.inner.is_animating()
    }

    pub fn pass(&mut self) -> Result<()> {
        self.runtime.inner.pass();
        self.runtime.rebuild_scene(0.0).context("Eluna Pass failed")?;
        self.bump_version();
        Ok(())
    }

    pub fn skip(&mut self) -> Result<()> {
        self.runtime.inner.skip();
        self.runtime
            .rebuild_scene(0.0)
            .context("Eluna scene rebuild after Skip failed")?;
        self.bump_version();
        Ok(())
    }

    pub fn packet(
        &self,
        width: i64,
        height: i64,
        rep_x: i64,
        rep_y: i64,
        alpha_readback: bool,
    ) -> Arc<EmoteRenderPacket> {
        Arc::new(EmoteRenderPacket {
            render_id: self.render_id,
            version: self.version,
            width: width.max(1).min(u32::MAX as i64) as u32,
            height: height.max(1).min(u32::MAX as i64) as u32,
            rep_x: rep_x as f32,
            rep_y: rep_y as f32,
            alpha_readback,
            scene: Arc::new(self.runtime.scene().clone()),
            textures: self.decoded_textures.clone(),
            hit_surface: self.hit_surface.clone(),
        })
    }

    fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1).max(1);
    }
}

fn decode_texture_source(
    source: &EmoteTextureSource,
    bytes: &[u8],
    spec: Option<&str>,
) -> Result<EmoteDecodedTexture> {
    let width = source.width.max(1);
    let height = source.height.max(1);
    let bit_count = source.bit_count.unwrap_or(32);
    if bit_count != 32 {
        bail!("unsupported Emote texture bit depth {bit_count}; expected 32-bit texture");
    }
    let expected = width as usize * height as usize * 4;

    let raw = if source
        .compress
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("RL"))
        .unwrap_or(false)
    {
        decode_rl(bytes, 4, expected)?
    } else if bytes.len() == expected {
        bytes.to_vec()
    } else {
        // Some PSB resources contain encoded PNG/BMP/DDS data rather than a
        // raw 32-bit plane. Reuse the project's mature image decoder.
        let image = image::load_from_memory(bytes)
            .context("Emote texture resource is neither raw RGBA/BGRA nor a decodable image")?
            .to_rgba8();
        if image.width() != width || image.height() != height {
            bail!(
                "decoded Emote texture size {}x{} does not match metadata {}x{}",
                image.width(),
                image.height(),
                width,
                height
            );
        }
        return Ok(EmoteDecodedTexture {
            width,
            height,
            rgba: Arc::new(image.into_raw()),
        });
    };

    let format = source.format.as_deref().unwrap_or("");
    let spec_upper = spec.unwrap_or("").to_ascii_uppercase();
    let format_upper = format.to_ascii_uppercase();
    let big_rgba_spec = matches!(spec_upper.as_str(), "COMMON" | "EMS" | "VITA" | "PSP" | "PS3");

    let mode = match format_upper.as_str() {
        "BERGBA8" => Raw32Mode::Rgba,
        "LERGBA8" | "BGRA8" | "ARGB8" | "A8R8G8B8" | "D3DFMTA8R8G8B8" => Raw32Mode::Bgra,
        "BGRX8" | "X8R8G8B8" | "D3DFMTX8R8G8B8" => Raw32Mode::Bgrx,
        "RGBX8" | "RGBX" => Raw32Mode::Rgbx,
        "RGBA" | "RGBA8" => {
            if big_rgba_spec { Raw32Mode::Rgba } else { Raw32Mode::Bgra }
        }
        _ => {
            if big_rgba_spec { Raw32Mode::Rgba } else { Raw32Mode::Bgra }
        }
    };

    let mut rgba = vec![0u8; expected];
    for (src, dst) in raw.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        match mode {
            Raw32Mode::Rgba => dst.copy_from_slice(src),
            Raw32Mode::Bgra => dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]),
            Raw32Mode::Bgrx => dst.copy_from_slice(&[src[2], src[1], src[0], 255]),
            Raw32Mode::Rgbx => dst.copy_from_slice(&[src[0], src[1], src[2], 255]),
        }
    }

    Ok(EmoteDecodedTexture {
        width,
        height,
        rgba: Arc::new(rgba),
    })
}

#[derive(Clone, Copy)]
enum Raw32Mode {
    Rgba,
    Bgra,
    Bgrx,
    Rgbx,
}

fn decode_rl(bytes: &[u8], align: usize, expected: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(expected);
    let mut pos = 0usize;
    while pos < bytes.len() && out.len() < expected {
        let cmd = bytes[pos];
        pos += 1;
        if cmd & 0x80 != 0 {
            let count = ((cmd ^ 0x80) as usize) + 3;
            if pos + align > bytes.len() {
                bail!("truncated Emote RL repeat command");
            }
            let unit = &bytes[pos..pos + align];
            pos += align;
            for _ in 0..count {
                out.extend_from_slice(unit);
            }
        } else {
            let count = (cmd as usize) + 1;
            let len = count
                .checked_mul(align)
                .ok_or_else(|| anyhow!("Emote RL literal length overflow"))?;
            if pos + len > bytes.len() {
                bail!("truncated Emote RL literal command");
            }
            out.extend_from_slice(&bytes[pos..pos + len]);
            pos += len;
        }
        if out.len() > expected {
            bail!("Emote RL output exceeds expected texture size");
        }
    }
    if out.len() != expected {
        bail!(
            "Emote RL output size mismatch: got {}, expected {}",
            out.len(),
            expected
        );
    }
    Ok(out)
}
