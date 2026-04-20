use serde::Serialize;

#[derive(Debug, thiserror::Error)]
#[error("failed to serialize tool output for '{tool_name}': {source}")]
pub struct ToolOutputError {
    tool_name: &'static str,
    #[source]
    source: serde_json::Error,
}

pub(crate) fn serialize_tool_output<T: Serialize>(
    tool_name: &'static str,
    value: T,
) -> Result<serde_json::Value, ToolOutputError> {
    serde_json::to_value(value).map_err(|source| ToolOutputError { tool_name, source })
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::serialize_tool_output;

    struct FailingSerialize;

    impl Serialize for FailingSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("boom"))
        }
    }

    #[test]
    fn serialize_tool_output_wraps_serde_failures() {
        let err = serialize_tool_output("demo", FailingSerialize).unwrap_err();
        assert_eq!(
            err.to_string(),
            "failed to serialize tool output for 'demo': boom"
        );
    }
}
