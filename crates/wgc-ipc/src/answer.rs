use tracing::debug;
use tracing::warn;
use wgc_auth::AccountId;
use wgc_auth::AccountRealm;
use wgc_auth::GameToken2;

use crate::command::Command;
use crate::command::LoginCheckResult;
use crate::command::SecretText;
use crate::command::Status;
use crate::message::CommandFrame;
use crate::responder::IpcResponder;
use crate::responder::Token1Failure;

/// Fills the outputs of one request in place; inputs stay as the client sent
/// them.
pub(crate) async fn answer(frame: &mut CommandFrame, responder: &dyn IpcResponder) {
    debug!(command = frame.command.kind().short_name(), uid = frame.uid.0, "answering WGC IPC command");

    match &mut frame.command {
        Command::CheckApiVersion(command) => command.compatible = true,
        Command::GetOverlayParams(command) => command.overlay_lib_path.clear(),
        Command::VerifyGameImported(command) => command.status = Status::SUCCESS,
        Command::GetGameInstallationState(command) => {
            command.status = Status::SUCCESS;
            command.can_start = true;
            // WGC installs only the full client, so both files read "high".
            command.client_type = "high".to_owned();
            command.app_type = "high".to_owned();
        }
        Command::CheckLoginIsEnabled(command) => {
            command.enabled = true;
            command.result = LoginCheckResult::ANSWERED;
        }
        Command::VerifyLogin(command) | Command::VerifyLegalDocsAccepted(command) => command.status = Status::SUCCESS,
        Command::GetAuthenticationOption(command) => match command.key.as_str() {
            "client_realm" => {
                command.value = realm_code(responder.client_realm()).to_owned();
                command.status = Status::SUCCESS;
            }
            // The client compares it with L"prod".
            "build_type" => {
                command.value = "prod".to_owned();
                command.status = Status::SUCCESS;
            }
            key => warn!(key, "unknown WGC authentication option, echoing the request"),
        },
        Command::GetLoginInfo(command) => match responder.login_info() {
            Some(info) => {
                command.account_realm = realm_code(info.realm).to_owned();
                command.user_name = info.nickname.as_str().to_owned();
                command.spa_id = info.spa_id.get().to_string();
                command.status = Status::SUCCESS;
                if command.account_complete.is_some() {
                    command.account_complete = Some(true);
                }
            }
            None => command.status = Status::NOT_LOGGED_IN,
        },
        Command::GetGameLanguage(command) => {
            command.language = responder.game_language().as_str().to_owned();
            command.status = Status::SUCCESS;
        }
        Command::LoadSpaToken2(command) => match responder.load_token2().await {
            Some(grant) => {
                command.spa_id = grant.spa_id.get().to_string();
                command.token2 = SecretText::new(grant.token2.expose_secret().to_owned());
                command.status = Status::SUCCESS;
            }
            None => command.status = Status::NO_DATA,
        },
        Command::QuerySpaToken1(command) => match responder.query_token1().await {
            Ok(grant) => {
                command.spa_id = grant.spa_id.get().to_string();
                command.token1 = SecretText::new(grant.token1.expose_secret().to_owned());
                command.status = Status::SUCCESS;
            }
            Err(failure) => {
                warn!(?failure, "no token1 for the game");
                command.status = match failure {
                    Token1Failure::Cancelled => Status::CANCELLED,
                    Token1Failure::Network => Status::NETWORK_ERROR,
                    Token1Failure::NoUser => Status::NO_DATA,
                };
            }
        },
        Command::SaveSpaToken2(command) => match command.spa_id.parse::<u64>() {
            Ok(spa_id) => {
                let token2 = GameToken2::new(command.token2.expose_secret().to_owned());
                responder.save_token2(AccountId::new(spa_id), token2).await;
                command.status = Status::SUCCESS;
            }
            Err(_) => warn!(spa_id = command.spa_id, "SaveSpaToken2 carries a malformed account id, token not stored"),
        },
    }
}

/// The realm attribute of `host` in the client's `res/scripts_config.xml`.
fn realm_code(realm: AccountRealm) -> &'static str {
    match realm {
        AccountRealm::Na => "na",
        AccountRealm::Eu => "eu",
        AccountRealm::Asia => "asia",
    }
}
