use anyhow::{anyhow, bail, Result};

use siglus_assets::scene_pck::CIndex;

/// All fields are little-endian i32 and all offsets are relative to the start of the chunk.
#[derive(Debug, Clone, Copy)]
pub struct ScnHeader {
    pub header_size: i32,
    pub scn_ofs: i32,
    pub scn_size: i32,
    pub str_index_list_ofs: i32,
    pub str_index_cnt: i32,
    pub str_list_ofs: i32,
    pub str_cnt: i32,
    pub label_list_ofs: i32,
    pub label_cnt: i32,
    pub z_label_list_ofs: i32,
    pub z_label_cnt: i32,
    pub cmd_label_list_ofs: i32,
    pub cmd_label_cnt: i32,
    pub scn_prop_list_ofs: i32,
    pub scn_prop_cnt: i32,
    pub scn_prop_name_index_list_ofs: i32,
    pub scn_prop_name_index_cnt: i32,
    pub scn_prop_name_list_ofs: i32,
    pub scn_prop_name_cnt: i32,
    pub scn_cmd_list_ofs: i32,
    pub scn_cmd_cnt: i32,
    pub scn_cmd_name_index_list_ofs: i32,
    pub scn_cmd_name_index_cnt: i32,
    pub scn_cmd_name_list_ofs: i32,
    pub scn_cmd_name_cnt: i32,
    pub call_prop_name_index_list_ofs: i32,
    pub call_prop_name_index_cnt: i32,
    pub call_prop_name_list_ofs: i32,
    pub call_prop_name_cnt: i32,
    pub namae_list_ofs: i32,
    pub namae_cnt: i32,
    pub read_flag_list_ofs: i32,
    pub read_flag_cnt: i32,
}

impl ScnHeader {
    pub fn read(chunk: &[u8]) -> Result<Self> {
        // `S_tnm_scn_header` has 33 i32 fields. We only read the early subset we need.
        let need = 33 * 4;
        if chunk.len() < need {
            bail!("scn: chunk too short for header");
        }
        let mut p = 0usize;
        let mut rd = || {
            let v = i32::from_le_bytes(chunk[p..p + 4].try_into().unwrap());
            p += 4;
            v
        };

        let header_size = rd();
        let scn_ofs = rd();
        let scn_size = rd();
        let str_index_list_ofs = rd();
        let str_index_cnt = rd();
        let str_list_ofs = rd();
        let str_cnt = rd();
        let label_list_ofs = rd();
        let label_cnt = rd();
        let z_label_list_ofs = rd();
        let z_label_cnt = rd();
        let cmd_label_list_ofs = rd();
        let cmd_label_cnt = rd();
        let scn_prop_list_ofs = rd();
        let scn_prop_cnt = rd();
        let scn_prop_name_index_list_ofs = rd();
        let scn_prop_name_index_cnt = rd();
        let scn_prop_name_list_ofs = rd();
        let scn_prop_name_cnt = rd();
        let scn_cmd_list_ofs = rd();
        let scn_cmd_cnt = rd();
        let scn_cmd_name_index_list_ofs = rd();
        let scn_cmd_name_index_cnt = rd();
        let scn_cmd_name_list_ofs = rd();
        let scn_cmd_name_cnt = rd();
        let call_prop_name_index_list_ofs = rd();
        let call_prop_name_index_cnt = rd();
        let call_prop_name_list_ofs = rd();
        let call_prop_name_cnt = rd();
        let namae_list_ofs = rd();
        let namae_cnt = rd();
        let read_flag_list_ofs = rd();
        let read_flag_cnt = rd();

        Ok(Self {
            header_size,
            scn_ofs,
            scn_size,
            str_index_list_ofs,
            str_index_cnt,
            str_cnt,
            str_list_ofs,
            label_list_ofs,
            label_cnt,
            z_label_list_ofs,
            z_label_cnt,
            cmd_label_list_ofs,
            cmd_label_cnt,
            scn_prop_list_ofs,
            scn_prop_cnt,
            scn_prop_name_index_list_ofs,
            scn_prop_name_index_cnt,
            scn_prop_name_list_ofs,
            scn_prop_name_cnt,
            scn_cmd_list_ofs,
            scn_cmd_cnt,
            scn_cmd_name_index_list_ofs,
            scn_cmd_name_index_cnt,
            scn_cmd_name_list_ofs,
            scn_cmd_name_cnt,
            call_prop_name_index_list_ofs,
            call_prop_name_index_cnt,
            call_prop_name_list_ofs,
            call_prop_name_cnt,
            namae_list_ofs,
            namae_cnt,
            read_flag_list_ofs,
            read_flag_cnt,
        })
    }
}

