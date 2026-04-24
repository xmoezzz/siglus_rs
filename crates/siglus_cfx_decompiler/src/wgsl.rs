use std::collections::{BTreeMap, BTreeSet};

use crate::ctab::{ConstantInfo, ConstantTable, RegisterSet, ValueType};
use crate::disasm::{
    mask_len, DeclUsage, Instruction, Opcode, RegisterKey, RegisterType, ResultModifier,
    SamplerTextureType, ShaderKind, SourceModifier,
};
use crate::disasm::{parse_shader, ShaderModel};

#[derive(Debug, Clone)]
struct DeclInfo {
    reg: RegisterKey,
    semantic: String,
}

#[derive(Debug, Clone)]
struct DefFloat {
    values: [f32; 4],
}

#[derive(Debug, Clone)]
struct DefInt {
    values: [i32; 4],
}

#[derive(Debug, Clone)]
struct DefBool {
    value: bool,
}

#[derive(Debug, Clone)]
struct Context<'a> {
    shader: &'a ShaderModel,
    ctab: Option<&'a ConstantTable>,
    decls: BTreeMap<RegisterKey, DeclInfo>,
    sampler_decls: BTreeMap<u16, SamplerTextureType>,
    def_float: BTreeMap<u16, DefFloat>,
    def_int: BTreeMap<u16, DefInt>,
    def_bool: BTreeMap<u16, DefBool>,
    used_inputs: BTreeSet<RegisterKey>,
    used_outputs: BTreeSet<RegisterKey>,
    used_temps: BTreeSet<RegisterKey>,
    used_samplers: BTreeSet<u16>,
    used_consts: BTreeSet<u16>,
    used_int_consts: BTreeSet<u16>,
    used_bool_consts: BTreeSet<u16>,
    uses_lit: bool,
    uses_dst: bool,
}

pub fn decompile_wgsl(data: &[u8], ctab: Option<&ConstantTable>) -> String {
    match parse_shader(data) {
        Ok(shader) => {
            let ctx = analyze(&shader, ctab);
            emit_wgsl(&ctx)
        }
        Err(_) => String::new(),
    }
}

fn emit_wgsl(ctx: &Context<'_>) -> String {
    let mut out = String::new();
    emit_register_bindings(&mut out, ctx);
    emit_texture_bindings(&mut out, ctx);
    emit_def_constants(&mut out, ctx);
    emit_helpers(&mut out, ctx);
    emit_structs(&mut out, ctx);
    emit_main(&mut out, ctx);
    out
}

fn emit_register_bindings(out: &mut String, ctx: &Context<'_>) {
    if !ctx.used_consts.is_empty() {
        out.push_str("struct FloatRegs {\n");
        out.push_str("    c: array<vec4<f32>, 256>,\n");
        out.push_str("};\n");
        out.push_str("@group(0) @binding(0) var<uniform> float_regs: FloatRegs;\n\n");
    }
    if !ctx.used_int_consts.is_empty() {
        out.push_str("struct IntRegs {\n");
        out.push_str("    i: array<vec4<i32>, 16>,\n");
        out.push_str("};\n");
        out.push_str("@group(0) @binding(1) var<uniform> int_regs: IntRegs;\n\n");
    }
    if !ctx.used_bool_consts.is_empty() {
        out.push_str("struct BoolRegs {\n");
        out.push_str("    b: array<u32, 16>,\n");
        out.push_str("};\n");
        out.push_str("@group(0) @binding(2) var<uniform> bool_regs: BoolRegs;\n\n");
    }
}

fn emit_texture_bindings(out: &mut String, ctx: &Context<'_>) {
    for sampler in &ctx.used_samplers {
        let tex_binding = (*sampler as u32) * 2;
        let samp_binding = tex_binding + 1;
        let tex_ty = wgsl_texture_type(sampler_texture_type(ctx, *sampler));
        out.push_str(&format!("@group(1) @binding({}) var tex_s{}: {};\n", tex_binding, sampler, tex_ty));
        out.push_str(&format!("@group(1) @binding({}) var samp_s{}: sampler;\n", samp_binding, sampler));
    }
    if !ctx.used_samplers.is_empty() {
        out.push('\n');
    }
}

fn emit_def_constants(out: &mut String, ctx: &Context<'_>) {
    let mut any = false;
    for (idx, def) in &ctx.def_float {
        out.push_str(&format!(
            "const c{}: vec4<f32> = vec4<f32>({}, {}, {}, {});\n",
            idx,
            fmt_f32(def.values[0]),
            fmt_f32(def.values[1]),
            fmt_f32(def.values[2]),
            fmt_f32(def.values[3])
        ));
        any = true;
    }
    for (idx, def) in &ctx.def_int {
        out.push_str(&format!(
            "const i{}: vec4<i32> = vec4<i32>({}, {}, {}, {});\n",
            idx, def.values[0], def.values[1], def.values[2], def.values[3]
        ));
        any = true;
    }
    for (idx, def) in &ctx.def_bool {
        out.push_str(&format!("const b{}: bool = {};\n", idx, if def.value { "true" } else { "false" }));
        any = true;
    }
    if any {
        out.push('\n');
    }
}

fn emit_helpers(out: &mut String, ctx: &Context<'_>) {
    if ctx.uses_lit {
        out.push_str("fn sm2_lit(v: vec4<f32>) -> vec4<f32> {\n");
        out.push_str("    let y = max(v.x, 0.0);\n");
        out.push_str("    let z = select(0.0, pow(max(v.y, 0.0), v.w), v.x > 0.0 && v.y > 0.0);\n");
        out.push_str("    return vec4<f32>(1.0, y, z, 1.0);\n");
        out.push_str("}\n\n");
    }
    if ctx.uses_dst {
        out.push_str("fn sm2_dst(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {\n");
        out.push_str("    return vec4<f32>(1.0, a.y * b.y, a.z, b.w);\n");
        out.push_str("}\n\n");
    }
}

