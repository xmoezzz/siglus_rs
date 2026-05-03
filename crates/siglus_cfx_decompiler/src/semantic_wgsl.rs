use crate::disasm::ShaderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VertexMode {
    Base,
    D3,
    D3Fog,
    D3Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelMode {
    V0,
    V1Fog,
    V2Light,
}

#[derive(Debug, Clone, Copy, Default)]
struct PixelFlags {
    tex: bool,
    mrbd: bool,
    rgb: bool,
    tonecurve: bool,
    mask: bool,
    mul: bool,
    screen: bool,
}

#[derive(Debug, Clone, Copy)]
struct RegularTechnique {
    vertex: VertexMode,
    pixel: PixelMode,
    flags: PixelFlags,
}

pub fn rewrite_wgsl_for_stage(technique_name: &str, kind: ShaderKind) -> Option<String> {
    if let Some(t) = parse_regular_technique(technique_name) {
        return match kind {
            ShaderKind::Vertex => Some(regular_vertex_wgsl(t)),
            ShaderKind::Pixel => Some(regular_fragment_wgsl(t)),
        };
    }

    if kind == ShaderKind::Pixel {
        return special_fragment_wgsl(technique_name);
    }

    None
}

fn parse_regular_technique(name: &str) -> Option<RegularTechnique> {
    let rest = name.strip_prefix("tec_v")?;
    let (vpart, ppart) = rest.split_once("_p_")?;
    let vertex = match vpart {
        "" => VertexMode::Base,
        "_d3" => VertexMode::D3,
        "_d3_fog" => VertexMode::D3Fog,
        "_d3_light" => VertexMode::D3Light,
        _ => return None,
    };

    let pixel = if ppart.starts_with("v0") {
        PixelMode::V0
    } else if ppart.starts_with("v1") {
        PixelMode::V1Fog
    } else if ppart.starts_with("v2") {
        PixelMode::V2Light
    } else {
        return None;
    };

    let flags = PixelFlags {
        tex: has_flag(ppart, "tex"),
        mrbd: has_flag(ppart, "mrbd"),
        rgb: has_flag(ppart, "rgb"),
        tonecurve: has_flag(ppart, "tonecurve"),
        mask: has_flag(ppart, "mask"),
        mul: has_flag(ppart, "mul"),
        screen: has_flag(ppart, "screen"),
    };

    Some(RegularTechnique { vertex, pixel, flags })
}

fn has_flag(s: &str, flag: &str) -> bool {
    s.split('_').any(|part| part == flag)
}

fn regular_vertex_wgsl(t: RegularTechnique) -> String {
    let needs_d3 = !matches!(t.vertex, VertexMode::Base);
    let needs_normal = matches!(t.vertex, VertexMode::D3Light);
    let needs_mask_uv = t.flags.mask;

    let mut out = String::new();
    if needs_d3 {
        push_uniforms(&mut out);
    }

    out.push_str("struct VSInput {\n");
    out.push_str("    @location(0) position: vec4<f32>,\n");
    if needs_normal {
        out.push_str("    @location(1) normal: vec3<f32>,\n");
    }
    out.push_str("    @location(2) diffuse: vec4<f32>,\n");
    out.push_str("    @location(4) texture_uv: vec2<f32>,\n");
    if needs_mask_uv {
        out.push_str("    @location(5) mask_uv: vec2<f32>,\n");
    }
    out.push_str("};\n\n");

    out.push_str("struct VSOutput {\n");
    out.push_str("    @builtin(position) position: vec4<f32>,\n");
    out.push_str("    @location(2) diffuse: vec4<f32>,\n");
    out.push_str("    @location(4) texture_uv: vec2<f32>,\n");
    match t.vertex {
        VertexMode::Base | VertexMode::D3 => {
            if needs_mask_uv {
                out.push_str("    @location(5) mask_uv: vec2<f32>,\n");
            }
        }
        VertexMode::D3Fog => {
            out.push_str("    @location(5) world_pos: vec4<f32>,\n");
            out.push_str("    @location(6) proj_pos: vec4<f32>,\n");
        }
        VertexMode::D3Light => {
            out.push_str("    @location(5) normal: vec3<f32>,\n");
            out.push_str("    @location(6) world_pos: vec4<f32>,\n");
            out.push_str("    @location(7) proj_pos: vec4<f32>,\n");
        }
    }
    out.push_str("};\n\n");

    if needs_d3 {
        out.push_str("fn mul_vec4_mat4(v: vec4<f32>, m: mat4x4<f32>) -> vec4<f32> {\n");
        out.push_str("    return vec4<f32>(dot(v, m[0]), dot(v, m[1]), dot(v, m[2]), dot(v, m[3]));\n");
        out.push_str("}\n\n");
        if needs_normal {
            out.push_str("fn mul_vec3_mat4(v: vec3<f32>, m: mat4x4<f32>) -> vec3<f32> {\n");
            out.push_str("    let x = dot(vec4<f32>(v, 0.0), m[0]);\n");
            out.push_str("    let y = dot(vec4<f32>(v, 0.0), m[1]);\n");
            out.push_str("    let z = dot(vec4<f32>(v, 0.0), m[2]);\n");
            out.push_str("    return vec3<f32>(x, y, z);\n");
            out.push_str("}\n\n");
        }
    }

    out.push_str("@vertex\n");
    out.push_str("fn main(input: VSInput) -> VSOutput {\n");
    out.push_str("    var output: VSOutput;\n");
    match t.vertex {
        VertexMode::Base => {
            out.push_str("    output.position = input.position;\n");
            out.push_str("    output.diffuse = input.diffuse;\n");
        }
        VertexMode::D3 => {
            out.push_str("    var position = mul_vec4_mat4(input.position, u.g_mat_world);\n");
            out.push_str("    position = mul_vec4_mat4(position, u.g_mat_view_proj);\n");
            out.push_str("    output.position = position;\n");
            out.push_str("    output.diffuse = input.diffuse * u.g_mtrl_diffuse;\n");
        }
        VertexMode::D3Fog => {
            out.push_str("    var position = mul_vec4_mat4(input.position, u.g_mat_world);\n");
            out.push_str("    output.world_pos = position;\n");
            out.push_str("    position = mul_vec4_mat4(position, u.g_mat_view_proj);\n");
            out.push_str("    output.proj_pos = position;\n");
            out.push_str("    output.position = position;\n");
            out.push_str("    output.diffuse = input.diffuse * u.g_mtrl_diffuse;\n");
        }
        VertexMode::D3Light => {
            out.push_str("    var position = mul_vec4_mat4(input.position, u.g_mat_world);\n");
            out.push_str("    output.world_pos = position;\n");
            out.push_str("    position = mul_vec4_mat4(position, u.g_mat_view_proj);\n");
            out.push_str("    output.proj_pos = position;\n");
            out.push_str("    output.position = position;\n");
            out.push_str("    output.normal = mul_vec3_mat4(input.normal, u.g_mat_world);\n");
            out.push_str("    output.diffuse = input.diffuse * u.g_mtrl_diffuse;\n");
        }
    }
    out.push_str("    output.texture_uv = input.texture_uv;\n");
    if needs_mask_uv {
        out.push_str("    output.mask_uv = input.mask_uv;\n");
    }
    out.push_str("    return output;\n");
    out.push_str("}\n");
    out
}

fn regular_fragment_wgsl(t: RegularTechnique) -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, t.flags.tex, t.flags.mask, t.flags.tonecurve, matches!(t.pixel, PixelMode::V1Fog | PixelMode::V2Light), false, false, false);
    push_regular_helpers(&mut out, t);

    out.push_str("struct PSInput {\n");
    out.push_str("    @location(2) diffuse: vec4<f32>,\n");
    if t.flags.tex {
        out.push_str("    @location(4) texcoord0: vec2<f32>,\n");
    }
    if t.flags.mask {
        out.push_str("    @location(5) mask_uv: vec2<f32>,\n");
    }
    match t.pixel {
        PixelMode::V0 => {}
        PixelMode::V1Fog => {
            out.push_str("    @location(5) world_pos: vec4<f32>,\n");
            out.push_str("    @location(6) proj_pos: vec4<f32>,\n");
        }
        PixelMode::V2Light => {
            out.push_str("    @location(5) normal: vec3<f32>,\n");
            out.push_str("    @location(6) world_pos: vec4<f32>,\n");
            out.push_str("    @location(7) proj_pos: vec4<f32>,\n");
        }
    }
    out.push_str("};\n\n");

    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    var color = input.diffuse;\n");
    if t.flags.tex {
        out.push_str("    let texture_uv = vec2<f32>(input.texcoord0.x, input.texcoord0.y);\n");
        out.push_str("    color = color * textureSample(tex00, samp_tex00, texture_uv);\n");
    }
    if t.flags.mrbd || t.flags.rgb || t.flags.tonecurve || t.flags.mul || t.flags.screen || !matches!(t.pixel, PixelMode::V0) {
        out.push_str("    let color_org = color;\n");
        if matches!(t.pixel, PixelMode::V2Light) {
            out.push_str("    if (u.g_light_pos.w > 0.5) {\n");
            out.push_str("        color = calc_light(color, input.world_pos, input.normal);\n");
            out.push_str("    }\n");
        }
        if matches!(t.pixel, PixelMode::V1Fog | PixelMode::V2Light) {
            out.push_str("    if (u.g_fog_range.x > 0.5) {\n");
            out.push_str("        color = calc_fog(color, input.world_pos, input.proj_pos);\n");
            out.push_str("    }\n");
        }
        if t.flags.tonecurve || t.flags.mrbd {
            out.push_str("    let mono_y = dot(vec4<f32>(0.2989, 0.5886, 0.1145, 0.0), color);\n");
        }
        if t.flags.tonecurve {
            out.push_str("    color = tonecurve(color, mono_y);\n");
        }
        if t.flags.mrbd {
            out.push_str("    let reverse = vec4<f32>(1.0) - color;\n");
            out.push_str("    color = mix(color, reverse, u.c[0].y);\n");
            out.push_str("    color = mix(color, vec4<f32>(mono_y), u.c[0].x);\n");
            out.push_str("    color = color + vec4<f32>(u.c[0].z);\n");
            out.push_str("    color = color - vec4<f32>(u.c[0].w);\n");
        }
        if t.flags.rgb {
            out.push_str("    color = mix(color, u.c[1], u.c[1].w);\n");
            out.push_str("    color = color + u.c[2];\n");
        }
        if t.flags.mul {
            out.push_str("    color = mix(vec4<f32>(1.0), color, color_org.a);\n");
        }
        if t.flags.screen {
            out.push_str("    color = mix(vec4<f32>(0.0), color, color_org.a);\n");
        }
        out.push_str("    color.a = color_org.a;\n");
    }
    if t.flags.mask {
        out.push_str("    let mask_color = textureSample(tex01, samp_tex01, input.mask_uv);\n");
        out.push_str("    color = color * mask_color;\n");
    }
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}