fn read_indexed_utf16_name_map(
    chunk: &[u8],
    index_list_ofs: usize,
    count: usize,
    list_ofs: usize,
) -> Result<std::collections::HashMap<u32, String>> {
    let mut out = std::collections::HashMap::new();
    if index_list_ofs + count * 8 > chunk.len() || list_ofs > chunk.len() {
        return Ok(out);
    }
    for i in 0..count {
        let idx = CIndex::read(chunk, index_list_ofs + i * 8)?;
        let o = idx.offset.max(0) as usize;
        let n = idx.size.max(0) as usize;
        let byte_off = list_ofs
            .checked_add(o * 2)
            .ok_or_else(|| anyhow!("scn: name offset overflow"))?;
        let byte_end = byte_off
            .checked_add(n * 2)
            .ok_or_else(|| anyhow!("scn: name size overflow"))?;
        if byte_end > chunk.len() {
            continue;
        }
        let mut u16s = Vec::with_capacity(n);
        for j in 0..n {
            let p = byte_off + j * 2;
            let w = u16::from_le_bytes([chunk[p], chunk[p + 1]]);
            if w == 0 {
                break;
            }
            u16s.push(w);
        }
        let s = String::from_utf16_lossy(&u16s);
        if !s.is_empty() {
            out.insert(i as u32, s);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct SceneStream<'a> {
    pub chunk: &'a [u8],
    pub header: ScnHeader,
    pub scn: &'a [u8],
    pub str_index_list: &'a [u8],
    pub str_list: &'a [u8],
    pub label_list: &'a [u8],
    pub z_label_list: &'a [u8],
    pub scn_prop_name_map: std::collections::HashMap<u32, String>,
    pub scn_cmd_name_map: std::collections::HashMap<u32, String>,
    pub call_prop_name_map: std::collections::HashMap<u32, String>,
    pub pc: usize,
}

impl<'a> SceneStream<'a> {
    pub fn new(chunk: &'a [u8]) -> Result<Self> {
        let header = ScnHeader::read(chunk)?;
        let scn_ofs = header.scn_ofs.max(0) as usize;
        let scn_size = header.scn_size.max(0) as usize;
        let scn_end = scn_ofs
            .checked_add(scn_size)
            .ok_or_else(|| anyhow!("scn: scn_size overflow"))?;
        if scn_end > chunk.len() {
            bail!("scn: scn stream out of bounds");
        }
        let scn = &chunk[scn_ofs..scn_end];

        let str_index_list_ofs = header.str_index_list_ofs.max(0) as usize;
        let str_index_cnt = header.str_index_cnt.max(0) as usize;
        let str_index_list_end = str_index_list_ofs
            .checked_add(str_index_cnt * 8)
            .ok_or_else(|| anyhow!("scn: str_index_list overflow"))?;
        if str_index_list_end > chunk.len() {
            bail!("scn: str_index_list out of bounds");
        }
        let str_index_list = &chunk[str_index_list_ofs..str_index_list_end];

        let str_list_ofs = header.str_list_ofs.max(0) as usize;
        if str_list_ofs > chunk.len() {
            bail!("scn: str_list_ofs out of bounds");
        }
        let str_list = &chunk[str_list_ofs..];

        let label_list_ofs = header.label_list_ofs.max(0) as usize;
        let label_cnt = header.label_cnt.max(0) as usize;
        let label_list_end = label_list_ofs
            .checked_add(label_cnt * 4)
            .ok_or_else(|| anyhow!("scn: label_list overflow"))?;
        if label_list_end > chunk.len() {
            bail!("scn: label_list out of bounds");
        }
        let label_list = &chunk[label_list_ofs..label_list_end];

        let z_label_list_ofs = header.z_label_list_ofs.max(0) as usize;
        let z_label_cnt = header.z_label_cnt.max(0) as usize;
        let z_label_list_end = z_label_list_ofs
            .checked_add(z_label_cnt * 4)
            .ok_or_else(|| anyhow!("scn: z_label_list overflow"))?;
        if z_label_list_end > chunk.len() {
            bail!("scn: z_label_list out of bounds");
        }
        let z_label_list = &chunk[z_label_list_ofs..z_label_list_end];

        let scn_prop_name_map = read_indexed_utf16_name_map(
            chunk,
            header.scn_prop_name_index_list_ofs.max(0) as usize,
            header.scn_prop_name_cnt.max(0) as usize,
            header.scn_prop_name_list_ofs.max(0) as usize,
        )?;
        let scn_cmd_name_map = read_indexed_utf16_name_map(
            chunk,
            header.scn_cmd_name_index_list_ofs.max(0) as usize,
            header.scn_cmd_name_cnt.max(0) as usize,
            header.scn_cmd_name_list_ofs.max(0) as usize,
        )?;
        let call_prop_name_map = read_indexed_utf16_name_map(
            chunk,
            header.call_prop_name_index_list_ofs.max(0) as usize,
            header.call_prop_name_cnt.max(0) as usize,
            header.call_prop_name_list_ofs.max(0) as usize,
        )?;

        Ok(Self {
            chunk,
            header,
            scn,
            str_index_list,
            str_list,
            label_list,
            z_label_list,
            scn_prop_name_map,
            scn_cmd_name_map,
            call_prop_name_map,
            pc: 0,
        })
    }

    pub fn eof(&self) -> bool {
        self.pc >= self.scn.len()
    }

    pub fn get_prg_cntr(&self) -> usize {
        self.pc
    }

    pub fn set_prg_cntr(&mut self, prg_cntr: usize) -> Result<()> {
        if prg_cntr > self.scn.len() {
            bail!("scn: prg_cntr out of bounds");
        }
        self.pc = prg_cntr;
        Ok(())
    }

    pub fn jump_to_label(&mut self, label_no: usize) -> Result<()> {
        let cnt = self.header.label_cnt.max(0) as usize;
        if label_no >= cnt {
            bail!("scn: label_no out of range");
        }
        let off = label_no * 4;
        let label_offset = i32::from_le_bytes(self.label_list[off..off + 4].try_into().unwrap());
        self.set_prg_cntr(label_offset.max(0) as usize)
    }

    pub fn jump_to_z_label(&mut self, z_no: usize) -> Result<()> {
        let cnt = self.header.z_label_cnt.max(0) as usize;
        if z_no >= cnt {
            bail!("scn: z_label out of range");
        }
        let off = z_no * 4;
        let z_offset = i32::from_le_bytes(self.z_label_list[off..off + 4].try_into().unwrap());
        self.set_prg_cntr(z_offset.max(0) as usize)
    }

    pub fn scn_cmd_offset(&self, cmd_no: usize) -> Result<usize> {
        let cnt = self.header.scn_cmd_cnt.max(0) as usize;
        if cmd_no >= cnt {
            bail!("scn: scn_cmd_no out of range");
        }
        let ofs = self.header.scn_cmd_list_ofs.max(0) as usize;
        let byte_ofs = ofs
            .checked_add(cmd_no * 4)
            .ok_or_else(|| anyhow!("scn: scn_cmd_list overflow"))?;
        let byte_end = byte_ofs
            .checked_add(4)
            .ok_or_else(|| anyhow!("scn: scn_cmd entry overflow"))?;
        if byte_end > self.chunk.len() {
            bail!("scn: scn_cmd_list out of bounds");
        }
        let cmd_offset = i32::from_le_bytes(self.chunk[byte_ofs..byte_end].try_into().unwrap());
        let prg = cmd_offset.max(0) as usize;
        if prg > self.scn.len() {
            bail!("scn: scn_cmd offset out of bounds");
        }
        Ok(prg)
    }

    pub fn pop_u8(&mut self) -> Result<u8> {
        if self.pc + 1 > self.scn.len() {
            bail!("scn: pop_u8 past end");
        }
        let v = self.scn[self.pc];
        self.pc += 1;
        Ok(v)
    }

    pub fn pop_u16(&mut self) -> Result<u16> {
        if self.pc + 2 > self.scn.len() {
            bail!("scn: pop_u16 past end");
        }
        let v = u16::from_le_bytes(self.scn[self.pc..self.pc + 2].try_into().unwrap());
        self.pc += 2;
        Ok(v)
    }

    pub fn pop_i32(&mut self) -> Result<i32> {
        if self.pc + 4 > self.scn.len() {
            bail!("scn: pop_i32 past end");
        }
        let v = i32::from_le_bytes(self.scn[self.pc..self.pc + 4].try_into().unwrap());
        self.pc += 4;
        Ok(v)
    }

    pub fn pop_str(&mut self) -> Result<String> {
        let str_id = self.pop_i32()?;
        self.get_string(str_id as usize)
    }

    pub fn get_string(&self, str_id: usize) -> Result<String> {
        let str_cnt = self.header.str_cnt.max(0) as usize;
        if str_id >= str_cnt {
            bail!("scn: str_id out of range");
        }
        let idx = CIndex::read(self.str_index_list, str_id * 8)?;
        let o = idx.offset.max(0) as usize;
        let n = idx.size.max(0) as usize;

        let byte_off = o
            .checked_mul(2)
            .ok_or_else(|| anyhow!("scn: str offset overflow"))?;
        let byte_end = byte_off
            .checked_add(n * 2)
            .ok_or_else(|| anyhow!("scn: str size overflow"))?;
        if byte_end > self.str_list.len() {
            bail!("scn: str data out of bounds");
        }

        // XOR per-u16: wchar ^= (28807 * str_index)
        let key = (28807u32).wrapping_mul(str_id as u32) as u16;

        let mut u16s = Vec::with_capacity(n);
        for j in 0..n {
            let p = byte_off + j * 2;
            let mut w = u16::from_le_bytes([self.str_list[p], self.str_list[p + 1]]);
            w ^= key;
            if w == 0 {
                break;
            }
            u16s.push(w);
        }
        Ok(String::from_utf16_lossy(&u16s))
    }
}
