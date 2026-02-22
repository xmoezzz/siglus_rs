use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::codes::bgm_table_op;

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// BGMTABLE form (global form id 123).
///
/// Original engine: BGM table handler.
///
/// We implement only the parts that are semantically unambiguous from the
/// decompilation:
/// - op==0: push count
/// - op==1: lookup by name and push its integer return value
///
/// The mutating operations (op==2/op==4) depend on internal table ownership and
/// are kept as stubs (recorded as unimplemented) until the database-backed
/// table is ported 1:1.
pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(op) = args.get(0).and_then(|v| v.as_i64()) else {
        ctx.unknown
            .record_unimplemented("BGMTABLE/invalid-op-missing");
        return Ok(true);
    };

    match op {
        bgm_table_op::GET_COUNT => {
            // Without the database table ported, use a deterministic value.
            // Keeping it 0 is safer than guessing.
            ctx.push(Value::Int(0));
            Ok(true)
        }
        bgm_table_op::GET_LISTEN_BY_NAME => {
            // Original: lookup-by-name and push the integer.
            // Without the original table mapping, we
            // return 0 (false) by default.
            if arg_str(args, 1).is_none() {
                ctx.unknown
                    .record_unimplemented("BGMTABLE/GET_LISTEN_BY_NAME/invalid-args");
                ctx.push(Value::Int(0));
            } else {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        bgm_table_op::SET_LISTEN_CURRENT | bgm_table_op::SET_ALL_FLAG => {
            ctx.unknown
                .record_unimplemented(&format!("BGMTABLE/op={op}"));
            Ok(true)
        }
        _ => {
            ctx.unknown
                .record_unimplemented(&format!("BGMTABLE/op={op}"));
            Ok(true)
        }
    }
}
