//! Typed models of the `Ipc::Commands::*` types WGC answers. A request and
//! its response share one layout: the client fills the inputs, WGC fills the
//! outputs and echoes everything else. Field order follows the loaders in
//! `wgc_api.dll` 22.1.0.8738.

use std::fmt;

use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

use crate::error::CodecError;
use crate::message::ClassVersion;
use crate::wire::Reader;
use crate::wire::Writer;

/// The `u32` result code in a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status(pub u32);

impl Status {
    /// VerifyLogin and GetGameInstallationState accept nothing else.
    pub const SUCCESS: Status = Status(1);
    /// QuerySpaToken1: the user declined.
    pub const CANCELLED: Status = Status(2);
    /// GetLoginInfo: no account is signed in.
    pub const NOT_LOGGED_IN: Status = Status(3);
    /// QuerySpaToken1: the token request failed on the network.
    pub const NETWORK_ERROR: Status = Status(4);
    /// LoadSpaToken2: no stored token. QuerySpaToken1: no signed-in user.
    pub const NO_DATA: Status = Status(5);
}

/// GetOverlayParams graphics API selector, as the client sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphicsApi(pub u32);

/// VerifyLogin and VerifyLegalDocsAccepted request kind, as the client sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationType(pub u32);

/// The CheckLoginIsEnabled result. Unlike [`Status`], zero is success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginCheckResult(pub u32);

impl LoginCheckResult {
    /// `enabled` holds the answer.
    pub const ANSWERED: LoginCheckResult = LoginCheckResult(0);
    /// The client reports "game is not imported into WGC".
    pub const NOT_IMPORTED: LoginCheckResult = LoginCheckResult(1);
}

