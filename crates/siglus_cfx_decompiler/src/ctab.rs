use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterSet {
    Bool,
    Int4,
    Float4,
    Sampler,
    Unknown(u16),
}

impl RegisterSet {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::Bool,
            1 => Self::Int4,
            2 => Self::Float4,
            3 => Self::Sampler,
            x => Self::Unknown(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    Scalar,
    Vector,
    MatrixRows,
    MatrixColumns,
    Object,
    Struct,
    Unknown(u16),
}

impl TypeClass {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::Scalar,
            1 => Self::Vector,
            2 => Self::MatrixRows,
            3 => Self::MatrixColumns,
            4 => Self::Object,
            5 => Self::Struct,
            x => Self::Unknown(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Void,
    Bool,
    Int,
    Float,
    String,
    Texture,
    Texture1D,
    Texture2D,
    Texture3D,
    TextureCube,
    Sampler,
    Sampler1D,
    Sampler2D,
    Sampler3D,
    SamplerCube,
    PixelShader,
    VertexShader,
    PixelFragment,
    VertexFragment,
    Unsupported,
    Unknown(u16),
}

impl ValueType {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::Void,
            1 => Self::Bool,
            2 => Self::Int,
            3 => Self::Float,
            4 => Self::String,
            5 => Self::Texture,
            6 => Self::Texture1D,
            7 => Self::Texture2D,
            8 => Self::Texture3D,
            9 => Self::TextureCube,
            10 => Self::Sampler,
            11 => Self::Sampler1D,
            12 => Self::Sampler2D,
            13 => Self::Sampler3D,
            14 => Self::SamplerCube,
            15 => Self::PixelShader,
            16 => Self::VertexShader,
            17 => Self::PixelFragment,
            18 => Self::VertexFragment,
            19 => Self::Unsupported,
            x => Self::Unknown(x),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructMemberInfo {
    pub name: String,
    pub type_info: TypeInfo,
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub class: TypeClass,
    pub value_type: ValueType,
    pub rows: u16,
    pub columns: u16,
    pub elements: u16,
    pub struct_members: u16,
    pub struct_member_info_offset: u32,
    pub members: Vec<StructMemberInfo>,
}

impl TypeInfo {
    pub fn hlsl_type_name(&self) -> String {
        if self.class == TypeClass::Struct {
            return "struct".to_string();
        }
        match self.value_type {
            ValueType::Sampler | ValueType::Sampler2D => "sampler2D".to_string(),
            ValueType::Sampler1D => "sampler1D".to_string(),
            ValueType::Sampler3D => "sampler3D".to_string(),
            ValueType::SamplerCube => "samplerCUBE".to_string(),
            ValueType::Texture | ValueType::Texture2D => "texture".to_string(),
            ValueType::Texture1D => "texture1D".to_string(),
            ValueType::Texture3D => "texture3D".to_string(),
            ValueType::TextureCube => "textureCUBE".to_string(),
            ValueType::Bool => vector_or_matrix_type("bool", self.rows, self.columns, self.class),
            ValueType::Int => vector_or_matrix_type("int", self.rows, self.columns, self.class),
            ValueType::Float => vector_or_matrix_type("float", self.rows, self.columns, self.class),
            _ => "float4".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstantInfo {
    pub name: String,
    pub register_set: RegisterSet,
    pub register_index: u16,
    pub register_count: u16,
    pub type_info: Option<TypeInfo>,
}

impl ConstantInfo {
    pub fn register_name(&self) -> String {
        match self.register_set {
            RegisterSet::Bool => format!("b{}", self.register_index),
            RegisterSet::Int4 => format!("i{}", self.register_index),
            RegisterSet::Float4 => format!("c{}", self.register_index),
            RegisterSet::Sampler => format!("s{}", self.register_index),
            RegisterSet::Unknown(_) => format!("u{}", self.register_index),
        }
    }

    pub fn hlsl_decl_type(&self) -> String {
        let Some(t) = &self.type_info else {
            return match self.register_set {
                RegisterSet::Sampler => "sampler2D".to_string(),
                RegisterSet::Bool => "bool".to_string(),
                RegisterSet::Int4 => "int4".to_string(),
                RegisterSet::Float4 => "float4".to_string(),
                RegisterSet::Unknown(_) => "float4".to_string(),
            };
        };

        if t.class == TypeClass::Struct {
            return format!("{}_type", sanitize_type_name(&self.name));
        }

        match t.value_type {
            ValueType::Sampler | ValueType::Sampler2D => "sampler2D".to_string(),
            ValueType::Sampler1D => "sampler1D".to_string(),
            ValueType::Sampler3D => "sampler3D".to_string(),
            ValueType::SamplerCube => "samplerCUBE".to_string(),
            ValueType::Texture | ValueType::Texture2D => "texture".to_string(),
            ValueType::Texture1D => "texture1D".to_string(),
            ValueType::Texture3D => "texture3D".to_string(),
            ValueType::TextureCube => "textureCUBE".to_string(),
            ValueType::Bool => vector_or_matrix_type("bool", t.rows, t.columns, t.class),
            ValueType::Int => vector_or_matrix_type("int", t.rows, t.columns, t.class),
            ValueType::Float => vector_or_matrix_type("float", t.rows, t.columns, t.class),
            _ => match self.register_set {
                RegisterSet::Sampler => "sampler2D".to_string(),
                RegisterSet::Bool => "bool".to_string(),
                RegisterSet::Int4 => "int4".to_string(),
                RegisterSet::Float4 => "float4".to_string(),
                RegisterSet::Unknown(_) => "float4".to_string(),
            },
        }
    }

    pub fn struct_type_name(&self) -> Option<String> {
        let t = self.type_info.as_ref()?;
        if t.class == TypeClass::Struct {
            Some(format!("{}_type", sanitize_type_name(&self.name)))
        } else {
            None
        }
    }
}

fn sanitize_type_name(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        let ok = if i == 0 {
            ch == '_' || ch.is_ascii_alphabetic()
        } else {
            ch == '_' || ch.is_ascii_alphanumeric()
        };
        out.push(if ok { ch } else { '_' });
    }
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{}", out)
    } else {
        out
    }
}

fn vector_or_matrix_type(base: &str, rows: u16, columns: u16, class: TypeClass) -> String {
    match class {
        TypeClass::Scalar => base.to_string(),
        TypeClass::Vector => format!("{}{}", base, columns.max(rows).max(1)),
        TypeClass::MatrixRows | TypeClass::MatrixColumns => {
            if rows > 1 && columns > 1 {
                format!("{}{}x{}", base, rows, columns)
            } else {
                format!("{}4", base)
            }
        }
        _ => {
            if rows > 1 && columns > 1 {
                format!("{}{}x{}", base, rows, columns)
            } else if columns > 1 {
                format!("{}{}", base, columns)
            } else {
                base.to_string()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstantTable {
    pub creator: Option<String>,
    pub version: u32,
    pub flags: u32,
    pub target: Option<String>,
    pub constants: Vec<ConstantInfo>,
}

impl ConstantTable {
    pub fn find_register_name(&self, register_set: RegisterSet, index: u16) -> Option<&str> {
        self.constants
            .iter()
            .find(|c| c.register_set == register_set && c.register_index == index)
            .map(|c| c.name.as_str())
    }

    pub fn hlsl_uniforms(&self) -> String {
        let mut out = String::new();

        for c in &self.constants {
            if let (Some(type_name), Some(type_info)) = (c.struct_type_name(), c.type_info.as_ref()) {
                out.push_str(&format!("struct {} {{\n", type_name));
                for m in &type_info.members {
                    out.push_str(&format!("    {} {};\n", m.type_info.hlsl_type_name(), m.name));
                }
                out.push_str("};\n");
            }
        }

        for c in &self.constants {
            let ty = c.hlsl_decl_type();
            let reg = c.register_name();
            out.push_str(&format!("uniform {} {};\n", ty, c.name));
        }
        out
    }
}

#[derive(Debug, Clone)]
pub enum CtabError {
    TooSmall,
    BadMagic,
    OutOfBounds(&'static str, usize),
    InvalidStringOffset(u32),
}

impl fmt::Display for CtabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall => write!(f, "CTAB payload is too small"),
            Self::BadMagic => write!(f, "CTAB magic is missing"),
            Self::OutOfBounds(what, off) => write!(f, "{} is out of bounds at offset {}", what, off),
            Self::InvalidStringOffset(off) => write!(f, "invalid CTAB string offset {}", off),
        }
    }
}

impl std::error::Error for CtabError {}

fn read_u16(data: &[u8], off: usize) -> Result<u16, CtabError> {
    let b = data.get(off..off + 2).ok_or(CtabError::OutOfBounds("u16", off))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(data: &[u8], off: usize) -> Result<u32, CtabError> {
    let b = data.get(off..off + 4).ok_or(CtabError::OutOfBounds("u32", off))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_c_string(data: &[u8], off: u32) -> Result<String, CtabError> {
    let off = off as usize;
    if off >= data.len() {
        return Err(CtabError::InvalidStringOffset(off as u32));
    }
    let mut end = off;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    Ok(String::from_utf8_lossy(&data[off..end]).to_string())
}

fn parse_type_info(data: &[u8], off: usize, depth: usize) -> Result<TypeInfo, CtabError> {
    if depth > 16 {
        return Err(CtabError::OutOfBounds("recursive type info", off));
    }
    if off + 16 > data.len() {
        return Err(CtabError::OutOfBounds("type info", off));
    }

    let class = TypeClass::from_u16(read_u16(data, off)?);
    let value_type = ValueType::from_u16(read_u16(data, off + 2)?);
    let rows = read_u16(data, off + 4)?;
    let columns = read_u16(data, off + 6)?;
    let elements = read_u16(data, off + 8)?;
    let struct_members = read_u16(data, off + 10)?;
    let struct_member_info_offset = read_u32(data, off + 12)?;

    let mut members = Vec::new();
    if class == TypeClass::Struct && struct_member_info_offset != 0 {
        let member_base = struct_member_info_offset as usize;
        for idx in 0..struct_members as usize {
            let m_off = member_base + idx * 8;
            if m_off + 8 > data.len() {
                break;
            }
            let name_off = read_u32(data, m_off)?;
            let type_off = read_u32(data, m_off + 4)? as usize;
            let name = read_c_string(data, name_off).unwrap_or_else(|_| format!("member_{}", idx));
            if type_off != 0 && type_off + 16 <= data.len() {
                let type_info = parse_type_info(data, type_off, depth + 1)?;
                members.push(StructMemberInfo { name, type_info });
            }
        }
    }

    Ok(TypeInfo {
        class,
        value_type,
        rows,
        columns,
        elements,
        struct_members,
        struct_member_info_offset,
        members,
    })
}

pub fn parse_ctab(payload: &[u8]) -> Result<ConstantTable, CtabError> {
    if payload.len() < 28 {
        return Err(CtabError::TooSmall);
    }
    if payload.get(0..4) != Some(b"CTAB") {
        return Err(CtabError::BadMagic);
    }

    let size = read_u32(payload, 4)? as usize;
    let table_len = if size >= 28 && size <= payload.len() { size } else { payload.len() };
    let data = &payload[..table_len];

    let creator_off = read_u32(data, 8)?;
    let version = read_u32(data, 12)?;
    let constants_count = read_u32(data, 16)? as usize;
    let constant_info_off = read_u32(data, 20)? as usize;
    let flags = read_u32(data, 24)?;
    let target_off = if data.len() >= 32 { read_u32(data, 28).unwrap_or(0) } else { 0 };

    let creator = if creator_off != 0 { read_c_string(data, creator_off).ok() } else { None };
    let target = if target_off != 0 { read_c_string(data, target_off).ok() } else { None };

    let mut constants = Vec::with_capacity(constants_count);
    for idx in 0..constants_count {
        let off = constant_info_off + idx * 20;
        if off + 20 > data.len() {
            break;
        }

        let name_off = read_u32(data, off)?;
        let register_set = RegisterSet::from_u16(read_u16(data, off + 4)?);
        let register_index = read_u16(data, off + 6)?;
        let register_count = read_u16(data, off + 8)?;
        let type_info_off = read_u32(data, off + 12)? as usize;

        let name = read_c_string(data, name_off).unwrap_or_else(|_| format!("unnamed_{}", idx));
        let type_info = if type_info_off != 0 && type_info_off + 16 <= data.len() {
            parse_type_info(data, type_info_off, 0).ok()
        } else {
            None
        };

        constants.push(ConstantInfo {
            name,
            register_set,
            register_index,
            register_count,
            type_info,
        });
    }

    Ok(ConstantTable {
        creator,
        version,
        flags,
        target,
        constants,
    })
}