fn emit_structs(out: &mut String, ctx: &Context<'_>) {
    let input_name = input_struct_name(ctx.shader.kind);
    let output_name = output_struct_name(ctx.shader.kind);

    out.push_str(&format!("struct {} {{\n", input_name));
    for decl in ctx.decls.values() {
        let field = input_field_name(decl.reg);
        let ty = wgsl_input_field_type(ctx, decl.reg);
        let attr = input_attr(ctx.shader.kind, &decl.semantic);
        out.push_str(&format!("    {} {}: {},\n", attr, field, ty));
    }
    out.push_str("};\n\n");

    out.push_str(&format!("struct {} {{\n", output_name));
    for reg in &ctx.used_outputs {
        let field = output_field_name(*reg);
        let sem = output_semantic(ctx, *reg);
        let ty = wgsl_output_field_type(*reg);
        let attr = output_attr(ctx.shader.kind, &sem, *reg);
        out.push_str(&format!("    {} {}: {},\n", attr, field, ty));
    }
    out.push_str("};\n\n");
}

fn emit_main(out: &mut String, ctx: &Context<'_>) {
    let stage = match ctx.shader.kind {
        ShaderKind::Vertex => "vertex",
        ShaderKind::Pixel => "fragment",
    };
    let input_name = input_struct_name(ctx.shader.kind);
    let output_name = output_struct_name(ctx.shader.kind);
    out.push_str(&format!("@{}\n", stage));
    out.push_str(&format!("fn main(input: {}) -> {} {{\n", input_name, output_name));
    out.push_str(&format!("    var output: {};\n", output_name));

    for reg in &ctx.used_outputs {
        out.push_str(&format!("    output.{} = {};\n", output_field_name(*reg), zero_value(wgsl_output_field_type(*reg))));
    }
    for reg in &ctx.used_temps {
        out.push_str(&format!("    var {}: {} = {};\n", temp_name(*reg, ctx.shader.kind), temp_type(*reg), zero_value(temp_type(*reg))));
    }
    if !ctx.used_outputs.is_empty() || !ctx.used_temps.is_empty() {
        out.push('\n');
    }

    let mut indent = 1usize;
    for inst in &ctx.shader.instructions {
        emit_instruction(out, ctx, inst, &mut indent);
    }

    out.push_str("    return output;\n");
    out.push_str("}\n");
}

fn emit_instruction(out: &mut String, ctx: &Context<'_>, inst: &Instruction, indent: &mut usize) {
    match inst.opcode {
        Opcode::Comment | Opcode::Dcl | Opcode::Def | Opcode::DefI | Opcode::DefB | Opcode::Nop | Opcode::End => {}
        Opcode::TexKill => {
            if let Some(reg) = inst.dest_register() {
                let expr = format!("{}{}", register_base(ctx, reg), wgsl_mask_suffix(inst.dest_write_mask()));
                let n = mask_len(inst.dest_write_mask());
                if n == 1 {
                    line(out, *indent, &format!("if ({} < 0.0) {{ discard; }}", expr));
                } else {
                    line(out, *indent, &format!("if (any({} < {})) {{ discard; }}", expr, zero_vector(n)));
                }
            }
        }
        Opcode::If => {
            let cond = source_expr(ctx, inst, 0, 1);
            line(out, *indent, &format!("if ({}) {{", scalar_bool_expr(cond)));
            *indent += 1;
        }
        Opcode::IfC => {
            let a = source_expr(ctx, inst, 0, 4);
            let b = source_expr(ctx, inst, 1, 4);
            line(out, *indent, &format!("if ({}) {{", compare_all_expr(a, cmp_op(inst.comparison()), b, 4)));
            *indent += 1;
        }
        Opcode::Else => {
            if *indent > 0 { *indent -= 1; }
            line(out, *indent, "} else {");
            *indent += 1;
        }
        Opcode::EndIf => {
            if *indent > 0 { *indent -= 1; }
            line(out, *indent, "}");
        }
        Opcode::Break => line(out, *indent, "break;"),
        Opcode::BreakC => {
            let a = source_expr(ctx, inst, 0, 4);
            let b = source_expr(ctx, inst, 1, 4);
            line(out, *indent, &format!("if ({}) {{ break; }}", compare_all_expr(a, cmp_op(inst.comparison()), b, 4)));
        }
        Opcode::Rep => {
            let n = source_expr(ctx, inst, 0, 1);
            line(out, *indent, &format!("for (var _rep{}: i32 = 0; _rep{} < i32({}); _rep{} = _rep{} + 1) {{", inst.offset, inst.offset, n, inst.offset, inst.offset));
            *indent += 1;
        }
        Opcode::Loop => {
            line(out, *indent, &format!("for (var _loop{}: i32 = 0; ; _loop{} = _loop{} + 1) {{", inst.offset, inst.offset, inst.offset));
            *indent += 1;
        }
        Opcode::EndRep | Opcode::EndLoop => {
            if *indent > 0 { *indent -= 1; }
            line(out, *indent, "}");
        }
        Opcode::Ret => line(out, *indent, "return output;"),
        _ if inst.opcode.has_destination() => {
            if let Some((dst, rhs)) = assignment_expr(ctx, inst) {
                line(out, *indent, &format!("{} = {};", dst, rhs));
            }
        }
        _ => {}
    }
}

