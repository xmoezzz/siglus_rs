use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};

use crate::assets::RgbaImage;
use crate::image_manager::{ImageId, ImageManager};

#[derive(Debug, Clone)]
struct ToneCurveRow {
    lut_r: [u8; 256],
    lut_g: [u8; 256],
    lut_b: [u8; 256],
    sat: i32,
}

#[derive(Debug, Default)]
pub struct ToneCurveRuntime {
    rows: Option<Vec<Option<ToneCurveRow>>>,
    cache: HashMap<(ImageId, u64, i32), ImageId>,
    source_path: Option<PathBuf>,
    lut_image_id: Option<ImageId>,
}

impl ToneCurveRuntime {
    pub fn new(project_dir: &Path) -> Self {
        let mut out = Self::default();
        if let Some(path) = find_tonecurve_path(project_dir) {
            out.source_path = Some(path.clone());
            out.rows = load_tonecurve_rows(&path).ok();
        }
        out
    }

    pub fn has_table(&self) -> bool {
        self.rows.is_some()
    }

    pub fn shader_binding(
        &mut self,
        images: &mut ImageManager,
        tonecurve_no: i32,
    ) -> Option<(ImageId, f32, f32)> {
        let idx = tonecurve_no.max(0) as usize;
        let row = self.rows.as_ref()?.get(idx)?.as_ref()?.clone();
        let lut_id = self.ensure_lut_image(images)?;
        let row_y = ((idx.min(255) as f32) + 0.5) / 256.0;
        let sat = if row.sat < 0 {
            ((-row.sat) as f32 / 100.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Some((lut_id, row_y, sat))
    }

    fn ensure_lut_image(&mut self, images: &mut ImageManager) -> Option<ImageId> {
        if let Some(id) = self.lut_image_id {
            return Some(id);
        }
        let rows = self.rows.as_ref()?;
        let mut rgba = vec![0u8; 256 * 256 * 4];
        for y in 0..256usize {
            for x in 0..256usize {
                let idx = (y * 256 + x) * 4;
                if let Some(Some(row)) = rows.get(y) {
                    rgba[idx] = row.lut_r[x];
                    rgba[idx + 1] = row.lut_g[x];
                    rgba[idx + 2] = row.lut_b[x];
                } else {
                    let v = x as u8;
                    rgba[idx] = v;
                    rgba[idx + 1] = v;
                    rgba[idx + 2] = v;
                }
                rgba[idx + 3] = 255;
            }
        }
        let id = images.insert_image(RgbaImage {
            width: 256,
            height: 256,
            rgba,
        });
        self.lut_image_id = Some(id);
        Some(id)
    }

    pub fn apply_cached(
        &mut self,
        images: &mut ImageManager,
        base_id: ImageId,
        tonecurve_no: i32,
    ) -> Option<ImageId> {
        let rows = self.rows.as_ref()?;
        let row = rows.get(tonecurve_no.max(0) as usize)?.as_ref()?;
        let (base, version) = images.get_entry(base_id)?;
        if let Some(id) = self.cache.get(&(base_id, version, tonecurve_no)).copied() {
            return Some(id);
        }
        let toned = apply_tonecurve_to_image(base, row);
        let toned_id = images.insert_image(toned);
        self.cache
            .insert((base_id, version, tonecurve_no), toned_id);
        Some(toned_id)
    }
}

fn apply_tonecurve_to_image(src: &RgbaImage, row: &ToneCurveRow) -> RgbaImage {
    let mut out = src.clone();
    let mono_amt = if row.sat < 0 {
        ((-row.sat) as f32 / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for px in out.rgba.chunks_exact_mut(4) {
        let mut r = px[0] as f32;
        let mut g = px[1] as f32;
        let mut b = px[2] as f32;
        if mono_amt > 0.0 {
            let gray = (0.299 * r + 0.587 * g + 0.114 * b).round();
            r = r * (1.0 - mono_amt) + gray * mono_amt;
            g = g * (1.0 - mono_amt) + gray * mono_amt;
            b = b * (1.0 - mono_amt) + gray * mono_amt;
        }
        let ri = r.round().clamp(0.0, 255.0) as usize;
        let gi = g.round().clamp(0.0, 255.0) as usize;
        let bi = b.round().clamp(0.0, 255.0) as usize;
        px[0] = row.lut_r[ri];
        px[1] = row.lut_g[gi];
        px[2] = row.lut_b[bi];
    }
    out
}

fn find_tonecurve_path(project_dir: &Path) -> Option<PathBuf> {
    let cfg = load_gameexe_config(project_dir)?;
    let rel = cfg.get_unquoted("TONECURVE_FILE")?;
    if rel.trim().is_empty() {
        return None;
    }
    let p = PathBuf::from(rel.trim());
    if p.is_absolute() {
        return p.is_file().then_some(p);
    }
    let direct = project_dir.join(&p);
    if direct.is_file() {
        return Some(direct);
    }
    let dat = project_dir.join("dat").join(&p);
    if dat.is_file() {
        return Some(dat);
    }
    None
}

fn load_gameexe_config(project_dir: &Path) -> Option<GameexeConfig> {
    let path = find_gameexe_path(project_dir)?;
    let raw = std::fs::read(&path).ok()?;
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
    {
        let text = String::from_utf8(raw).ok()?;
        return Some(GameexeConfig::from_text(&text));
    }
    let opt = GameexeDecodeOptions::from_project_dir(project_dir).ok()?;
    let (text, _report) = decode_gameexe_dat_bytes(&raw, &opt).ok()?;
    Some(GameexeConfig::from_text(&text))
}

fn find_gameexe_path(project_dir: &Path) -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "Gameexe.dat",
        "Gameexe.ini",
        "gameexe.dat",
        "gameexe.ini",
        "GameexeEN.dat",
        "GameexeEN.ini",
        "GameexeZH.dat",
        "GameexeZH.ini",
        "GameexeZHTW.dat",
        "GameexeZHTW.ini",
        "GameexeDE.dat",
        "GameexeDE.ini",
        "GameexeES.dat",
        "GameexeES.ini",
        "GameexeFR.dat",
        "GameexeFR.ini",
        "GameexeID.dat",
        "GameexeID.ini",
    ];
    for name in CANDIDATES {
        let p = project_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn read_i32_le(data: &[u8], off: usize) -> Option<i32> {
    let bytes: [u8; 4] = data.get(off..off + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(bytes))
}

fn load_tonecurve_rows(path: &Path) -> Result<Vec<Option<ToneCurveRow>>> {
    let data = std::fs::read(path)?;
    if data.len() < 8 {
        anyhow::bail!("tonecurve file too small");
    }
    let cnt = read_i32_le(&data, 4).unwrap_or(0).clamp(0, 256) as usize;
    let mut out = vec![None; 256];
    for i in 0..cnt {
        let off = read_i32_le(&data, 8 + i * 4).unwrap_or(0);
        if off <= 0 {
            continue;
        }
        let off = off as usize;
        if off + 16 * 4 + 768 > data.len() {
            continue;
        }
        let typ = read_i32_le(&data, off).unwrap_or(0);
        let base = off + 16 * 4;
        let mut lut_r = [0u8; 256];
        let mut lut_g = [0u8; 256];
        let mut lut_b = [0u8; 256];
        lut_r.copy_from_slice(&data[base..base + 256]);
        lut_g.copy_from_slice(&data[base + 256..base + 512]);
        lut_b.copy_from_slice(&data[base + 512..base + 768]);
        let sat = if typ == 1 {
            read_i32_le(&data, base + 768).unwrap_or(0)
        } else {
            0
        };
        out[i] = Some(ToneCurveRow {
            lut_r,
            lut_g,
            lut_b,
            sat,
        });
    }
    Ok(out)
}