/// A token carried in a command. `Debug` hides it so logs and snapshots do not, and it is
/// wiped on drop.
#[derive(Clone, PartialEq, Eq, Default, Zeroize, ZeroizeOnDrop)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(text: String) -> SecretText {
        SecretText(text)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretText({} chars)", self.0.encode_utf16().count())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckApiVersion {
    pub game_folder: String,
    /// The client's `wgc_api.dll` version, for example `22.1.0.8738`.
    pub api_version: String,
    pub compatible: bool,
    /// Class version 1.
    pub send_stat_event: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetOverlayParams {
    pub client_app_dir: String,
    pub x64: bool,
    /// Empty disables the overlay. The command has no status.
    pub overlay_lib_path: String,
    /// Class version 2.
    pub graphics_api: Option<GraphicsApi>,
    /// Class version 3.
    pub auto_connect: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyGameImported {
    pub game_folder: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetGameInstallationState {
    pub status: Status,
    pub game_folder: String,
    /// False makes the client fail with "mandatory update unfinished".
    pub can_start: bool,
    pub parts: Vec<String>,
    /// `client_type` from the install's game_info.xml.
    pub client_type: String,
    /// `app_type` from the install's app_type.xml.
    pub app_type: String,
    /// Two DLC string lists; their meaning is not established.
    pub dlc: [Vec<String>; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckLoginIsEnabled {
    pub game_folder: String,
    pub enabled: bool,
    pub result: LoginCheckResult,
}

/// VerifyLogin and VerifyLegalDocsAccepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    pub verification_type: VerificationType,
    pub game_folder: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetAuthenticationOption {
    pub game_folder: String,
    pub key: String,
    pub value: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetLoginInfo {
    /// Compared with the `client_realm` option, case included.
    pub account_realm: String,
    pub user_name: String,
    /// Decimal SPA account id.
    pub spa_id: String,
    pub status: Status,
    /// Class version 2.
    pub game_folder: Option<String>,
    /// Class version 2. At most 63 characters; its meaning is not established.
    pub unnamed: Option<String>,
    /// Class version 3.
    pub account_complete: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetGameLanguage {
    pub language: String,
    pub status: Status,
    pub game_folder: String,
}

/// LoadSpaToken2 and SaveSpaToken2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaToken2 {
    pub game_folder: String,
    /// Decimal SPA account id.
    pub spa_id: String,
    pub token2: SecretText,
    pub description: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySpaToken1 {
    pub game_folder: String,
    /// Decimal SPA account id.
    pub spa_id: String,
    pub token1: SecretText,
    pub status: Status,
    pub login_dialog_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    CheckApiVersion(CheckApiVersion),
    GetOverlayParams(GetOverlayParams),
    VerifyGameImported(VerifyGameImported),
    GetGameInstallationState(GetGameInstallationState),
    CheckLoginIsEnabled(CheckLoginIsEnabled),
    VerifyLogin(Verification),
    VerifyLegalDocsAccepted(Verification),
    GetAuthenticationOption(GetAuthenticationOption),
    GetLoginInfo(GetLoginInfo),
    GetGameLanguage(GetGameLanguage),
    LoadSpaToken2(SpaToken2),
    SaveSpaToken2(SpaToken2),
    QuerySpaToken1(QuerySpaToken1),
}

impl Command {
    pub fn kind(&self) -> CommandKind {
        match self {
            Command::CheckApiVersion(_) => CommandKind::CheckApiVersion,
            Command::GetOverlayParams(_) => CommandKind::GetOverlayParams,
            Command::VerifyGameImported(_) => CommandKind::VerifyGameImported,
            Command::GetGameInstallationState(_) => CommandKind::GetGameInstallationState,
            Command::CheckLoginIsEnabled(_) => CommandKind::CheckLoginIsEnabled,
            Command::VerifyLogin(_) => CommandKind::VerifyLogin,
            Command::VerifyLegalDocsAccepted(_) => CommandKind::VerifyLegalDocsAccepted,
            Command::GetAuthenticationOption(_) => CommandKind::GetAuthenticationOption,
            Command::GetLoginInfo(_) => CommandKind::GetLoginInfo,
            Command::GetGameLanguage(_) => CommandKind::GetGameLanguage,
            Command::LoadSpaToken2(_) => CommandKind::LoadSpaToken2,
            Command::SaveSpaToken2(_) => CommandKind::SaveSpaToken2,
            Command::QuerySpaToken1(_) => CommandKind::QuerySpaToken1,
        }
    }

    pub(crate) fn decode(
        kind: CommandKind,
        version: ClassVersion,
        reader: &mut Reader<'_>,
    ) -> Result<Command, CodecError> {
        let command = match kind {
            CommandKind::CheckApiVersion => Command::CheckApiVersion(CheckApiVersion {
                game_folder: reader.wstring()?,
                api_version: reader.string()?,
                compatible: reader.bool()?,
                send_stat_event: version.read_since(1, || reader.bool())?,
            }),
            CommandKind::GetOverlayParams => Command::GetOverlayParams(GetOverlayParams {
                client_app_dir: reader.wstring()?,
                x64: reader.bool()?,
                overlay_lib_path: reader.wstring()?,
                graphics_api: version.read_since(2, || reader.u32().map(GraphicsApi))?,
                auto_connect: version.read_since(3, || reader.bool())?,
            }),
            CommandKind::VerifyGameImported => Command::VerifyGameImported(VerifyGameImported {
                game_folder: reader.wstring()?,
                status: Status(reader.u32()?),
            }),
            CommandKind::GetGameInstallationState => Command::GetGameInstallationState(GetGameInstallationState {
                status: Status(reader.u32()?),
                game_folder: reader.wstring()?,
                can_start: reader.bool()?,
                parts: reader.wstring_vec()?,
                client_type: reader.wstring()?,
                app_type: reader.wstring()?,
                dlc: [reader.wstring_vec()?, reader.wstring_vec()?],
            }),
            CommandKind::CheckLoginIsEnabled => Command::CheckLoginIsEnabled(CheckLoginIsEnabled {
                game_folder: reader.wstring()?,
                enabled: reader.bool()?,
                result: LoginCheckResult(reader.u32()?),
            }),
            CommandKind::VerifyLogin => Command::VerifyLogin(Verification::decode(reader)?),
            CommandKind::VerifyLegalDocsAccepted => Command::VerifyLegalDocsAccepted(Verification::decode(reader)?),
            CommandKind::GetAuthenticationOption => Command::GetAuthenticationOption(GetAuthenticationOption {
                game_folder: reader.wstring()?,
                key: reader.string()?,
                value: reader.wstring()?,
                status: Status(reader.u32()?),
            }),
            CommandKind::GetLoginInfo => Command::GetLoginInfo(GetLoginInfo {
                account_realm: reader.wstring()?,
                user_name: reader.wstring()?,
                spa_id: reader.wstring()?,
                status: Status(reader.u32()?),
                game_folder: version.read_since(2, || reader.wstring())?,
                unnamed: version.read_since(2, || reader.wstring())?,
                account_complete: version.read_since(3, || reader.bool())?,
            }),
            CommandKind::GetGameLanguage => Command::GetGameLanguage(GetGameLanguage {
                language: reader.wstring()?,
                status: Status(reader.u32()?),
                game_folder: reader.wstring()?,
            }),
            CommandKind::LoadSpaToken2 => Command::LoadSpaToken2(SpaToken2::decode(reader)?),
            CommandKind::SaveSpaToken2 => Command::SaveSpaToken2(SpaToken2::decode(reader)?),
            CommandKind::QuerySpaToken1 => Command::QuerySpaToken1(QuerySpaToken1 {
                game_folder: reader.wstring()?,
                spa_id: reader.wstring()?,
                token1: SecretText(reader.wstring()?),
                status: Status(reader.u32()?),
                login_dialog_allowed: reader.bool()?,
            }),
        };

        Ok(command)
    }

    pub(crate) fn encode(&self, version: ClassVersion, writer: &mut Writer) -> Result<(), CodecError> {
        let name = self.kind().short_name();
        match self {
            Command::CheckApiVersion(command) => {
                writer.wstring(&command.game_folder);
                writer.string(&command.api_version);
                writer.bool(command.compatible);
                version
                    .write_since(1, (name, "send_stat_event"), command.send_stat_event, |value| writer.bool(value))?;
            }
            Command::GetOverlayParams(command) => {
                writer.wstring(&command.client_app_dir);
                writer.bool(command.x64);
                writer.wstring(&command.overlay_lib_path);
                version.write_since(2, (name, "graphics_api"), command.graphics_api, |value| writer.u32(value.0))?;
                version.write_since(3, (name, "auto_connect"), command.auto_connect, |value| writer.bool(value))?;
            }
            Command::VerifyGameImported(command) => {
                writer.wstring(&command.game_folder);
                writer.u32(command.status.0);
            }
            Command::GetGameInstallationState(command) => {
                writer.u32(command.status.0);
                writer.wstring(&command.game_folder);
                writer.bool(command.can_start);
                writer.wstring_vec(&command.parts);
                writer.wstring(&command.client_type);
                writer.wstring(&command.app_type);
                writer.wstring_vec(&command.dlc[0]);
                writer.wstring_vec(&command.dlc[1]);
            }
            Command::CheckLoginIsEnabled(command) => {
                writer.wstring(&command.game_folder);
                writer.bool(command.enabled);
                writer.u32(command.result.0);
            }
            Command::VerifyLogin(command) | Command::VerifyLegalDocsAccepted(command) => command.encode(writer),
            Command::GetAuthenticationOption(command) => {
                writer.wstring(&command.game_folder);
                writer.string(&command.key);
                writer.wstring(&command.value);
                writer.u32(command.status.0);
            }
            Command::GetLoginInfo(command) => {
                writer.wstring(&command.account_realm);
                writer.wstring(&command.user_name);
                writer.wstring(&command.spa_id);
                writer.u32(command.status.0);
                version.write_since(2, (name, "game_folder"), command.game_folder.as_deref(), |value| {
                    writer.wstring(value)
                })?;
                version.write_since(2, (name, "unnamed"), command.unnamed.as_deref(), |value| writer.wstring(value))?;
                version
                    .write_since(3, (name, "account_complete"), command.account_complete, |value| writer.bool(value))?;
            }
            Command::GetGameLanguage(command) => {
                writer.wstring(&command.language);
                writer.u32(command.status.0);
                writer.wstring(&command.game_folder);
            }
            Command::LoadSpaToken2(command) | Command::SaveSpaToken2(command) => command.encode(writer),
            Command::QuerySpaToken1(command) => {
                writer.wstring(&command.game_folder);
                writer.wstring(&command.spa_id);
                writer.wstring(command.token1.expose_secret());
                writer.u32(command.status.0);
                writer.bool(command.login_dialog_allowed);
            }
        }

        Ok(())
    }
}

impl Verification {
    fn decode(reader: &mut Reader<'_>) -> Result<Verification, CodecError> {
        Ok(Verification {
            verification_type: VerificationType(reader.u32()?),
            game_folder: reader.wstring()?,
            status: Status(reader.u32()?),
        })
    }

    fn encode(&self, writer: &mut Writer) {
        writer.u32(self.verification_type.0);
        writer.wstring(&self.game_folder);
        writer.u32(self.status.0);
    }
}

impl SpaToken2 {
    fn decode(reader: &mut Reader<'_>) -> Result<SpaToken2, CodecError> {
        Ok(SpaToken2 {
            game_folder: reader.wstring()?,
            spa_id: reader.wstring()?,
            token2: SecretText(reader.wstring()?),
            description: reader.wstring()?,
            status: Status(reader.u32()?),
        })
    }

    fn encode(&self, writer: &mut Writer) {
        writer.wstring(&self.game_folder);
        writer.wstring(&self.spa_id);
        writer.wstring(self.token2.expose_secret());
        writer.wstring(&self.description);
        writer.u32(self.status.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    CheckApiVersion,
    GetOverlayParams,
    VerifyGameImported,
    GetGameInstallationState,
    CheckLoginIsEnabled,
    VerifyLogin,
    VerifyLegalDocsAccepted,
    GetAuthenticationOption,
    GetLoginInfo,
    GetGameLanguage,
    LoadSpaToken2,
    SaveSpaToken2,
    QuerySpaToken1,
}

impl CommandKind {
    const NAMESPACE: &str = "Ipc::Commands::";

    const ALL: [CommandKind; 13] = [
        CommandKind::CheckApiVersion,
        CommandKind::GetOverlayParams,
        CommandKind::VerifyGameImported,
        CommandKind::GetGameInstallationState,
        CommandKind::CheckLoginIsEnabled,
        CommandKind::VerifyLogin,
        CommandKind::VerifyLegalDocsAccepted,
        CommandKind::GetAuthenticationOption,
        CommandKind::GetLoginInfo,
        CommandKind::GetGameLanguage,
        CommandKind::LoadSpaToken2,
        CommandKind::SaveSpaToken2,
        CommandKind::QuerySpaToken1,
    ];

    pub fn short_name(self) -> &'static str {
        match self {
            CommandKind::CheckApiVersion => "CheckApiVersion",
            CommandKind::GetOverlayParams => "GetOverlayParams",
            CommandKind::VerifyGameImported => "VerifyGameImported",
            CommandKind::GetGameInstallationState => "GetGameInstallationState",
            CommandKind::CheckLoginIsEnabled => "CheckLoginIsEnabled",
            CommandKind::VerifyLogin => "VerifyLogin",
            CommandKind::VerifyLegalDocsAccepted => "VerifyLegalDocsAccepted",
            CommandKind::GetAuthenticationOption => "GetAuthenticationOption",
            CommandKind::GetLoginInfo => "GetLoginInfo",
            CommandKind::GetGameLanguage => "GetGameLanguage",
            CommandKind::LoadSpaToken2 => "LoadSpaToken2",
            CommandKind::SaveSpaToken2 => "SaveSpaToken2",
            CommandKind::QuerySpaToken1 => "QuerySpaToken1",
        }
    }

    /// The cereal polymorphic name, for example `Ipc::Commands::CheckApiVersion`.
    pub fn type_name(self) -> String {
        format!("{}{}", Self::NAMESPACE, self.short_name())
    }

    pub fn from_type_name(name: &str) -> Option<CommandKind> {
        let short = name.strip_prefix(Self::NAMESPACE)?;

        Self::ALL.into_iter().find(|kind| kind.short_name() == short)
    }
}