fn assignment_expr(ctx: &Context<'_>, inst: &Instruction) -> Option<(String, String)> {
    let reg = inst.dest_register()?;
    let mask = inst.dest_write_mask();
    let dst_count = mask_len(mask);
    let dst = format!("{}{}", register_base(ctx, reg), wgsl_mask_suffix(mask));

    let (raw_rhs, raw_width) = match inst.opcode {
        Opcode::Mov | Opcode::MovA => (source_expr(ctx, inst, 1, dst_count), dst_count),
        Opcode::Add => (bin(ctx, inst, dst_count, "+"), dst_count),
        Opcode::Sub => (bin(ctx, inst, dst_count, "-"), dst_count),
        Opcode::Mul => (bin(ctx, inst, dst_count, "*"), dst_count),
        Opcode::Mad => (format!("({} * {} + {})", source_expr(ctx, inst, 1, dst_count), source_expr(ctx, inst, 2, dst_count), source_expr(ctx, inst, 3, dst_count)), dst_count),
        Opcode::Rcp => (format!("(1.0 / {})", source_expr(ctx, inst, 1, 1)), 1),
        Opcode::Rsq => (format!("inverseSqrt({})", source_expr(ctx, inst, 1, 1)), 1),
        Opcode::Dp3 => (format!("dot({}, {})", source_expr(ctx, inst, 1, 3), source_expr(ctx, inst, 2, 3)), 1),
        Opcode::Dp4 => (format!("dot({}, {})", source_expr(ctx, inst, 1, 4), source_expr(ctx, inst, 2, 4)), 1),
        Opcode::Min => (format!("min({}, {})", source_expr(ctx, inst, 1, dst_count), source_expr(ctx, inst, 2, dst_count)), dst_count),
        Opcode::Max => (format!("max({}, {})", source_expr(ctx, inst, 1, dst_count), source_expr(ctx, inst, 2, dst_count)), dst_count),
        Opcode::Slt => (select_float(ctx, inst, dst_count, "<"), dst_count),
        Opcode::Sge => (select_float(ctx, inst, dst_count, ">="), dst_count),
        Opcode::Exp | Opcode::ExpP => (format!("exp2({})", source_expr(ctx, inst, 1, 1)), 1),
        Opcode::Log | Opcode::LogP => (format!("log2({})", source_expr(ctx, inst, 1, 1)), 1),
        Opcode::Lit => (format!("sm2_lit({})", source_expr(ctx, inst, 1, 4)), 4),
        Opcode::Dst => (format!("sm2_dst({}, {})", source_expr(ctx, inst, 1, 4), source_expr(ctx, inst, 2, 4)), 4),
        Opcode::Lrp => (format!("mix({}, {}, {})", source_expr(ctx, inst, 3, dst_count), source_expr(ctx, inst, 2, dst_count), source_expr(ctx, inst, 1, dst_count)), dst_count),
        Opcode::Frc => (format!("fract({})", source_expr(ctx, inst, 1, dst_count)), dst_count),
        Opcode::Pow => (format!("pow({}, {})", source_expr(ctx, inst, 1, 1), source_expr(ctx, inst, 2, 1)), 1),
        Opcode::Crs => (format!("cross({}, {})", source_expr(ctx, inst, 1, 3), source_expr(ctx, inst, 2, 3)), 3),
        Opcode::Sgn => (format!("sign({})", source_expr(ctx, inst, 1, dst_count)), dst_count),
        Opcode::Abs => (format!("abs({})", source_expr(ctx, inst, 1, dst_count)), dst_count),
        Opcode::Nrm => (format!("normalize({})", source_expr(ctx, inst, 1, 3)), 3),
        Opcode::SinCos => {
            let width = if dst_count <= 1 { 1 } else { 2 };
            (sincos_expr(ctx, inst, dst_count), width)
        }
        Opcode::Cmp => (select_expr(ctx, inst, dst_count, ">="), dst_count),
        Opcode::Cnd => (select_expr(ctx, inst, dst_count, ">"), dst_count),
        Opcode::Dp2Add => (format!("(dot({}, {}) + {})", source_expr(ctx, inst, 1, 2), source_expr(ctx, inst, 2, 2), source_expr(ctx, inst, 3, 1)), 1),
        Opcode::M4x4 => (matrix_mul_expr(ctx, inst, 4, 4), 4),
        Opcode::M4x3 => (matrix_mul_expr(ctx, inst, 4, 3), 3),
        Opcode::M3x4 => (matrix_mul_expr(ctx, inst, 3, 4), 4),
        Opcode::M3x3 => (matrix_mul_expr(ctx, inst, 3, 3), 3),
        Opcode::M3x2 => (matrix_mul_expr(ctx, inst, 3, 2), 2),
        Opcode::Tex | Opcode::TexLdl | Opcode::TexLdd => (texture_expr(ctx, inst), 4),
        Opcode::TexCoord => (source_expr(ctx, inst, 1, dst_count), dst_count),
        Opcode::Dsx => (format!("dpdx({})", source_expr(ctx, inst, 1, dst_count)), dst_count),
        Opcode::Dsy => (format!("dpdy({})", source_expr(ctx, inst, 1, dst_count)), dst_count),
        _ => return None,
    };

    let mut rhs = coerce_expr_width(raw_rhs, raw_width, dst_count);
    rhs = apply_result_modifier(rhs, inst.dest_modifier(), dst_count);
    Some((dst, rhs))
}
fn bin(ctx: &Context<'_>, inst: &Instruction, n: usize, op: &str) -> String {
    format!("({} {} {})", source_expr(ctx, inst, 1, n), op, source_expr(ctx, inst, 2, n))
}

