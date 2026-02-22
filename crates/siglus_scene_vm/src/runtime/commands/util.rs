use crate::runtime::Value;

/// In VM-driven execution, we append metadata to every command call:
///   (ret_form: int, al_id: int, element: [i32]).
///
/// Named commands generally don't care, so we strip it when present.
pub fn strip_vm_meta(args: &[Value]) -> &[Value] {
    if args.len() >= 3 {
        let n = args.len();
        // VM appends: Element(elm), Int(al_id), Int(ret_form)
        if matches!(args[n - 3], Value::Element(_))
            && matches!(args[n - 2], Value::Int(_))
            && matches!(args[n - 1], Value::Int(_))
        {
            return &args[..n - 3];
        }
    }
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