fn push_regular_helpers(out: &mut String, t: RegularTechnique) {
    if t.flags.tonecurve {
        out.push_str("fn tonecurve(color_in: vec4<f32>, mono_y: f32) -> vec4<f32> {\n");
        out.push_str("    var color = mix(color_in, vec4<f32>(mono_y), u.c[3].g);\n");
        out.push_str("    var tonecurve_pos = vec2<f32>(color.r, u.c[3].r);\n");
        out.push_str("    var tonecurve_color = textureSample(tex02, samp_tex02, tonecurve_pos);\n");
        out.push_str("    color.r = tonecurve_color.r;\n");
        out.push_str("    tonecurve_pos = vec2<f32>(color.g, u.c[3].r);\n");
        out.push_str("    tonecurve_color = textureSample(tex02, samp_tex02, tonecurve_pos);\n");
        out.push_str("    color.g = tonecurve_color.g;\n");
        out.push_str("    tonecurve_pos = vec2<f32>(color.b, u.c[3].r);\n");
        out.push_str("    tonecurve_color = textureSample(tex02, samp_tex02, tonecurve_pos);\n");
        out.push_str("    color.b = tonecurve_color.b;\n");
        out.push_str("    return color;\n");
        out.push_str("}\n\n");
    }
    if matches!(t.pixel, PixelMode::V2Light) {
        push_calc_light(out);
    }
    if matches!(t.pixel, PixelMode::V1Fog | PixelMode::V2Light) {
        push_calc_fog(out);
    }
}