fn select_float(ctx: &Context<'_>, inst: &Instruction, n: usize, op: &str) -> String {
    let a = source_expr(ctx, inst, 1, n);
    let b = source_expr(ctx, inst, 2, n);
    let zero = zero_vector(n);
    let one = one_vector(n);
    format!("select({}, {}, {} {} {})", zero, one, a, op, b)
}

fn select_expr(ctx: &Context<'_>, inst: &Instruction, n: usize, op: &str) -> String {
    let a = source_expr(ctx, inst, 1, n);
    let yes = source_expr(ctx, inst, 2, n);
    let no = source_expr(ctx, inst, 3, n);
    let pivot = if op == ">" { half_vector(n) } else { zero_vector(n) };
    format!("select({}, {}, {} {} {})", no, yes, a, op, pivot)
}

fn sincos_expr(ctx: &Context<'_>, inst: &Instruction, n: usize) -> String {
    if n <= 1 {
        format!("cos({})", source_expr(ctx, inst, 1, 1))
    } else {
        format!("vec2<f32>(cos({}), sin({}))", source_expr(ctx, inst, 1, 1), source_expr(ctx, inst, 1, 1))
    }
}

fn matrix_mul_expr(ctx: &Context<'_>, inst: &Instruction, vec_len: usize, out_len: usize) -> String {
    let v = source_expr(ctx, inst, 1, vec_len);
    let mut rows = Vec::new();
    if let Some(reg) = inst.source_register(2) {
        if reg.ty == RegisterType::Const {
            for i in 0..out_len {
                rows.push(format!("dot({}, {})", v, const_row_expr(ctx, reg.number + i as u16)));
            }
        } else {
            for _ in 0..out_len {
                rows.push(format!("dot({}, {})", v, source_expr(ctx, inst, 2, vec_len)));
            }
        }
    }
    vector_constructor(&rows)
}

fn texture_expr(ctx: &Context<'_>, inst: &Instruction) -> String {
    let sampler = inst.source_register(2).map(|r| r.number).unwrap_or(0);
    let sampler_ty = sampler_texture_type(ctx, sampler);
    let dim = sampler_ty.hlsl_dim();
    let tex = format!("tex_s{}", sampler);
    let samp = format!("samp_s{}", sampler);
    let controls = inst.texld_controls();
    match inst.opcode {
        Opcode::TexLdl => {
            let coord = source_expr(ctx, inst, 1, dim);
            let lod = source_component_expr(ctx, inst, 1, 3);
            format!("textureSampleLevel({}, {}, {}, {})", tex, samp, coord, lod)
        }
        Opcode::TexLdd => {
            let coord = source_expr(ctx, inst, 1, dim);
            let ddx = source_expr(ctx, inst, 3, dim);
            let ddy = source_expr(ctx, inst, 4, dim);
            format!("textureSampleGrad({}, {}, {}, {}, {})", tex, samp, coord, ddx, ddy)
        }
        Opcode::Tex if controls == 1 => {
            let coord = source_expr(ctx, inst, 1, dim);
            let w = source_component_expr(ctx, inst, 1, 3);
            format!("textureSample({}, {}, ({} / {}))", tex, samp, coord, w)
        }
        Opcode::Tex if controls == 2 => {
            let coord = source_expr(ctx, inst, 1, dim);
            let bias = source_component_expr(ctx, inst, 1, 3);
            format!("textureSampleBias({}, {}, {}, {})", tex, samp, coord, bias)
        }
        _ => {
            let coord = source_expr(ctx, inst, 1, dim);
            format!("textureSample({}, {}, {})", tex, samp, coord)
        }
    }
}
fn source_expr(ctx: &Context<'_>, inst: &Instruction, param_index: usize, count: usize) -> String {
    let Some(reg) = inst.source_register(param_index) else { return zero_vector(count); };
    let count = count.clamp(1, 4);
    let mut expr = register_base(ctx, reg);
    if !register_is_scalar_source(ctx, reg) {
        let swz = inst.source_swizzle(param_index);
        expr.push_str(&wgsl_source_swizzle_suffix(swz, count));
    }
    apply_source_modifier(expr, inst.source_modifier(param_index), count)
}

fn source_component_expr(ctx: &Context<'_>, inst: &Instruction, param_index: usize, component: usize) -> String {
    let Some(reg) = inst.source_register(param_index) else { return "0.0".to_string(); };
    let mut expr = register_base(ctx, reg);
    if !register_is_scalar_source(ctx, reg) {
        let swz = inst.source_swizzle(param_index);
        let comp = swz[component.clamp(0, 3)];
        expr.push_str(component_suffix(comp));
    }
    apply_source_modifier(expr, inst.source_modifier(param_index), 1)
}

