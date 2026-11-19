use serde_json::Value;

pub(crate) fn compare_deep(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(left, right)| compare_deep(left, right))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(key, value)| {
                b.get(key).is_some_and(|other| compare_deep(value, other))
            })
        }
        _ => a == b,
    }
}