fn special_fragment_wgsl(name: &str) -> Option<String> {
    match name {
        "tec_tex2_mask" => Some(wgsl_tex2_mask()),
        "tec_tex1_shimi" => Some(wgsl_tex1_shimi(false)),
        "tec_tex1_shimi_inv" => Some(wgsl_tex1_shimi(true)),
        "tec_tex2_raster_h" => Some(wgsl_tex2_raster(true)),
        "tec_tex2_raster_v" => Some(wgsl_tex2_raster(false)),
        "tec_tex1_raster_h" => Some(wgsl_tex1_raster(true)),
        "tec_tex1_raster_v" => Some(wgsl_tex1_raster(false)),
        "tec_tex2_explosion_blur" => Some(wgsl_tex2_explosion_blur()),
        "tec_tex1_explosion_blur" => Some(wgsl_tex1_explosion_blur()),
        "tec_tex1_mosaic" => Some(wgsl_tex1_mosaic()),
        _ => None,
    }
}

fn push_uniforms(out: &mut String) {
    out.push_str("struct SiglusUniforms {\n");
    out.push_str("    g_mat_world: mat4x4<f32>,\n");
    out.push_str("    g_mat_view_proj: mat4x4<f32>,\n");
    out.push_str("    g_mtrl_diffuse: vec4<f32>,\n");
    out.push_str("    g_camera_pos: vec4<f32>,\n");
    out.push_str("    g_camera_dir: vec4<f32>,\n");
    out.push_str("    g_light_pos: vec4<f32>,\n");
    out.push_str("    g_light_ambient: vec4<f32>,\n");
    out.push_str("    g_fog_param: vec4<f32>,\n");
    out.push_str("    g_fog_range: vec4<f32>,\n");
    out.push_str("    c: array<vec4<f32>, 4>,\n");
    out.push_str("};\n");
    out.push_str("@group(0) @binding(0) var<uniform> u: SiglusUniforms;\n\n");
}

