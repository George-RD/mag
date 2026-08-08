use std::borrow::Cow;

use rmcp::model::CallToolRequestParams;

pub(crate) fn tool_request(
    name: impl Into<Cow<'static, str>>,
    arguments: serde_json::Value,
) -> CallToolRequestParams {
    let arguments = arguments
        .as_object()
        .expect("tool arguments should be an object")
        .clone();
    CallToolRequestParams::new(name).with_arguments(arguments)
}
