pub(crate) fn canonical_json_string(value: &serde_json::Value) -> String {
    let sorted = sorted_json_value(value);
    serde_json::to_string(&sorted).unwrap_or_default()
}

fn sorted_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<_> = map.iter().collect();
            sorted.sort_by_key(|(key, _)| key.as_str());

            let sorted_map: serde_json::Map<String, serde_json::Value> = sorted
                .into_iter()
                .map(|(key, value)| (key.clone(), sorted_json_value(value)))
                .collect();

            serde_json::Value::Object(sorted_map)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(sorted_json_value).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_string_sorts_object_keys_recursively() {
        let first = serde_json::json!({
            "z": 1,
            "a": {
                "b": true,
                "a": false
            },
            "m": [
                { "y": 2, "x": 1 },
                { "b": 4, "a": 3 }
            ]
        });
        let second = serde_json::json!({
            "m": [
                { "x": 1, "y": 2 },
                { "a": 3, "b": 4 }
            ],
            "a": {
                "a": false,
                "b": true
            },
            "z": 1
        });

        assert_eq!(
            canonical_json_string(&first),
            canonical_json_string(&second)
        );
        assert_eq!(
            canonical_json_string(&first),
            r#"{"a":{"a":false,"b":true},"m":[{"x":1,"y":2},{"a":3,"b":4}],"z":1}"#
        );
    }

    #[test]
    fn canonical_json_string_preserves_array_order() {
        let first = serde_json::json!([{ "b": 1, "a": 2 }, { "b": 3, "a": 4 }]);
        let second = serde_json::json!([{ "a": 4, "b": 3 }, { "a": 2, "b": 1 }]);

        assert_ne!(
            canonical_json_string(&first),
            canonical_json_string(&second)
        );
    }
}