fn push_texture_bindings(out: &mut String, tex00: bool, tex01: bool, tex02: bool, tex03: bool, tex00_point: bool, _tex01_point: bool, _tex02_point: bool) {
    if tex00 || tex00_point {
        out.push_str("@group(1) @binding(0) var tex00: texture_2d<f32>;\n");
    }
    if tex00 {
        out.push_str("@group(1) @binding(1) var samp_tex00: sampler;\n");
    }
    if tex01 {
        out.push_str("@group(1) @binding(2) var tex01: texture_2d<f32>;\n");
        out.push_str("@group(1) @binding(3) var samp_tex01: sampler;\n");
    }
    if tex02 {
        out.push_str("@group(1) @binding(4) var tex02: texture_2d<f32>;\n");
        out.push_str("@group(1) @binding(5) var samp_tex02: sampler;\n");
    }
    if tex03 {
        out.push_str("@group(1) @binding(6) var tex03: texture_2d<f32>;\n");
        out.push_str("@group(1) @binding(7) var samp_tex03: sampler;\n");
    }
    if tex00_point {
        out.push_str("@group(1) @binding(8) var samp_tex00_point: sampler;\n");
    }
    if tex00 || tex01 || tex02 || tex03 || tex00_point {
        out.push_str("\n");
    }
}

fn push_ps_input_texcoord(out: &mut String) {
    out.push_str("struct PSInput {\n");
    out.push_str("    @location(2) diffuse: vec4<f32>,\n");
    out.push_str("    @location(4) texcoord0: vec2<f32>,\n");
    out.push_str("};\n\n");
}

fn push_calc_light(out: &mut String) {
    out.push_str("fn calc_light(color_in: vec4<f32>, world_pos: vec4<f32>, normal: vec3<f32>) -> vec4<f32> {\n");
    out.push_str("    var color = color_in;\n");
    out.push_str("    let dir_point = u.g_light_pos.xyz - world_pos.xyz;\n");
    out.push_str("    let distance_point = length(dir_point);\n");
    out.push_str("    var light_power = dot(normalize(normal), normalize(dir_point));\n");
    out.push_str("    light_power = light_power * (1.0 - distance_point / 2000.0);\n");
    out.push_str("    light_power = clamp(light_power, 0.0, 1.0);\n");
    out.push_str("    color = color * vec4<f32>(light_power);\n");
    out.push_str("    color = color * u.g_light_ambient;\n");
    out.push_str("    return color;\n");
    out.push_str("}\n\n");
}

