use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};

use crate::lzss::lzss_unpack_lenient;

#[derive(Debug, Clone, Copy)]
pub struct CIndex {
    pub offset: i32,
    pub size: i32,
}

impl CIndex {
    pub fn read(buf: &[u8], off: usize) -> Result<Self> {
        if off + 8 > buf.len() {
            bail!("scene_pck: CIndex out of bounds");
        }
        let offset = i32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let size = i32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
        Ok(Self { offset, size })
    }
}

/// All fields are little-endian i32.
#[derive(Debug, Clone, Copy)]
pub struct PackScnHeader {
    pub header_size: i32,
    pub inc_prop_list_ofs: i32,
    pub inc_prop_cnt: i32,
    pub inc_prop_name_index_list_ofs: i32,
    pub inc_prop_name_index_cnt: i32,
    pub inc_prop_name_list_ofs: i32,
    pub inc_prop_name_cnt: i32,
    pub inc_cmd_list_ofs: i32,
    pub inc_cmd_cnt: i32,
    pub inc_cmd_name_index_list_ofs: i32,
    pub inc_cmd_name_index_cnt: i32,
    pub inc_cmd_name_list_ofs: i32,
    pub inc_cmd_name_cnt: i32,
    pub scn_name_index_list_ofs: i32,
    pub scn_name_index_cnt: i32,
    pub scn_name_list_ofs: i32,
    pub scn_name_cnt: i32,
    pub scn_data_index_list_ofs: i32,
    pub scn_data_index_cnt: i32,
    pub scn_data_list_ofs: i32,
    pub scn_data_cnt: i32,
    pub scn_data_exe_angou_mod: i32,
    pub original_source_header_size: i32,
}

