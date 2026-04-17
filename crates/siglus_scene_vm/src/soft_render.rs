use crate::assets::RgbaImage;
use crate::image_manager::ImageManager;
use crate::layer::{ClipRect, RenderSprite, SpriteBlend, SpriteFit, SpriteSizeMode};
use crate::render_math::sprite_quad_points;

fn uses_depth_pipeline(sprite: &crate::layer::Sprite) -> bool {
    sprite.camera_enabled
        || sprite.billboard
        || sprite.z.abs() > f32::EPSILON
        || sprite.pivot_z.abs() > f32::EPSILON
        || (sprite.scale_z - 1.0).abs() > 1e-6
        || sprite.rotate_x.abs() > f32::EPSILON
        || sprite.rotate_y.abs() > f32::EPSILON
}

pub fn render_to_image(
    images: &ImageManager,
    sprites: &[RenderSprite],
    width: u32,
    height: u32,
) -> RgbaImage {
    let mut out = vec![
        0u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    let win_w = width as i32;
    let win_h = height as i32;
    let mut depth_buf = vec![f32::INFINITY; (width as usize).saturating_mul(height as usize)];

    for s in sprites {
        let sprite = &s.sprite;
        let Some(img_id) = sprite.image_id else {
            continue;
        };
        let Some(img) = images.get(img_id) else {
            continue;
        };
        let tone_img = sprite.tonecurve_image_id.and_then(|id| images.get(id));
        let mask_img = sprite.mask_image_id.and_then(|id| images.get(id));
        let wipe_src_img = sprite.wipe_src_image_id.and_then(|id| images.get(id));

        let (src_left, src_top, src_right, src_bottom) =
            match src_clip_rect(sprite.src_clip, img.width, img.height) {
                Ok(v) => v,
                Err(_) => continue,
            };
        let src_w = (src_right - src_left).max(1.0);
        let src_h = (src_bottom - src_top).max(1.0);

        let (dst_x, dst_y, dst_w, dst_h) = match sprite.fit {
            SpriteFit::FullScreen => (0.0f32, 0.0f32, win_w as f32, win_h as f32),
            SpriteFit::PixelRect => {
                let (w, h) = match sprite.size_mode {
                    SpriteSizeMode::Intrinsic => (src_w, src_h),
                    SpriteSizeMode::Explicit { width, height } => (width as f32, height as f32),
                };
                (sprite.x as f32, sprite.y as f32, w, h)
            }
        };

        if dst_w <= 0.0 || dst_h <= 0.0 {
            continue;
        }

        let Some([p0, p1, p2, p3]) = sprite_quad_points(
            sprite,
            dst_x,
            dst_y,
            dst_w,
            dst_h,
            width as f32,
            height as f32,
        ) else {
            continue;
        };

        let mut min_x = p0.x.min(p1.x).min(p2.x).min(p3.x).floor() as i32;
        let mut max_x = p0.x.max(p1.x).max(p2.x).max(p3.x).ceil() as i32;
        let mut min_y = p0.y.min(p1.y).min(p2.y).min(p3.y).floor() as i32;
        let mut max_y = p0.y.max(p1.y).max(p2.y).max(p3.y).ceil() as i32;

        min_x = min_x.clamp(0, win_w);
        max_x = max_x.clamp(0, win_w);
        min_y = min_y.clamp(0, win_h);
        max_y = max_y.clamp(0, win_h);

        if let Some(sci) = dst_scissor_rect(sprite.dst_clip, width, height) {
            let sci_left = sci.x as i32;
            let sci_top = sci.y as i32;
            let sci_right = (sci.x + sci.w) as i32;
            let sci_bottom = (sci.y + sci.h) as i32;
            min_x = min_x.max(sci_left);
            max_x = max_x.min(sci_right);
            min_y = min_y.max(sci_top);
            max_y = max_y.min(sci_bottom);
        }

        if min_x >= max_x || min_y >= max_y {
            continue;
        }

        let alpha = (sprite.alpha as f32) / 255.0;
        let tr = (sprite.tr as f32) / 255.0;
        let mono = (sprite.mono as f32) / 255.0;
        let reverse = (sprite.reverse as f32) / 255.0;
        let bright = (sprite.bright as f32) / 255.0;
        let dark = (sprite.dark as f32) / 255.0;
        let color_rate = (sprite.color_rate as f32) / 255.0;
        let color_add_r = (sprite.color_add_r as f32) / 255.0;
        let color_add_g = (sprite.color_add_g as f32) / 255.0;
        let color_add_b = (sprite.color_add_b as f32) / 255.0;
        let color_r = (sprite.color_r as f32) / 255.0;
        let color_g = (sprite.color_g as f32) / 255.0;
        let color_b = (sprite.color_b as f32) / 255.0;

        let use_depth = uses_depth_pipeline(sprite);
        let tris = [
            (
                [
                    (p0.x, p0.y, p0.depth),
                    (p1.x, p1.y, p1.depth),
                    (p2.x, p2.y, p2.depth),
                ],
                [
                    (src_left, src_top),
                    (src_right, src_top),
                    (src_right, src_bottom),
                ],
            ),
            (
                [
                    (p0.x, p0.y, p0.depth),
                    (p2.x, p2.y, p2.depth),
                    (p3.x, p3.y, p3.depth),
                ],
                [
                    (src_left, src_top),
                    (src_right, src_bottom),
                    (src_left, src_bottom),
                ],
            ),
        ];

        for (tri, uv_tri) in tris {
            let area = edge(
                (tri[0].0, tri[0].1),
                (tri[1].0, tri[1].1),
                (tri[2].0, tri[2].1),
            );
            if area.abs() <= f32::EPSILON {
                continue;
            }
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let p = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = edge((tri[1].0, tri[1].1), (tri[2].0, tri[2].1), p) / area;
                    let w1 = edge((tri[2].0, tri[2].1), (tri[0].0, tri[0].1), p) / area;
                    let w2 = edge((tri[0].0, tri[0].1), (tri[1].0, tri[1].1), p) / area;
                    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                        continue;
                    }

                    let depth = w0 * tri[0].2 + w1 * tri[1].2 + w2 * tri[2].2;
                    let u = w0 * uv_tri[0].0 + w1 * uv_tri[1].0 + w2 * uv_tri[2].0;
                    let v = w0 * uv_tri[0].1 + w1 * uv_tri[1].1 + w2 * uv_tri[2].1;
                    if u < src_left || v < src_top || u >= src_right || v >= src_bottom {
                        continue;
                    }
                    let sx = u.floor() as i32;
                    let sy = v.floor() as i32;
                    if sx < 0 || sy < 0 || sx >= img.width as i32 || sy >= img.height as i32 {
                        continue;
                    }

                    let z_idx = (y as u32 * width + x as u32) as usize;
                    if use_depth {
                        if depth > depth_buf[z_idx] + 1e-6 {
                            continue;
                        }
                        depth_buf[z_idx] = depth;
                    }

                    let src = sample_base_with_wipe(
                        sprite,
                        img,
                        wipe_src_img.as_deref().map(|v| &**v),
                        u,
                        v,
                    );
                    let sr = src[0];
                    let sg = src[1];
                    let sb = src[2];
                    let sa = src[3];

                    let mut r = sr;
                    let mut g = sg;
                    let mut b = sb;

                    if let Some(tone_img) = tone_img.as_ref() {
                        let sat = sprite.tonecurve_sat.clamp(0.0, 1.0);
                        if sat > 0.0 {
                            let gray = r * 0.299 + g * 0.587 + b * 0.114;
                            r = r + sat * (gray - r);
                            g = g + sat * (gray - g);
                            b = b + sat * (gray - b);
                        }
                        let row = (sprite.tonecurve_row.clamp(0.0, 1.0) * tone_img.height as f32)
                            .floor()
                            .clamp(0.0, (tone_img.height.saturating_sub(1)) as f32)
                            as u32;
                        let ri = (r.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u32;
                        let gi = (g.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u32;
                        let bi = (b.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u32;
                        r = tone_img.rgba[((row * tone_img.width
                            + ri.min(tone_img.width.saturating_sub(1)))
                            * 4) as usize] as f32
                            / 255.0;
                        g = tone_img.rgba[((row * tone_img.width
                            + gi.min(tone_img.width.saturating_sub(1)))
                            * 4
                            + 1) as usize] as f32
                            / 255.0;
                        b = tone_img.rgba[((row * tone_img.width
                            + bi.min(tone_img.width.saturating_sub(1)))
                            * 4
                            + 2) as usize] as f32
                            / 255.0;
                    }

                    if reverse > 0.0 {
                        r = r + reverse * ((1.0 - r) - r);
                        g = g + reverse * ((1.0 - g) - g);
                        b = b + reverse * ((1.0 - b) - b);
                    }
                    if mono > 0.0 {
                        let gray = r * 0.299 + g * 0.587 + b * 0.114;
                        r = r + mono * (gray - r);
                        g = g + mono * (gray - g);
                        b = b + mono * (gray - b);
                    }

                    r = (r + bright).clamp(0.0, 1.0);
                    g = (g + bright).clamp(0.0, 1.0);
                    b = (b + bright).clamp(0.0, 1.0);
                    r = (r - dark).clamp(0.0, 1.0);
                    g = (g - dark).clamp(0.0, 1.0);
                    b = (b - dark).clamp(0.0, 1.0);

                    if color_rate > 0.0 {
                        r = r + color_rate * (color_r - r);
                        g = g + color_rate * (color_g - g);
                        b = b + color_rate * (color_b - b);
                    }
                    r = (r + color_add_r).clamp(0.0, 1.0);
                    g = (g + color_add_g).clamp(0.0, 1.0);
                    b = (b + color_add_b).clamp(0.0, 1.0);

                    let mut mask_alpha = sa;
                    if let Some(mask_img) = mask_img.as_ref() {
                        let mx = sx + sprite.mask_offset_x;
                        let my = sy + sprite.mask_offset_y;
                        if mx >= 0
                            && my >= 0
                            && mx < mask_img.width as i32
                            && my < mask_img.height as i32
                        {
                            let mi = ((my as u32 * mask_img.width + mx as u32) * 4) as usize;
                            let mr = mask_img.rgba[mi] as f32 / 255.0;
                            let mg = mask_img.rgba[mi + 1] as f32 / 255.0;
                            let mb = mask_img.rgba[mi + 2] as f32 / 255.0;
                            let ma = mask_img.rgba[mi + 3] as f32 / 255.0;
                            mask_alpha *=
                                ((mr * 0.299 + mg * 0.587 + mb * 0.114) * ma).clamp(0.0, 1.0);
                        } else {
                            mask_alpha = 0.0;
                        }
                    }
                    if sprite.mask_mode == 1 {
                        mask_alpha = sr * 0.299 + sg * 0.587 + sb * 0.114;
                    }
                    if sprite.alpha_test && mask_alpha <= 0.0 {
                        continue;
                    }
                    let a = (mask_alpha * alpha * tr).clamp(0.0, 1.0);
                    if a <= 0.0 {
                        continue;
                    }

                    let src_premul = [r * a, g * a, b * a, a];
                    let dst_idx = ((y as u32 * width + x as u32) * 4) as usize;
                    let dr = out[dst_idx] as f32 / 255.0;
                    let dg = out[dst_idx + 1] as f32 / 255.0;
                    let db = out[dst_idx + 2] as f32 / 255.0;
                    let da = out[dst_idx + 3] as f32 / 255.0;

                    let dst = [dr, dg, db, da];
                    let blended = if sprite.alpha_blend {
                        blend_pixel(dst, src_premul, sprite.blend)
                    } else {
                        src_premul
                    };

                    out[dst_idx] = (blended[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                    out[dst_idx + 1] = (blended[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                    out[dst_idx + 2] = (blended[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                    out[dst_idx + 3] = (blended[3].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }

    RgbaImage {
        width,
        height,
        rgba: out,
    }
}

fn sample_rgba_norm(img: &RgbaImage, u: f32, v: f32) -> [f32; 4] {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let x = (u * img.width as f32)
        .floor()
        .clamp(0.0, (img.width.saturating_sub(1)) as f32) as u32;
    let y = (v * img.height as f32)
        .floor()
        .clamp(0.0, (img.height.saturating_sub(1)) as f32) as u32;
    let i = ((y * img.width + x) * 4) as usize;
    [
        img.rgba[i] as f32 / 255.0,
        img.rgba[i + 1] as f32 / 255.0,
        img.rgba[i + 2] as f32 / 255.0,
        img.rgba[i + 3] as f32 / 255.0,
    ]
}

fn raster_amp(progress: f32) -> f32 {
    let rp = (1.0 - progress).clamp(1e-4, 1.0);
    1.0 - (((1.0 - rp) * 100.0).log10() + 1.0) / 3.0
}

fn sample_base_with_wipe(
    sprite: &crate::layer::Sprite,
    img: &RgbaImage,
    wipe_src_img: Option<&RgbaImage>,
    u_px: f32,
    v_px: f32,
) -> [f32; 4] {
    let mut u = (u_px / img.width as f32).clamp(0.0, 1.0);
    let mut v = (v_px / img.height as f32).clamp(0.0, 1.0);
    match sprite.wipe_fx_mode {
        1 => {
            let cu = sprite.wipe_fx_params[0].max(1e-5);
            let cv = (cu * sprite.wipe_fx_params[1].max(1e-5)).max(1e-5);
            u = (u / cu).floor() * cu;
            v = (v / cv).floor() * cv;
            sample_rgba_norm(img, u, v)
        }
        2 | 3 => {
            let fraction_num = sprite.wipe_fx_params[0].max(1.0);
            let wave_num = sprite.wipe_fx_params[1];
            let power = sprite.wipe_fx_params[2];
            let progress = sprite.wipe_fx_params[3];
            let mut tex_coord_for_sin = if sprite.wipe_fx_mode == 2 {
                v * fraction_num
            } else {
                u * fraction_num
            };
            tex_coord_for_sin = tex_coord_for_sin.fract();
            tex_coord_for_sin = (tex_coord_for_sin - fraction_num * 0.1) / fraction_num;
            let delta = (std::f32::consts::PI * progress * power
                + tex_coord_for_sin * std::f32::consts::PI * wave_num)
                .sin()
                * raster_amp(progress);
            if sprite.wipe_fx_mode == 2 {
                u += delta;
            } else {
                v += delta;
            }
            sample_rgba_norm(img, u, v)
        }
        4 => {
            let center = (sprite.wipe_fx_params[0], sprite.wipe_fx_params[1]);
            let blur_power = sprite.wipe_fx_params[2];
            let blur_coeff = sprite.wipe_fx_params[3].max(0.0);
            let mut dir = (center.0 - u, center.1 - v);
            let len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
            if len <= 1e-5 || blur_power <= 1e-5 {
                return sample_rgba_norm(img, u, v);
            }
            let texel = 1.0 / (img.width.max(img.height).max(1) as f32);
            dir.0 = dir.0 / len * texel * blur_power * len * blur_coeff;
            dir.1 = dir.1 / len * texel * blur_power * len * blur_coeff;
            let taps = [
                (0.0, 0.19f32),
                (1.0, 0.17),
                (2.0, 0.15),
                (3.0, 0.13),
                (4.0, 0.11),
                (5.0, 0.09),
                (6.0, 0.07),
                (7.0, 0.05),
                (8.0, 0.03),
                (9.0, 0.01),
            ];
            let mut out = [0.0f32; 4];
            for (k, w) in taps {
                let s = sample_rgba_norm(img, u + dir.0 * k, v + dir.1 * k);
                for i in 0..4 {
                    out[i] += s[i] * w;
                }
            }
            out
        }
        5 => {
            let mut c = sample_rgba_norm(img, u, v);
            let fade = sprite.wipe_fx_params[0].clamp(0.0, 1.0);
            let progress = sprite.wipe_fx_params[1].clamp(0.0, 1.0);
            let brightness = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            if brightness > progress {
                c[3] *= (fade * (1.0 - progress)).clamp(0.0, 1.0);
            }
            c
        }
        6 => {
            let mut c = sample_rgba_norm(img, u, v);
            let fade = sprite.wipe_fx_params[0].clamp(0.0, 1.0);
            let progress = sprite.wipe_fx_params[1].clamp(0.0, 1.0);
            let brightness = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            if brightness < 1.0 - progress {
                c[3] *= (fade * (1.0 - progress)).clamp(0.0, 1.0);
            }
            c
        }
        10 => {
            let Some(src) = wipe_src_img else {
                return sample_rgba_norm(img, u, v);
            };
            let cut = sprite.wipe_fx_params[0].max(1e-5);
            let aspect = sprite.wipe_fx_params[1].max(1e-5);
            let progress = sprite.wipe_fx_params[2].clamp(0.0, 1.0);
            let variant = sprite.wipe_fx_params[3] as i32;
            let cu = cut;
            let cv = (cut * aspect).max(1e-5);
            let uu = (u / cu).floor() * cu;
            let vv = (v / cv).floor() * cv;
            let oldc = sample_rgba_norm(src, uu, vv);
            let newc = sample_rgba_norm(img, uu, vv);
            if variant == 230 {
                if progress < 0.5 {
                    oldc
                } else {
                    newc
                }
            } else if sprite.tonecurve_sat < 0.5 {
                mix_rgba(newc, oldc, 1.0 - progress)
            } else {
                mix_rgba(oldc, newc, progress)
            }
        }
        11 | 12 => {
            let Some(src) = wipe_src_img else {
                return sample_rgba_norm(img, u, v);
            };
            let fraction_num = sprite.wipe_fx_params[0].max(1.0);
            let wave_num = sprite.wipe_fx_params[1];
            let power = sprite.wipe_fx_params[2];
            let progress = sprite.wipe_fx_params[3].clamp(0.0, 1.0);
            let mut tex_coord_for_sin = if sprite.wipe_fx_mode == 11 {
                v * fraction_num
            } else {
                u * fraction_num
            };
            tex_coord_for_sin = tex_coord_for_sin.fract();
            tex_coord_for_sin = (tex_coord_for_sin - fraction_num * 0.1) / fraction_num;
            let delta = (std::f32::consts::PI * progress * power
                + tex_coord_for_sin * std::f32::consts::PI * wave_num)
                .sin()
                * raster_amp(progress);
            let (nu, nv) = if sprite.wipe_fx_mode == 11 {
                (u + delta, v)
            } else {
                (u, v + delta)
            };
            let oldc = sample_rgba_norm(src, nu, nv);
            let newc = sample_rgba_norm(img, nu, nv);
            mix_rgba(oldc, newc, progress)
        }
        13 => {
            let Some(src) = wipe_src_img else {
                return sample_rgba_norm(img, u, v);
            };
            let center = (sprite.wipe_fx_params[0], sprite.wipe_fx_params[1]);
            let blur_power = sprite.wipe_fx_params[2];
            let blur_coeff = sprite.wipe_fx_params[3].max(0.0);
            let mixv = sprite.tonecurve_row.clamp(0.0, 1.0);
            let oldc = sample_explosion_src(src, u, v, center, blur_power, blur_coeff);
            let newc = sample_explosion_src(img, u, v, center, blur_power, blur_coeff);
            mix_rgba(oldc, newc, mixv)
        }
        _ => sample_rgba_norm(img, u, v),
    }
}

fn mix_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let tt = t.clamp(0.0, 1.0);
    [
        a[0] * (1.0 - tt) + b[0] * tt,
        a[1] * (1.0 - tt) + b[1] * tt,
        a[2] * (1.0 - tt) + b[2] * tt,
        a[3] * (1.0 - tt) + b[3] * tt,
    ]
}

fn sample_explosion_src(
    img: &RgbaImage,
    u: f32,
    v: f32,
    center: (f32, f32),
    blur_power: f32,
    blur_coeff: f32,
) -> [f32; 4] {
    let mut dir = (center.0 - u, center.1 - v);
    let len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
    if len <= 1e-5 || blur_power <= 1e-5 {
        return sample_rgba_norm(img, u, v);
    }
    let texel = 1.0 / (img.width.max(img.height).max(1) as f32);
    dir.0 = dir.0 / len * texel * blur_power * len * blur_coeff;
    dir.1 = dir.1 / len * texel * blur_power * len * blur_coeff;
    let taps = [
        (0.0, 0.19f32),
        (1.0, 0.17),
        (2.0, 0.15),
        (3.0, 0.13),
        (4.0, 0.11),
        (5.0, 0.09),
        (6.0, 0.07),
        (7.0, 0.05),
        (8.0, 0.03),
        (9.0, 0.01),
    ];
    let mut out = [0.0f32; 4];
    for (k, w) in taps {
        let s = sample_rgba_norm(img, u + dir.0 * k, v + dir.1 * k);
        for i in 0..4 {
            out[i] += s[i] * w;
        }
    }
    out
}

fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (p.0 - a.0) * (b.1 - a.1) - (p.1 - a.1) * (b.0 - a.0)
}

fn src_clip_rect(
    clip: Option<ClipRect>,
    img_w: u32,
    img_h: u32,
) -> Result<(f32, f32, f32, f32), ()> {
    if let Some(c) = clip {
        let mut left = c.left.max(0) as f32;
        let mut top = c.top.max(0) as f32;
        let mut right = c.right.max(0) as f32;
        let mut bottom = c.bottom.max(0) as f32;
        let max_w = img_w as f32;
        let max_h = img_h as f32;
        left = left.min(max_w);
        right = right.min(max_w);
        top = top.min(max_h);
        bottom = bottom.min(max_h);
        if right <= left || bottom <= top {
            return Ok((0.0, 0.0, max_w, max_h));
        }
        Ok((left, top, right, bottom))
    } else {
        Ok((0.0, 0.0, img_w as f32, img_h as f32))
    }
}

fn dst_scissor_rect(clip: Option<ClipRect>, win_w: u32, win_h: u32) -> Option<ScissorRect> {
    let c = clip?;
    let mut left = c.left.max(0) as i64;
    let mut top = c.top.max(0) as i64;
    let mut right = c.right.max(0) as i64;
    let mut bottom = c.bottom.max(0) as i64;
    let max_w = win_w as i64;
    let max_h = win_h as i64;
    left = left.min(max_w);
    right = right.min(max_w);
    top = top.min(max_h);
    bottom = bottom.min(max_h);
    if right <= left || bottom <= top {
        return Some(ScissorRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
    }
    Some(ScissorRect {
        x: left as u32,
        y: top as u32,
        w: (right - left) as u32,
        h: (bottom - top) as u32,
    })
}

#[derive(Debug, Copy, Clone)]
struct ScissorRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn blend_pixel(dst: [f32; 4], src: [f32; 4], blend: SpriteBlend) -> [f32; 4] {
    let sa = src[3].clamp(0.0, 1.0);
    let da = dst[3].clamp(0.0, 1.0);
    let out_a = sa + da * (1.0 - sa);
    match blend {
        SpriteBlend::Normal => [
            src[0] + dst[0] * (1.0 - sa),
            src[1] + dst[1] * (1.0 - sa),
            src[2] + dst[2] * (1.0 - sa),
            out_a,
        ],
        SpriteBlend::Add => [
            (src[0] + dst[0]).min(1.0),
            (src[1] + dst[1]).min(1.0),
            (src[2] + dst[2]).min(1.0),
            out_a,
        ],
        SpriteBlend::Sub => [
            (dst[0] - src[0]).max(0.0),
            (dst[1] - src[1]).max(0.0),
            (dst[2] - src[2]).max(0.0),
            out_a,
        ],
        SpriteBlend::Mul => [
            dst[0] * ((1.0 - sa) + src[0]),
            dst[1] * ((1.0 - sa) + src[1]),
            dst[2] * ((1.0 - sa) + src[2]),
            out_a,
        ],
        SpriteBlend::Screen => [
            src[0] * (1.0 - dst[0]) + dst[0],
            src[1] * (1.0 - dst[1]) + dst[1],
            src[2] * (1.0 - dst[2]) + dst[2],
            out_a,
        ],
        SpriteBlend::Overlay => {
            let overlay = |d: f32, s: f32| {
                if d <= 0.5 {
                    2.0 * d * s
                } else {
                    1.0 - 2.0 * (1.0 - d) * (1.0 - s)
                }
            };
            [
                overlay(dst[0], src[0]) * sa + dst[0] * (1.0 - sa),
                overlay(dst[1], src[1]) * sa + dst[1] * (1.0 - sa),
                overlay(dst[2], src[2]) * sa + dst[2] * (1.0 - sa),
                out_a,
            ]
        }
    }
}