fn push_calc_fog(out: &mut String) {
    out.push_str("fn calc_fog(color_in: vec4<f32>, world_pos: vec4<f32>, proj_pos: vec4<f32>) -> vec4<f32> {\n");
    out.push_str("    var color = color_in;\n");
    out.push_str("    let camera_dir = u.g_camera_pos.xyz - world_pos.xyz;\n");
    out.push_str("    let camera_distance = length(camera_dir);\n");
    out.push_str("    let fog_uv = vec2<f32>((proj_pos.x / proj_pos.w + 1.0) / 2.0 * u.g_fog_param.z + u.g_fog_param.x, 1.0 - (proj_pos.y / proj_pos.w + 1.0) / 2.0) * u.g_fog_param.w + vec2<f32>(u.g_fog_param.y);\n");
    out.push_str("    let fog_color = textureSample(tex03, samp_tex03, fog_uv);\n");
    out.push_str("    var fog_rate = (1.0 - 0.0) / (u.g_fog_range.z - u.g_fog_range.y) * (camera_distance - u.g_fog_range.y);\n");
    out.push_str("    fog_rate = clamp(fog_rate, 0.0, 1.0);\n");
    out.push_str("    color = vec4<f32>(mix(color.rgb, fog_color.rgb, fog_rate), color.a);\n");
    out.push_str("    return color;\n");
    out.push_str("}\n\n");
}

fn push_brightness(out: &mut String) {
    out.push_str("fn get_rgb_brightness(rgb: vec4<f32>) -> f32 {\n");
    out.push_str("    return dot(vec4<f32>(0.2989, 0.5886, 0.1145, 0.0), rgb);\n");
    out.push_str("}\n\n");
}

fn push_in_uv_range(out: &mut String) {
    out.push_str("fn in_uv_range(uv: vec2<f32>) -> bool {\n");
    out.push_str("    return uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;\n");
    out.push_str("}\n\n");
}

fn push_log10(out: &mut String) {
    out.push_str("fn log10_scalar(v: f32) -> f32 {\n");
    out.push_str("    return log(v) / log(10.0);\n");
    out.push_str("}\n\n");
}

fn wgsl_tex2_mask() -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, true, true, false, false, false, false, false);
    push_ps_input_texcoord2(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let tex = textureSample(tex00, samp_tex00, input.texcoord0);\n");
    out.push_str("    let mask = textureSample(tex01, samp_tex01, input.texcoord1);\n");
    out.push_str("    let fade = mix(256.0, 2.0, u.c[0].r);\n");
    out.push_str("    var color = (u.c[0] * fade + mask * (fade - 1.0) - vec4<f32>(fade - 1.0)) * tex;\n");
    out.push_str("    color = vec4<f32>(tex.r, tex.g, tex.b, color.a);\n");
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}

fn push_ps_input_texcoord2(out: &mut String) {
    out.push_str("struct PSInput {\n");
    out.push_str("    @location(2) diffuse: vec4<f32>,\n");
    out.push_str("    @location(4) texcoord0: vec2<f32>,\n");
    out.push_str("    @location(5) texcoord1: vec2<f32>,\n");
    out.push_str("};\n\n");
}

fn wgsl_tex1_shimi(inv: bool) -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, true, false, false, false, false, false, false);
    push_brightness(&mut out);
    push_ps_input_texcoord(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let tex = textureSample(tex00, samp_tex00, input.texcoord0);\n");
    out.push_str("    var color = tex;\n");
    if inv {
        out.push_str("    if (get_rgb_brightness(color) < 1.0 - u.c[0].w) {\n");
    } else {
        out.push_str("    if (get_rgb_brightness(color) > u.c[0].w) {\n");
    }
    out.push_str("        color.a = tex.a * (u.c[0].x - mix(u.c[0].x, 0.0, u.c[0].w));\n");
    out.push_str("    }\n");
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}

