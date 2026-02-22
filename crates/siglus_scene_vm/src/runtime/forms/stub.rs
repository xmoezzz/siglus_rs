use crate::runtime::{CommandContext, OpCode, Value};
use anyhow::Result;

/// Fallback handler for unknown global forms.
///
/// This keeps the VM running by:
/// - recording the unknown form ID and any element chain,
/// - pushing a conservative default return value (usually integer 0).
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    ctx.unknown.record_code(OpCode::form(form_id));
    for v in args {
        if let Value::Element(chain) = v {
            ctx.unknown.record_element_chain(form_id, chain, "STUB");
        }
    }

    // Many global elements are used in expressions and expect a return value.
    // We try to infer return form if it is passed (al_id, ret_form) after an element chain.
    // If inference fails, return integer 0.
    let mut ret_form: Option<i64> = None;
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(_) = v {
            if i + 2 < args.len() {
                if let (Some(_al_id), Some(rf)) = (args[i + 1].as_i64(), args[i + 2].as_i64()) {
                    ret_form = Some(rf);
                }
            }
        }
    }

    // Heuristic: most scripts treat ret_form==2 as string (based on observed patterns).
    if matches!(ret_form, Some(2)) {
        ctx.push(Value::Str(String::new()));
    } else {
        ctx.push(Value::Int(0));
    }
    Ok(true)
}
