use crate::error::{Error, Result};
use crate::reader::{checked_range, read_i32_array, read_index_array, Index, Reader};

#[derive(Debug, Clone)]
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
    pub const INT_COUNT: usize = 33;
    pub const BYTE_SIZE: usize = Self::INT_COUNT * 4;

    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < Self::BYTE_SIZE {
            return Err(Error::new("scene data is smaller than S_tnm_scn_header"));
        }
        let mut r = Reader::new(data);
        let header = Self {
            header_size: r.read_i32()?,
            scn_ofs: r.read_i32()?,
            scn_size: r.read_i32()?,
            str_index_list_ofs: r.read_i32()?,
            str_index_cnt: r.read_i32()?,
            str_list_ofs: r.read_i32()?,
            str_cnt: r.read_i32()?,
            label_list_ofs: r.read_i32()?,
            label_cnt: r.read_i32()?,
            z_label_list_ofs: r.read_i32()?,
            z_label_cnt: r.read_i32()?,
            cmd_label_list_ofs: r.read_i32()?,
            cmd_label_cnt: r.read_i32()?,
            scn_prop_list_ofs: r.read_i32()?,
            scn_prop_cnt: r.read_i32()?,
            scn_prop_name_index_list_ofs: r.read_i32()?,
            scn_prop_name_index_cnt: r.read_i32()?,
            scn_prop_name_list_ofs: r.read_i32()?,
            scn_prop_name_cnt: r.read_i32()?,
            scn_cmd_list_ofs: r.read_i32()?,
            scn_cmd_cnt: r.read_i32()?,
            scn_cmd_name_index_list_ofs: r.read_i32()?,
            scn_cmd_name_index_cnt: r.read_i32()?,
            scn_cmd_name_list_ofs: r.read_i32()?,
            scn_cmd_name_cnt: r.read_i32()?,
            call_prop_name_index_list_ofs: r.read_i32()?,
            call_prop_name_index_cnt: r.read_i32()?,
            call_prop_name_list_ofs: r.read_i32()?,
            call_prop_name_cnt: r.read_i32()?,
            namae_list_ofs: r.read_i32()?,
            namae_cnt: r.read_i32()?,
            read_flag_list_ofs: r.read_i32()?,
            read_flag_cnt: r.read_i32()?,
        };
        if header.header_size as usize != Self::BYTE_SIZE {
            return Err(Error::new(format!(
                "scene header_size is {}, expected {} from S_tnm_scn_header",
                header.header_size,
                Self::BYTE_SIZE
            )));
        }
        Ok(header)
    }
}

#[derive(Debug, Clone)]
pub struct ScnProp {
    pub form: i32,
    pub size: i32,
}

#[derive(Debug, Clone)]
pub struct ScnCmd {
    pub offset: i32,
}

#[derive(Debug, Clone)]
pub struct CmdLabel {
    pub cmd_id: i32,
    pub offset: i32,
}

#[derive(Debug, Clone)]
pub struct ReadFlag {
    pub line_no: i32,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub name: Option<String>,
    pub header: ScnHeader,
    pub code: Vec<u8>,
    pub strings: Vec<String>,
    pub labels: Vec<i32>,
    pub z_labels: Vec<i32>,
    pub cmd_labels: Vec<CmdLabel>,
    pub scn_props: Vec<ScnProp>,
    pub scn_prop_names: Vec<String>,
    pub scn_cmds: Vec<ScnCmd>,
    pub scn_cmd_names: Vec<String>,
    pub call_prop_names: Vec<String>,
    pub namae_list: Vec<i32>,
    pub read_flags: Vec<ReadFlag>,
    pub pack_inc_prop_cnt: usize,
    pub pack_inc_cmd_cnt: usize,
    pub pack_inc_props: Vec<ScnProp>,
    pub pack_inc_prop_names: Vec<String>,
    pub pack_inc_cmd_names: Vec<String>,
}