fn wgsl_tex2_raster(horizontal: bool) -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, true, true, false, false, false, false, false);
    push_in_uv_range(&mut out);
    push_log10(&mut out);
    push_ps_input_texcoord(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let fraction_num = u.c[0].x;\n");
    if horizontal {
        out.push_str("    var tex_coord_for_sin = floor(input.texcoord0.y * fraction_num);\n");
    } else {
        out.push_str("    var tex_coord_for_sin = floor(input.texcoord0.x * fraction_num);\n");
    }
    out.push_str("    tex_coord_for_sin = tex_coord_for_sin - fraction_num * 0.1;\n");
    out.push_str("    tex_coord_for_sin = tex_coord_for_sin / fraction_num;\n");
    out.push_str("    var raster_power = 1.0;\n");
    out.push_str("    var st_tex_rate = 0.0;\n");
    out.push_str("    var ed_tex_rate = 0.0;\n");
    out.push_str("    if (u.c[0].w < 0.5) {\n");
    out.push_str("        raster_power = 1.0 - mix(1.0, 0.0, u.c[0].w * 2.0);\n");
    out.push_str("        st_tex_rate = mix(2.0, 0.0, u.c[0].w * 2.0);\n");
    out.push_str("        if (st_tex_rate > 1.0) { st_tex_rate = 1.0; }\n");
    out.push_str("    } else {\n");
    out.push_str("        raster_power = 1.0 - mix(0.0, 2.0, u.c[0].w - 0.5);\n");
    out.push_str("        ed_tex_rate = mix(0.0, 4.0, u.c[0].w - 0.5);\n");
    out.push_str("        if (ed_tex_rate > 1.0) { ed_tex_rate = 1.0; }\n");
    out.push_str("    }\n");
    out.push_str("    let raster_offset = sin(3.14 * u.c[0].w * u.c[0].z + tex_coord_for_sin * 3.14 * u.c[0].y) * (1.0 - (log10_scalar((1.0 - raster_power) * 100.0) + 1.0) / 3.0);\n");
    if horizontal {
        out.push_str("    let raster_uv = vec2<f32>(input.texcoord0.x + raster_offset, input.texcoord0.y);\n");
    } else {
        out.push_str("    let raster_uv = vec2<f32>(input.texcoord0.x, input.texcoord0.y + raster_offset);\n");
    }
    out.push_str("    var tex0 = textureSample(tex00, samp_tex00, raster_uv);\n");
    out.push_str("    var tex1 = textureSample(tex01, samp_tex01, raster_uv);\n");
    out.push_str("    if (!in_uv_range(raster_uv)) {\n");
    out.push_str("        tex0 = vec4<f32>(0.0);\n");
    out.push_str("        tex1 = vec4<f32>(0.0);\n");
    out.push_str("    }\n");
    out.push_str("    let color = tex1 * st_tex_rate + tex0 * ed_tex_rate;\n");
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}

fn wgsl_tex1_raster(horizontal: bool) -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, true, false, false, false, false, false, false);
    push_in_uv_range(&mut out);
    push_log10(&mut out);
    push_ps_input_texcoord(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let fraction_num = u.c[0].x;\n");
    if horizontal {
        out.push_str("    var tex_coord_for_sin = floor(input.texcoord0.y * fraction_num);\n");
    } else {
        out.push_str("    var tex_coord_for_sin = floor(input.texcoord0.x * fraction_num);\n");
    }
    out.push_str("    tex_coord_for_sin = tex_coord_for_sin - fraction_num * 0.1;\n");
    out.push_str("    tex_coord_for_sin = tex_coord_for_sin / fraction_num;\n");
    out.push_str("    let raster_power = 1.0 - u.c[0].w;\n");
    out.push_str("    let tex_rate = u.c[0].w;\n");
    out.push_str("    let raster_offset = sin(3.14 * u.c[0].w * u.c[0].z + tex_coord_for_sin * 3.14 * u.c[0].y) * (1.0 - (log10_scalar((1.0 - raster_power) * 100.0) + 1.0) / 3.0);\n");
    if horizontal {
        out.push_str("    let raster_uv = vec2<f32>(input.texcoord0.x + raster_offset, input.texcoord0.y);\n");
    } else {
        out.push_str("    let raster_uv = vec2<f32>(input.texcoord0.x, input.texcoord0.y + raster_offset);\n");
    }
    out.push_str("    var tex = textureSample(tex00, samp_tex00, raster_uv);\n");
    out.push_str("    if (!in_uv_range(raster_uv)) {\n");
    out.push_str("        tex = vec4<f32>(0.0);\n");
    out.push_str("    }\n");
    out.push_str("    var color = tex;\n");
    out.push_str("    color.a = tex.a * tex_rate;\n");
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}

