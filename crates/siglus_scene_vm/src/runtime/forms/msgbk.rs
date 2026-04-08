//! Global MSGBK form (message backlog).
//!
//! In the original engine, `GLOBAL.MSGBK` provides a small API for building the message backlog ("message history"):
//!   - INSERT_MSG(str)
//!   - ADD_KOE(int koe_no, [int chara_no])
//!   - ADD_NAMAE(str)
//!   - ADD_MSG(str)
//!   - GO_NEXT_MSG()
//!
//! Titles differ in element-code assignments, so this implementation:
//!   - relies on element-chain structure rather than numeric codes
//!   - does conservative operation classification from argument shapes
//!
//! There are no getters in the public script surface for this object, so runtime
//! focuses on storing enough state for future UI integration.

use anyhow::Result;

use crate::runtime::globals::{MsgBackAtom, MsgBackState};
use crate::runtime::{CommandContext, Value};

fn find_chain(args: &[Value]) -> Option<(usize, Vec<i32>)> {
    for (i, v) in args.iter().enumerate().rev() {
        if let Value::Element(e) = v {
            return Some((i, e.clone()));
        }
    }
    None
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

fn as_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

fn msgbk_state_mut(ctx: &mut CommandContext, form_id: u32) -> &mut MsgBackState {
    ctx.globals.msgbk_forms.entry(form_id).or_default()
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = find_chain(args) else {
        return Ok(false);
    };
    if chain.is_empty() {
        return Ok(false);
    }

    let form_id = chain[0] as u32;
    let st = msgbk_state_mut(ctx, form_id);

    // Script args are between the synthetic op-id and Element(chain).
    let script_args = if chain_pos >= 1 {
        &args[1..chain_pos]
    } else {
        &[][..]
    };

    // Minimal chain:
    //   [FORM_MSGBK, op]
    // Return the MSGBK element itself if no op.
    if chain.len() < 2 {
        ctx.push(Value::Int(0));
        return Ok(true);
    }
    let _op = chain[1];

    // Conservative classification by parameter shape.
    // - 0 args: GO_NEXT_MSG
    // - int args: ADD_KOE
    // - string args: ADD_NAMAE/ADD_MSG/INSERT_MSG (treated as text atom)
    if script_args.is_empty() {
        st.next();
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    if let Some(n0) = script_args.get(0).and_then(as_i64) {
        // ADD_KOE
        let chara_no = script_args.get(1).and_then(as_i64).unwrap_or(-1);
        st.cur.atoms.push(MsgBackAtom::Koe {
            koe_no: n0,
            chara_no,
        });
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    if let Some(s0) = script_args.get(0).and_then(as_str) {
        // Treat all string ops as text atoms. Many titles don't rely on the
        // distinction between ADD_NAMAE and ADD_MSG for control flow.
        st.cur.atoms.push(MsgBackAtom::Text(s0.to_string()));
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // Unknown signature: ignore.
    ctx.push(Value::Int(0));
    Ok(true)
}
