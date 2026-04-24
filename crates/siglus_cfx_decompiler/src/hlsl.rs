use std::collections::{BTreeMap, BTreeSet};

use crate::ctab::{ConstantInfo, ConstantTable, RegisterSet, TypeClass, ValueType};
use crate::disasm::{
    mask_len, DeclUsage, Instruction, Opcode, RegisterKey, RegisterType, ResultModifier,
    SamplerTextureType, ShaderKind, SourceModifier,
};
use crate::disasm::{parse_shader, ShaderModel};

#[derive(Debug, Clone)]
struct DeclInfo {
    reg: RegisterKey,
    semantic: String,
    sampler_type: Option<SamplerTextureType>,
}

#[derive(Debug, Clone)]
struct DefFloat {
    reg: RegisterKey,
    values: [f32; 4],
}

#[derive(Debug, Clone)]
struct DefInt {
    reg: RegisterKey,
    values: [i32; 4],
}

#[derive(Debug, Clone)]
struct DefBool {
    reg: RegisterKey,
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
}

pub fn decompile_hlsl(data: &[u8], ctab: Option<&ConstantTable>) -> String {
    match parse_shader(data) {
        Ok(shader) => decompile_model(&shader, ctab),
        Err(_) => String::new(),
    }
}

pub fn decompile_model(shader: &ShaderModel, ctab: Option<&ConstantTable>) -> String {
    let ctx = analyze(shader, ctab);
    decompile_direct(&ctx)
}

fn decompile_direct(ctx: &Context<'_>) -> String {
    let shader = ctx.shader;
    let mut out = String::new();
    emit_uniforms(&mut out, ctx);
    emit_def_constants(&mut out, ctx);
    emit_structs(&mut out, ctx);
    emit_main(&mut out, ctx);

    out
}

