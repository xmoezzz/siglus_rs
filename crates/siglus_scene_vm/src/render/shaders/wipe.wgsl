struct WipeUniform {
    kind_progress: vec4<f32>,
    option0: vec4<f32>,
    option1: vec4<f32>,
    option2: vec4<f32>,
    option3: vec4<f32>,
};

@group(0) @binding(0) var under_tex: texture_2d<f32>;
@group(0) @binding(1) var under_smp: sampler;
@group(0) @binding(2) var current_tex: texture_2d<f32>;
@group(0) @binding(3) var current_smp: sampler;
@group(0) @binding(4) var next_tex: texture_2d<f32>;
@group(0) @binding(5) var next_smp: sampler;
@group(0) @binding(6) var mask_tex: texture_2d<f32>;
@group(0) @binding(7) var mask_smp: sampler;
@group(0) @binding(8) var<uniform> wipe: WipeUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0)
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

fn option(index: i32) -> f32 {
    if (index < 4) { return wipe.option0[index]; }
    if (index < 8) { return wipe.option1[index - 4]; }
    if (index < 12) { return wipe.option2[index - 8]; }
    return wipe.option3[index - 12];
}

fn inside(uv: vec2<f32>) -> bool {
    return all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
}

fn sample_or_zero(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>) -> vec4<f32> {
    if (!inside(uv)) { return vec4<f32>(0.0); }
    return textureSample(tex, smp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

fn alpha_over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    let out_a = src.a + dst.a * (1.0 - src.a);
    if (out_a <= 0.000001) { return vec4<f32>(0.0); }
    let rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / out_a;
    return vec4<f32>(rgb, out_a);
}

fn mask_fade(mode: i32) -> f32 {
    switch mode {
        case 0: { return 0.0; }
        case 1: { return 0.5; }
        case 2: { return 0.75; }
        case 3: { return 0.875; }
        case 4: { return 0.9375; }
        case 5: { return 0.96875; }
        case 6: { return 0.984375; }
        case 7: { return 0.9921875; }
        default: { return 1.0; }
    }
}

fn mask_reveal(progress: f32, threshold: f32, fade: f32) -> f32 {
    if (fade <= 0.000001) { return select(0.0, 1.0, progress >= threshold); }
    return clamp((progress - threshold * (1.0 - fade)) / fade, 0.0, 1.0);
}

fn luminance(c: vec4<f32>) -> f32 {
    return dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114)) * c.a;
}

fn rect_sample(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, rect: vec4<f32>, alpha: f32) -> vec4<f32> {
    let local = (uv - rect.xy) / max(rect.zw, vec2<f32>(0.000001));
    let c = sample_or_zero(tex, smp, local);
    return vec4<f32>(c.rgb, c.a * alpha);
}

