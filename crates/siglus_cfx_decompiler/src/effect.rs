use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::cfx::ShaderBlob;
use crate::disasm::ShaderKind;

const FX_MAGIC_LE: u32 = 0xfeff_0901;
const FX_MAGIC_BYTESWAPPED: u32 = 0x0109_fffe;
const D3DXPC_SCALAR: u32 = 0;
const D3DXPC_VECTOR: u32 = 1;
const D3DXPC_MATRIX_ROWS: u32 = 2;
const D3DXPC_MATRIX_COLUMNS: u32 = 3;
const D3DXPC_OBJECT: u32 = 4;
const D3DXPC_STRUCT: u32 = 5;

const D3DXPT_STRING: u32 = 4;
const D3DXPT_TEXTURE: u32 = 5;
const D3DXPT_TEXTURE1D: u32 = 6;
const D3DXPT_TEXTURE2D: u32 = 7;
const D3DXPT_TEXTURE3D: u32 = 8;
const D3DXPT_TEXTURECUBE: u32 = 9;
const D3DXPT_SAMPLER: u32 = 10;
const D3DXPT_SAMPLER1D: u32 = 11;
const D3DXPT_SAMPLER2D: u32 = 12;
const D3DXPT_SAMPLER3D: u32 = 13;
const D3DXPT_SAMPLERCUBE: u32 = 14;
const D3DXPT_PIXELSHADER: u32 = 15;
const D3DXPT_VERTEXSHADER: u32 = 16;