fn can_use_folded_writer(shader: &ShaderModel) -> bool {
    shader.instructions.iter().all(|inst| {
        matches!(
            inst.opcode,
            Opcode::Comment
                | Opcode::Dcl
                | Opcode::Def
                | Opcode::DefI
                | Opcode::DefB
                | Opcode::Nop
                | Opcode::End
                | Opcode::Mov
                | Opcode::MovA
                | Opcode::Add
                | Opcode::Sub
                | Opcode::Mul
                | Opcode::Mad
                | Opcode::Rcp
                | Opcode::Rsq
                | Opcode::Dp3
                | Opcode::Dp4
                | Opcode::Min
                | Opcode::Max
                | Opcode::Slt
                | Opcode::Sge
                | Opcode::Exp
                | Opcode::ExpP
                | Opcode::Log
                | Opcode::LogP
                | Opcode::Frc
                | Opcode::Pow
                | Opcode::Abs
                | Opcode::Nrm
                | Opcode::SinCos
                | Opcode::Cmp
                | Opcode::Cnd
                | Opcode::Crs
                | Opcode::Dp2Add
                | Opcode::M4x4
                | Opcode::M4x3
                | Opcode::M3x4
                | Opcode::M3x3
                | Opcode::M3x2
                | Opcode::Tex
                | Opcode::TexLdl
                | Opcode::TexLdd
                | Opcode::TexKill
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ComponentKey {
    reg: RegisterKey,
    component: usize,
}

#[derive(Debug, Clone)]
struct FoldedState {
    values: BTreeMap<ComponentKey, String>,
    clip_exprs: Vec<String>,
}

fn decompile_folded(ctx: &Context<'_>) -> String {
    let mut state = FoldedState { values: BTreeMap::new(), clip_exprs: Vec::new() };
    initialize_folded_state(ctx, &mut state);

    for inst in &ctx.shader.instructions {
        fold_instruction(ctx, &mut state, inst);
    }

    let mut out = String::new();
    emit_uniforms(&mut out, ctx);
    emit_def_constants(&mut out, ctx);
    emit_structs(&mut out, ctx);

    let input_name = input_struct_name(ctx.shader.kind);
    let output_name = output_struct_name(ctx.shader.kind);
    out.push_str(&format!("{} main({} input) {{\n", output_name, input_name));
    out.push_str(&format!("    {} output;\n", output_name));

    for clip in &state.clip_exprs {
        out.push_str(&format!("    clip({});\n", clip));
    }

    for reg in &ctx.used_outputs {
        let ty = output_field_type(*reg);
        let value = folded_vector_expr(ctx, &state, *reg, vector_len_for_type(ty));
        out.push_str(&format!("    output.{} = {};\n", output_field_name(*reg), value));
    }

    out.push_str("    return output;\n");
    out.push_str("}\n");
    out
}

fn initialize_folded_state(ctx: &Context<'_>, state: &mut FoldedState) {
    for decl in ctx.decls.values() {
        let len = vector_len_for_type(input_field_type(ctx, decl.reg));
        for component in 0..len {
            let key = ComponentKey { reg: decl.reg, component };
            state.values.insert(key, format!("input.{}{}", input_field_name(decl.reg), component_suffix(component)));
        }
    }

    for reg in &ctx.used_outputs {
        for component in 0..vector_len_for_type(output_field_type(*reg)) {
            state.values.insert(ComponentKey { reg: *reg, component }, scalar_zero(output_field_type(*reg)).to_string());
        }
    }
}

fn fold_instruction(ctx: &Context<'_>, state: &mut FoldedState, inst: &Instruction) {
    match inst.opcode {
        Opcode::Comment | Opcode::Dcl | Opcode::Nop | Opcode::End => {}
        Opcode::Def | Opcode::DefI | Opcode::DefB => fold_def_instruction(state, inst),
        Opcode::TexKill => {
            if let Some(reg) = inst.dest_register() {
                let mut comps = Vec::new();
                for component in components_from_mask(inst.dest_write_mask()) {
                    comps.push(component_value(ctx, state, reg, component));
                }
                if !comps.is_empty() {
                    state.clip_exprs.push(vector_constructor(&comps));
                }
            }
        }
        _ if inst.opcode.has_destination() => {
            let Some(dst_reg) = inst.dest_register() else { return; };
            let dst_components = components_from_mask(inst.dest_write_mask());
            let old = state.values.clone();
            let mut next_values: Vec<(ComponentKey, String)> = Vec::new();
            for (write_index, dst_component) in dst_components.iter().copied().enumerate() {
                if let Some(mut expr) = folded_instruction_component(ctx, &old, inst, write_index, dst_component) {
                    expr = apply_result_modifier(expr, inst.dest_modifier());
                    next_values.push((ComponentKey { reg: dst_reg, component: dst_component }, expr));
                }
            }
            for (key, expr) in next_values {
                state.values.insert(key, expr);
            }
        }
        _ => {}
    }
}

fn fold_def_instruction(state: &mut FoldedState, inst: &Instruction) {
    let Some(reg) = inst.dest_register() else { return; };
    match inst.opcode {
        Opcode::Def => {
            for component in 0..4 {
                state.values.insert(ComponentKey { reg, component }, fmt_f32(inst.get_float_param(component + 1)));
            }
        }
        Opcode::DefI => {
            for component in 0..4 {
                state.values.insert(ComponentKey { reg, component }, inst.get_int_param(component + 1).to_string());
            }
        }
        Opcode::DefB => {
            state.values.insert(ComponentKey { reg, component: 0 }, if inst.get_int_param(1) != 0 { "true".to_string() } else { "false".to_string() });
        }
        _ => {}
    }
}

fn folded_instruction_component(
    ctx: &Context<'_>,
    old: &BTreeMap<ComponentKey, String>,
    inst: &Instruction,
    write_index: usize,
    dst_component: usize,
) -> Option<String> {
    let c = dst_component;
    let s = |param: usize, component: usize| source_component_expr(ctx, old, inst, param, component);
    Some(match inst.opcode {
        Opcode::Mov | Opcode::MovA => s(1, c),
        Opcode::Add => format!("({} + {})", s(1, c), s(2, c)),
        Opcode::Sub => format!("({} - {})", s(1, c), s(2, c)),
        Opcode::Mul => format!("({} * {})", s(1, c), s(2, c)),
        Opcode::Mad => format!("({} * {} + {})", s(1, c), s(2, c), s(3, c)),
        Opcode::Rcp => format!("(1.0 / {})", s(1, c)),
        Opcode::Rsq => format!("rsqrt({})", s(1, c)),
        Opcode::Dp3 => dot_expr_from_components(|component| s(1, component), |component| s(2, component), 3),
        Opcode::Dp4 => dot_expr_from_components(|component| s(1, component), |component| s(2, component), 4),
        Opcode::Min => format!("min({}, {})", s(1, c), s(2, c)),
        Opcode::Max => format!("max({}, {})", s(1, c), s(2, c)),
        Opcode::Slt => format!("({} < {} ? 1.0 : 0.0)", s(1, c), s(2, c)),
        Opcode::Sge => format!("({} >= {} ? 1.0 : 0.0)", s(1, c), s(2, c)),
        Opcode::Exp | Opcode::ExpP => format!("exp2({})", s(1, c)),
        Opcode::Log | Opcode::LogP => format!("log2({})", s(1, c)),
        Opcode::Frc => format!("frac({})", s(1, c)),
        Opcode::Pow => format!("pow({}, {})", s(1, c), s(2, c)),
        Opcode::Abs => format!("abs({})", s(1, c)),
        Opcode::Nrm => normalize_component_expr(|component| s(1, component), c),
        Opcode::SinCos => {
            if write_index == 0 { format!("cos({})", s(1, c)) } else { format!("sin({})", s(1, c)) }
        }
        Opcode::Cmp => format!("({} >= 0 ? {} : {})", s(1, c), s(2, c), s(3, c)),
        Opcode::Cnd => format!("({} > 0.5 ? {} : {})", s(1, c), s(2, c), s(3, c)),
        Opcode::Crs => cross_component_expr(|component| s(1, component), |component| s(2, component), c),
        Opcode::Dp2Add => format!("(({} * {}) + ({} * {}) + {})", s(1, 0), s(2, 0), s(1, 1), s(2, 1), s(3, 0)),
        Opcode::M4x4 => matrix_component_expr(ctx, inst, old, 4, c),
        Opcode::M4x3 => matrix_component_expr(ctx, inst, old, 4, c),
        Opcode::M3x4 => matrix_component_expr(ctx, inst, old, 3, c),
        Opcode::M3x3 => matrix_component_expr(ctx, inst, old, 3, c),
        Opcode::M3x2 => matrix_component_expr(ctx, inst, old, 3, c),
        Opcode::Tex | Opcode::TexLdl | Opcode::TexLdd => texture_component_expr(ctx, old, inst, c),
        _ => return None,
    })
}

fn source_component_expr(
    ctx: &Context<'_>,
    old: &BTreeMap<ComponentKey, String>,
    inst: &Instruction,
    param_index: usize,
    component_index: usize,
) -> String {
    let Some(reg) = inst.source_register(param_index) else { return "0.0".to_string(); };
    let mut component = component_index.min(3);
    if reg.ty != RegisterType::MiscType || reg.number != 1 {
        component = inst.source_swizzle(param_index)[component];
    }
    let expr = component_value_from(old, ctx, reg, component);
    apply_source_modifier(expr, inst.source_modifier(param_index))
}

fn component_value(ctx: &Context<'_>, state: &FoldedState, reg: RegisterKey, component: usize) -> String {
    component_value_from(&state.values, ctx, reg, component)
}

fn component_value_from(values: &BTreeMap<ComponentKey, String>, ctx: &Context<'_>, reg: RegisterKey, component: usize) -> String {
    if let Some(v) = values.get(&ComponentKey { reg, component }) {
        return v.clone();
    }

    match reg.ty {
        RegisterType::Input | RegisterType::Texture | RegisterType::MiscType => format!("input.{}{}", input_field_name(reg), component_suffix(component)),
        RegisterType::Const => const_component_expr(ctx, reg.number, component),
        RegisterType::ConstInt => format!("{}{}", int_const_expr(ctx, reg.number), component_suffix(component)),
        RegisterType::ConstBool => bool_const_expr(ctx, reg.number),
        RegisterType::Sampler => sampler_name(ctx, reg.number),
        RegisterType::ColorOut | RegisterType::DepthOut | RegisterType::RastOut | RegisterType::AttrOut | RegisterType::Output => {
            format!("output.{}{}", output_field_name(reg), component_suffix(component))
        }
        _ => format!("{}{}", temp_name(reg, ctx.shader.kind), component_suffix(component)),
    }
}

fn const_component_expr(ctx: &Context<'_>, index: u16, component: usize) -> String {
    let base = const_row_expr(ctx, index);
    format!("{}{}", base, component_suffix(component))
}

fn folded_vector_expr(ctx: &Context<'_>, state: &FoldedState, reg: RegisterKey, len: usize) -> String {
    let mut comps = Vec::new();
    for component in 0..len {
        comps.push(component_value(ctx, state, reg, component));
    }
    vector_constructor(&comps)
}

fn vector_constructor(comps: &[String]) -> String {
    match comps.len() {
        0 => "0.0".to_string(),
        1 => comps[0].clone(),
        2 => format!("float2({}, {})", comps[0], comps[1]),
        3 => format!("float3({}, {}, {})", comps[0], comps[1], comps[2]),
        _ => format!("float4({}, {}, {}, {})", comps[0], comps[1], comps[2], comps[3]),
    }
}

fn dot_expr_from_components<F, G>(mut a: F, mut b: G, len: usize) -> String
where
    F: FnMut(usize) -> String,
    G: FnMut(usize) -> String,
{
    let mut parts = Vec::new();
    for component in 0..len {
        parts.push(format!("({} * {})", a(component), b(component)));
    }
    format!("({})", parts.join(" + "))
}

fn normalize_component_expr<F>(mut value: F, component: usize) -> String
where
    F: FnMut(usize) -> String,
{
    let x = value(0);
    let y = value(1);
    let z = value(2);
    let denom = format!("sqrt(({} * {}) + ({} * {}) + ({} * {}))", x, x, y, y, z, z);
    format!("({} / {})", value(component.min(2)), denom)
}

fn cross_component_expr<F, G>(mut a: F, mut b: G, component: usize) -> String
where
    F: FnMut(usize) -> String,
    G: FnMut(usize) -> String,
{
    match component {
        0 => format!("(({}) * ({}) - ({}) * ({}))", a(1), b(2), a(2), b(1)),
        1 => format!("(({}) * ({}) - ({}) * ({}))", a(2), b(0), a(0), b(2)),
        _ => format!("(({}) * ({}) - ({}) * ({}))", a(0), b(1), a(1), b(0)),
    }
}

fn matrix_component_expr(
    ctx: &Context<'_>,
    inst: &Instruction,
    old: &BTreeMap<ComponentKey, String>,
    vec_len: usize,
    component: usize,
) -> String {
    let terms = (0..vec_len)
        .map(|i| {
            let v = source_component_expr(ctx, old, inst, 1, i);
            let m = if let Some(reg) = inst.source_register(2) {
                if reg.ty == RegisterType::Const {
                    const_component_expr(ctx, reg.number + component as u16, i)
                } else {
                    source_component_expr(ctx, old, inst, 2, i)
                }
            } else {
                "0.0".to_string()
            };
            format!("({} * {})", v, m)
        })
        .collect::<Vec<_>>();
    format!("({})", terms.join(" + "))
}

fn texture_component_expr(ctx: &Context<'_>, old: &BTreeMap<ComponentKey, String>, inst: &Instruction, component: usize) -> String {
    let sampler = inst.source_register(2).map(|r| r.number).unwrap_or(0);
    let sampler_name = sampler_name(ctx, sampler);
    let sampler_ty = sampler_texture_type(ctx, sampler);
    let dim = sampler_ty.hlsl_dim();
    let coord_components = if inst.opcode == Opcode::Tex { dim } else { 4 };
    let coord = (0..coord_components)
        .map(|i| source_component_expr(ctx, old, inst, 1, i))
        .collect::<Vec<_>>();
    let coord_expr = vector_constructor(&coord);
    let base = texture_intrinsic(inst, sampler_ty);
    let sample = match inst.opcode {
        Opcode::TexLdd => {
            let ddx = (0..dim).map(|i| source_component_expr(ctx, old, inst, 3, i)).collect::<Vec<_>>();
            let ddy = (0..dim).map(|i| source_component_expr(ctx, old, inst, 4, i)).collect::<Vec<_>>();
            format!("{}({}, {}, {}, {})", base, sampler_name, coord_expr, vector_constructor(&ddx), vector_constructor(&ddy))
        }
        _ => format!("{}({}, {})", base, sampler_name, coord_expr),
    };
    format!("{}{}", sample, component_suffix(component))
}

fn components_from_mask(mask: u8) -> Vec<usize> {
    let m = if mask == 0 { 0xf } else { mask };
    let mut out = Vec::new();
    for component in 0..4 {
        if (m & (1 << component)) != 0 {
            out.push(component);
        }
    }
    out
}

fn vector_len_for_type(ty: &str) -> usize {
    match ty {
        "float" | "bool" => 1,
        "float2" | "int2" | "bool2" => 2,
        "float3" | "int3" | "bool3" => 3,
        _ => 4,
    }
}

fn scalar_zero(ty: &str) -> &'static str {
    match ty {
        "bool" | "bool4" => "false",
        _ => "0.0",
    }
}

fn component_suffix(component: usize) -> &'static str {
    match component {
        0 => ".x",
        1 => ".y",
        2 => ".z",
        _ => ".w",
    }
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
    };

    for inst in &shader.instructions {
        match inst.opcode {
            Opcode::Dcl => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::Sampler {
                        let sampler_ty = inst.decl_sampler_type();
                        ctx.sampler_decls.insert(reg.number, sampler_ty);
                        ctx.used_samplers.insert(reg.number);
                    } else {
                        let semantic = semantic_from_decl(shader.kind, inst);
                        ctx.decls.insert(reg, DeclInfo { reg, semantic, sampler_type: None });
                    }
                }
            }
            Opcode::Def => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::Const {
                        ctx.def_float.insert(reg.number, DefFloat {
                            reg,
                            values: [inst.get_float_param(1), inst.get_float_param(2), inst.get_float_param(3), inst.get_float_param(4)],
                        });
                    }
                }
            }
            Opcode::DefI => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::ConstInt {
                        ctx.def_int.insert(reg.number, DefInt {
                            reg,
                            values: [inst.get_int_param(1), inst.get_int_param(2), inst.get_int_param(3), inst.get_int_param(4)],
                        });
                    }
                }
            }
            Opcode::DefB => {
                if let Some(reg) = inst.dest_register() {
                    if reg.ty == RegisterType::ConstBool {
                        ctx.def_bool.insert(reg.number, DefBool { reg, value: inst.get_int_param(1) != 0 });
                    }
                }
            }
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
            if inst.opcode == Opcode::Dcl || inst.opcode == Opcode::Def || inst.opcode == Opcode::DefI || inst.opcode == Opcode::DefB {
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
        ctx.decls.insert(reg, DeclInfo { reg, semantic, sampler_type: None });
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
    }
    if let Some(reg) = inst.dest_register() {
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

fn emit_uniforms(out: &mut String, ctx: &Context<'_>) {
    if let Some(ctab) = ctx.ctab {
        for c in &ctab.constants {
            let ty = c.hlsl_decl_type();
            out.push_str(&format!("uniform {} {};\n", ty, sanitize_ident(&c.name)));
        }
    }

    let mut emitted_sampler_fallback = false;
    for sampler in &ctx.used_samplers {
        if sampler_constant(ctx, *sampler).is_some() {
            continue;
        }
        let ty = sampler_type_name(ctx.sampler_decls.get(sampler).copied().unwrap_or(SamplerTextureType::TwoD));
        out.push_str(&format!("uniform {} s{};\n", ty, sampler));
        emitted_sampler_fallback = true;
    }
    if ctx.ctab.is_some() || emitted_sampler_fallback {
        out.push('\n');
    }
}

fn emit_def_constants(out: &mut String, ctx: &Context<'_>) {
    let mut any = false;
    for (idx, def) in &ctx.def_float {
        if float_constant(ctx, *idx).is_some() {
            continue;
        }
        out.push_str(&format!(
            "static const float4 c{} = float4({}, {}, {}, {});\n",
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
            "static const int4 i{} = int4({}, {}, {}, {});\n",
            idx, def.values[0], def.values[1], def.values[2], def.values[3]
        ));
        any = true;
    }
    for (idx, def) in &ctx.def_bool {
        out.push_str(&format!("static const bool b{} = {};\n", idx, if def.value { "true" } else { "false" }));
        any = true;
    }
    if any {
        out.push('\n');
    }
}

fn emit_structs(out: &mut String, ctx: &Context<'_>) {
    let input_name = input_struct_name(ctx.shader.kind);
    let output_name = output_struct_name(ctx.shader.kind);

    out.push_str(&format!("struct {} {{\n", input_name));
    for decl in ctx.decls.values() {
        let field = input_field_name(decl.reg);
        let ty = input_field_type(ctx, decl.reg);
        out.push_str(&format!("    {} {} : {};\n", ty, field, decl.semantic));
    }
    out.push_str("};\n\n");

    out.push_str(&format!("struct {} {{\n", output_name));
    for reg in &ctx.used_outputs {
        let field = output_field_name(*reg);
        let sem = output_semantic(ctx, *reg);
        let ty = output_field_type(*reg);
        out.push_str(&format!("    {} {} : {};\n", ty, field, sem));
    }
    out.push_str("};\n\n");
}

fn emit_main(out: &mut String, ctx: &Context<'_>) {
    let input_name = input_struct_name(ctx.shader.kind);
    let output_name = output_struct_name(ctx.shader.kind);
    out.push_str(&format!("{} main({} input) {{\n", output_name, input_name));
    out.push_str(&format!("    {} output;\n", output_name));

    for reg in &ctx.used_outputs {
        out.push_str(&format!("    output.{} = {};\n", output_field_name(*reg), zero_value(output_field_type(*reg))));
    }
    for reg in &ctx.used_temps {
        out.push_str(&format!("    {} {} = {};\n", temp_type(*reg), temp_name(*reg, ctx.shader.kind), zero_value(temp_type(*reg))));
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
                let expr = format!("{}{}", register_base(ctx, reg), hlsl_mask_suffix(inst.dest_write_mask()));
                line(out, *indent, &format!("clip({});", expr));
            }
        }
        Opcode::If => {
            let cond = source_expr(ctx, inst, 0, 1);
            line(out, *indent, &format!("if ({}) {{", cond));
            *indent += 1;
        }
        Opcode::IfC => {
            let a = source_expr(ctx, inst, 0, 4);
            let b = source_expr(ctx, inst, 1, 4);
            line(out, *indent, &format!("if (all({} {} {})) {{", a, cmp_op(inst.comparison()), b));
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
            line(out, *indent, &format!("if (all({} {} {})) break;", a, cmp_op(inst.comparison()), b));
        }
        Opcode::Rep => {
            let n = source_expr(ctx, inst, 0, 1);
            line(out, *indent, &format!("for (int _rep{} = 0; _rep{} < (int)({}); ++_rep{}) {{", inst.offset, inst.offset, n, inst.offset));
            *indent += 1;
        }
        Opcode::Loop => {
            line(out, *indent, &format!("for (int _loop{} = 0; ; ++_loop{}) {{", inst.offset, inst.offset));
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
    let dst = format!("{}{}", register_base(ctx, reg), hlsl_mask_suffix(mask));
    let n = mask_len(mask);
    let mut rhs = match inst.opcode {
        Opcode::Mov | Opcode::MovA => source_expr(ctx, inst, 1, n),
        Opcode::Add => bin(ctx, inst, n, "+"),
        Opcode::Sub => bin(ctx, inst, n, "-"),
        Opcode::Mul => bin(ctx, inst, n, "*"),
        Opcode::Mad => format!("({} * {} + {})", source_expr(ctx, inst, 1, n), source_expr(ctx, inst, 2, n), source_expr(ctx, inst, 3, n)),
        Opcode::Rcp => format!("(1.0 / {})", source_expr(ctx, inst, 1, 1)),
        Opcode::Rsq => format!("rsqrt({})", source_expr(ctx, inst, 1, 1)),
        Opcode::Dp3 => format!("dot({}, {})", source_expr(ctx, inst, 1, 3), source_expr(ctx, inst, 2, 3)),
        Opcode::Dp4 => format!("dot({}, {})", source_expr(ctx, inst, 1, 4), source_expr(ctx, inst, 2, 4)),
        Opcode::Min => format!("min({}, {})", source_expr(ctx, inst, 1, n), source_expr(ctx, inst, 2, n)),
        Opcode::Max => format!("max({}, {})", source_expr(ctx, inst, 1, n), source_expr(ctx, inst, 2, n)),
        Opcode::Slt => format!("(1.0 - step({}, {}))", source_expr(ctx, inst, 2, n), source_expr(ctx, inst, 1, n)),
        Opcode::Sge => format!("step({}, {})", source_expr(ctx, inst, 2, n), source_expr(ctx, inst, 1, n)),
        Opcode::Exp | Opcode::ExpP => format!("exp2({})", source_expr(ctx, inst, 1, 1)),
        Opcode::Log | Opcode::LogP => format!("log2({})", source_expr(ctx, inst, 1, 1)),
        Opcode::Lit => format!("lit({})", source_expr(ctx, inst, 1, 4)),
        Opcode::Dst => format!("dst({}, {})", source_expr(ctx, inst, 1, 4), source_expr(ctx, inst, 2, 4)),
        Opcode::Lrp => format!("lerp({}, {}, {})", source_expr(ctx, inst, 3, n), source_expr(ctx, inst, 2, n), source_expr(ctx, inst, 1, n)),
        Opcode::Frc => format!("frac({})", source_expr(ctx, inst, 1, n)),
        Opcode::Pow => format!("pow({}, {})", source_expr(ctx, inst, 1, 1), source_expr(ctx, inst, 2, 1)),
        Opcode::Abs => format!("abs({})", source_expr(ctx, inst, 1, n)),
        Opcode::Nrm => format!("normalize({})", source_expr(ctx, inst, 1, 3)),
        Opcode::SinCos => {
            if mask & 1 != 0 && mask_len(mask) == 1 {
                format!("cos({})", source_expr(ctx, inst, 1, 1))
            } else if mask & 2 != 0 && mask_len(mask) == 1 {
                format!("sin({})", source_expr(ctx, inst, 1, 1))
            } else {
                format!("float2(cos({0}), sin({0}))", source_expr(ctx, inst, 1, 1))
            }
        }
        Opcode::Cmp => format!("({} >= 0 ? {} : {})", source_expr(ctx, inst, 1, n), source_expr(ctx, inst, 3, n), source_expr(ctx, inst, 2, n)),
        Opcode::Cnd => format!("({} > 0.5 ? {} : {})", source_expr(ctx, inst, 1, n), source_expr(ctx, inst, 2, n), source_expr(ctx, inst, 3, n)),
        Opcode::Crs => format!("cross({}, {})", source_expr(ctx, inst, 1, 3), source_expr(ctx, inst, 2, 3)),
        Opcode::Dp2Add => format!("(dot({}, {}) + {})", source_expr(ctx, inst, 1, 2), source_expr(ctx, inst, 2, 2), source_expr(ctx, inst, 3, 1)),
        Opcode::M4x4 => matrix_mul_expr(ctx, inst, 4, 4),
        Opcode::M4x3 => matrix_mul_expr(ctx, inst, 4, 3),
        Opcode::M3x4 => matrix_mul_expr(ctx, inst, 3, 4),
        Opcode::M3x3 => matrix_mul_expr(ctx, inst, 3, 3),
        Opcode::M3x2 => matrix_mul_expr(ctx, inst, 3, 2),
        Opcode::Tex | Opcode::TexLdl | Opcode::TexLdd => texture_expr(ctx, inst),
        Opcode::TexCoord => source_expr(ctx, inst, 1, n),
        Opcode::Dsx => format!("ddx({})", source_expr(ctx, inst, 1, n)),
        Opcode::Dsy => format!("ddy({})", source_expr(ctx, inst, 1, n)),
        Opcode::Sgn => format!("sign({})", source_expr(ctx, inst, 1, n)),
        _ => return None,
    };
    rhs = apply_result_modifier(rhs, inst.dest_modifier());
    Some((dst, rhs))
}

fn bin(ctx: &Context<'_>, inst: &Instruction, n: usize, op: &str) -> String {
    format!("({} {} {})", source_expr(ctx, inst, 1, n), op, source_expr(ctx, inst, 2, n))
}

fn matrix_mul_expr(ctx: &Context<'_>, inst: &Instruction, vec_len: usize, out_len: usize) -> String {
    let v = source_expr(ctx, inst, 1, vec_len);
    if let Some(reg) = inst.source_register(2) {
        if reg.ty == RegisterType::Const {
            if let Some(name) = matrix_constant_name(ctx, reg.number, out_len) {
                return format!("mul({}, {})", v, name);
            }
            let rows = (0..out_len)
                .map(|i| const_row_expr(ctx, reg.number + i as u16))
                .collect::<Vec<_>>()
                .join(", ");
            let ty = format!("float{}x{}", out_len, vec_len);
            return format!("mul({}, {}({}))", v, ty, rows);
        }
    }
    format!("mul({}, {})", v, source_expr(ctx, inst, 2, vec_len))
}

fn texture_expr(ctx: &Context<'_>, inst: &Instruction) -> String {
    let sampler = inst.source_register(2).map(|r| r.number).unwrap_or(0);
    let sampler_name = sampler_name(ctx, sampler);
    let sampler_ty = sampler_texture_type(ctx, sampler);
    let dim = sampler_ty.hlsl_dim();
    let coord = source_expr(ctx, inst, 1, if inst.opcode == Opcode::Tex { dim } else { 4 });
    let base = texture_intrinsic(inst, sampler_ty);
    match inst.opcode {
        Opcode::TexLdd => {
            let ddx = source_expr(ctx, inst, 3, dim);
            let ddy = source_expr(ctx, inst, 4, dim);
            format!("{}({}, {}, {}, {})", base, sampler_name, coord, ddx, ddy)
        }
        _ => format!("{}({}, {})", base, sampler_name, coord),
    }
}

fn texture_intrinsic(inst: &Instruction, sampler_ty: SamplerTextureType) -> &'static str {
    let controls = inst.texld_controls();
    match (inst.opcode, sampler_ty, controls) {
        (Opcode::TexLdl, SamplerTextureType::Cube, _) => "texCUBElod",
        (Opcode::TexLdl, SamplerTextureType::Volume, _) => "tex3Dlod",
        (Opcode::TexLdl, _, _) => "tex2Dlod",
        (Opcode::TexLdd, SamplerTextureType::Cube, _) => "texCUBEgrad",
        (Opcode::TexLdd, SamplerTextureType::Volume, _) => "tex3Dgrad",
        (Opcode::TexLdd, _, _) => "tex2Dgrad",
        (Opcode::Tex, SamplerTextureType::Cube, 1) => "texCUBEproj",
        (Opcode::Tex, SamplerTextureType::Cube, 2) => "texCUBEbias",
        (Opcode::Tex, SamplerTextureType::Cube, _) => "texCUBE",
        (Opcode::Tex, SamplerTextureType::Volume, 1) => "tex3Dproj",
        (Opcode::Tex, SamplerTextureType::Volume, 2) => "tex3Dbias",
        (Opcode::Tex, SamplerTextureType::Volume, _) => "tex3D",
        (Opcode::Tex, _, 1) => "tex2Dproj",
        (Opcode::Tex, _, 2) => "tex2Dbias",
        (Opcode::Tex, _, _) => "tex2D",
        _ => "tex2D",
    }
}

