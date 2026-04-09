use crate::assets::RgbaImage;
use crate::image_manager::ImageManager;
use crate::layer::{ClipRect, RenderSprite, SpriteBlend, SpriteFit, SpriteSizeMode};

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

    for s in sprites {
        let sprite = &s.sprite;
        let Some(img_id) = sprite.image_id else {
            continue;
        };
        let Some(img) = images.get(img_id) else {
            continue;
        };

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

        let scissor = dst_scissor_rect(sprite.dst_clip, width, height);

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

        let p0 = transform_point(0.0, 0.0, dst_x, dst_y, sprite);
        let p1 = transform_point(dst_w, 0.0, dst_x, dst_y, sprite);
        let p2 = transform_point(dst_w, dst_h, dst_x, dst_y, sprite);
        let p3 = transform_point(0.0, dst_h, dst_x, dst_y, sprite);

        let mut min_x = p0.0.min(p1.0).min(p2.0).min(p3.0).floor() as i32;
        let mut max_x = p0.0.max(p1.0).max(p2.0).max(p3.0).ceil() as i32;
        let mut min_y = p0.1.min(p1.1).min(p2.1).min(p3.1).floor() as i32;
        let mut max_y = p0.1.max(p1.1).max(p2.1).max(p3.1).ceil() as i32;

        min_x = min_x.clamp(0, win_w);
        max_x = max_x.clamp(0, win_w);
        min_y = min_y.clamp(0, win_h);
        max_y = max_y.clamp(0, win_h);

        if let Some(sci) = scissor {
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

        let inv_scale_x = if sprite.scale_x.abs() > f32::EPSILON {
            1.0 / sprite.scale_x
        } else {
            continue;
        };
        let inv_scale_y = if sprite.scale_y.abs() > f32::EPSILON {
            1.0 / sprite.scale_y
        } else {
            continue;
        };
        let (sin_r, cos_r) = sprite.rotate.sin_cos();

        for y in min_y..max_y {
            for x in min_x..max_x {
                let world_x = x as f32 + 0.5;
                let world_y = y as f32 + 0.5;

                let rx = world_x - (dst_x + sprite.pivot_x);
                let ry = world_y - (dst_y + sprite.pivot_y);
                let sx = rx * cos_r + ry * sin_r;
                let sy = -rx * sin_r + ry * cos_r;
                let lx = sx * inv_scale_x;
                let ly = sy * inv_scale_y;
                let px = lx + sprite.pivot_x;
                let py = ly + sprite.pivot_y;

                if px < 0.0 || py < 0.0 || px >= dst_w || py >= dst_h {
                    continue;
                }

                let u = src_left + (px / dst_w) * src_w;
                let v = src_top + (py / dst_h) * src_h;
                if u < src_left || v < src_top || u >= src_right || v >= src_bottom {
                    continue;
                }
                let sx = u.floor() as i32;
                let sy = v.floor() as i32;
                if sx < 0 || sy < 0 || sx >= img.width as i32 || sy >= img.height as i32 {
                    continue;
                }

                let src_idx = ((sy as u32 * img.width + sx as u32) * 4) as usize;
                let sr = img.rgba[src_idx] as f32 / 255.0;
                let sg = img.rgba[src_idx + 1] as f32 / 255.0;
                let sb = img.rgba[src_idx + 2] as f32 / 255.0;
                let sa = img.rgba[src_idx + 3] as f32 / 255.0;

                let mut r = sr;
                let mut g = sg;
                let mut b = sb;

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
                r = (r * (1.0 - dark)).clamp(0.0, 1.0);
                g = (g * (1.0 - dark)).clamp(0.0, 1.0);
                b = (b * (1.0 - dark)).clamp(0.0, 1.0);

                if color_rate > 0.0 {
                    r = r + color_rate * (color_r - r);
                    g = g + color_rate * (color_g - g);
                    b = b + color_rate * (color_b - b);
                }
                r = (r + color_add_r).clamp(0.0, 1.0);
                g = (g + color_add_g).clamp(0.0, 1.0);
                b = (b + color_add_b).clamp(0.0, 1.0);

                let mut mask_alpha = sa;
                if sprite.mask_mode == 1 {
                    mask_alpha = sr * 0.299 + sg * 0.587 + sb * 0.114;
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
                let blended = blend_pixel(dst, src_premul, sprite.blend);

                out[dst_idx] = (blended[0].clamp(0.0, 1.0) * 255.0).round() as u8;
                out[dst_idx + 1] = (blended[1].clamp(0.0, 1.0) * 255.0).round() as u8;
                out[dst_idx + 2] = (blended[2].clamp(0.0, 1.0) * 255.0).round() as u8;
                out[dst_idx + 3] = (blended[3].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

    RgbaImage {
        width,
        height,
        rgba: out,
    }
}

fn transform_point(
    px: f32,
    py: f32,
    dst_x: f32,
    dst_y: f32,
    sprite: &crate::layer::Sprite,
) -> (f32, f32) {
    let pivot_x = sprite.pivot_x;
    let pivot_y = sprite.pivot_y;
    let lx = px - pivot_x;
    let ly = py - pivot_y;
    let sx = lx * sprite.scale_x;
    let sy = ly * sprite.scale_y;
    let (sin_r, cos_r) = sprite.rotate.sin_cos();
    let rx = sx * cos_r - sy * sin_r;
    let ry = sx * sin_r + sy * cos_r;
    (dst_x + pivot_x + rx, dst_y + pivot_y + ry)
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
        SpriteBlend::Mul => [src[0] * dst[0], src[1] * dst[1], src[2] * dst[2], out_a],
        SpriteBlend::Screen => [
            src[0] * (1.0 - dst[0]) + dst[0],
            src[1] * (1.0 - dst[1]) + dst[1],
            src[2] * (1.0 - dst[2]) + dst[2],
            out_a,
        ],
    }
}
