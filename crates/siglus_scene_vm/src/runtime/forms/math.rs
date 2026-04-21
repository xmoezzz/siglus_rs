use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::forms::codes::elm_value;
use crate::runtime::{CommandContext, Value};

const TNM_ANGLE_UNIT: f64 = 10.0;

fn round_half_away_from_zero(v: f64) -> i64 {
    if v > 0.0 {
        (v + 0.5) as i64
    } else if v < 0.0 {
        (v - 0.5) as i64
    } else {
        0
    }
}

fn repeat_char(c: char, n: usize) -> String {
    std::iter::repeat(c).take(n).collect()
}

fn tostr_pad(num: i64, len: i64, fill: char) -> String {
    let len = len.max(0) as usize;

    if num == 0 {
        if len <= 1 {
            return "0".to_string();
        }
        let mut out = repeat_char(fill, len - 1);
        out.push('0');
        return out;
    }

    if num > 0 {
        let digits = num.to_string();
        if len <= digits.len() {
            return digits;
        }
        let mut out = repeat_char(fill, len - digits.len());
        out.push_str(&digits);
        return out;
    }

    // Negative number.
    let abs = num.abs().to_string();
    let total_digits = abs.len() + 1; // '-' + abs
    if len <= total_digits {
        return format!("-{}", abs);
    }

    let mut out = String::new();
    out.push('-');
    out.push_str(&repeat_char(fill, len - total_digits));
    out.push_str(&abs);
    out
}

fn to_zenkaku_ascii(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '0'..='9' => char::from_u32('０' as u32 + (ch as u32 - '0' as u32)).unwrap(),
            'A'..='Z' => char::from_u32('Ａ' as u32 + (ch as u32 - 'A' as u32)).unwrap(),
            'a'..='z' => char::from_u32('ａ' as u32 + (ch as u32 - 'a' as u32)).unwrap(),
            ' ' => '　',
            '-' => '－',
            '+' => '＋',
            '.' => '．',
            ',' => '，',
            ':' => '：',
            ';' => '；',
            '/' => '／',
            '\\' => '＼',
            '(' => '（',
            ')' => '）',
            '[' => '［',
            ']' => '］',
            _ => ch,
        })
        .collect()
}

fn timetable_arg(v: &Value) -> Option<(f64, f64, f64, i64)> {
    let Value::List(items) = v.unwrap_named() else {
        return None;
    };
    if items.len() < 3 {
        return None;
    }
    let start_time = items.first().and_then(Value::as_i64)? as f64;
    let end_time = items.get(1).and_then(Value::as_i64)? as f64;
    let end_value = items.get(2).and_then(Value::as_i64)? as f64;
    let speed_type = items.get(3).and_then(Value::as_i64).unwrap_or(0);
    Some((start_time, end_time, end_value, speed_type))
}

fn timetable_value(params: &[Value]) -> i64 {
    let now_time = params.first().and_then(Value::as_i64).unwrap_or(0) as f64;
    let rep_time = params.get(1).and_then(Value::as_i64).unwrap_or(0) as f64;
    let mut start_value = params.get(2).and_then(Value::as_i64).unwrap_or(0) as f64;
    let mut ret_value = start_value;
    let now_time = now_time - rep_time;

    for arg in params.iter().skip(3) {
        let Some((start_time, end_time, end_value, speed_type)) = timetable_arg(arg) else {
            continue;
        };
        if now_time < start_time {
            ret_value = start_value;
            break;
        } else if now_time >= end_time {
            ret_value = end_value;
        } else {
            let duration = end_time - start_time;
            if duration == 0.0 {
                ret_value = end_value;
            } else if speed_type == 1 {
                let t = now_time - start_time;
                ret_value = (end_value - start_value) * t * t / duration / duration + start_value;
            } else if speed_type == 2 {
                let t = now_time - end_time;
                ret_value = -(end_value - start_value) * t * t / duration / duration + end_value;
            } else {
                ret_value = (end_value - start_value) * (now_time - start_time) / duration + start_value;
            }
            break;
        }
        start_value = end_value;
    }

    round_half_away_from_zero(ret_value)
}

