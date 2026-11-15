//! Byte compatibility against requests captured from the World of Warships
//! client under Proton, and against a reply the client accepted.

use wgc_ipc::Message;
use wgc_ipc::command::Command;
use wgc_ipc::command::CommandKind;
use wgc_ipc::command::GetAuthenticationOption;
use wgc_ipc::command::GetLoginInfo;
use wgc_ipc::command::Status;
use wgc_ipc::message::ClassVersion;
use wgc_ipc::message::CommandFlags;
use wgc_ipc::message::CommandFrame;
use wgc_ipc::message::CommandUid;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

/// Requests the stub and the client sent to a pipe server under Proton.
const CAPTURED_REQUESTS: [&str; 10] = [
    "run3_check_api_version.bin",
    "run4_check_api_version.bin",
    "run4_get_overlay_params.bin",
    "client_verify_game_imported.bin",
    "client_get_game_installation_state.bin",
    "client_check_login_is_enabled.bin",
    "client_verify_login.bin",
    "client_get_authentication_option_realm.bin",
    "client_get_authentication_option_build.bin",
    "client_get_login_info.bin",
];

#[test]
fn captured_requests_decode() {
    let decoded: Vec<_> =
        CAPTURED_REQUESTS.into_iter().map(|name| (name, Message::decode(&fixture(name)).unwrap())).collect();

    insta::assert_debug_snapshot!(decoded);
}

#[test]
fn captured_requests_reencode_byte_for_byte() {
    for name in CAPTURED_REQUESTS {
        let bytes = fixture(name);

        let reencoded = Message::decode(&bytes).unwrap().encode().unwrap();

        assert_eq!(*reencoded, bytes, "{name}");
    }
}

#[test]
fn every_truncation_of_a_capture_is_rejected() {
    let bytes = fixture("run4_get_overlay_params.bin");

    for len in 0..bytes.len() {
        assert!(Message::decode(&bytes[..len]).is_err(), "decoded a {len}-byte prefix");
    }
}

#[test]
fn trailing_bytes_are_rejected() {
    let mut bytes = fixture("run4_get_overlay_params.bin");
    bytes.push(0);

    insta::assert_debug_snapshot!(Message::decode(&bytes).unwrap_err(), @r"
    TrailingBytes {
        remaining: 1,
    }
    ");
}

#[test]
fn unknown_command_type_is_reported_by_name() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0xEA, 0xDC, 200, 0, 0, 0]);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0001u32.to_le_bytes());
    let name = b"Ipc::Commands::GetHardwareId";
    bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
    bytes.extend_from_slice(name);

    insta::assert_debug_snapshot!(Message::decode(&bytes).unwrap_err(), @r#"
    UnknownCommand {
        name: "Ipc::Commands::GetHardwareId",
    }
    "#);
}

/// The second use of a type in one message repeats only its id, and the
/// base class version appears once.
#[test]
fn repeated_command_type_is_encoded_by_id() {
    let option = |uid: &str, key: &str| CommandFrame {
        class_version: ClassVersion(0),
        base_class_version: ClassVersion(0),
        uid: CommandUid(uid.to_owned()),
        flags: CommandFlags(0),
        command: Command::GetAuthenticationOption(GetAuthenticationOption {
            game_folder: "C".to_owned(),
            key: key.to_owned(),
            value: String::new(),
            status: Status(0),
        }),
    };
    let login = CommandFrame {
        class_version: ClassVersion(3),
        base_class_version: ClassVersion(0),
        uid: CommandUid("B".to_owned()),
        flags: CommandFlags(1),
        command: Command::GetLoginInfo(GetLoginInfo {
            account_realm: String::new(),
            user_name: String::new(),
            spa_id: String::new(),
            status: Status(0),
            game_folder: Some(String::new()),
            unnamed: Some(String::new()),
            account_complete: Some(false),
        }),
    };
    let message = Message { commands: vec![option("A", "x"), login, option("C", "y")] };

    let bytes = message.encode().unwrap();

    assert_eq!(Message::decode(&bytes).unwrap(), message);
    insta::assert_snapshot!(hex_lines(&bytes));
}

#[test]
fn class_version_and_optional_fields_must_agree() {
    let frame = CommandFrame {
        class_version: ClassVersion(1),
        base_class_version: ClassVersion(0),
        uid: CommandUid(String::new()),
        flags: CommandFlags(0),
        command: Command::GetLoginInfo(GetLoginInfo {
            account_realm: String::new(),
            user_name: String::new(),
            spa_id: String::new(),
            status: Status(0),
            game_folder: Some(String::new()),
            unnamed: None,
            account_complete: None,
        }),
    };

    let err = Message { commands: vec![frame] }.encode().unwrap_err();

    insta::assert_debug_snapshot!(err, @r#"
    VersionedField {
        command: "GetLoginInfo",
        field: "game_folder",
        version: 1,
    }
    "#);
}

#[test]
fn type_names_round_trip() {
    let kind = CommandKind::from_type_name("Ipc::Commands::QuerySpaToken1").unwrap();

    assert_eq!(kind, CommandKind::QuerySpaToken1);
    assert_eq!(kind.type_name(), "Ipc::Commands::QuerySpaToken1");
    assert_eq!(CommandKind::from_type_name("QuerySpaToken1"), None);
}

fn hex_lines(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .map(|chunk| chunk.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}