fn move_rect(direction: i32, mode: i32, progress: f32, incoming: bool) -> vec4<f32> {
    if (mode == 0) { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
    if (mode == 1) {
        var start = vec2<f32>(0.0);
        var end = vec2<f32>(0.0);
        if (incoming) {
            if (direction == 0) { start.y = -1.0; }
            else if (direction == 1) { start.y = 1.0; }
            else if (direction == 2) { start.x = -1.0; }
            else { start.x = 1.0; }
        } else {
            if (direction == 0) { end.y = 1.0; }
            else if (direction == 1) { end.y = -1.0; }
            else if (direction == 2) { end.x = 1.0; }
            else { end.x = -1.0; }
        }
        return vec4<f32>(mix(start, end, progress), 1.0, 1.0);
    }
    let scale = max(select(1.0 - progress, progress, incoming), 0.000001);
    var rect = vec4<f32>(0.0, 0.0, 1.0, 1.0);
    if (direction <= 1) {
        rect.w = scale;
        if (direction == 1) { rect.y = 1.0 - scale; }
    } else {
        rect.z = scale;
        if (direction == 3) { rect.x = 1.0 - scale; }
    }
    return rect;
}

fn scale_rect(mode: i32, scale_value: f32) -> vec4<f32> {
    let s = max(scale_value, 0.000001);
    var anchor = vec2<f32>(0.5, 0.5);
    var scale = vec2<f32>(s, s);
    switch mode {
        case 1: { anchor = vec2<f32>(0.0, 0.0); }
        case 2: { anchor = vec2<f32>(1.0, 0.0); }
        case 3: { anchor = vec2<f32>(0.0, 1.0); }
        case 4: { anchor = vec2<f32>(1.0, 1.0); }
        case 5: { scale.x = 1.0; }
        case 6: { scale.y = 1.0; }
        case 7: { anchor = vec2<f32>(0.0, 0.0); scale.x = 1.0; }
        case 8: { anchor = vec2<f32>(0.0, 1.0); scale.x = 1.0; }
        case 9: { anchor = vec2<f32>(0.0, 0.0); scale.y = 1.0; }
        case 10: { anchor = vec2<f32>(1.0, 0.0); scale.y = 1.0; }
        case 11: {
            anchor = vec2<f32>(option(2) / max(wipe.kind_progress.z, 1.0), option(3) / max(wipe.kind_progress.w, 1.0));
        }
        default: {}
    }
    return vec4<f32>(anchor * (vec2<f32>(1.0) - scale), scale);
}

fn scale_uv(mode: i32, rate_in: f32) -> vec4<f32> {
    var rate = rate_in;
    var vrate = rate_in;
    switch mode {
        case 0: { return vec4<f32>(0.5 - 0.5 * rate, 0.5 - 0.5 * vrate, rate, vrate); }
        case 1: { return vec4<f32>(0.0, 0.0, rate, vrate); }
        case 2: { return vec4<f32>(1.0 - rate, 0.0, rate, vrate); }
        case 3: { return vec4<f32>(0.0, 1.0 - vrate, rate, vrate); }
        case 4: { return vec4<f32>(1.0 - rate, 1.0 - vrate, rate, vrate); }
        case 5: { vrate = mix(0.49, 1.0, clamp(vrate, 0.0, 1.0)); return vec4<f32>(0.0, 1.0 - vrate, 1.0, 2.0 * vrate - 1.0); }
        case 6: { rate = mix(0.49, 1.0, clamp(rate, 0.0, 1.0)); return vec4<f32>(1.0 - rate, 0.0, 2.0 * rate - 1.0, 1.0); }
        case 7: { return vec4<f32>(0.0, 0.0, 1.0, vrate); }
        case 8: { return vec4<f32>(0.0, 1.0 - vrate, 1.0, vrate); }
        case 9: { return vec4<f32>(0.0, 0.0, rate, 1.0); }
        case 10: { return vec4<f32>(1.0 - rate, 0.0, rate, 1.0); }
        case 11: {
            let x = option(2) / max(wipe.kind_progress.z, 1.0);
            let y = option(3) / max(wipe.kind_progress.w, 1.0);
            return vec4<f32>(x - x * rate, y - y * vrate, rate, vrate);
        }
        default: { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
    }
}

fn sample_uv_box(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, box: vec4<f32>, alpha: f32) -> vec4<f32> {
    let sample_uv = box.xy + uv * box.zw;
    let c = textureSample(tex, smp, clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    return vec4<f32>(c.rgb, c.a * alpha);
}

fn sample_mosaic(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, size: f32) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(tex));
    let cell = max(vec2<f32>(1.0), vec2<f32>(size));
    let px = floor(uv * dims / cell) * cell + 0.5 * cell;
    return textureSample(tex, smp, clamp(px / dims, vec2<f32>(0.0), vec2<f32>(1.0)));
}

fn explosion(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, center: vec2<f32>, power: f32) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    let delta = (center - uv) * power / 16.0;
    for (var i = 0; i < 16; i = i + 1) {
        sum += textureSample(tex, smp, clamp(uv + delta * f32(i), vec2<f32>(0.0), vec2<f32>(1.0)));
    }
    return sum / 16.0;
}

fn raster_offset(uv: vec2<f32>, vertical: bool, fraction: f32, wave: f32, power: f32, progress: f32) -> vec2<f32> {
    let axis = select(uv.x, uv.y, vertical);
    let phase = axis * max(fraction, 1.0) * 6.28318530718 + progress * wave * 6.28318530718;
    let amp = sin(phase) * power;
    return select(vec2<f32>(amp, 0.0), vec2<f32>(0.0, amp), vertical);
}

fn wipe_shimi_source(color_in: vec4<f32>, fade: f32, progress: f32, reverse: bool) -> vec4<f32> {
    var color = color_in;
    let brightness = dot(vec3<f32>(0.299, 0.587, 0.114), color.rgb);
    let hide = select(brightness > progress, brightness < 1.0 - progress, reverse);
    if (hide) {
        // shader.cfx ps_tex1_shimi / ps_tex1_shimi_inv:
        //   color.a = tex.a * (c0.x - lerp(c0.x, 0.0, c0.w))
        // which simplifies to tex.a * fade * progress.
        color.a = color.a * max(fade * progress, 0.0);
    }
    return color;
}

