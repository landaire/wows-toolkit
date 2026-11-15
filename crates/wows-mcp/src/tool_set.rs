use std::pin::Pin;

use rmcp::ErrorData;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::JsonObject;
use rmcp::model::Tool;
use rmcp::model::ToolAnnotations;
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The future a [`ToolSet`] returns for one call. `Err` is a protocol error
/// (unknown tool, malformed arguments); a tool that ran and failed answers
/// `Ok` with [`CallToolResult::error`], which the client shows to the model.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + 'a>>;

/// A group of MCP tools an embedder adds to the server. The server lists the
/// tools of every set and routes each call to the set that declared the
/// tool's name.
pub trait ToolSet: Send + Sync + 'static {
    /// The tools this set answers. Read once when the server is built.
    fn tools(&self) -> Vec<Tool>;

    /// Runs `name`, which is always one of [`ToolSet::tools`].
    fn call<'a>(&'a self, name: &'a str, arguments: JsonObject) -> ToolFuture<'a>;

    /// Text appended to the server instructions sent at initialization.
    fn instructions(&self) -> Option<String> {
        None
    }
}

/// A read-only tool whose input schema comes from `A` and output schema
/// from `O`.
pub fn typed_tool<A, O>(name: &'static str, description: &'static str) -> Tool
where
    A: JsonSchema,
    O: JsonSchema + 'static,
{
    Tool::new(name, description, wows_query::tools::input_schema::<A>())
        .with_output_schema::<O>()
        .annotate(ToolAnnotations::new().read_only(true))
}

/// Decodes a tool's arguments; malformed arguments are a protocol error.
pub fn parse_arguments<A>(tool: &str, arguments: JsonObject) -> Result<A, ErrorData>
where
    A: DeserializeOwned,
{
    serde_json::from_value(Value::Object(arguments))
        .map_err(|error| ErrorData::invalid_params(format!("invalid arguments for {tool}: {error}"), None))
}

/// A successful answer carrying `answer` as structured content.
pub fn structured_answer<O>(answer: &O) -> Result<CallToolResult, ErrorData>
where
    O: Serialize,
{
    let value = serde_json::to_value(answer)
        .map_err(|error| ErrorData::internal_error(format!("failed to encode the answer: {error}"), None))?;

    Ok(CallToolResult::structured(value))
}

/// A tool failure shown to the model.
pub fn tool_failure(message: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.to_string())])
}