fn register_is_scalar_source(ctx: &Context<'_>, reg: RegisterKey) -> bool {
    match reg.ty {
        RegisterType::ConstBool | RegisterType::Loop | RegisterType::Label => true,
        RegisterType::MiscType if ctx.shader.kind == ShaderKind::Pixel && reg.number == 1 => true,
        _ => false,
    }
}
fn register_base(ctx: &Context<'_>, reg: RegisterKey) -> String {
    match reg.ty {
        RegisterType::Temp | RegisterType::TempFloat16 | RegisterType::Predicate => temp_name(reg, ctx.shader.kind),
        RegisterType::Texture if ctx.shader.kind == ShaderKind::Vertex => temp_name(reg, ctx.shader.kind),
        RegisterType::Texture | RegisterType::Input | RegisterType::MiscType => format!("input.{}", input_field_name(reg)),
        RegisterType::Const => const_row_expr(ctx, reg.number),
        RegisterType::ConstInt => int_const_expr(ctx, reg.number),
        RegisterType::ConstBool => bool_const_expr(ctx, reg.number),
        RegisterType::Sampler => format!("samp_s{}", reg.number),
        RegisterType::ColorOut | RegisterType::DepthOut | RegisterType::RastOut | RegisterType::AttrOut | RegisterType::Output => format!("output.{}", output_field_name(reg)),
        RegisterType::Loop => "_loop".to_string(),
        RegisterType::Label => format!("label{}", reg.number),
        _ => format!("u{}", reg.number),
    }
}

fn const_row_expr(ctx: &Context<'_>, index: u16) -> String {
    if ctx.def_float.contains_key(&index) {
        return format!("c{}", index);
    }
    format!("float_regs.c[{}]", index)
}

fn int_const_expr(ctx: &Context<'_>, index: u16) -> String {
    if ctx.def_int.contains_key(&index) {
        return format!("i{}", index);
    }
    format!("int_regs.i[{}]", index)
}

fn bool_const_expr(ctx: &Context<'_>, index: u16) -> String {
    if ctx.def_bool.contains_key(&index) {
        return format!("b{}", index);
    }
    format!("(bool_regs.b[{}] != 0u)", index)
}

fn analyze<'a>(shader: &'a ShaderModel, ctab: Option<&'a ConstantTable>) -> Context<'a> {
    let mut ctx = Context {
        shader,
        ctab,
        decls: BTreeMap::new(),
        sampler_decls: BTreeMap::new(),
        def_float: BTreeMap::new(),
        def_int: BTreeMap::new(),
        def_bool: BTreeMap::new(),
        used_inputs: BTreeSet::new(),
        used_outputs: BTreeSet::new(),
        used_temps: BTreeSet::new(),
        used_samplers: BTreeSet::new(),
        used_consts: BTreeSet::new(),
        used_int_consts: BTreeSet::new(),
        used_bool_consts: BTreeSet::new(),
        uses_lit: false,
        uses_dst: false,
    };

    for inst in &shader.instructions {
        match inst.opcode {
            Opcode::Dcl => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::Sampler {
                        ctx.sampler_decls.insert(reg.number, inst.decl_sampler_type());
                        ctx.used_samplers.insert(reg.number);
                    } else {
                        let semantic = semantic_from_decl(shader.kind, inst);
                        ctx.decls.insert(reg, DeclInfo { reg, semantic });
                    }
                }
            }
            Opcode::Def => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::Const {
                        ctx.def_float.insert(reg.number, DefFloat {
                            values: [inst.get_float_param(1), inst.get_float_param(2), inst.get_float_param(3), inst.get_float_param(4)],
                        });
                    }
                }
            }
            Opcode::DefI => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::ConstInt {
                        ctx.def_int.insert(reg.number, DefInt {
                            values: [inst.get_int_param(1), inst.get_int_param(2), inst.get_int_param(3), inst.get_int_param(4)],
                        });
                    }
                }
            }
            Opcode::DefB => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::ConstBool {
                        ctx.def_bool.insert(reg.number, DefBool { value: inst.get_int_param(1) != 0 });
                    }
                }
            }
            Opcode::Lit => ctx.uses_lit = true,
            Opcode::Dst => ctx.uses_dst = true,
            _ => {}
        }

        if inst.opcode == Opcode::End || inst.opcode == Opcode::Comment {
            continue;
        }

        if inst.opcode == Opcode::TexKill {
            if let Some(reg) = inst.dest_register() {
                classify_source_register(&mut ctx, reg);
            }
            continue;
        }

        if inst.opcode.has_destination() {
            if let Some(reg) = inst.dest_register() {
                classify_dest_register(&mut ctx, reg);
            }
        }

        let first_src = if inst.opcode.has_destination() { 1 } else { 0 };
        for pi in first_src..inst.params.len() {
            if matches!(inst.opcode, Opcode::Dcl | Opcode::Def | Opcode::DefI | Opcode::DefB) {
                continue;
            }
            if let Some(reg) = inst.source_register(pi) {
                classify_source_register(&mut ctx, reg);
            }
        }
    }

    infer_missing_decls(&mut ctx);
    ctx
}

fn classify_dest_register(ctx: &mut Context<'_>, reg: RegisterKey) {
    match reg.ty {
        RegisterType::Temp | RegisterType::TempFloat16 | RegisterType::Texture | RegisterType::Predicate => {
            if !(ctx.shader.kind == ShaderKind::Pixel && reg.ty == RegisterType::Texture) {
                ctx.used_temps.insert(reg);
            }
        }
        RegisterType::RastOut | RegisterType::AttrOut | RegisterType::Output | RegisterType::ColorOut | RegisterType::DepthOut => {
            ctx.used_outputs.insert(reg);
        }
        _ => {}
    }
}