impl Scene {
    pub fn parse(name: Option<String>, data: &[u8]) -> Result<Self> {
        let header = ScnHeader::parse(data)?;
        let code_range = checked_range(
            data.len(),
            header.scn_ofs,
            header.scn_size,
            "scene bytecode",
        )?;
        let code = data[code_range].to_vec();

        let str_indices = read_index_array(
            data,
            header.str_index_list_ofs,
            header.str_index_cnt,
            "scene string index list",
        )?;
        let strings = parse_encrypted_strings(data, header.str_list_ofs, &str_indices)?;
        if header.str_cnt != header.str_index_cnt {
            return Err(Error::new(format!(
                "scene string count mismatch: str_cnt={} str_index_cnt={}",
                header.str_cnt, header.str_index_cnt
            )));
        }

        let labels = read_i32_array(data, header.label_list_ofs, header.label_cnt, "label list")?;
        let z_labels = read_i32_array(
            data,
            header.z_label_list_ofs,
            header.z_label_cnt,
            "z-label list",
        )?;
        let cmd_labels = read_cmd_labels(data, header.cmd_label_list_ofs, header.cmd_label_cnt)?;
        let scn_props = read_scn_props(data, header.scn_prop_list_ofs, header.scn_prop_cnt)?;
        let scn_prop_name_indices = read_index_array(
            data,
            header.scn_prop_name_index_list_ofs,
            header.scn_prop_name_index_cnt,
            "scene property name index list",
        )?;
        let scn_prop_names = parse_plain_string_table(
            data,
            header.scn_prop_name_list_ofs,
            &scn_prop_name_indices,
            "scene property names",
        )?;
        let scn_cmds = read_scn_cmds(data, header.scn_cmd_list_ofs, header.scn_cmd_cnt)?;
        let scn_cmd_name_indices = read_index_array(
            data,
            header.scn_cmd_name_index_list_ofs,
            header.scn_cmd_name_index_cnt,
            "scene command name index list",
        )?;
        let scn_cmd_names = parse_plain_string_table(
            data,
            header.scn_cmd_name_list_ofs,
            &scn_cmd_name_indices,
            "scene command names",
        )?;
        let call_prop_name_indices = read_index_array(
            data,
            header.call_prop_name_index_list_ofs,
            header.call_prop_name_index_cnt,
            "call property name index list",
        )?;
        let call_prop_names = parse_plain_string_table(
            data,
            header.call_prop_name_list_ofs,
            &call_prop_name_indices,
            "call property names",
        )?;
        let namae_list =
            read_i32_array(data, header.namae_list_ofs, header.namae_cnt, "namae list")?;
        let read_flags = read_read_flags(data, header.read_flag_list_ofs, header.read_flag_cnt)?;

        Ok(Self {
            name,
            header,
            code,
            strings,
            labels,
            z_labels,
            cmd_labels,
            scn_props,
            scn_prop_names,
            scn_cmds,
            scn_cmd_names,
            call_prop_names,
            namae_list,
            read_flags,
            pack_inc_prop_cnt: 0,
            pack_inc_cmd_cnt: 0,
            pack_inc_props: Vec::new(),
            pack_inc_prop_names: Vec::new(),
            pack_inc_cmd_names: Vec::new(),
        })
    }

    pub fn string(&self, index: i32) -> String {
        if index < 0 {
            return format!("<bad-string:{index}>");
        }
        self.strings
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| format!("<bad-string:{index}>"))
    }
}

fn read_cmd_labels(data: &[u8], ofs: i32, cnt: i32) -> Result<Vec<CmdLabel>> {
    if cnt < 0 {
        return Err(Error::new("cmd label list has negative count"));
    }
    let range = checked_range(data.len(), ofs, cnt * 8, "cmd label list")?;
    let mut r = Reader::with_pos(data, range.start)?;
    let mut out = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        out.push(CmdLabel {
            cmd_id: r.read_i32()?,
            offset: r.read_i32()?,
        });
    }
    Ok(out)
}

fn read_scn_props(data: &[u8], ofs: i32, cnt: i32) -> Result<Vec<ScnProp>> {
    if cnt < 0 {
        return Err(Error::new("scene prop list has negative count"));
    }
    let range = checked_range(data.len(), ofs, cnt * 8, "scene prop list")?;
    let mut r = Reader::with_pos(data, range.start)?;
    let mut out = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        out.push(ScnProp {
            form: r.read_i32()?,
            size: r.read_i32()?,
        });
    }
    Ok(out)
}

