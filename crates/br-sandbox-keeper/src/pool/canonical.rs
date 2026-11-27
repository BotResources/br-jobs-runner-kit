use serde_json::Value;

pub(super) fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out);
    out
}

fn write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write(item, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_keys_of_an_object_are_sorted_at_every_level() {
        let a = json!({"b": 1, "a": {"y": [1, {"d": 1, "c": 2}], "x": null}});
        let b = json!({"a": {"x": null, "y": [1, {"c": 2, "d": 1}]}, "b": 1});
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(
            canonical(&a),
            r#"{"a":{"x":null,"y":[1,{"c":2,"d":1}]},"b":1}"#
        );
    }

    #[test]
    fn the_order_of_an_array_and_the_value_of_a_scalar_count() {
        assert_ne!(canonical(&json!([1, 2])), canonical(&json!([2, 1])));
        assert_ne!(canonical(&json!({"a": "1"})), canonical(&json!({"a": 1})));
        assert_eq!(canonical(&json!("a\"b")), r#""a\"b""#);
    }
}