fn classify_source_register(ctx: &mut Context<'_>, reg: RegisterKey) {
    match reg.ty {
        RegisterType::Input | RegisterType::MiscType => {
            ctx.used_inputs.insert(reg);
        }
        RegisterType::Texture => {
            if ctx.shader.kind == ShaderKind::Pixel {
                ctx.used_inputs.insert(reg);
            } else {
                ctx.used_temps.insert(reg);
            }
        }
        RegisterType::Temp | RegisterType::TempFloat16 | RegisterType::Predicate => {
            ctx.used_temps.insert(reg);
        }
        RegisterType::Const => {
            ctx.used_consts.insert(reg.number);
        }
        RegisterType::ConstInt => {
            ctx.used_int_consts.insert(reg.number);
        }
        RegisterType::ConstBool => {
            ctx.used_bool_consts.insert(reg.number);
        }
        RegisterType::Sampler => {
            ctx.used_samplers.insert(reg.number);
        }
        RegisterType::RastOut | RegisterType::AttrOut | RegisterType::Output | RegisterType::ColorOut | RegisterType::DepthOut => {
            ctx.used_outputs.insert(reg);
        }
        _ => {}
    }
}

fn infer_missing_decls(ctx: &mut Context<'_>) {
    for reg in ctx.used_inputs.clone() {
        if ctx.decls.contains_key(&reg) {
            continue;
        }
        let semantic = inferred_input_semantic(ctx.shader.kind, reg);
        ctx.decls.insert(reg, DeclInfo { reg, semantic });
    }
    for sampler in ctx.used_samplers.clone() {
        ctx.sampler_decls.entry(sampler).or_insert(SamplerTextureType::TwoD);
    }
    if ctx.shader.kind == ShaderKind::Pixel && ctx.used_outputs.is_empty() {
        ctx.used_outputs.insert(RegisterKey { ty: RegisterType::ColorOut, number: 0 });
    }
}

fn semantic_from_decl(kind: ShaderKind, inst: &Instruction) -> String {
    if let Some(reg) = inst.dest_register() {
        if reg.ty == RegisterType::MiscType {
            return match reg.number {
                0 => "VPOS".to_string(),
                1 => "VFACE".to_string(),
                _ => format!("TEXCOORD{}", reg.number),
            };
        }
        if kind == ShaderKind::Pixel {
            match reg.ty {
                RegisterType::Texture => return format!("TEXCOORD{}", reg.number),
                RegisterType::Input => return format!("COLOR{}", reg.number),
                _ => {}
            }
        }
    }
    let usage = inst.decl_usage();
    let index = inst.decl_index();
    let prefix = usage.semantic_prefix();
    if index == 0 && !matches!(usage, DeclUsage::TexCoord | DeclUsage::Color | DeclUsage::Position) {
        prefix.to_string()
    } else {
        format!("{}{}", prefix, index)
    }
}

fn inferred_input_semantic(kind: ShaderKind, reg: RegisterKey) -> String {
    match (kind, reg.ty) {
        (ShaderKind::Pixel, RegisterType::Texture) => format!("TEXCOORD{}", reg.number),
        (ShaderKind::Pixel, RegisterType::Input) => format!("COLOR{}", reg.number),
        (_, RegisterType::Input) => match reg.number {
            0 => "POSITION0".to_string(),
            1 => "NORMAL0".to_string(),
            n => format!("TEXCOORD{}", n.saturating_sub(2)),
        },
        (_, RegisterType::MiscType) => match reg.number {
            0 => "VPOS".to_string(),
            1 => "VFACE".to_string(),
            _ => format!("TEXCOORD{}", reg.number),
        },
        _ => format!("TEXCOORD{}", reg.number),
    }
}

fn output_semantic(ctx: &Context<'_>, reg: RegisterKey) -> String {
    if let Some(decl) = ctx.decls.get(&reg) {
        return decl.semantic.clone();
    }
    match reg.ty {
        RegisterType::ColorOut => format!("COLOR{}", reg.number),
        RegisterType::DepthOut => "DEPTH".to_string(),
        RegisterType::RastOut => match reg.number {
            0 => "POSITION0".to_string(),
            1 => "FOG".to_string(),
            2 => "PSIZE".to_string(),
            n => format!("TEXCOORD{}", n),
        },
        RegisterType::AttrOut => format!("COLOR{}", reg.number),
        RegisterType::Output => format!("TEXCOORD{}", reg.number),
        _ => format!("TEXCOORD{}", reg.number),
    }
}

fn input_attr(kind: ShaderKind, semantic: &str) -> String {
    let upper = semantic.to_ascii_uppercase();
    if kind == ShaderKind::Pixel && (upper == "VPOS" || upper == "POSITION" || upper == "POSITION0") {
        return "@builtin(position)".to_string();
    }
    if kind == ShaderKind::Pixel && upper == "VFACE" {
        return "@builtin(front_facing)".to_string();
    }
    format!("@location({})", semantic_location(semantic))
}
fn output_attr(kind: ShaderKind, semantic: &str, reg: RegisterKey) -> String {
    if kind == ShaderKind::Vertex && is_position_semantic(semantic) {
        "@builtin(position)".to_string()
    } else if reg.ty == RegisterType::DepthOut || semantic == "DEPTH" {
        "@builtin(frag_depth)".to_string()
    } else {
        format!("@location({})", semantic_location(semantic))
    }
}

fn semantic_location(semantic: &str) -> u32 {
    let upper = semantic.to_ascii_uppercase();
    if let Some(n) = parse_semantic_index(&upper, "POSITION") {
        return n;
    }
    if let Some(n) = parse_semantic_index(&upper, "NORMAL") {
        return 1 + n;
    }
    if let Some(n) = parse_semantic_index(&upper, "COLOR") {
        return 2 + n;
    }
    if let Some(n) = parse_semantic_index(&upper, "TEXCOORD") {
        return 4 + n;
    }
    if let Some(n) = parse_semantic_index(&upper, "BLENDWEIGHT") {
        return 12 + n;
    }
    if let Some(n) = parse_semantic_index(&upper, "BLENDINDICES") {
        return 14 + n;
    }
    if let Some(n) = parse_semantic_index(&upper, "TANGENT") {
        return 16 + n;
    }
    if let Some(n) = parse_semantic_index(&upper, "BINORMAL") {
        return 18 + n;
    }
    if upper == "FOG" {
        return 20;
    }
    if upper == "PSIZE" {
        return 21;
    }
    31
}