fn read_scn_cmds(data: &[u8], ofs: i32, cnt: i32) -> Result<Vec<ScnCmd>> {
    if cnt < 0 {
        return Err(Error::new("scene cmd list has negative count"));
    }
    let range = checked_range(data.len(), ofs, cnt * 4, "scene cmd list")?;
    let mut r = Reader::with_pos(data, range.start)?;
    let mut out = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        out.push(ScnCmd {
            offset: r.read_i32()?,
        });
    }
    Ok(out)
}

fn read_read_flags(data: &[u8], ofs: i32, cnt: i32) -> Result<Vec<ReadFlag>> {
    if cnt < 0 {
        return Err(Error::new("read flag list has negative count"));
    }
    let range = checked_range(data.len(), ofs, cnt * 4, "read flag list")?;
    let mut r = Reader::with_pos(data, range.start)?;
    let mut out = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        out.push(ReadFlag {
            line_no: r.read_i32()?,
        });
    }
    Ok(out)
}

fn parse_encrypted_strings(data: &[u8], base_ofs: i32, indices: &[Index]) -> Result<Vec<String>> {
    if base_ofs < 0 {
        return Err(Error::new("string table has negative base offset"));
    }
    let base = base_ofs as usize;
    let mut out = Vec::with_capacity(indices.len());
    for (logical_index, index) in indices.iter().enumerate() {
        if index.offset < 0 || index.size < 0 {
            return Err(Error::new(format!(
                "string index {logical_index} has negative offset or size"
            )));
        }
        let char_off = (index.offset as usize)
            .checked_mul(2)
            .ok_or_else(|| Error::new("string offset overflow"))?;
        let start = base
            .checked_add(char_off)
            .ok_or_else(|| Error::new("string offset overflow"))?;
        let byte_size = (index.size as usize)
            .checked_mul(2)
            .ok_or_else(|| Error::new("string byte size overflow"))?;
        let range = start..start + byte_size;
        if range.end > data.len() {
            return Err(Error::new(format!(
                "string index {logical_index} is outside scene data"
            )));
        }
        let key = (28807u32.wrapping_mul(logical_index as u32)) as u16;
        let mut words = Vec::with_capacity(index.size as usize);
        let mut r = Reader::with_pos(data, start)?;
        for _ in 0..index.size {
            words.push(r.read_u16()? ^ key);
        }
        out.push(String::from_utf16_lossy(&words));
    }
    Ok(out)
}

fn parse_plain_string_table(
    data: &[u8],
    base_ofs: i32,
    indices: &[Index],
    what: &str,
) -> Result<Vec<String>> {
    if base_ofs < 0 {
        return Err(Error::new(format!("{what} has negative base offset")));
    }
    let base = base_ofs as usize;
    let mut out = Vec::with_capacity(indices.len());
    for (i, index) in indices.iter().enumerate() {
        if index.offset < 0 || index.size < 0 {
            return Err(Error::new(format!(
                "{what} index {i} has negative offset or size"
            )));
        }
        let char_off = (index.offset as usize)
            .checked_mul(2)
            .ok_or_else(|| Error::new(format!("{what} offset overflow")))?;
        let start = base
            .checked_add(char_off)
            .ok_or_else(|| Error::new(format!("{what} offset overflow")))?;
        let byte_size = (index.size as usize)
            .checked_mul(2)
            .ok_or_else(|| Error::new(format!("{what} byte size overflow")))?;
        let range = start..start + byte_size;
        if range.end > data.len() {
            return Err(Error::new(format!(
                "{what} index {i} is outside scene data"
            )));
        }
        let mut words = Vec::with_capacity(index.size as usize);
        let mut r = Reader::with_pos(data, start)?;
        for _ in 0..index.size {
            words.push(r.read_u16()?);
        }
        out.push(String::from_utf16_lossy(&words));
    }
    Ok(out)
}
