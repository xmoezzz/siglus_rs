use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::excall_op;

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("EXCALL form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.unknown.record_unimplemented("EXCALL/invalid-op-type");
            return Ok(true);
        }
    };

    match op {
        excall_op::ARRAY_INDEX => {
            // Original: treated as an indexed element-array op.
            // We currently do not model element arrays.
            ctx.unknown.record_unimplemented("EXCALL/ARRAY_INDEX");
            ctx.push(Value::Int(0));
        }
        excall_op::OP_0
        | excall_op::OP_1
        | excall_op::OP_2
        | excall_op::OP_3
        | excall_op::OP_4
        | excall_op::OP_5
        | excall_op::OP_6
        | excall_op::OP_7
        | excall_op::OP_8
        | excall_op::OP_9
        | excall_op::OP_10
        | excall_op::OP_12
        | excall_op::OP_13 => {
            // We have ID coverage from the original engine, but most semantics
            // are not implemented yet. Do not guess.
            ctx.unknown.record_unimplemented(&format!("EXCALL/op={op}"));

            // A few ops are unambiguously boolean-returning in the original engine.
            if op == excall_op::OP_8 {
                // Original: returns *(byte*)(ctx + 2148) != 0
                ctx.push(Value::Int(if ctx.syscalls.flag_2148 { 1 } else { 0 }));
            } else if op == excall_op::OP_12 {
                // Original: returns *(byte*)(global + 204) != 0
                ctx.push(Value::Int(if ctx.syscalls.flag_204 { 1 } else { 0 }));
            }
        }
        _ => {
            ctx.unknown.record_unimplemented(&format!("EXCALL/op={op}"));
        }
    }

    Ok(true)
}