fn parse_semantic_index(s: &str, prefix: &str) -> Option<u32> {
    if !s.starts_with(prefix) {
        return None;
    }
    let rest = &s[prefix.len()..];
    if rest.is_empty() {
        Some(0)
    } else {
        rest.parse::<u32>().ok()
    }
}

fn is_position_semantic(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    upper == "POSITION" || upper == "POSITION0" || upper == "POSITIONT" || upper == "POSITIONT0"
}

fn wgsl_input_field_type(ctx: &Context<'_>, reg: RegisterKey) -> &'static str {
    match reg.ty {
        RegisterType::MiscType if reg.number == 1 => "bool",
        _ => {
            if let Some(decl) = ctx.decls.get(&reg) {
                if decl.semantic.starts_with("TEXCOORD") || decl.semantic.starts_with("COLOR") {
                    return "vec4<f32>";
                }
            }
            "vec4<f32>"
        }
    }
}

fn wgsl_output_field_type(reg: RegisterKey) -> &'static str {
    match reg.ty {
        RegisterType::DepthOut => "f32",
        RegisterType::RastOut if reg.number == 1 || reg.number == 2 => "f32",
        _ => "vec4<f32>",
    }
}

fn input_struct_name(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Vertex => "VSInput",
        ShaderKind::Pixel => "PSInput",
    }
}

fn output_struct_name(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Vertex => "VSOutput",
        ShaderKind::Pixel => "PSOutput",
    }
}

fn input_field_name(reg: RegisterKey) -> String {
    match reg.ty {
        RegisterType::Input => format!("v{}", reg.number),
        RegisterType::Texture => format!("t{}", reg.number),
        RegisterType::MiscType => match reg.number {
            0 => "vPos".to_string(),
            1 => "vFace".to_string(),
            _ => format!("vMisc{}", reg.number),
        },
        _ => format!("in{}", reg.number),
    }
}

fn output_field_name(reg: RegisterKey) -> String {
    match reg.ty {
        RegisterType::ColorOut => format!("oC{}", reg.number),
        RegisterType::DepthOut => "oDepth".to_string(),
        RegisterType::RastOut => match reg.number {
            0 => "oPos".to_string(),
            1 => "oFog".to_string(),
            2 => "oPts".to_string(),
            _ => format!("o{}", reg.number),
        },
        RegisterType::AttrOut => format!("oD{}", reg.number),
        RegisterType::Output => format!("o{}", reg.number),
        _ => format!("out{}", reg.number),
    }
}

fn temp_name(reg: RegisterKey, kind: ShaderKind) -> String {
    match reg.ty {
        RegisterType::Texture if kind == ShaderKind::Vertex => format!("a{}", reg.number),
        RegisterType::Predicate => format!("p{}", reg.number),
        RegisterType::TempFloat16 => format!("h{}", reg.number),
        _ => format!("r{}", reg.number),
    }
}

fn temp_type(reg: RegisterKey) -> &'static str {
    match reg.ty {
        RegisterType::Predicate => "vec4<bool>",
        RegisterType::Texture => "vec4<f32>",
        RegisterType::TempFloat16 => "vec4<f32>",
        _ => "vec4<f32>",
    }
}

fn zero_value(ty: &str) -> &'static str {
    match ty {
        "f32" => "0.0",
        "bool" => "false",
        "vec2<f32>" => "vec2<f32>(0.0)",
        "vec3<f32>" => "vec3<f32>(0.0)",
        "vec4<bool>" => "vec4<bool>(false)",
        _ => "vec4<f32>(0.0)",
    }
}

fn zero_vector(n: usize) -> String {
    match n {
        1 => "0.0".to_string(),
        2 => "vec2<f32>(0.0)".to_string(),
        3 => "vec3<f32>(0.0)".to_string(),
        _ => "vec4<f32>(0.0)".to_string(),
    }
}

fn one_vector(n: usize) -> String {
    match n {
        1 => "1.0".to_string(),
        2 => "vec2<f32>(1.0)".to_string(),
        3 => "vec3<f32>(1.0)".to_string(),
        _ => "vec4<f32>(1.0)".to_string(),
    }
}

fn half_vector(n: usize) -> String {
    match n {
        1 => "0.5".to_string(),
        2 => "vec2<f32>(0.5)".to_string(),
        3 => "vec3<f32>(0.5)".to_string(),
        _ => "vec4<f32>(0.5)".to_string(),
    }
}

fn vector_constructor(values: &[String]) -> String {
    match values.len() {
        0 => "0.0".to_string(),
        1 => values[0].clone(),
        2 => format!("vec2<f32>({}, {})", values[0], values[1]),
        3 => format!("vec3<f32>({}, {}, {})", values[0], values[1], values[2]),
        _ => format!("vec4<f32>({}, {}, {}, {})", values[0], values[1], values[2], values[3]),
    }
}

fn wgsl_mask_suffix(mask: u8) -> String {
    let m = if mask == 0 { 0xf } else { mask };
    if m == 0xf {
        String::new()
    } else {
        let mut s = String::from(".");
        for (i, c) in ['x', 'y', 'z', 'w'].iter().enumerate() {
            if (m & (1 << i)) != 0 {
                s.push(*c);
            }
        }
        s
    }
}