fn source_expr(ctx: &Context<'_>, inst: &Instruction, param_index: usize, count: usize) -> String {
    let Some(reg) = inst.source_register(param_index) else { return "0".to_string(); };
    let mut expr = register_base(ctx, reg);
    let swz = inst.source_swizzle(param_index);
    expr.push_str(&hlsl_swizzle_suffix(swz, count));
    apply_source_modifier(expr, inst.source_modifier(param_index))
}

fn register_base(ctx: &Context<'_>, reg: RegisterKey) -> String {
    match reg.ty {
        RegisterType::Temp | RegisterType::TempFloat16 | RegisterType::Predicate => temp_name(reg, ctx.shader.kind),
        RegisterType::Texture if ctx.shader.kind == ShaderKind::Vertex => temp_name(reg, ctx.shader.kind),
        RegisterType::Texture | RegisterType::Input | RegisterType::MiscType => format!("input.{}", input_field_name(reg)),
        RegisterType::Const => const_row_expr(ctx, reg.number),
        RegisterType::ConstInt => int_const_expr(ctx, reg.number),
        RegisterType::ConstBool => bool_const_expr(ctx, reg.number),
        RegisterType::Sampler => sampler_name(ctx, reg.number),
        RegisterType::ColorOut | RegisterType::DepthOut | RegisterType::RastOut | RegisterType::AttrOut | RegisterType::Output => format!("output.{}", output_field_name(reg)),
        RegisterType::Loop => "_loop".to_string(),
        RegisterType::Label => format!("label{}", reg.number),
        _ => format!("u{}", reg.number),
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
        RegisterType::Predicate => "bool4",
        RegisterType::Texture => "float4",
        RegisterType::TempFloat16 => "half4",
        _ => "float4",
    }
}