impl PackScnHeader {
    pub fn read(buf: &[u8], off: usize, has_signature: bool) -> Result<Self> {
        // header size is stored in the first i32 (no signature in older builds).
        let min_need = if has_signature { 8 + 4 } else { 4 };
        if off + min_need > buf.len() {
            bail!("scene_pck: header out of bounds");
        }
        let mut p = off;
        if has_signature {
            if &buf[off..off + 8] != b"pack_scn" {
                bail!("scene_pck: bad signature (expected pack_scn)");
            }
            p += 8;
        }
        let mut rd = || {
            let v = i32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
            p += 4;
            v
        };
        let header_size = rd();
        let mut out = Self {
            header_size,
            inc_prop_list_ofs: rd(),
            inc_prop_cnt: rd(),
            inc_prop_name_index_list_ofs: rd(),
            inc_prop_name_index_cnt: rd(),
            inc_prop_name_list_ofs: rd(),
            inc_prop_name_cnt: rd(),
            inc_cmd_list_ofs: rd(),
            inc_cmd_cnt: rd(),
            inc_cmd_name_index_list_ofs: rd(),
            inc_cmd_name_index_cnt: rd(),
            inc_cmd_name_list_ofs: rd(),
            inc_cmd_name_cnt: rd(),
            scn_name_index_list_ofs: rd(),
            scn_name_index_cnt: rd(),
            scn_name_list_ofs: rd(),
            scn_name_cnt: rd(),
            scn_data_index_list_ofs: rd(),
            scn_data_index_cnt: rd(),
            scn_data_list_ofs: rd(),
            scn_data_cnt: rd(),
            scn_data_exe_angou_mod: rd(),
            original_source_header_size: rd(),
        };

        // Optional extra fields in newer headers (ignored for now).
        let header_bytes = header_size.max(0) as usize;
        let base_fields_bytes = 23 * 4;
        let extra_bytes = header_bytes.saturating_sub(base_fields_bytes);
        let extra_fields = extra_bytes / 4;
        if extra_fields > 0 {
            for _ in 0..extra_fields {
                let _ = rd();
            }
        }

        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct ScenePckDecodeOptions {
    /// Optional 16-byte exe angou element table (`TNM_EXE_ANGOU_ELEMENT_CNT`).
    pub exe_angou_element: Option<Vec<u8>>,
    /// Optional easy angou code table (`TNM_EASY_ANGOU_CODE_SIZE`, typically 256).
    pub easy_angou_code: Option<Vec<u8>>,
}

impl Default for ScenePckDecodeOptions {
    fn default() -> Self {
        Self {
            exe_angou_element: None,
            easy_angou_code: None,
        }
    }
}

impl ScenePckDecodeOptions {
    pub fn from_project_dir(project_dir: &Path) -> Result<Self> {
        let exe = crate::key_toml::load_key16_from_project_dir(project_dir)?.map(|v| v.to_vec());
        Ok(Self {
            exe_angou_element: exe,
            easy_angou_code: Some(crate::keys::SCENE_KEY.to_vec()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ScenePck {
    pub buf: Vec<u8>,
    pub header: PackScnHeader,
    pub scn_name_map: HashMap<String, usize>,
}

impl ScenePck {
    pub fn load_and_rebuild(path: &Path, opt: &ScenePckDecodeOptions) -> Result<Self> {
        let mut tmp = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        if tmp.len() < 4 {
            bail!("scene_pck: file too short");
        }
        let has_signature = tmp.len() >= 8 && &tmp[0..8] == b"pack_scn";
        let header = PackScnHeader::read(&tmp, 0, has_signature)?;
        let scn_data_list_ofs = header.scn_data_list_ofs as usize;
        if scn_data_list_ofs > tmp.len() {
            bail!("scene_pck: scn_data_list_ofs out of bounds");
        }

        // Rebuild m_scn_data exactly like the original implementation: keep everything before scn_data_list_ofs,
        // then append decrypted/decompressed scene chunks contiguously.
        let mut out = tmp[..scn_data_list_ofs].to_vec();

        // Load original index list from the input.
        let idx_ofs = header.scn_data_index_list_ofs as usize;
        let scn_cnt = if header.scn_data_cnt > 0 {
            header.scn_data_cnt as usize
        } else {
            header.scn_data_index_cnt.max(0) as usize
        };
        if idx_ofs + scn_cnt * 8 > tmp.len() {
            bail!("scene_pck: scn_data_index_list out of bounds");
        }
        let mut idx_list: Vec<CIndex> = Vec::with_capacity(scn_cnt);
        for i in 0..scn_cnt {
            idx_list.push(CIndex::read(&tmp, idx_ofs + i * 8)?);
        }

        let mut offset = idx_list
            .get(0)
            .map(|x| x.offset.max(0) as usize)
            .unwrap_or(0);
        if out.len() < scn_data_list_ofs + offset {
            out.resize(scn_data_list_ofs + offset, 0);
        }

        for scn_no in 0..scn_cnt {
            let entry = idx_list[scn_no];
            let mut new_size = 0usize;

            if entry.size > 0 {
                let sp_off = scn_data_list_ofs
                    .checked_add(entry.offset.max(0) as usize)
                    .ok_or_else(|| anyhow!("scene_pck: offset overflow"))?;
                let sp_end = sp_off
                    .checked_add(entry.size as usize)
                    .ok_or_else(|| anyhow!("scene_pck: size overflow"))?;
                if sp_end > tmp.len() {
                    bail!(
                        "scene_pck: scn chunk out of bounds (scn_no={}, end={}, len={})",
                        scn_no,
                        sp_end,
                        tmp.len()
                    );
                }

                let chunk = &mut tmp[sp_off..sp_end];

                let out_chunk: Vec<u8>;
                if header.original_source_header_size > 0 {
                    // exe angou element XOR (optional)
                    if header.scn_data_exe_angou_mod != 0 {
                        if let Some(exe_el) = opt.exe_angou_element.as_deref() {
                            if exe_el.is_empty() {
                                // nothing
                            } else {
                                let mut eac = 0usize;
                                for b in chunk.iter_mut() {
                                    *b ^= exe_el[eac];
                                    eac += 1;
                                    if eac >= exe_el.len() {
                                        eac = 0;
                                    }
                                }
                            }
                        }
                    }

                    // easy angou XOR (optional)
                    if let Some(easy) = opt.easy_angou_code.as_deref() {
                        if !easy.is_empty() {
                            let mut eac = 0usize;
                            for b in chunk.iter_mut() {
                                *b ^= easy[eac];
                                eac += 1;
                                if eac >= easy.len() {
                                    eac = 0;
                                }
                            }
                        }
                    }

                    out_chunk = lzss_unpack_lenient(chunk)
                        .with_context(|| format!("scene_pck: lzss unpack scn_no={}", scn_no))?;
                } else {
                    // Easy-link mode: keep the chunk bytes as-is.
                    out_chunk = chunk.to_vec();
                }

                new_size = out_chunk.len();
                let dst_off = scn_data_list_ofs + offset;
                let need_len = dst_off
                    .checked_add(new_size)
                    .ok_or_else(|| anyhow!("scene_pck: out size overflow"))?;
                if out.len() < need_len {
                    out.resize(need_len, 0);
                }
                out[dst_off..dst_off + new_size].copy_from_slice(&out_chunk);
            }

            // Patch the index list inside the output buffer.
            let out_idx_ofs = header.scn_data_index_list_ofs as usize;
            let out_entry_ofs = out_idx_ofs + scn_no * 8;
            if out_entry_ofs + 8 > out.len() {
                bail!("scene_pck: output index list out of bounds");
            }
            out[out_entry_ofs..out_entry_ofs + 4].copy_from_slice(&(offset as i32).to_le_bytes());
            out[out_entry_ofs + 4..out_entry_ofs + 8]
                .copy_from_slice(&(new_size as i32).to_le_bytes());

            offset = offset
                .checked_add(new_size)
                .ok_or_else(|| anyhow!("scene_pck: offset overflow"))?;
        }

        // Build name map.
        let mut scn_name_map = HashMap::new();
        let name_idx_ofs = header.scn_name_index_list_ofs as usize;
        let name_cnt = header.scn_name_cnt.max(0) as usize;
        let name_list_ofs = header.scn_name_list_ofs as usize;
        if name_idx_ofs + name_cnt * 8 <= out.len() && name_list_ofs <= out.len() {
            for i in 0..name_cnt {
                let idx = CIndex::read(&out, name_idx_ofs + i * 8)?;
                let o = idx.offset.max(0) as usize;
                let n = idx.size.max(0) as usize;
                let byte_off = name_list_ofs
                    .checked_add(o * 2)
                    .ok_or_else(|| anyhow!("scene_pck: name offset overflow"))?;
                let byte_end = byte_off
                    .checked_add(n * 2)
                    .ok_or_else(|| anyhow!("scene_pck: name size overflow"))?;
                if byte_end > out.len() {
                    continue;
                }
                let mut u16s = Vec::with_capacity(n);
                for j in 0..n {
                    let p = byte_off + j * 2;
                    let w = u16::from_le_bytes([out[p], out[p + 1]]);
                    if w == 0 {
                        break;
                    }
                    u16s.push(w);
                }
                let s = String::from_utf16_lossy(&u16s);
                if !s.is_empty() {
                    scn_name_map.insert(s, i);
                }
            }
        }

        Ok(Self {
            buf: out,
            header,
            scn_name_map,
        })
    }

    pub fn scn_data_slice(&self, scn_no: usize) -> Result<&[u8]> {
        let scn_cnt = self.header.scn_data_cnt.max(0) as usize;
        if scn_no >= scn_cnt {
            bail!("scene_pck: scn_no out of range");
        }
        let idx_ofs = self.header.scn_data_index_list_ofs as usize;
        let entry = CIndex::read(&self.buf, idx_ofs + scn_no * 8)?;
        if entry.size <= 0 {
            return Ok(&[]);
        }
        let base = self.header.scn_data_list_ofs as usize;
        let off = base
            .checked_add(entry.offset.max(0) as usize)
            .ok_or_else(|| anyhow!("scene_pck: offset overflow"))?;
        let end = off
            .checked_add(entry.size as usize)
            .ok_or_else(|| anyhow!("scene_pck: size overflow"))?;
        if end > self.buf.len() {
            bail!("scene_pck: scn slice out of bounds");
        }
        Ok(&self.buf[off..end])
    }

    pub fn find_scene_no(&self, name_or_index: &str) -> Option<usize> {
        if let Ok(i) = name_or_index.parse::<usize>() {
            return Some(i);
        }
        self.scn_name_map.get(name_or_index).copied()
    }
}

/// Helper for typical game directory layout.
pub fn find_scene_pck_in_project(project_dir: &Path) -> Result<std::path::PathBuf> {
    let candidates = [
        project_dir.join("Scene.pck"),
        project_dir.join("scene.pck"),
        project_dir.join("Data").join("Scene.pck"),
        project_dir.join("data").join("Scene.pck"),
    ];
    for p in candidates {
        if p.is_file() {
            return Ok(p);
        }
    }
    bail!("scene_pck: Scene.pck not found under {}", project_dir.display());
}