fn wgsl_source_swizzle_suffix(swizzle: [usize; 4], count: usize) -> String {
    let count = count.clamp(1, 4);
    let identity = [0usize, 1, 2, 3];
    if count == 4 && swizzle == identity {
        return String::new();
    }
    let names = ['x', 'y', 'z', 'w'];
    let mut s = String::from(".");
    for i in 0..count {
        s.push(names[swizzle[i]]);
    }
    s
}

fn component_suffix(component: usize) -> &'static str {
    match component {
        0 => ".x",
        1 => ".y",
        2 => ".z",
        3 => ".w",
        _ => ".x",
    }
}

fn coerce_expr_width(expr: String, src_width: usize, dst_width: usize) -> String {
    let src_width = src_width.clamp(1, 4);
    let dst_width = dst_width.clamp(1, 4);
    if src_width == dst_width {
        return expr;
    }
    if src_width == 1 {
        return match dst_width {
            1 => expr,
            2 => format!("vec2<f32>({})", expr),
            3 => format!("vec3<f32>({})", expr),
            _ => format!("vec4<f32>({})", expr),
        };
    }
    if src_width > dst_width {
        let suffix = match dst_width {
            1 => ".x",
            2 => ".xy",
            3 => ".xyz",
            _ => "",
        };
        if suffix.is_empty() { expr } else { format!("({}){}", expr, suffix) }
    } else {
        match dst_width {
            2 => format!("vec2<f32>({}, 0.0)", expr),
            3 => format!("vec3<f32>({}, 0.0)", expr),
            _ => format!("vec4<f32>({}, 0.0)", expr),
        }
    }
}
fn apply_source_modifier(expr: String, modifier: SourceModifier, count: usize) -> String {
    match modifier {
        SourceModifier::None => expr,
        SourceModifier::Negate => format!("-({})", expr),
        SourceModifier::Bias => format!("(({}) - {})", expr, half_vector(count)),
        SourceModifier::BiasAndNegate => format!("-(({}) - {})", expr, half_vector(count)),
        SourceModifier::Sign => format!("((({}) - {}) * {})", expr, half_vector(count), one_vector(count)),
        SourceModifier::SignAndNegate => format!("-((({}) - {}) * {})", expr, half_vector(count), one_vector(count)),
        SourceModifier::Complement => format!("({} - ({}))", one_vector(count), expr),
        SourceModifier::X2 => format!("(({}) * 2.0)", expr),
        SourceModifier::X2AndNegate => format!("-(({}) * 2.0)", expr),
        SourceModifier::DivideByZ => format!("(({}) / ({}).z)", expr, expr),
        SourceModifier::DivideByW => format!("(({}) / ({}).w)", expr, expr),
        SourceModifier::Abs => format!("abs({})", expr),
        SourceModifier::AbsAndNegate => format!("-abs({})", expr),
        SourceModifier::Not => format!("!({})", expr),
        SourceModifier::Unknown(_) => expr,
    }
}

fn apply_result_modifier(expr: String, modifier: ResultModifier, count: usize) -> String {
    if modifier.saturate {
        format!("clamp({}, {}, {})", expr, zero_vector(count), one_vector(count))
    } else {
        expr
    }
}

fn scalar_bool_expr(expr: String) -> String {
    format!("({} != 0.0)", expr)
}

fn line(out: &mut String, indent: usize, s: &str) {
    for _ in 0..indent {
        out.push_str("    ");
    }
    out.push_str(s);
    out.push('\n');
}

fn compare_all_expr(a: String, op: &str, b: String, width: usize) -> String {
    if width <= 1 {
        format!("{} {} {}", a, op, b)
    } else {
        format!("all({} {} {})", a, op, b)
    }
}

fn cmp_op(code: u8) -> &'static str {
    match code {
        1 => ">",
        2 => "==",
        3 => ">=",
        4 => "<",
        5 => "!=",
        6 => "<=",
        _ => "!=",
    }
}

fn sampler_texture_type(ctx: &Context<'_>, sampler: u16) -> SamplerTextureType {
    if let Some(c) = sampler_constant(ctx, sampler) {
        if let Some(t) = &c.type_info {
            return match t.value_type {
                ValueType::SamplerCube => SamplerTextureType::Cube,
                ValueType::Sampler3D => SamplerTextureType::Volume,
                _ => SamplerTextureType::TwoD,
            };
        }
    }
    ctx.sampler_decls.get(&sampler).copied().unwrap_or(SamplerTextureType::TwoD)
}

fn wgsl_texture_type(ty: SamplerTextureType) -> &'static str {
    match ty {
        SamplerTextureType::Cube => "texture_cube<f32>",
        SamplerTextureType::Volume => "texture_3d<f32>",
        SamplerTextureType::TwoD | SamplerTextureType::Unknown => "texture_2d<f32>",
    }
}

fn sampler_constant<'a>(ctx: &'a Context<'_>, sampler: u16) -> Option<&'a ConstantInfo> {
    ctx.ctab?.constants.iter().find(|c| c.register_set == RegisterSet::Sampler && c.register_index == sampler)
}

fn fmt_f32(v: f32) -> String {
    if v.is_nan() {
        "(0.0 / 0.0)".to_string()
    } else if v.is_infinite() {
        if v.is_sign_positive() { "(1.0 / 0.0)".to_string() } else { "(-1.0 / 0.0)".to_string() }
    } else {
        let mut s = format!("{:.9}", v);
        while s.contains('.') && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.push('0');
        }
        if !s.contains('.') && !s.contains('e') && !s.contains('E') {
            s.push_str(".0");
        }
        s
    }
}