fn xorshift32(state: &mut u32) -> u32 {
    if *state == 0 {
        // Non-zero default seed.
        *state = 0x1234_5678;
    }
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let parsed = prop_access::parse_element_chain_ctx(ctx, form_id, args);
    let mut chain_pos: Option<usize> = None;
    let mut chain: Option<&[i32]> = None;
    if let Some((pos, ch)) = parsed {
        chain_pos = Some(pos);
        chain = Some(ch);
    }

    let (op, params, al_id) = if let Some(pos) = chain_pos {
        let ch = chain.unwrap();
        let op = ch.get(1).copied();
        let al_id = crate::runtime::forms::prop_access::current_vm_meta(ctx).0;
        (
            op,
            crate::runtime::forms::prop_access::script_args(args, pos),
            al_id,
        )
    } else {
        let op = args.get(0).and_then(|v| v.as_i64()).map(|v| v as i32);
        let params = if args.len() >= 2 { &args[1..] } else { &[] };
        (op, params, None)
    };

    let p_int = |i: usize| -> i64 { params.get(i).and_then(|v| v.as_i64()).unwrap_or(0) };
    let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

    let Some(op) = op else {
        if let Some(direct_op) = args.get(0).and_then(|v| v.as_i64()) {
            prop_access::store_or_push_direct_prop(ctx, form_id, direct_op as i32, args, 1);
            return Ok(true);
        }
        ctx.push(Value::Int(0));
        return Ok(true);
    };

    // MAX
    if ctx.ids.math_max != 0 && op == ctx.ids.math_max {
        let a = p_int(0);
        let b = p_int(1);
        ctx.push(Value::Int(a.max(b)));
        return Ok(true);
    }
    // MIN
    if ctx.ids.math_min != 0 && op == ctx.ids.math_min {
        let a = p_int(0);
        let b = p_int(1);
        ctx.push(Value::Int(a.min(b)));
        return Ok(true);
    }
    // LIMIT(min, value, max)
    if ctx.ids.math_limit != 0 && op == ctx.ids.math_limit {
        let a = p_int(0);
        let v = p_int(1);
        let b = p_int(2);
        ctx.push(Value::Int(v.clamp(a.min(b), a.max(b))));
        return Ok(true);
    }
    // ABS
    if ctx.ids.math_abs != 0 && op == ctx.ids.math_abs {
        ctx.push(Value::Int(p_int(0).abs()));
        return Ok(true);
    }
    // RAND(min, max)
    if ctx.ids.math_rand != 0 && op == ctx.ids.math_rand {
        let a = p_int(0);
        let b = p_int(1);
        let lo = a.min(b);
        let hi = a.max(b);
        if lo == hi {
            ctx.push(Value::Int(lo));
            return Ok(true);
        }
        let span = (hi - lo + 1) as u64;
        let r = xorshift32(&mut ctx.globals.rng_state) as u64;
        ctx.push(Value::Int(lo + (r % span) as i64));
        return Ok(true);
    }

    // SQRT(num, scale)
    if ctx.ids.math_sqrt != 0 && op == ctx.ids.math_sqrt {
        let num = p_int(0).max(0) as f64;
        let scale = p_int(1) as f64;
        let v = (num.sqrt() * scale) as i64;
        ctx.push(Value::Int(v));
        return Ok(true);
    }
    // LOG(num, scale)
    if ctx.ids.math_log != 0 && op == ctx.ids.math_log {
        let num = p_int(0).max(1) as f64;
        let scale = p_int(1) as f64;
        let v = (num.ln() * scale) as i64;
        ctx.push(Value::Int(v));
        return Ok(true);
    }
    // LOG2
    if ctx.ids.math_log2 != 0 && op == ctx.ids.math_log2 {
        let num = p_int(0).max(1) as f64;
        let scale = p_int(1) as f64;
        let v = (num.log2() * scale) as i64;
        ctx.push(Value::Int(v));
        return Ok(true);
    }
    // LOG10
    if ctx.ids.math_log10 != 0 && op == ctx.ids.math_log10 {
        let num = p_int(0).max(1) as f64;
        let scale = p_int(1) as f64;
        let v = (num.log10() * scale) as i64;
        ctx.push(Value::Int(v));
        return Ok(true);
    }

    // SIN(angle, scale)
    if ctx.ids.math_sin != 0 && op == ctx.ids.math_sin {
        let angle = p_int(0) as f64;
        let scale = p_int(1) as f64;
        let deg = angle / TNM_ANGLE_UNIT;
        let rad = deg.to_radians();
        ctx.push(Value::Int((rad.sin() * scale) as i64));
        return Ok(true);
    }
    // COS(angle, scale)
    if ctx.ids.math_cos != 0 && op == ctx.ids.math_cos {
        let angle = p_int(0) as f64;
        let scale = p_int(1) as f64;
        let deg = angle / TNM_ANGLE_UNIT;
        let rad = deg.to_radians();
        ctx.push(Value::Int((rad.cos() * scale) as i64));
        return Ok(true);
    }
    // TAN(angle, scale)
    if ctx.ids.math_tan != 0 && op == ctx.ids.math_tan {
        let angle = p_int(0) as f64;
        let scale = p_int(1) as f64;
        let deg = angle / TNM_ANGLE_UNIT;
        let rad = deg.to_radians();
        ctx.push(Value::Int((rad.tan() * scale) as i64));
        return Ok(true);
    }

    // ARCSIN(num, denom)
    if ctx.ids.math_arcsin != 0 && op == ctx.ids.math_arcsin {
        let num = p_int(0) as f64;
        let denom = p_int(1) as f64;
        let ret = if denom == 0.0 {
            0
        } else {
            let mut x = num / denom;
            x = x.clamp(-1.0, 1.0);
            round_half_away_from_zero(x.asin().to_degrees() * TNM_ANGLE_UNIT)
        };
        ctx.push(Value::Int(ret));
        return Ok(true);
    }
    // ARCCOS
    if ctx.ids.math_arccos != 0 && op == ctx.ids.math_arccos {
        let num = p_int(0) as f64;
        let denom = p_int(1) as f64;
        let ret = if denom == 0.0 {
            0
        } else {
            let mut x = num / denom;
            x = x.clamp(-1.0, 1.0);
            round_half_away_from_zero(x.acos().to_degrees() * TNM_ANGLE_UNIT)
        };
        ctx.push(Value::Int(ret));
        return Ok(true);
    }
    // ARCTAN
    if ctx.ids.math_arctan != 0 && op == ctx.ids.math_arctan {
        let num = p_int(0) as f64;
        let denom = p_int(1) as f64;
        let ret = if denom == 0.0 {
            0
        } else {
            round_half_away_from_zero((num / denom).atan().to_degrees() * TNM_ANGLE_UNIT)
        };
        ctx.push(Value::Int(ret));
        return Ok(true);
    }

    // DISTANCE(x1,y1,x2,y2)
    if ctx.ids.math_distance != 0 && op == ctx.ids.math_distance {
        let x1 = p_int(0);
        let y1 = p_int(1);
        let x2 = p_int(2);
        let y2 = p_int(3);
        let dx = (x2 - x1) as f64;
        let dy = (y2 - y1) as f64;
        ctx.push(Value::Int(((dx * dx + dy * dy).sqrt()) as i64));
        return Ok(true);
    }

    // ANGLE(x1,y1,x2,y2)
    if ctx.ids.math_angle != 0 && op == ctx.ids.math_angle {
        let x1 = p_int(0);
        let y1 = p_int(1);
        let x2 = p_int(2);
        let y2 = p_int(3);
        let dy = (y2 - y1) as f64;
        let dx = (x2 - x1) as f64;
        let mut ret = round_half_away_from_zero(dy.atan2(dx).to_degrees() * TNM_ANGLE_UNIT);
        ret = (ret + (360.0 * TNM_ANGLE_UNIT) as i64).rem_euclid((360.0 * TNM_ANGLE_UNIT) as i64);
        ctx.push(Value::Int(ret));
        return Ok(true);
    }

    // LINEAR(x0,x1,y1,x2,y2)
    if ctx.ids.math_linear != 0 && op == ctx.ids.math_linear {
        let x0 = p_int(0);
        let x1 = p_int(1);
        let y1 = p_int(2);
        let x2 = p_int(3);
        let y2 = p_int(4);
        if x1 == x2 {
            ctx.push(Value::Int(y1));
        } else {
            let v = ((y2 - y1) as f64 * (x0 - x1) as f64 / (x2 - x1) as f64 + y1 as f64) as i64;
            ctx.push(Value::Int(v));
        }
        return Ok(true);
    }

    // TIMETABLE(now_time, rep_time, start_value, [start_time,end_time,end_value,speed_type]...)
    if op == elm_value::MATH_TIMETABLE {
        ctx.push(Value::Int(timetable_value(params)));
        return Ok(true);
    }

    // TOSTR
    if ctx.ids.math_tostr != 0 && op == ctx.ids.math_tostr {
        let s = if matches!(al_id, Some(1)) {
            let num = p_int(0);
            let len = p_int(1);
            tostr_pad(num, len, ' ')
        } else {
            p_int(0).to_string()
        };
        ctx.push(Value::Str(s));
        return Ok(true);
    }

    // TOSTR_ZERO
    if ctx.ids.math_tostr_zero != 0 && op == ctx.ids.math_tostr_zero {
        let num = p_int(0);
        let len = p_int(1);
        ctx.push(Value::Str(tostr_pad(num, len, '0')));
        return Ok(true);
    }

    // TOSTR_ZEN
    if op == elm_value::MATH_TOSTR_ZEN {
        let s = if matches!(al_id, Some(1)) {
            let num = p_int(0);
            let len = p_int(1);
            tostr_pad(num, len, ' ')
        } else {
            p_int(0).to_string()
        };
        ctx.push(Value::Str(to_zenkaku_ascii(&s)));
        return Ok(true);
    }

    // TOSTR_ZEN_ZERO
    if op == elm_value::MATH_TOSTR_ZEN_ZERO {
        let num = p_int(0);
        let len = p_int(1);
        ctx.push(Value::Str(to_zenkaku_ascii(&tostr_pad(num, len, '0'))));
        return Ok(true);
    }

    // TOSTR_BY_CODE
    if op == elm_value::MATH_TOSTR_BY_CODE {
        let code = (p_int(0) & 0xffff) as u32;
        let s = char::from_u32(code).map(|c| c.to_string()).unwrap_or_default();
        ctx.push(Value::Str(s));
        return Ok(true);
    }

    Ok(false)
}