fn wgsl_tex2_explosion_blur() -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, true, true, false, false, false, false, false);
    push_ps_input_texcoord(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let center_texel = vec2<f32>(u.c[0].z, u.c[0].w);\n");
    out.push_str("    var dir = center_texel - input.texcoord0;\n");
    out.push_str("    let len = length(dir);\n");
    out.push_str("    dir = normalize(dir) * vec2<f32>(u.c[0].x, u.c[0].x);\n");
    out.push_str("    dir = dir * u.c[1].x * len * u.c[1].y;\n");
    out.push_str("    let color0 = textureSample(tex01, samp_tex01, input.texcoord0) * 0.40 + textureSample(tex01, samp_tex01, input.texcoord0 + dir * 2.0) * 0.24 + textureSample(tex01, samp_tex01, input.texcoord0 + dir * 4.0) * 0.16 + textureSample(tex01, samp_tex01, input.texcoord0 + dir * 6.0) * 0.14 + textureSample(tex01, samp_tex01, input.texcoord0 + dir * 8.0) * 0.06;\n");
    out.push_str("    let color1 = textureSample(tex00, samp_tex00, input.texcoord0) * 0.40 + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 2.0) * 0.24 + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 4.0) * 0.16 + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 6.0) * 0.14 + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 8.0) * 0.06;\n");
    out.push_str("    return input.diffuse.a * color0 + (1.0 - input.diffuse.a) * color1;\n");
    out.push_str("}\n");
    out
}

fn wgsl_tex1_explosion_blur() -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, true, false, false, false, false, false, false);
    push_ps_input_texcoord(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let center_texel = vec2<f32>(u.c[0].z, u.c[0].w);\n");
    out.push_str("    var dir = center_texel - input.texcoord0;\n");
    out.push_str("    let len = length(dir);\n");
    out.push_str("    dir = normalize(dir) * vec2<f32>(u.c[0].x, u.c[0].x);\n");
    out.push_str("    dir = dir * u.c[1].x * len * u.c[1].y;\n");
    out.push_str("    var color = textureSample(tex00, samp_tex00, input.texcoord0) * 0.19;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir) * 0.17;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 2.0) * 0.15;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 3.0) * 0.13;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 4.0) * 0.11;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 5.0) * 0.09;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 6.0) * 0.07;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 7.0) * 0.05;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 8.0) * 0.03;\n");
    out.push_str("    color = color + textureSample(tex00, samp_tex00, input.texcoord0 + dir * 9.0) * 0.01;\n");
    out.push_str("    color.a = input.diffuse.a;\n");
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}

fn wgsl_tex1_mosaic() -> String {
    let mut out = String::new();
    push_uniforms(&mut out);
    push_texture_bindings(&mut out, false, false, false, false, true, false, false);
    push_ps_input_texcoord(&mut out);
    out.push_str("@fragment\n");
    out.push_str("fn main(input: PSInput) -> @location(0) vec4<f32> {\n");
    out.push_str("    let cut_per_texel_u = u.c[0].x;\n");
    out.push_str("    let cut_per_texel_u_inv = 1.0 / cut_per_texel_u;\n");
    out.push_str("    let cut_per_texel_v = u.c[0].x * u.c[0].y;\n");
    out.push_str("    let cut_per_texel_v_inv = 1.0 / cut_per_texel_v;\n");
    out.push_str("    let tc = vec2<f32>(floor(cut_per_texel_u_inv * input.texcoord0.x) * cut_per_texel_u, floor(cut_per_texel_v_inv * input.texcoord0.y) * cut_per_texel_v);\n");
    out.push_str("    var color = textureSample(tex00, samp_tex00_point, tc);\n");
    out.push_str("    color.a = input.diffuse.a;\n");
    out.push_str("    return color;\n");
    out.push_str("}\n");
    out
}
