use std::sync::Arc;
use std::sync::Mutex;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use wgc_auth::AccountId;
use wgc_auth::AccountRealm;
use wgc_auth::GameToken1;
use wgc_auth::GameToken2;
use wgc_auth::Nickname;
use wgc_ipc::BoxFuture;
use wgc_ipc::GameLanguage;
use wgc_ipc::IpcResponder;
use wgc_ipc::LoginInfo;
use wgc_ipc::Message;
use wgc_ipc::RelayListener;
use wgc_ipc::Token1Failure;
use wgc_ipc::Token1Grant;
use wgc_ipc::Token2Grant;
use wgc_ipc::command::Command;
use wgc_ipc::command::GetGameLanguage;
use wgc_ipc::command::QuerySpaToken1;
use wgc_ipc::command::SecretText;
use wgc_ipc::command::SpaToken2;
use wgc_ipc::command::Status;
use wgc_ipc::command::Verification;
use wgc_ipc::command::VerificationType;
use wgc_ipc::handle_message;
use wgc_ipc::message::ClassVersion;
use wgc_ipc::message::CommandFlags;
use wgc_ipc::message::CommandFrame;
use wgc_ipc::message::CommandUid;
use wgc_ipc::serve_stream;

const SPA_ID: u64 = 1034874630;
const GAME_FOLDER: &str = r"Z:\games\World_of_Warships";

struct FakeResponder {
    signed_in: bool,
    token2: Option<String>,
    token1: Result<String, Token1Failure>,
    saved: Mutex<Vec<(AccountId, String)>>,
}

impl FakeResponder {
    fn signed_in() -> FakeResponder {
        FakeResponder {
            signed_in: true,
            token2: Some("stored-token2".to_owned()),
            token1: Ok("ABCDEFGH".to_owned()),
            saved: Mutex::new(Vec::new()),
        }
    }

    fn signed_out() -> FakeResponder {
        FakeResponder {
            signed_in: false,
            token2: None,
            token1: Err(Token1Failure::NoUser),
            saved: Mutex::new(Vec::new()),
        }
    }
}

impl IpcResponder for FakeResponder {
    fn client_realm(&self) -> AccountRealm {
        AccountRealm::Na
    }

    fn login_info(&self) -> Option<LoginInfo> {
        self.signed_in.then(|| LoginInfo {
            realm: AccountRealm::Na,
            nickname: Nickname::new("Captain"),
            spa_id: AccountId::new(SPA_ID),
        })
    }

    fn game_language(&self) -> GameLanguage {
        GameLanguage::new("en")
    }

    fn query_token1(&self) -> BoxFuture<'_, Result<Token1Grant, Token1Failure>> {
        let result = self
            .token1
            .clone()
            .map(|token| Token1Grant { spa_id: AccountId::new(SPA_ID), token1: GameToken1::new(token) });
        Box::pin(async move { result })
    }

    fn load_token2(&self) -> BoxFuture<'_, Option<Token2Grant>> {
        let grant = self
            .token2
            .clone()
            .map(|token| Token2Grant { spa_id: AccountId::new(SPA_ID), token2: GameToken2::new(token) });
        Box::pin(async move { grant })
    }

    fn save_token2(&self, spa_id: AccountId, token2: GameToken2) -> BoxFuture<'_, ()> {
        self.saved.lock().unwrap().push((spa_id, token2.expose_secret().to_owned()));
        Box::pin(async {})
    }
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

fn frame(class_version: u32, command: Command) -> CommandFrame {
    CommandFrame {
        class_version: ClassVersion(class_version),
        base_class_version: ClassVersion(0),
        uid: CommandUid("0123456789ABCDEF0123456789ABCDEF".to_owned()),
        flags: CommandFlags(0),
        command,
    }
}

/// Requests the client sent under Proton.
const CAPTURED_CLIENT_REQUESTS: [&str; 7] = [
    "client_verify_game_imported.bin",
    "client_get_game_installation_state.bin",
    "client_check_login_is_enabled.bin",
    "client_verify_login.bin",
    "client_get_authentication_option_realm.bin",
    "client_get_authentication_option_build.bin",
    "client_get_login_info.bin",
];

/// Requests not captured yet, with the inputs the research expects the
/// client to send.
fn uncaptured_requests() -> Vec<CommandFrame> {
    let folder = || GAME_FOLDER.to_owned();
    vec![
        frame(
            0,
            Command::VerifyLegalDocsAccepted(Verification {
                verification_type: VerificationType(1),
                game_folder: folder(),
                status: Status(0),
            }),
        ),
        frame(
            0,
            Command::GetGameLanguage(GetGameLanguage {
                language: String::new(),
                status: Status(0),
                game_folder: folder(),
            }),
        ),
        frame(
            0,
            Command::LoadSpaToken2(SpaToken2 {
                game_folder: folder(),
                spa_id: String::new(),
                token2: SecretText::default(),
                description: String::new(),
                status: Status(0),
            }),
        ),
        frame(
            0,
            Command::QuerySpaToken1(QuerySpaToken1 {
                game_folder: folder(),
                spa_id: String::new(),
                token1: SecretText::default(),
                status: Status(0),
                login_dialog_allowed: true,
            }),
        ),
    ]
}

async fn answer_each(responder: &FakeResponder) -> Vec<Command> {
    let mut requests: Vec<Vec<u8>> = CAPTURED_CLIENT_REQUESTS.into_iter().map(fixture).collect();
    requests.extend(
        uncaptured_requests().into_iter().map(|frame| Message { commands: vec![frame] }.encode().unwrap().to_vec()),
    );

    let mut replies = Vec::new();
    for request in requests {
        let reply = Message::decode(&handle_message(&request, responder).await).unwrap();
        replies.push(reply.commands.into_iter().next().unwrap().command);
    }

    replies
}