fn triangular_parameter(kind: i32, reverse: bool, progress: f32) -> f32 {
    var value = 0.0;
    if (kind == 0) {
        value = 1.0 - progress;
    } else if (kind == 10) {
        value = progress;
    } else {
        let threshold = clamp(f32(kind) / 10.0, 0.000001, 0.999999);
        value = select(
            (1.0 - progress) / (1.0 - threshold),
            progress / threshold,
            progress < threshold,
        );
    }
    return clamp(select(value, 1.0 - value, reverse), 0.0, 1.0);
}

fn affected_color(uv: vec2<f32>) -> vec4<f32> {
    let kind = i32(round(wipe.kind_progress.x));
    let p = clamp(wipe.kind_progress.y, 0.0, 1.0);
    let current = textureSample(current_tex, current_smp, uv);
    let next = textureSample(next_tex, next_smp, uv);

    // After WIPE starts, FRONT is the newly prepared scene and NEXT is the
    // saved old scene.  C_tnm_wnd::disp_proc_wipe_for_cross_fade draws
    // under+NEXT first, then fades the under+FRONT wipe buffer in from
    // progress 0 to 255.  Types 1 and 2 are the corresponding fixed FRONT and
    // fixed NEXT modes.  These inputs are complete scenes, not isolated
    // transparent target layers.
    if (kind == 0) {
        if (p <= 0.0) { return next; }
        if (p >= 1.0) { return current; }
        return mix(next, current, p);
    }
    if (kind == 1) { return current; }
    if (kind == 2) { return next; }

    // C++ mask and raster wipes always transition from NEXT (the saved old
    // scene) to FRONT (the prepared new scene). Keep the legacy endpoint
    // behavior for the other effect families until each of those families is
    // audited independently against eng_disp_wipe.cpp.
    let old_to_new_family =
        (kind >= 5 && kind < 200) || kind == 900 || kind == 901 || kind == 220 || kind == 221;
    if (old_to_new_family) {
        if (p <= 0.0) { return next; }
        if (p >= 1.0) { return current; }
    } else {
        if (p <= 0.0) { return current; }
        if (p >= 1.0) { return next; }
    }
    if (kind == 200) {
        let direction = i32(option(0)) % 4;
        let cmode = i32(option(1));
        let nmode = i32(option(2));
        let c = rect_sample(current_tex, current_smp, uv, move_rect(direction, cmode, p, false), 1.0);
        let n = rect_sample(next_tex, next_smp, uv, move_rect(direction, nmode, p, true), 1.0);
        return select(alpha_over(n, c), alpha_over(c, n), cmode == 0);
    }
    if (kind == 210 || kind == 211) {
        let incoming = kind == 210;
        let base = select(current, next, incoming);
        let moving_tex_next = incoming;
        let rect = scale_rect(i32(option(0)), select(1.0 - p, p, incoming));
        let alpha = select(select(1.0, 1.0 - p, i32(option(1)) == 1), select(1.0, p, i32(option(1)) == 1), incoming);
        let moving = select(rect_sample(current_tex, current_smp, uv, rect, alpha), rect_sample(next_tex, next_smp, uv, rect, alpha), moving_tex_next);
        return alpha_over(base, moving);
    }
    if (kind == 212) {
        if (p < 0.5) { return sample_uv_box(current_tex, current_smp, uv, scale_uv(i32(option(0)), mix(1.0, 0.001, p * 2.0)), 1.0); }
        return sample_uv_box(next_tex, next_smp, uv, scale_uv(i32(option(0)), mix(0.001, 1.0, (p - 0.5) * 2.0)), 1.0);
    }
    if (kind == 213) {
        let n = sample_uv_box(next_tex, next_smp, uv, scale_uv(i32(option(0)), mix(0.333, 1.0, p)), p);
        return alpha_over(current, n);
    }
    if (kind == 214) {
        let c = sample_uv_box(current_tex, current_smp, uv, scale_uv(i32(option(0)), mix(1.0, 0.333, p)), 1.0 - p);
        return alpha_over(next, c);
    }
    if (kind == 215) {
        let sx = clamp(option(2) / max(wipe.kind_progress.z, 1.0), 0.0, 1.0);
        let sy = clamp(option(3) / max(wipe.kind_progress.w, 1.0), 0.0, 1.0);
        let ex = clamp(option(4) / max(wipe.kind_progress.z, 1.0), 0.0, 1.0);
        let ey = clamp(option(5) / max(wipe.kind_progress.w, 1.0), 0.0, 1.0);
        let specified = vec4<f32>(min(sx, ex), min(sy, ey), max(abs(ex - sx), 1.0 / max(wipe.kind_progress.z, 1.0)), max(abs(ey - sy), 1.0 / max(wipe.kind_progress.w, 1.0)));
        let alpha_mode = i32(option(0));
        let alpha = select(select(1.0, 1.0 - p, alpha_mode == 2), p, alpha_mode == 1);
        if (i32(option(1)) == 0) {
            let rect = mix(specified, vec4<f32>(0.0, 0.0, 1.0, 1.0), p);
            return alpha_over(current, rect_sample(next_tex, next_smp, uv, rect, alpha));
        }
        let rect = mix(vec4<f32>(0.0, 0.0, 1.0, 1.0), specified, p);
        return alpha_over(next, rect_sample(current_tex, current_smp, uv, rect, alpha));
    }
    if (kind == 220 || kind == 221) {
        let vertical = i32(option(0)) == 0;
        let dim = select(wipe.kind_progress.z, wipe.kind_progress.w, vertical);
        let fraction = dim / max(option(1), 1.0);
        if (kind == 220) {
            // C++ cross-raster renders NEXT (the pre-wipe/old scene) into
            // wipe buffer 1 and FRONT (the prepared/new scene) into buffer 0.
            // The transition therefore has to start at NEXT and finish at
            // FRONT. Keep both endpoints undistorted, with the opposite image
            // carrying the full raster displacement away from its endpoint.
            let offset = raster_offset(uv, vertical, fraction, option(2), option(3) / max(dim, 1.0), p);
            let old_scene = sample_or_zero(next_tex, next_smp, uv - offset * p);
            let new_scene = sample_or_zero(current_tex, current_smp, uv + offset * (1.0 - p));
            return mix(old_scene, new_scene, p);
        }

        // C++ single-raster chooses which stage is rendered directly and which
        // one is processed through the wipe buffer. option[4]==0 means
        // base=NEXT(old), processed=FRONT(new), wpf=p. option[4]!=0 means
        // base=FRONT(new), processed=NEXT(old), wpf=1-p. In both cases the
        // visible result progresses old -> new.
        let reverse = i32(option(4)) != 0;
        let t = select(p, 1.0 - p, reverse);
        let offset = raster_offset(uv, vertical, fraction, option(2), option(3) / max(dim, 1.0), t);
        if (!reverse) {
            let warped_new = sample_or_zero(current_tex, current_smp, uv + offset * (1.0 - t));
            return alpha_over(next, vec4<f32>(warped_new.rgb, warped_new.a * t));
        }
        let warped_old = sample_or_zero(next_tex, next_smp, uv + offset * (1.0 - t));
        return alpha_over(current, vec4<f32>(warped_old.rgb, warped_old.a * t));
    }
    if (kind == 230 || kind == 231) {
        if (kind == 230) {
            let first = p < 0.5;
            let local = select((p - 0.5) * 2.0, p * 2.0, first);
            let size = mix(1.0, max(option(0), 1.0), select(1.0 - local, local, first));
            return select(sample_mosaic(next_tex, next_smp, uv, size), sample_mosaic(current_tex, current_smp, uv, size), first);
        }
        let use_next = i32(option(1)) != 0;
        let size = mix(max(option(0), 1.0), 1.0, p);
        let src = select(sample_mosaic(current_tex, current_smp, uv, size), sample_mosaic(next_tex, next_smp, uv, size), use_next);
        return vec4<f32>(src.rgb, src.a * select(1.0 - p, p, use_next));
    }
    if (kind >= 240 && kind <= 243) {
        // C_tnm_wnd::disp_proc_wipe_for_explosion_blur_get_stage returns the
        // stage drawn underneath. The opposite stage is the sprite processed
        // by the explosion-blur technique.
        var base_is_current = false;
        if (kind == 241) { base_is_current = i32(option(7)) == 0; }
        if (kind == 243) { base_is_current = i32(option(5)) == 0; }
        let processed_is_next = base_is_current;
        let base = select(next, current, base_is_current);
        let processed = select(current, next, processed_is_next);

        let alpha_type = i32(select(option(0), option(2), kind <= 241));
        let alpha_reverse = i32(select(option(1), option(3), kind <= 241)) != 0;
        // The C++ value is rp.tr (transparency), so convert it to visible alpha.
        let visible_alpha = 1.0 - triangular_parameter(alpha_type, alpha_reverse, p);

        let power_type = i32(select(option(2), option(4), kind <= 241));
        let power_reverse = i32(select(option(3), option(5), kind <= 241)) != 0;
        let power = triangular_parameter(power_type, power_reverse, p);
        let coefficient = max(select(option(4), option(6), kind <= 241), 0.0);

        var center = vec2<f32>(0.5);
        if (kind <= 241) {
            center = vec2<f32>(
                option(0) / max(wipe.kind_progress.z, 1.0),
                option(1) / max(wipe.kind_progress.w, 1.0),
            );
        } else {
            let seed = option(15);
            center = fract(vec2<f32>(sin(seed * 12.9898), sin(seed * 78.233)) * 43758.5453);
        }
        let blurred = select(
            explosion(current_tex, current_smp, uv, center, power * coefficient),
            explosion(next_tex, next_smp, uv, center, power * coefficient),
            processed_is_next,
        );
        let processed_color = vec4<f32>(blurred.rgb, processed.a * visible_alpha);
        return alpha_over(base, processed_color);
    }
    if (kind == 50) {
        // Original type 50 draws NEXT first and then draws FRONT through
        // tec_tex1_shimi / tec_tex1_shimi_inv. option[0] selects the fade
        // constant and option[1] selects the inverse technique.
        let processed = wipe_shimi_source(
            current,
            mask_fade(i32(option(0))),
            p,
            i32(option(1)) != 0,
        );
        return alpha_over(next, processed);
    }
    if (kind == 901) {
        let max_root = sqrt(max(option(1) / 1000.0, 0.000001));
        let root_now = select(max_root * p, max_root * (1.0 - p), i32(option(0)) != 0);
        let scale = max(root_now * root_now, 0.000001);
        let angle = radians(360.0 * option(2) * p);
        let cs = cos(angle);
        let sn = sin(angle);
        let d = uv - vec2<f32>(0.5);
        var muv = vec2<f32>(cs * d.x + sn * d.y, -sn * d.x + cs * d.y) / scale + vec2<f32>(0.5);
        let cells = clamp(option(3), 0.0, 64.0);
        var mask_value = 1.0;
        if (cells > 0.0) {
            muv = fract(muv * cells);
            if (i32(option(4)) != 0 && (muv.x < 0.03 || muv.y < 0.03 || muv.x > 0.97 || muv.y > 0.97)) { mask_value = 1.0; }
            else { mask_value = luminance(textureSample(mask_tex, mask_smp, muv)); }
        } else if (inside(muv)) {
            mask_value = luminance(textureSample(mask_tex, mask_smp, muv));
        }
        return mix(current, next, select(0.0, 1.0, mask_value >= 0.5));
    }
    if (kind == 900 || (kind >= 5 && kind < 200)) {
        let mask_value = luminance(textureSample(mask_tex, mask_smp, uv));
        let reveal = mask_reveal(p, 1.0 - mask_value, mask_fade(i32(option(0))));
        // After C_elm_stage_list::wipe(), FRONT is the prepared/new scene and
        // NEXT is the saved pre-wipe/old scene. C++ disp_proc_wipe_for_mask()
        // draws NEXT first, then draws FRONT into wipe buffer 0 and reveals
        // that buffer through the mask. Therefore reveal=0 must show NEXT and
        // reveal=1 must show FRONT. The previous order was exactly reversed,
        // which is especially obvious for blind masks (102/122/142/152).
        return mix(next, current, reveal);
    }
    return mix(current, next, p);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let kind = i32(round(wipe.kind_progress.x));
    if (kind == 0 || kind == 1 || kind == 2) {
        // The cross fades on their own, ahead of the effect families.
        let p = clamp(wipe.kind_progress.y, 0.0, 1.0);
        let current = textureSample(current_tex, current_smp, uv);
        let next = textureSample(next_tex, next_smp, uv);
        let t = select(select(p, 1.0, kind == 1), 0.0, kind == 2);
        return mix(next, current, t);
    }
    let under = textureSample(under_tex, under_smp, uv);
    return alpha_over(under, affected_color(uv));
}