fn input_field_type(ctx: &Context<'_>, reg: RegisterKey) -> &'static str {
    match reg.ty {
        RegisterType::MiscType if reg.number == 1 => "float",
        _ => {
            if let Some(decl) = ctx.decls.get(&reg) {
                if decl.semantic.starts_with("TEXCOORD") {
                    return "float4";
                }
                if decl.semantic.starts_with("COLOR") {
                    return "float4";
                }
            }
            "float4"
        }
    }
}

fn output_field_type(reg: RegisterKey) -> &'static str {
    match reg.ty {
        RegisterType::DepthOut => "float",
        RegisterType::RastOut if reg.number == 1 || reg.number == 2 => "float",
        _ => "float4",
    }
}

fn input_struct_name(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Vertex => "VS_INPUT",
        ShaderKind::Pixel => "PS_INPUT",
    }
}

fn output_struct_name(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Vertex => "VS_OUTPUT",
        ShaderKind::Pixel => "PS_OUTPUT",
    }
}

fn zero_value(ty: &str) -> &'static str {
    match ty {
        "float" => "0.0",
        "float2" => "float2(0.0, 0.0)",
        "float3" => "float3(0.0, 0.0, 0.0)",
        "half4" => "half4(0.0, 0.0, 0.0, 0.0)",
        "bool4" => "bool4(false, false, false, false)",
        _ => "float4(0.0, 0.0, 0.0, 0.0)",
    }
}