fn secret(command: &Command) -> Option<&str> {
    match command {
        Command::LoadSpaToken2(command) => Some(command.token2.expose_secret()),
        Command::QuerySpaToken1(command) => Some(command.token1.expose_secret()),
        _ => None,
    }
}

#[tokio::test]
async fn signed_in_answers() {
    let replies = answer_each(&FakeResponder::signed_in()).await;

    insta::assert_debug_snapshot!(replies);
    let secrets: Vec<_> = replies.iter().filter_map(secret).collect();
    assert_eq!(secrets, ["stored-token2", "ABCDEFGH"]);
}

#[tokio::test]
async fn signed_out_answers() {
    insta::assert_debug_snapshot!(answer_each(&FakeResponder::signed_out()).await);
}

#[tokio::test]
async fn captured_check_api_version_gets_the_accepted_reply() {
    let reply = handle_message(&fixture("run4_check_api_version.bin"), &FakeResponder::signed_in()).await;

    assert_eq!(*reply, fixture("run4_check_api_version_reply.bin"));
}

#[tokio::test]
async fn captured_get_overlay_params_is_answered_unchanged() {
    let request = fixture("run4_get_overlay_params.bin");

    let reply = handle_message(&request, &FakeResponder::signed_in()).await;

    assert_eq!(*reply, request);
}

#[tokio::test]
async fn save_token2_reaches_the_responder() {
    let responder = FakeResponder::signed_in();
    let request = frame(
        0,
        Command::SaveSpaToken2(SpaToken2 {
            game_folder: GAME_FOLDER.to_owned(),
            spa_id: SPA_ID.to_string(),
            token2: SecretText::new("rotated-token2".to_owned()),
            description: String::new(),
            status: Status(0),
        }),
    );
    let bytes = Message { commands: vec![request] }.encode().unwrap();

    let reply = Message::decode(&handle_message(&bytes, &responder).await).unwrap();

    let Command::SaveSpaToken2(saved) = &reply.commands[0].command else {
        panic!("reply changed the command type");
    };
    assert_eq!(saved.status, Status::SUCCESS);
    assert_eq!(*responder.saved.lock().unwrap(), [(AccountId::new(SPA_ID), "rotated-token2".to_owned())]);
}

#[tokio::test]
async fn unmodeled_requests_are_echoed() {
    let request = b"\xEA\xDC\xC8\x00\x00\x00 not a modeled command".to_vec();

    assert_eq!(*handle_message(&request, &FakeResponder::signed_in()).await, request);
}

#[tokio::test]
async fn stream_carries_length_prefixed_messages() {
    let (mut client, server) = tokio::io::duplex(4096);
    let responder = FakeResponder::signed_in();
    let request = fixture("run4_check_api_version.bin");

    let serve = serve_stream(server, &responder);
    let exchange = async {
        for _ in 0..2 {
            client.write_all(&(request.len() as u32).to_le_bytes()).await.unwrap();
            client.write_all(&request).await.unwrap();
        }
        let mut replies = Vec::new();
        for _ in 0..2 {
            let mut len = [0u8; 4];
            client.read_exact(&mut len).await.unwrap();
            let mut reply = vec![0u8; u32::from_le_bytes(len) as usize];
            client.read_exact(&mut reply).await.unwrap();
            replies.push(reply);
        }
        drop(client);
        replies
    };
    let (served, replies) = tokio::join!(serve, exchange);

    served.unwrap();
    assert_eq!(replies, vec![fixture("run4_check_api_version_reply.bin"); 2]);
}

#[tokio::test]
async fn relay_listener_requires_the_token() {
    let listener = RelayListener::bind().await.unwrap();
    let addr = listener.local_addr();
    let env = listener.relay_env();
    let token_hex = env.iter().find(|(key, _)| key == "WGC_RELAY_TOKEN").unwrap().1.to_str().unwrap().to_owned();
    let token = wgc_relay_protocol::RelayToken::parse_hex(&token_hex).unwrap();
    let server = tokio::spawn(listener.serve(Arc::new(FakeResponder::signed_in())));
    let request = fixture("run4_check_api_version.bin");

    let mut intruder = tokio::net::TcpStream::connect(addr).await.unwrap();
    intruder.write_all(&[0u8; 32]).await.unwrap();
    intruder.write_all(&(request.len() as u32).to_le_bytes()).await.unwrap();
    intruder.write_all(&request).await.unwrap();
    // The server closes with unread input, which may surface as a reset.
    let mut rejected = Vec::new();
    let _ = intruder.read_to_end(&mut rejected).await;

    let mut relay = tokio::net::TcpStream::connect(addr).await.unwrap();
    relay.write_all(token.as_bytes()).await.unwrap();
    relay.write_all(&(request.len() as u32).to_le_bytes()).await.unwrap();
    relay.write_all(&request).await.unwrap();
    let mut len = [0u8; 4];
    relay.read_exact(&mut len).await.unwrap();
    let mut reply = vec![0u8; u32::from_le_bytes(len) as usize];
    relay.read_exact(&mut reply).await.unwrap();
    server.abort();

    assert!(rejected.is_empty(), "wrong token got {} reply bytes", rejected.len());
    assert_eq!(reply, fixture("run4_check_api_version_reply.bin"));
}
