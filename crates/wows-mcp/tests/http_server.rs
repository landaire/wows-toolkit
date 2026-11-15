//! Streamable HTTP serving with an embedder's tool set: tool registration,
//! shutdown and bind failures.

use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::model::JsonObject;
use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use wows_mcp::BuildError;
use wows_mcp::McpServer;
use wows_mcp::ToolFuture;
use wows_mcp::ToolSet;
use wows_mcp::Transport;
use wows_mcp::tool_set::parse_arguments;
use wows_mcp::tool_set::structured_answer;
use wows_mcp::tool_set::typed_tool;

const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EchoArgs {
    /// Text to repeat.
    text: String,
}

#[derive(Serialize, JsonSchema)]
struct EchoAnswer {
    text: String,
    length: usize,
}

struct EchoTools;

impl ToolSet for EchoTools {
    fn tools(&self) -> Vec<Tool> {
        vec![typed_tool::<EchoArgs, EchoAnswer>("echo", "Repeat a text and give its length.")]
    }

    fn call<'a>(&'a self, name: &'a str, arguments: JsonObject) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: EchoArgs = parse_arguments(name, arguments)?;
            let length = args.text.len();

            structured_answer(&EchoAnswer { text: args.text, length })
        })
    }
}

struct Running {
    address: SocketAddr,
    shutdown: CancellationToken,
    task: JoinHandle<()>,
}

async fn start() -> Running {
    let server = McpServer::builder(Transport::Http("127.0.0.1:0".parse().unwrap()))
        .tool_set(Arc::new(EchoTools))
        .build()
        .await
        .unwrap();
    let address = server.http_endpoint().unwrap().address();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { server.run(shutdown).await.unwrap() }
    });

    Running { address, shutdown, task }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    /// The JSON-RPC message of a JSON or event-stream body.
    fn message(&self) -> Value {
        if self.header("content-type").is_some_and(|kind| kind.starts_with("application/json")) {
            return serde_json::from_str(&self.body).unwrap();
        }

        self.body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|data| !data.is_empty())
            .find_map(|data| serde_json::from_str::<Value>(data).ok().filter(|value| value.get("id").is_some()))
            .unwrap_or_else(|| panic!("no JSON-RPC response in {:?}", self.body))
    }
}

struct Request<'a> {
    host: String,
    session: Option<&'a str>,
    body: Value,
}

impl<'a> Request<'a> {
    fn new(address: SocketAddr, body: Value) -> Self {
        Self { host: address.to_string(), session: None, body }
    }

    async fn send(self, address: SocketAddr) -> Response {
        let body = self.body.to_string();
        let mut head = format!(
            "POST /mcp HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {PROTOCOL_VERSION}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.host,
            body.len()
        );
        if let Some(session) = self.session {
            head.push_str(&format!("Mcp-Session-Id: {session}\r\n"));
        }
        head.push_str("\r\n");

        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();

        parse_response(&raw)
    }
}

fn parse_response(raw: &[u8]) -> Response {
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").expect("response has a header block");
    let head = std::str::from_utf8(&raw[..split]).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect();

    let mut payload = &raw[split + 4..];
    let chunked = headers
        .iter()
        .any(|(key, value)| key.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked"));
    let body = if chunked {
        let mut body = Vec::new();
        loop {
            let line_end = payload.windows(2).position(|window| window == b"\r\n").unwrap();
            let size = usize::from_str_radix(std::str::from_utf8(&payload[..line_end]).unwrap().trim(), 16).unwrap();
            payload = &payload[line_end + 2..];
            if size == 0 {
                break;
            }
            body.extend_from_slice(&payload[..size]);
            payload = &payload[size + 2..];
        }
        body
    } else {
        payload.to_vec()
    };

    Response { status, headers, body: String::from_utf8(body).unwrap() }
}

fn initialize(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "wows-mcp-test", "version": "0"}
        }
    })
}

/// Initializes a session and returns its id.
async fn open_session(address: SocketAddr) -> String {
    let response = Request::new(address, initialize(1)).send(address).await;
    assert_eq!(response.status, 200, "{}", response.body);
    let session = response.header("mcp-session-id").expect("session id").to_owned();

    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let mut request = Request::new(address, initialized);
    request.session = Some(&session);
    let response = request.send(address).await;
    assert_eq!(response.status, 202, "{}", response.body);

    session
}

async fn rpc(address: SocketAddr, session: &str, id: u64, method: &str, params: Value) -> Value {
    let mut request = Request::new(address, json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    request.session = Some(session);
    let response = request.send(address).await;
    assert_eq!(response.status, 200, "{}", response.body);

    response.message()
}

#[tokio::test]
async fn an_embedder_tool_is_listed_and_called() {
    let server = start().await;
    let session = open_session(server.address).await;

    let listed = rpc(server.address, &session, 2, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    let echo = tools.iter().find(|tool| tool["name"] == "echo").expect("echo is listed");
    assert_eq!(echo["inputSchema"]["required"], json!(["text"]));
    assert_eq!(echo["outputSchema"]["properties"]["length"]["type"], "integer");
    assert_eq!(echo["annotations"]["readOnlyHint"], true);

    let called =
        rpc(server.address, &session, 3, "tools/call", json!({"name": "echo", "arguments": {"text": "hello"}})).await;
    assert_eq!(called["result"]["structuredContent"], json!({"text": "hello", "length": 5}));
    assert_eq!(called["result"]["isError"], false);
}

#[tokio::test]
async fn malformed_arguments_and_unknown_tools_are_protocol_errors() {
    let server = start().await;
    let session = open_session(server.address).await;

    let malformed =
        rpc(server.address, &session, 2, "tools/call", json!({"name": "echo", "arguments": {"txt": 1}})).await;
    assert_eq!(malformed["error"]["code"], -32602);
    assert!(malformed["error"]["message"].as_str().unwrap().starts_with("invalid arguments for echo"));

    let unknown = rpc(server.address, &session, 3, "tools/call", json!({"name": "absent", "arguments": {}})).await;
    assert_eq!(unknown["error"]["code"], -32602);
}

#[tokio::test]
async fn duplicate_tool_names_fail_the_build() {
    let result =
        McpServer::builder(Transport::Stdio).tool_set(Arc::new(EchoTools)).tool_set(Arc::new(EchoTools)).build().await;

    let error = result.err().expect("duplicate tools are rejected");
    assert!(matches!(error.current_context(), BuildError::DuplicateTool { name } if name == "echo"));
}

#[tokio::test]
async fn shutdown_stops_serving() {
    let server = start().await;

    server.shutdown.cancel();
    server.task.await.unwrap();

    assert!(TcpStream::connect(server.address).await.is_err());
}

#[tokio::test]
async fn an_occupied_port_fails_the_build() {
    let occupant = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupant.local_addr().unwrap();

    let result = McpServer::builder(Transport::Http(address)).tool_set(Arc::new(EchoTools)).build().await;

    let error = result.err().expect("the port is occupied");
    assert_eq!(error.current_context().listen_error_kind(), Some(std::io::ErrorKind::AddrInUse));
}