#[derive(Debug, Clone)]
pub struct EffectFile {
    pub tag: u32,
    pub table_base: usize,
    pub start_offset: usize,
    pub parameter_count: u32,
    pub technique_count: u32,
    pub object_count: u32,
    pub string_count: u32,
    pub resource_count: u32,
    pub parameters: Vec<EffectParameter>,
    pub techniques: Vec<Technique>,
    pub objects: Vec<EffectObject>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EffectParameter {
    pub index: usize,
    pub name: String,
    pub semantic: String,
    pub value_type: u32,
    pub class: u32,
    pub rows: u32,
    pub columns: u32,
    pub elements: u32,
    pub bytes: u32,
    pub flags: u32,
    pub object_id: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Technique {
    pub index: usize,
    pub name: String,
    pub annotation_count: u32,
    pub passes: Vec<Pass>,
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub index: usize,
    pub name: String,
    pub annotation_count: u32,
    pub states: Vec<State>,
    pub vertex_shader: Option<ShaderRef>,
    pub pixel_shader: Option<ShaderRef>,
}

#[derive(Debug, Clone)]
pub struct State {
    pub index: usize,
    pub operation: u32,
    pub operation_name: String,
    pub class_name: String,
    pub state_index: u32,
    pub typedef_offset: u32,
    pub value_offset: u32,
    pub parameter: EffectParameter,
    pub value: StateValue,
    pub resource_usage: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum StateValue {
    Empty,
    ObjectId(u32),
    Int(Vec<i32>),
    Float(Vec<f32>),
    Bool(Vec<bool>),
    StringObject { object_id: u32, text: Option<String> },
    Raw { offset: u32, bytes: usize },
}

#[derive(Debug, Clone)]
pub struct ShaderRef {
    pub kind: ShaderKind,
    pub object_id: Option<u32>,
    pub object_data_offset: Option<usize>,
    pub object_size: usize,
    pub shader_index: Option<usize>,
    pub shader_offset: Option<usize>,
    pub unresolved_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EffectObject {
    pub id: usize,
    pub data_offset: Option<usize>,
    pub size: usize,
    pub owner_name: Option<String>,
    pub owner_type: Option<u32>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let off = self.pos;
        let b = self
            .data
            .get(off..off + 4)
            .ok_or_else(|| format!("truncated dword at 0x{off:x}"))?;
        self.pos += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn skip_dwords(&mut self, count: u32) -> Result<(), String> {
        let bytes = (count as usize)
            .checked_mul(4)
            .ok_or_else(|| "dword skip overflow".to_string())?;
        let new_pos = self
            .pos
            .checked_add(bytes)
            .ok_or_else(|| "reader overflow".to_string())?;
        if new_pos > self.data.len() {
            return Err(format!("skip beyond EOF: 0x{:x} > 0x{:x}", new_pos, self.data.len()));
        }
        self.pos = new_pos;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TypeDef {
    name: String,
    semantic: String,
    value_type: u32,
    class: u32,
    rows: u32,
    columns: u32,
    elements: u32,
    member_count: u32,
    bytes: u32,
}

pub fn parse_effect(data: &[u8], shaders: &[ShaderBlob]) -> Result<EffectFile, String> {
    if data.len() < 8 {
        return Err("file is too short for a D3DX effect header".to_string());
    }

    let tag = read_u32_at(data, 0)?;
    if tag != FX_MAGIC_LE && tag != FX_MAGIC_BYTESWAPPED {
        return Err(format!("not a D3DX9 compiled effect tag: 0x{tag:08x}"));
    }

    let table_base = 8usize;
    let start_rel = read_u32_at(data, 4)? as usize;
    let start_offset = table_base
        .checked_add(start_rel)
        .ok_or_else(|| "effect start offset overflow".to_string())?;
    if start_offset + 16 > data.len() {
        return Err(format!("effect start offset outside file: 0x{start_offset:x}"));
    }

    let mut notes = Vec::new();
    let mut objects = Vec::new();
    let mut r = Reader::new(data, start_offset);
    let parameter_count = r.read_u32()?;
    let technique_count = r.read_u32()?;
    let unknown = r.read_u32()?;
    let object_count = r.read_u32()?;
    if unknown != 0 {
        notes.push(format!("effect header unknown dword is 0x{unknown:08x}"));
    }
    for id in 0..object_count as usize {
        objects.push(EffectObject { id, ..EffectObject::default() });
    }

    let mut parameters = Vec::new();
    for index in 0..parameter_count as usize {
        let parameter = parse_top_level_parameter(data, table_base, &mut r, index, &mut objects)?;
        parameters.push(parameter);
    }

    let mut techniques = Vec::new();
    for index in 0..technique_count as usize {
        let technique = parse_technique(data, table_base, &mut r, index, &mut objects)?;
        techniques.push(technique);
    }

    // D3DX9 compiled-effect order is: string_count, resource_count,
    // string object payloads, then resource payloads.  Reading string
    // payloads before resource_count shifts the stream and makes pass
    // shader objects impossible to resolve.
    let string_count = if r.pos + 4 <= data.len() { r.read_u32()? } else { 0 };
    let resource_count = if r.pos + 4 <= data.len() { r.read_u32()? } else { 0 };

    for _ in 0..string_count {
        let object_id = r.read_u32()? as usize;
        copy_object_data(data, &mut r, object_id, &mut objects, &mut notes)?;
    }

    for resource_index in 0..resource_count {
        match parse_resource_like(data, &mut r, &mut techniques, &mut parameters, &mut objects, &mut notes) {
            Ok(()) => {}
            Err(e) => {
                notes.push(format!("resource {resource_index} parse stopped: {e}"));
                break;
            }
        }
    }

    resolve_shader_refs(&mut techniques, &objects, shaders);

    Ok(EffectFile {
        tag,
        table_base,
        start_offset,
        parameter_count,
        technique_count,
        object_count,
        string_count,
        resource_count,
        parameters,
        techniques,
        objects,
        notes,
    })
}

fn parse_top_level_parameter(
    data: &[u8],
    base: usize,
    r: &mut Reader<'_>,
    index: usize,
    objects: &mut [EffectObject],
) -> Result<EffectParameter, String> {
    let typedef_offset = r.read_u32()?;
    let value_offset = r.read_u32()?;
    let flags = r.read_u32()?;
    let annotation_count = r.read_u32()?;
    let td = parse_typedef_at(data, base, typedef_offset)?;
    let value = parse_value_at(data, base, value_offset, &td, objects)?;
    let mut param = parameter_from_typedef(index, td, flags, value.object_id());
    if let Some(id) = param.object_id {
        if let Some(obj) = objects.get_mut(id as usize) {
            obj.owner_name = Some(param.name.clone());
            obj.owner_type = Some(param.value_type);
        }
    }
    r.skip_dwords(annotation_count.saturating_mul(2))?;
    Ok(param)
}

fn parse_technique(
    data: &[u8],
    base: usize,
    r: &mut Reader<'_>,
    index: usize,
    objects: &mut [EffectObject],
) -> Result<Technique, String> {
    let name_offset = r.read_u32()?;
    let name = read_name_at(data, base, name_offset).unwrap_or_else(|_| format!("technique_{index}"));
    let annotation_count = r.read_u32()?;
    let pass_count = r.read_u32()?;
    r.skip_dwords(annotation_count.saturating_mul(2))?;

    let mut passes = Vec::new();
    for pass_index in 0..pass_count as usize {
        passes.push(parse_pass(data, base, r, pass_index, objects)?);
    }

    Ok(Technique { index, name, annotation_count, passes })
}

fn parse_pass(
    data: &[u8],
    base: usize,
    r: &mut Reader<'_>,
    index: usize,
    objects: &mut [EffectObject],
) -> Result<Pass, String> {
    let name_offset = r.read_u32()?;
    let name = read_name_at(data, base, name_offset).unwrap_or_else(|_| format!("pass{index}"));
    let annotation_count = r.read_u32()?;
    let state_count = r.read_u32()?;
    r.skip_dwords(annotation_count.saturating_mul(2))?;

    let mut states = Vec::new();
    let mut vertex_shader = None;
    let mut pixel_shader = None;
    for state_index in 0..state_count as usize {
        let state = parse_state(data, base, r, state_index, objects)?;
        if is_vertex_shader_state(&state) {
            vertex_shader = Some(ShaderRef {
                kind: ShaderKind::Vertex,
                object_id: state.value.object_id(),
                object_data_offset: None,
                object_size: 0,
                shader_index: None,
                shader_offset: None,
                unresolved_reason: None,
            });
        } else if is_pixel_shader_state(&state) {
            pixel_shader = Some(ShaderRef {
                kind: ShaderKind::Pixel,
                object_id: state.value.object_id(),
                object_data_offset: None,
                object_size: 0,
                shader_index: None,
                shader_offset: None,
                unresolved_reason: None,
            });
        }
        states.push(state);
    }

    Ok(Pass { index, name, annotation_count, states, vertex_shader, pixel_shader })
}

fn parse_state(
    data: &[u8],
    base: usize,
    r: &mut Reader<'_>,
    index: usize,
    objects: &mut [EffectObject],
) -> Result<State, String> {
    let operation = r.read_u32()?;
    let state_index = r.read_u32()?;
    let typedef_offset = r.read_u32()?;
    let td = parse_typedef_at(data, base, typedef_offset)?;
    let value_offset = r.read_u32()?;
    let parsed = parse_value_at(data, base, value_offset, &td, objects)?;
    let parameter = parameter_from_typedef(index, td, 0, parsed.object_id());
    let (operation_name, class_name) = operation_info(operation);
    Ok(State {
        index,
        operation,
        operation_name: operation_name.to_string(),
        class_name: class_name.to_string(),
        state_index,
        typedef_offset,
        value_offset,
        parameter,
        value: parsed.value,
        resource_usage: None,
    })
}

#[derive(Debug, Clone)]
struct ParsedValue {
    value: StateValue,
}

impl ParsedValue {
    fn object_id(&self) -> Option<u32> {
        self.value.object_id()
    }
}

impl StateValue {
    pub fn object_id(&self) -> Option<u32> {
        match self {
            StateValue::ObjectId(id) => Some(*id),
            StateValue::StringObject { object_id, .. } => Some(*object_id),
            _ => None,
        }
    }
}

fn parse_value_at(
    data: &[u8],
    base: usize,
    value_offset: u32,
    td: &TypeDef,
    objects: &mut [EffectObject],
) -> Result<ParsedValue, String> {
    let off = checked_base_offset(base, value_offset)?;
    if off > data.len() {
        return Err(format!("value offset outside file: 0x{off:x}"));
    }

    let value = if td.class == D3DXPC_OBJECT || is_object_type(td.value_type) {
        if off + 4 > data.len() {
            StateValue::Empty
        } else {
            let object_id = read_u32_at(data, off)?;
            if let Some(obj) = objects.get_mut(object_id as usize) {
                if !td.name.is_empty() {
                    obj.owner_name = Some(td.name.clone());
                }
                obj.owner_type = Some(td.value_type);
            }
            if td.value_type == D3DXPT_STRING {
                let text = objects.get(object_id as usize)
                    .and_then(|o| o.data_offset)
                    .and_then(|pos| read_nul_string(data, pos).ok());
                StateValue::StringObject { object_id, text }
            } else {
                StateValue::ObjectId(object_id)
            }
        }
    } else if td.value_type == 3 {
        let count = scalar_count(td).min(256);
        let mut values = Vec::new();
        for i in 0..count {
            let p = off + (i as usize) * 4;
            if p + 4 <= data.len() {
                values.push(f32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]));
            }
        }
        StateValue::Float(values)
    } else if td.value_type == 2 {
        let count = scalar_count(td).min(256);
        let mut values = Vec::new();
        for i in 0..count {
            let p = off + (i as usize) * 4;
            if p + 4 <= data.len() {
                values.push(i32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]));
            }
        }
        StateValue::Int(values)
    } else if td.value_type == 1 {
        let count = scalar_count(td).min(256);
        let mut values = Vec::new();
        for i in 0..count {
            let p = off + (i as usize) * 4;
            if p + 4 <= data.len() {
                values.push(read_u32_at(data, p)? != 0);
            }
        }
        StateValue::Bool(values)
    } else {
        StateValue::Raw { offset: value_offset, bytes: td.bytes as usize }
    };

    Ok(ParsedValue { value })
}

fn scalar_count(td: &TypeDef) -> u32 {
    let elems = if td.elements == 0 { 1 } else { td.elements };
    (td.rows.max(1)).saturating_mul(td.columns.max(1)).saturating_mul(elems)
}

fn parse_typedef_at(data: &[u8], base: usize, offset: u32) -> Result<TypeDef, String> {
    let pos = checked_base_offset(base, offset)?;
    let mut r = Reader::new(data, pos);
    let value_type = r.read_u32()?;
    let class = r.read_u32()?;
    let name_offset = r.read_u32()?;
    let name = read_name_at(data, base, name_offset).unwrap_or_default();
    let semantic_offset = r.read_u32()?;
    let semantic = read_name_at(data, base, semantic_offset).unwrap_or_default();
    let elements = r.read_u32()?;

    // Consume the same typedef fields as Wine/ReactOS d3dx_parse_effect_typedef().
    // Scalar/vector/matrix typedefs all carry row/column words.  The previous build
    // under-consumed scalar/vector typedefs, which shifted subsequent state parsing.
    let (rows, columns, member_count, object_size) = match class {
        D3DXPC_VECTOR => {
            let columns = r.read_u32()?;
            let rows = r.read_u32()?;
            (rows, columns, 0, rows.saturating_mul(columns).saturating_mul(4))
        }
        D3DXPC_SCALAR | D3DXPC_MATRIX_ROWS | D3DXPC_MATRIX_COLUMNS => {
            let rows = r.read_u32()?;
            let columns = r.read_u32()?;
            (rows, columns, 0, rows.saturating_mul(columns).saturating_mul(4))
        }
        D3DXPC_STRUCT => {
            let members = r.read_u32()?;
            (0, 0, members, 0)
        }
        D3DXPC_OBJECT => (0, 0, 0, 4),
        _ => (0, 0, 0, 0),
    };

    let elem_count = if elements == 0 { 1 } else { elements };
    let bytes = if class == D3DXPC_STRUCT {
        0
    } else if class == D3DXPC_OBJECT {
        if matches!(value_type, D3DXPT_SAMPLER | D3DXPT_SAMPLER1D | D3DXPT_SAMPLER2D | D3DXPT_SAMPLER3D | D3DXPT_SAMPLERCUBE) {
            0
        } else {
            object_size.saturating_mul(elem_count)
        }
    } else {
        object_size.saturating_mul(elem_count)
    };

    Ok(TypeDef { name, semantic, value_type, class, rows, columns, elements, member_count, bytes })
}

fn parameter_from_typedef(index: usize, td: TypeDef, flags: u32, object_id: Option<u32>) -> EffectParameter {
    EffectParameter {
        index,
        name: td.name,
        semantic: td.semantic,
        value_type: td.value_type,
        class: td.class,
        rows: td.rows,
        columns: td.columns,
        elements: td.elements,
        bytes: td.bytes,
        flags,
        object_id,
    }
}

fn copy_object_data(
    data: &[u8],
    r: &mut Reader<'_>,
    object_id: usize,
    objects: &mut [EffectObject],
    notes: &mut Vec<String>,
) -> Result<(), String> {
    let size = r.read_u32()? as usize;
    let start = r.pos;
    let end = start
        .checked_add(size)
        .ok_or_else(|| "object data size overflow".to_string())?;
    if end > data.len() {
        return Err(format!("object {object_id} data outside file: 0x{end:x}"));
    }
    if let Some(obj) = objects.get_mut(object_id) {
        obj.data_offset = Some(start);
        obj.size = size;
    } else {
        notes.push(format!("object data references out-of-range object id {object_id}"));
    }
    r.pos = align4(end);
    if r.pos > data.len() {
        return Err(format!("object {object_id} aligned data outside file"));
    }
    Ok(())
}

fn parse_resource_like(
    data: &[u8],
    r: &mut Reader<'_>,
    techniques: &mut [Technique],
    parameters: &mut [EffectParameter],
    objects: &mut [EffectObject],
    notes: &mut Vec<String>,
) -> Result<(), String> {
    // Wine d3dx_parse_resource() reads five dwords identifying the target state or
    // top-level sampler state, then d3dx9_copy_data() consumes a size-prefixed object
    // payload for that state's parameter.object_id.
    let rec_start = r.pos;
    let technique_index = r.read_u32()?;
    let index = r.read_u32()?;
    let element_index = r.read_u32()?;
    let state_index = r.read_u32()?;
    let usage = r.read_u32()?;

    let object_id = if technique_index == 0xffff_ffff {
        let param = parameters
            .get(index as usize)
            .ok_or_else(|| format!("resource parameter index {index} outside parameter table"))?;
        if element_index != 0xffff_ffff {
            notes.push(format!(
                "resource at 0x{rec_start:x} references parameter element {element_index}; using parent object id"
            ));
        }
        param.object_id.ok_or_else(|| {
            format!("resource at 0x{rec_start:x} top-level parameter {index} has no object id")
        })?
    } else {
        let tech = techniques
            .get_mut(technique_index as usize)
            .ok_or_else(|| format!("resource technique index {technique_index} outside technique table"))?;
        let pass = tech
            .passes
            .get_mut(index as usize)
            .ok_or_else(|| format!("resource pass index {index} outside technique {technique_index}"))?;
        let state = pass
            .states
            .get_mut(state_index as usize)
            .ok_or_else(|| format!("resource state index {state_index} outside technique {technique_index} pass {index}"))?;
        state.resource_usage = Some(usage);
        state.parameter.object_id.ok_or_else(|| {
            format!(
                "resource at 0x{rec_start:x} technique {technique_index} pass {index} state {state_index} has no object id"
            )
        })?
    };

    copy_object_data(data, r, object_id as usize, objects, notes)?;
    Ok(())
}

fn resolve_shader_refs(techniques: &mut [Technique], objects: &[EffectObject], shaders: &[ShaderBlob]) {
    for tech in techniques {
        for pass in &mut tech.passes {
            if let Some(sr) = &mut pass.vertex_shader {
                resolve_one_shader_ref(sr, objects, shaders);
            }
            if let Some(sr) = &mut pass.pixel_shader {
                resolve_one_shader_ref(sr, objects, shaders);
            }
        }
    }
}

fn resolve_one_shader_ref(sr: &mut ShaderRef, objects: &[EffectObject], shaders: &[ShaderBlob]) {
    let Some(id) = sr.object_id else {
        sr.unresolved_reason = Some("state did not contain an object id".to_string());
        return;
    };
    let Some(obj) = objects.get(id as usize) else {
        sr.unresolved_reason = Some(format!("object id {id} is outside object table"));
        return;
    };
    sr.object_data_offset = obj.data_offset;
    sr.object_size = obj.size;
    let Some(data_off) = obj.data_offset else {
        sr.unresolved_reason = Some(format!("object id {id} has no copied data payload"));
        return;
    };

    let by_offset = shaders
        .iter()
        .find(|s| s.kind == sr.kind && s.offset == data_off);
    let by_range = shaders.iter().find(|s| {
        s.kind == sr.kind
            && data_off >= s.offset
            && data_off < s.end_offset
            && s.offset >= data_off.saturating_sub(16)
    });
    let by_size = shaders.iter().find(|s| {
        s.kind == sr.kind
            && obj.size == s.bytes.len()
            && obj.data_offset == Some(s.offset)
    });
    let matched = by_offset.or(by_size).or(by_range);
    if let Some(shader) = matched {
        sr.shader_index = Some(shader.index);
        sr.shader_offset = Some(shader.offset);
        sr.unresolved_reason = None;
    } else {
        sr.unresolved_reason = Some(format!(
            "object id {id} payload offset=0x{data_off:x} size={} did not match any scanned {:?} shader",
            obj.size, sr.kind
        ));
    }
}

fn is_object_type(t: u32) -> bool {
    matches!(
        t,
        D3DXPT_STRING
            | D3DXPT_TEXTURE
            | D3DXPT_TEXTURE1D
            | D3DXPT_TEXTURE2D
            | D3DXPT_TEXTURE3D
            | D3DXPT_TEXTURECUBE
            | D3DXPT_SAMPLER
            | D3DXPT_SAMPLER1D
            | D3DXPT_SAMPLER2D
            | D3DXPT_SAMPLER3D
            | D3DXPT_SAMPLERCUBE
            | D3DXPT_PIXELSHADER
            | D3DXPT_VERTEXSHADER
    )
}

fn is_vertex_shader_state(state: &State) -> bool {
    state.operation == 146 || state.parameter.value_type == D3DXPT_VERTEXSHADER
}

fn is_pixel_shader_state(state: &State) -> bool {
    state.operation == 147 || state.parameter.value_type == D3DXPT_PIXELSHADER
}

pub fn operation_info(op: u32) -> (&'static str, &'static str) {
    match op {
        0 => ("ZEnable", "RenderState"),
        1 => ("FillMode", "RenderState"),
        2 => ("ShadeMode", "RenderState"),
        3 => ("ZWriteEnable", "RenderState"),
        4 => ("AlphaTestEnable", "RenderState"),
        5 => ("LastPixel", "RenderState"),
        6 => ("SrcBlend", "RenderState"),
        7 => ("DestBlend", "RenderState"),
        8 => ("CullMode", "RenderState"),
        9 => ("ZFunc", "RenderState"),
        10 => ("AlphaRef", "RenderState"),
        11 => ("AlphaFunc", "RenderState"),
        12 => ("DitherEnable", "RenderState"),
        13 => ("AlphaBlendEnable", "RenderState"),
        14 => ("FogEnable", "RenderState"),
        15 => ("SpecularEnable", "RenderState"),
        16 => ("FogColor", "RenderState"),
        17 => ("FogTableMode", "RenderState"),
        18 => ("FogStart", "RenderState"),
        19 => ("FogEnd", "RenderState"),
        20 => ("FogDensity", "RenderState"),
        21 => ("RangeFogEnable", "RenderState"),
        22 => ("StencilEnable", "RenderState"),
        23 => ("StencilFail", "RenderState"),
        24 => ("StencilZFail", "RenderState"),
        25 => ("StencilPass", "RenderState"),
        26 => ("StencilFunc", "RenderState"),
        27 => ("StencilRef", "RenderState"),
        28 => ("StencilMask", "RenderState"),
        29 => ("StencilWriteMask", "RenderState"),
        30 => ("TextureFactor", "RenderState"),
        31 => ("Wrap0", "RenderState"),
        32 => ("Wrap1", "RenderState"),
        33 => ("Wrap2", "RenderState"),
        34 => ("Wrap3", "RenderState"),
        35 => ("Wrap4", "RenderState"),
        36 => ("Wrap5", "RenderState"),
        37 => ("Wrap6", "RenderState"),
        38 => ("Wrap7", "RenderState"),
        39 => ("Clipping", "RenderState"),
        40 => ("Lighting", "RenderState"),
        41 => ("Ambient", "RenderState"),
        42 => ("FogVertexMode", "RenderState"),
        43 => ("ColorVertex", "RenderState"),
        44 => ("LocalViewer", "RenderState"),
        45 => ("NormalizeNormals", "RenderState"),
        46 => ("DiffuseMaterialSource", "RenderState"),
        47 => ("SpecularMaterialSource", "RenderState"),
        48 => ("AmbientMaterialSource", "RenderState"),
        49 => ("EmissiveMaterialSource", "RenderState"),
        50 => ("VertexBlend", "RenderState"),
        51 => ("ClipPlaneEnable", "RenderState"),
        52 => ("PointSize", "RenderState"),
        53 => ("PointSizeMin", "RenderState"),
        54 => ("PointSpriteEnable", "RenderState"),
        55 => ("PointScaleEnable", "RenderState"),
        56 => ("PointScaleA", "RenderState"),
        57 => ("PointScaleB", "RenderState"),
        58 => ("PointScaleC", "RenderState"),
        59 => ("MultiSampleAntiAlias", "RenderState"),
        60 => ("MultiSampleMask", "RenderState"),
        61 => ("PatchEdgeStyle", "RenderState"),
        62 => ("DebugMonitorToken", "RenderState"),
        63 => ("PointSizeMax", "RenderState"),
        64 => ("IndexedVertexBlendEnable", "RenderState"),
        65 => ("ColorWriteEnable", "RenderState"),
        66 => ("TweenFactor", "RenderState"),
        67 => ("BlendOp", "RenderState"),
        68 => ("PositionDegree", "RenderState"),
        69 => ("NormalDegree", "RenderState"),
        70 => ("ScissorTestEnable", "RenderState"),
        71 => ("SlopeScaleDepthBias", "RenderState"),
        72 => ("AntiAliasedLineEnable", "RenderState"),
        73 => ("MinTessellationLevel", "RenderState"),
        74 => ("MaxTessellationLevel", "RenderState"),
        75 => ("AdaptiveTessX", "RenderState"),
        76 => ("AdaptiveTessY", "RenderState"),
        77 => ("AdaptiveTessZ", "RenderState"),
        78 => ("AdaptiveTessW", "RenderState"),
        79 => ("EnableAdaptiveTessellation", "RenderState"),
        80 => ("TwoSidedStencilMode", "RenderState"),
        81 => ("CCWStencilFail", "RenderState"),
        82 => ("CCWStencilZFail", "RenderState"),
        83 => ("CCWStencilPass", "RenderState"),
        84 => ("CCWStencilFunc", "RenderState"),
        85 => ("ColorWriteEnable1", "RenderState"),
        86 => ("ColorWriteEnable2", "RenderState"),
        87 => ("ColorWriteEnable3", "RenderState"),
        88 => ("BlendFactor", "RenderState"),
        89 => ("SRGBWriteEnable", "RenderState"),
        90 => ("DepthBias", "RenderState"),
        91 => ("Wrap8", "RenderState"),
        92 => ("Wrap9", "RenderState"),
        93 => ("Wrap10", "RenderState"),
        94 => ("Wrap11", "RenderState"),
        95 => ("Wrap12", "RenderState"),
        96 => ("Wrap13", "RenderState"),
        97 => ("Wrap14", "RenderState"),
        98 => ("Wrap15", "RenderState"),
        99 => ("SeparateAlphaBlendEnable", "RenderState"),
        100 => ("SrcBlendAlpha", "RenderState"),
        101 => ("DestBlendAlpha", "RenderState"),
        102 => ("BlendOpAlpha", "RenderState"),
        146 => ("VertexShader", "VertexShader"),
        147 => ("PixelShader", "PixelShader"),
        _ => ("UnknownState", "Unknown"),
    }
}

fn read_u32_at(data: &[u8], off: usize) -> Result<u32, String> {
    let b = data
        .get(off..off + 4)
        .ok_or_else(|| format!("truncated dword at 0x{off:x}"))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn checked_base_offset(base: usize, offset: u32) -> Result<usize, String> {
    base.checked_add(offset as usize)
        .ok_or_else(|| format!("offset overflow: base=0x{base:x} offset=0x{offset:x}"))
}

fn read_name_at(data: &[u8], base: usize, offset: u32) -> Result<String, String> {
    if offset == 0 {
        return Ok(String::new());
    }
    let pos = checked_base_offset(base, offset)?;
    let len = read_u32_at(data, pos)? as usize;
    let start = pos + 4;
    let end = start
        .checked_add(len)
        .ok_or_else(|| "name length overflow".to_string())?;
    let bytes = data
        .get(start..end)
        .ok_or_else(|| format!("name outside file at 0x{pos:x} len={len}"))?;
    let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Ok(String::from_utf8_lossy(&bytes[..nul]).to_string())
}

fn read_nul_string(data: &[u8], pos: usize) -> Result<String, String> {
    let bytes = data.get(pos..).ok_or_else(|| format!("string offset outside file: 0x{pos:x}"))?;
    let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Ok(String::from_utf8_lossy(&bytes[..nul]).to_string())
}

fn align4(v: usize) -> usize {
    (v + 3) & !3
}

pub fn safe_name(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

pub fn format_effect_map(effect: &EffectFile, shaders: &[ShaderBlob]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "tag: 0x{:08x}", effect.tag);
    let _ = writeln!(s, "table_base: 0x{:08x}", effect.table_base);
    let _ = writeln!(s, "start_offset: 0x{:08x}", effect.start_offset);
    let _ = writeln!(s, "parameters: {}", effect.parameter_count);
    let _ = writeln!(s, "techniques: {}", effect.technique_count);
    let _ = writeln!(s, "objects: {}", effect.object_count);
    let _ = writeln!(s, "strings: {}", effect.string_count);
    let _ = writeln!(s, "resources: {}", effect.resource_count);
    if !effect.notes.is_empty() {
        let _ = writeln!(s, "notes:");
        for note in &effect.notes {
            let _ = writeln!(s, "  - {note}");
        }
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "top_level_parameters:");
    for p in &effect.parameters {
        let _ = writeln!(
            s,
            "  [{}] {} type={} class={} rows={} cols={} elems={} bytes={} object_id={:?} semantic={}",
            p.index, p.name, p.value_type, p.class, p.rows, p.columns, p.elements, p.bytes, p.object_id, p.semantic
        );
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "objects:");
    for o in &effect.objects {
        let _ = writeln!(
            s,
            "  [{}] owner={:?} type={:?} offset={} size={}",
            o.id,
            o.owner_name,
            o.owner_type,
            o.data_offset.map(|v| format!("0x{v:08x}")).unwrap_or_else(|| "none".to_string()),
            o.size
        );
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "shader_blobs:");
    for sh in shaders {
        let _ = writeln!(s, "  [{}] {} offset=0x{:08x} size={} ctab={}", sh.index, sh.profile(), sh.offset, sh.bytes.len(), sh.ctab.is_some());
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "techniques:");
    for tech in &effect.techniques {
        let _ = writeln!(s, "technique [{}] {} passes={}", tech.index, tech.name, tech.passes.len());
        for pass in &tech.passes {
            let _ = writeln!(s, "  pass [{}] {} states={}", pass.index, pass.name, pass.states.len());
            if let Some(vs) = &pass.vertex_shader {
                write_shader_ref(&mut s, "    VS", vs);
            }
            if let Some(ps) = &pass.pixel_shader {
                write_shader_ref(&mut s, "    PS", ps);
            }
            for state in &pass.states {
                let _ = writeln!(
                    s,
                    "    state [{}] op={} {} class={} index={} type={} param={} usage={:?} value={}",
                    state.index,
                    state.operation,
                    state.operation_name,
                    state.class_name,
                    state.state_index,
                    state.parameter.value_type,
                    state.parameter.name,
                    state.resource_usage,
                    format_state_value(&state.value)
                );
            }
        }
    }
    s
}

fn write_shader_ref(s: &mut String, label: &str, sr: &ShaderRef) {
    let _ = writeln!(
        s,
        "{} object_id={:?} object_offset={} object_size={} shader_index={:?} shader_offset={} unresolved={:?}",
        label,
        sr.object_id,
        sr.object_data_offset.map(|v| format!("0x{v:08x}")).unwrap_or_else(|| "none".to_string()),
        sr.object_size,
        sr.shader_index,
        sr.shader_offset.map(|v| format!("0x{v:08x}")).unwrap_or_else(|| "none".to_string()),
        sr.unresolved_reason
    );
}

fn format_state_value(v: &StateValue) -> String {
    match v {
        StateValue::Empty => "empty".to_string(),
        StateValue::ObjectId(id) => format!("object_id({id})"),
        StateValue::Int(xs) => format!("int{:?}", xs),
        StateValue::Float(xs) => format!("float{:?}", xs),
        StateValue::Bool(xs) => format!("bool{:?}", xs),
        StateValue::StringObject { object_id, text } => format!("string_object({object_id}, {:?})", text),
        StateValue::Raw { offset, bytes } => format!("raw(offset=0x{offset:x}, bytes={bytes})"),
    }
}

pub fn used_shader_indices(effect: &EffectFile) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for tech in &effect.techniques {
        for pass in &tech.passes {
            if let Some(vs) = &pass.vertex_shader {
                if let Some(i) = vs.shader_index {
                    out.insert(i);
                }
            }
            if let Some(ps) = &pass.pixel_shader {
                if let Some(i) = ps.shader_index {
                    out.insert(i);
                }
            }
        }
    }
    out
}

pub fn technique_shader_outputs<'a>(
    effect: &'a EffectFile,
    shaders: &'a [ShaderBlob],
) -> Vec<(String, &'a ShaderBlob)> {
    let by_index: BTreeMap<usize, &ShaderBlob> = shaders.iter().map(|s| (s.index, s)).collect();
    let mut out = Vec::new();
    for tech in &effect.techniques {
        let tech_name = safe_name(&tech.name, &format!("technique_{}", tech.index));
        for pass in &tech.passes {
            let pass_name = safe_name(&pass.name, &format!("pass{}", pass.index));
            let prefix_base = format!("t{:04}_{}__p{:02}_{}", tech.index, tech_name, pass.index, pass_name);
            if let Some(vs) = &pass.vertex_shader {
                if let Some(idx) = vs.shader_index {
                    if let Some(blob) = by_index.get(&idx) {
                        out.push((format!("{}__vs", prefix_base), *blob));
                    }
                }
            }
            if let Some(ps) = &pass.pixel_shader {
                if let Some(idx) = ps.shader_index {
                    if let Some(blob) = by_index.get(&idx) {
                        out.push((format!("{}__ps", prefix_base), *blob));
                    }
                }
            }
        }
    }
    out
}

pub fn write_outputs_for_blob(
    out_dir: &Path,
    prefix: &str,
    blob: &ShaderBlob,
    hlsl: &str,
    asm: &str,
    ctab_text: Option<&str>,
) -> std::io::Result<()> {
    std::fs::write(out_dir.join("bytecode").join(format!("{prefix}.bin")), &blob.bytes)?;
    std::fs::write(out_dir.join("hlsl").join(format!("{prefix}.hlsl")), hlsl)?;
    std::fs::write(out_dir.join("asm").join(format!("{prefix}.asm")), asm)?;
    if let Some(ctab) = ctab_text {
        std::fs::write(out_dir.join("hlsl").join(format!("{prefix}.ctab.txt")), ctab)?;
    }
    Ok(())
}
