use crate::runtime::Value;

/// Command arguments are now the original script arguments.
/// The VM no longer appends synthetic element/al_id/ret_form metadata here.
pub fn strip_vm_meta(args: &[Value]) -> &[Value] {
    args
}

pub fn arg_as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(x) => Some(*x),
        _ => None,
    }
}

pub fn arg_as_usize(v: &Value) -> Option<usize> {
    arg_as_i64(v).and_then(|x| usize::try_from(x).ok())
}

pub fn arg_as_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}