fn hlsl_mask_suffix(mask: u8) -> String {
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

fn hlsl_swizzle_suffix(swizzle: [usize; 4], count: usize) -> String {
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

fn apply_source_modifier(expr: String, modifier: SourceModifier) -> String {
    match modifier {
        SourceModifier::None => expr,
        SourceModifier::Negate => format!("-({})", expr),
        SourceModifier::Bias => format!("(({}) - 0.5)", expr),
        SourceModifier::BiasAndNegate => format!("-(({}) - 0.5)", expr),
        SourceModifier::Sign => format!("((({}) - 0.5) * 2.0)", expr),
        SourceModifier::SignAndNegate => format!("-((({}) - 0.5) * 2.0)", expr),
        SourceModifier::Complement => format!("(1.0 - ({}))", expr),
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

fn apply_result_modifier(expr: String, modifier: ResultModifier) -> String {
    if modifier.saturate {
        format!("saturate({})", expr)
    } else {
        expr
    }
}

fn line(out: &mut String, indent: usize, s: &str) {
    for _ in 0..indent {
        out.push_str("    ");
    }
    out.push_str(s);
    out.push('\n');
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

fn sampler_type_name(ty: SamplerTextureType) -> &'static str {
    match ty {
        SamplerTextureType::Cube => "samplerCUBE",
        SamplerTextureType::Volume => "sampler3D",
        SamplerTextureType::TwoD | SamplerTextureType::Unknown => "sampler2D",
    }
}

fn sampler_name(ctx: &Context<'_>, sampler: u16) -> String {
    sampler_constant(ctx, sampler)
        .map(|c| sanitize_ident(&c.name))
        .unwrap_or_else(|| format!("s{}", sampler))
}

fn sampler_constant<'a>(ctx: &'a Context<'_>, sampler: u16) -> Option<&'a ConstantInfo> {
    ctx.ctab?.constants.iter().find(|c| c.register_set == RegisterSet::Sampler && c.register_index == sampler)
}

fn float_constant<'a>(ctx: &'a Context<'_>, index: u16) -> Option<(&'a ConstantInfo, u16)> {
    let ctab = ctx.ctab?;
    ctab.constants.iter().find_map(|c| {
        if c.register_set == RegisterSet::Float4
            && index >= c.register_index
            && index < c.register_index.saturating_add(c.register_count)
        {
            Some((c, index - c.register_index))
        } else {
            None
        }
    })
}

fn matrix_constant_name(ctx: &Context<'_>, index: u16, rows_needed: usize) -> Option<String> {
    let (c, row) = float_constant(ctx, index)?;
    if row != 0 || c.register_count < rows_needed as u16 {
        return None;
    }
    let Some(t) = &c.type_info else { return None; };
    match t.class {
        TypeClass::MatrixRows | TypeClass::MatrixColumns => Some(sanitize_ident(&c.name)),
        _ => None,
    }
}

fn const_row_expr(ctx: &Context<'_>, index: u16) -> String {
    if let Some((c, row)) = float_constant(ctx, index) {
        let name = sanitize_ident(&c.name);
        if c.register_count <= 1 {
            return name;
        }
        if let Some(t) = &c.type_info {
            match t.class {
                TypeClass::MatrixRows | TypeClass::MatrixColumns => return format!("{}[{}]", name, row),
                TypeClass::Vector | TypeClass::Scalar => return name,
                _ => {}
            }
        }
        return format!("{}[{}]", name, row);
    }
    format!("c{}", index)
}

fn int_const_expr(ctx: &Context<'_>, index: u16) -> String {
    if let Some(ctab) = ctx.ctab {
        if let Some(c) = ctab.constants.iter().find(|c| c.register_set == RegisterSet::Int4 && c.register_index == index) {
            return sanitize_ident(&c.name);
        }
    }
    format!("i{}", index)
}

fn bool_const_expr(ctx: &Context<'_>, index: u16) -> String {
    if let Some(ctab) = ctx.ctab {
        if let Some(c) = ctab.constants.iter().find(|c| c.register_set == RegisterSet::Bool && c.register_index == index) {
            return sanitize_ident(&c.name);
        }
    }
    format!("b{}", index)
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

fn sanitize_ident(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        let ok = ch == '_' || ch.is_ascii_alphanumeric();
        if i == 0 {
            if ok && (ch == '_' || ch.is_ascii_alphabetic()) {
                out.push(ch);
            } else {
                out.push('_');
                if ok {
                    out.push(ch);
                }
            }
        } else if ok {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() { "unnamed".to_string() } else { out }
}
