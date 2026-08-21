//! Async HTTP client for the WGNI login flow.

use std::time::Duration;
use std::time::SystemTime;

use wgc_core::TrackingId;

use crate::error::AuthError;
use crate::identity::AccountId;
use crate::identity::Nickname;
use crate::models;
use crate::pow::PowChallenge;
use crate::pow::PowError;
use crate::pow::PowSolution;
use crate::pow::{
    self,
};
use crate::realm::AccountRealm;
use crate::session::StoredSession;
use crate::token::AccessToken;
use crate::token::ExchangeCode;
use crate::token::SecretPassword;
use crate::token::TokenExpiry;
use crate::token::TokenPair;

/// WGNI API path version.
///
/// The live EU endpoint answers the v2 challenge path with a valid PoW
/// challenge (HTTP 200); the v3 challenge path returns HTTP 403 (CSRF, browser
/// Referer required) to a non-browser client, so this crate speaks v2. Switch
/// this constant if a future capture shows v3 usable headlessly.
const WGNI_API_VERSION: &str = "v2";

/// WGC's own user-agent string, matching the convention already established
/// in `wgc-catalog`'s client so WGNI sees the same client identity a real
/// Game Center install would send.
const USER_AGENT: &str = "WargamingGameCenter/26.04.01.3190";

/// Wall-clock budget for the proof-of-work search.
const POW_TIMEOUT: Duration = Duration::from_secs(30);

/// The token endpoint hands back a 202 + Location that resolves to the real
/// token; poll that Location this many times, this far apart, before treating
/// a still-pending login as awaiting an out-of-band second factor.
const ASYNC_TOKEN_POLLS: usize = 15;
const ASYNC_TOKEN_POLL_INTERVAL: Duration = Duration::from_secs(2);

const EXCHANGE_CODE_LEN: usize = 32;
const EXCHANGE_CODE_ALPHABET: &[u8] = b"0123456789ABCDEF";

/// Login input. Not stored; consumed by `login`/`login_with_two_factor`.
#[derive(Debug)]
pub struct AccountCredentials {
    pub username: String,
    pub password: SecretPassword,
}

/// A 2FA retry submits exactly one of these, never both or neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TwoFactorCode {
    Otp(String),
    Backup(String),
}

pub struct AuthClient {
    http: reqwest::Client,
    realm: AccountRealm,
    tracking_id: TrackingId,
}

impl AuthClient {
    pub fn new(realm: AccountRealm, tracking_id: TrackingId) -> AuthClient {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            // The challenge response sets a short-lived session cookie
            // (npprod_wgni_sessionid); the token POST must carry it back so the
            // server ties the solved PoW to the challenge it issued.
            .cookie_store(true)
            .build()
            // Static builder config (fixed UA, default TLS backend); a
            // failure here means the platform's TLS/root-cert setup is
            // broken, not a runtime data problem this crate can recover
            // from (same precedent as realm.rs's realm-table `.expect()`).
            .expect("failed to build WGNI HTTP client");
        AuthClient { http, realm, tracking_id }
    }

    /// Full password-grant login: challenge -> solve PoW -> token -> exchange
    /// -> account info. Returns `TwoFactorRequired`/`CaptchaRequired` as Err
    /// variants rather than a partial success.
    pub async fn login(&self, credentials: AccountCredentials) -> Result<StoredSession, AuthError> {
        self.login_inner(&credentials, None).await
    }

    /// Resubmit after `AuthError::TwoFactorRequired`, with either an OTP or
    /// a backup code.
    pub async fn login_with_two_factor(
        &self,
        credentials: AccountCredentials,
        twofactor_token: String,
        code: TwoFactorCode,
    ) -> Result<StoredSession, AuthError> {
        self.login_inner(&credentials, Some((twofactor_token, code))).await
    }

    async fn login_inner(
        &self,
        credentials: &AccountCredentials,
        twofactor: Option<(String, TwoFactorCode)>,
    ) -> Result<StoredSession, AuthError> {
        let challenge_wire = self.request_challenge().await?;
        let challenge = to_pow_challenge(&challenge_wire);
        let solution = solve_pow(challenge).await?;
        let success = self.request_token_by_password(credentials, solution, twofactor.as_ref()).await?;
        // The password-grant success body carries no `user` field; identity
        // comes from the account/info call below. When `user` is present
        // (older/other shapes), cross-check it as a consistency guard.
        let user_id = success.user;
        let access_token = AccessToken::new(success.access_token);
        let expiry = expiry_from(success.expires_in);
        let exchange_code = self.request_token_exchange(&access_token).await?;
        let tokens = TokenPair { access_token, exchange_code };
        let (account_id, nickname) = self.request_account_info(&tokens).await?;
        if let Some(user_id) = user_id
            && account_id.get() != user_id
        {
            return Err(AuthError::BadResponse {
                detail: format!("account/info sub {} does not match token user {}", account_id.get(), user_id),
            });
        }
        Ok(StoredSession { realm: self.realm, account_id, nickname, tokens, expiry })
    }

    /// Lightweight authenticated probe (`account/info`) to check whether a
    /// stored `TokenPair` is still valid, without a full re-login.
    pub async fn verify(&self, tokens: &TokenPair) -> Result<(AccountId, Nickname), AuthError> {
        self.request_account_info(tokens).await
    }

    /// Best-effort server-side revoke of `account/credentials/revoke/oauth/token/`.
    /// Under the current `WGNI_API_VERSION` this call may simply 404; callers
    /// must delete the local keyring entry regardless of whether it returns
    /// `Err`.
    pub async fn logout(&self, tokens: &TokenPair) -> Result<(), AuthError> {
        let url = self.endpoint("account/credentials/revoke/oauth/token/");
        let bearer = bearer_header(tokens);
        let response = self
            .http
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, bearer)
            .send()
            .await
            .map_err(|source| AuthError::Request { url: url.clone(), source })?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        // Best-effort: a revoke failure body is diagnostic only, and a body
        // read can itself fail (same precedent as wgc-catalog's client).
        let body = response.text().await.unwrap_or_default();
        Err(AuthError::BadResponse { detail: format!("revoke endpoint returned HTTP {}: {}", status.as_u16(), body) })
    }

    async fn request_challenge(&self) -> Result<models::PowChallengeWire, AuthError> {
        let url = self.endpoint("account/credentials/create/oauth/token/challenge/");
        let response =
            self.http.post(&url).send().await.map_err(|source| AuthError::Request { url: url.clone(), source })?;
        let status = response.status();
        // HTTP 409 on the challenge endpoint means banned, not a generic
        // failure.
        if status.as_u16() == 409 {
            return Err(AuthError::AccountBanned);
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AuthError::BadResponse {
                detail: format!("challenge endpoint returned HTTP {}: {}", status.as_u16(), body),
            });
        }
        let body: models::ChallengeResponse =
            response.json().await.map_err(|source| AuthError::Request { url: url.clone(), source })?;
        if body.captcha.is_some() || body.captcha_challenges.is_some() {
            // Fail closed: this crate never renders/solves a CAPTCHA.
            return Err(AuthError::CaptchaRequired);
        }
        let pow = body
            .pow
            .ok_or_else(|| AuthError::BadResponse { detail: "challenge response missing pow object".to_string() })?;
        // pow::solve only implements the hashcash/Keccak-512 algorithm; a
        // different algorithm here would silently compute a doomed solution
        // instead of failing clearly.
        if pow.algorithm.name != "hashcash" {
            return Err(AuthError::BadResponse {
                detail: format!("unsupported proof-of-work algorithm {:?}", pow.algorithm.name),
            });
        }
        Ok(pow)
    }

    async fn request_token_by_password(
        &self,
        credentials: &AccountCredentials,
        pow: PowSolution,
        twofactor: Option<&(String, TwoFactorCode)>,
    ) -> Result<models::TokenSuccessResponse, AuthError> {
        let url = self.endpoint("account/credentials/create/oauth/token/");
        let realm_config = self.realm.config();
        let pow_str = pow.get().to_string();
        let mut form: Vec<(&str, &str)> = vec![
            ("username", credentials.username.as_str()),
            ("password", credentials.password.expose_secret()),
            ("grant_type", "urn:wargaming:params:oauth:grant-type:basic"),
            ("client_id", realm_config.client_id.as_str()),
            ("tid", self.tracking_id.as_str()),
            ("pow", &pow_str),
        ];
        if let Some((token, code)) = twofactor {
            form.push(("twofactor_token", token.as_str()));
            match code {
                TwoFactorCode::Otp(v) => form.push(("otp_code", v.as_str())),
                TwoFactorCode::Backup(v) => form.push(("backup_code", v.as_str())),
            }
        }
        self.post_form_expect_token(&url, &form).await
    }

    async fn request_token_exchange(&self, access_token: &AccessToken) -> Result<ExchangeCode, AuthError> {
        let exchange_code = ExchangeCode::from_raw(generate_exchange_code());
        let url = self.endpoint("account/credentials/create/oauth/token/");
        let realm_config = self.realm.config();
        let form: [(&str, &str); 5] = [
            ("grant_type", "urn:wargaming:params:oauth:grant-type:access-token"),
            ("access_token", access_token.expose_secret()),
            ("client_id", realm_config.client_id.as_str()),
            ("exchange_code", exchange_code.as_str()),
            ("tid", self.tracking_id.as_str()),
        ];
        self.post_form_expect_token(&url, &form).await?;
        Ok(exchange_code)
    }

    async fn request_account_info(&self, tokens: &TokenPair) -> Result<(AccountId, Nickname), AuthError> {
        let url = self.endpoint("account/info/");
        let bearer = bearer_header(tokens);
        let form = [("fields", "nickname")];
        let response = self
            .http
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, bearer)
            .form(&form)
            .send()
            .await
            .map_err(|source| AuthError::Request { url: url.clone(), source })?;
        let (status, body_text) = self.resolve_async(response, &url).await?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AuthError::SessionExpired);
        }
        if !status.is_success() {
            return Err(AuthError::BadResponse {
                detail: format!("account/info returned HTTP {}: {}", status.as_u16(), body_text),
            });
        }
        let info: models::AccountInfoResponse = serde_json::from_str(&body_text).map_err(|source| {
            AuthError::BadResponse { detail: format!("could not parse account/info response: {source}") }
        })?;
        Ok((AccountId::new(info.sub), Nickname::new(info.nickname)))
    }

    /// POST a form body to `url`, parsing a 2xx JSON body as a success token
    /// response and classifying a non-2xx body via `classify_token_error`.
    async fn post_form_expect_token(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> Result<models::TokenSuccessResponse, AuthError> {
        let response = self
            .http
            .post(url)
            .form(form)
            .send()
            .await
            .map_err(|source| AuthError::Request { url: url.to_string(), source })?;
        let (status, body_text) = self.resolve_async(response, url).await?;
        if status.is_success() {
            return serde_json::from_str(&body_text).map_err(|source| AuthError::BadResponse {
                detail: format!("could not parse token response: {source}"),
            });
        }
        // The error body may not even be JSON (proxy/5xx page); a failed
        // parse here just means no structured error_description to classify
        // on, which classify_token_error already handles via `None`.
        let error: Option<models::TokenErrorResponse> = serde_json::from_str(&body_text).ok();
        let description = error.as_ref().and_then(|e| e.error_description.clone());
        let twofactor_token = error.and_then(|e| e.twofactor_token);
        Err(classify_token_error(status.as_u16(), description.as_deref(), twofactor_token))
    }

    /// WGNI answers a token grant and account/info with `202 Accepted` plus a
    /// `Location` that materializes the real result; follow it until it settles
    /// to a final status. A `Location`-chained 202 that never settles within
    /// the budget means the login is waiting on an out-of-band second factor
    /// (an authenticator approval or emailed code). `context_url` names the
    /// originating request for error reporting.
    async fn resolve_async(
        &self,
        response: reqwest::Response,
        context_url: &str,
    ) -> Result<(reqwest::StatusCode, String), AuthError> {
        let mut status = response.status();
        let mut next = location_of(&response);
        let mut body =
            response.text().await.map_err(|source| AuthError::Request { url: context_url.to_string(), source })?;
        if status.as_u16() != 202 {
            return Ok((status, body));
        }
        let mut location = next.take().ok_or_else(|| AuthError::BadResponse {
            detail: format!("{context_url} returned 202 without a Location"),
        })?;
        for _ in 0..ASYNC_TOKEN_POLLS {
            let polled = self
                .http
                .get(&location)
                .send()
                .await
                .map_err(|source| AuthError::Request { url: location.clone(), source })?;
            status = polled.status();
            let chained = location_of(&polled);
            body = polled.text().await.map_err(|source| AuthError::Request { url: location.clone(), source })?;
            if status.as_u16() != 202 {
                return Ok((status, body));
            }
            if let Some(chained) = chained {
                location = chained;
            }
            tokio::time::sleep(ASYNC_TOKEN_POLL_INTERVAL).await;
        }
        Err(AuthError::VerificationPending)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("https://{}/id/api/{}/{}", self.realm.config().domain_wgnet, WGNI_API_VERSION, path)
    }
}

fn location_of(response: &reqwest::Response) -> Option<String> {
    response.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
}

fn bearer_header(tokens: &TokenPair) -> String {
    format!("Bearer {}:{}", tokens.access_token.expose_secret(), tokens.exchange_code.as_str())
}

fn to_pow_challenge(wire: &models::PowChallengeWire) -> PowChallenge {
    PowChallenge {
        algorithm_version: wire.algorithm.version.clone(),
        complexity: wire.complexity,
        timestamp: wire.timestamp.clone(),
        resource: wire.algorithm.resource.clone(),
        extension: wire.algorithm.extension.clone(),
        random_string: wire.random_string.clone(),
    }
}

fn expiry_from(expires_in: Option<u64>) -> TokenExpiry {
    match expires_in {
        Some(secs) => TokenExpiry::KnownAt(SystemTime::now() + Duration::from_secs(secs)),
        None => TokenExpiry::Unknown,
    }
}

/// CPU-bound; run off the async executor. `pow::solve` is single-threaded
/// for this phase (see pow.rs).
async fn solve_pow(challenge: PowChallenge) -> Result<PowSolution, AuthError> {
    let result =
        tokio::task::spawn_blocking(move || pow::solve(&challenge, POW_TIMEOUT)).await.map_err(|join_err| {
            AuthError::BadResponse { detail: format!("proof-of-work task did not complete: {join_err}") }
        })?;
    result.map_err(|PowError::Timeout| AuthError::ProofOfWorkTimeout)
}

/// Client-minted 32-char hex-ish nonce: 32 characters
/// drawn from `[0-9A-F]`, not server-issued.
fn generate_exchange_code() -> String {
    (0..EXCHANGE_CODE_LEN)
        .map(|_| EXCHANGE_CODE_ALPHABET[rand::random_range(0..EXCHANGE_CODE_ALPHABET.len())] as char)
        .collect()
}

/// Classifies a WGNI token-endpoint error response into a structured
/// `AuthError`. Internally matches on WG's raw `error_description` string to
/// *choose* the variant (classification at the boundary); every documented
/// string gets its own variant, and only a genuinely unrecognized string
/// falls through to `Rejected`.
fn classify_token_error(
    http_status: u16,
    error_description: Option<&str>,
    twofactor_token: Option<String>,
) -> AuthError {
    let Some(description) = error_description else {
        return AuthError::BadResponse {
            detail: format!("WGNI token endpoint returned HTTP {http_status} with no error_description"),
        };
    };
    match description {
        "twofactor_required" => match twofactor_token {
            Some(token) => AuthError::TwoFactorRequired { twofactor_token: token },
            None => AuthError::BadResponse {
                detail: "WGNI reported twofactor_required with no twofactor_token".to_string(),
            },
        },
        "twofactor_invalid"
        | "Invalid otp_code parameter value."
        | "Invalid twofactor token."
        | "Invalid backup_code parameter value." => AuthError::TwoFactorInvalidCode,
        // These two wire strings are not yet observed; they follow the
        // snake_case `twofactor_*` convention of the confirmed strings above
        // and stay best-effort until a live capture confirms them.
        "twofactor_login_locked" => AuthError::TwoFactorLoginLocked,
        "twofactor_banned" => AuthError::TwoFactorBanned,
        "account_not_found" => AuthError::AccountNotFound,
        "Invalid username parameter value."
        | "Invalid password parameter value."
        | "Request is missing password parameter." => AuthError::InvalidCredentials,
        other => AuthError::Rejected { description: other.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_builds_without_panicking() {
        let tracking_id = TrackingId::parse("test-tracking-id").expect("valid");
        let _client = AuthClient::new(AccountRealm::Eu, tracking_id);
    }

    #[test]
    fn endpoint_builds_versioned_url() {
        let tracking_id = TrackingId::parse("test-tracking-id").expect("valid");
        let client = AuthClient::new(AccountRealm::Eu, tracking_id);
        assert_eq!(client.endpoint("account/info/"), "https://eu.wargaming.net/id/api/v2/account/info/");
    }

    #[test]
    fn exchange_code_is_32_hex_chars() {
        let code = generate_exchange_code();
        assert_eq!(code.len(), 32);
        assert!(code.chars().all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c)), "code {code} has non-hex chars");
    }

    #[test]
    fn exchange_code_generation_is_not_constant() {
        // Not a strict randomness proof, just a smoke check that two draws
        // differ (astronomically unlikely to collide by chance).
        assert_ne!(generate_exchange_code(), generate_exchange_code());
    }

    #[test]
    fn classify_twofactor_required_carries_token() {
        let err = classify_token_error(400, Some("twofactor_required"), Some("tft-123".to_string()));
        assert!(matches!(err, AuthError::TwoFactorRequired { twofactor_token } if twofactor_token == "tft-123"));
    }

    #[test]
    fn classify_twofactor_required_without_token_is_bad_response() {
        let err = classify_token_error(400, Some("twofactor_required"), None);
        assert!(matches!(err, AuthError::BadResponse { .. }));
    }

    #[test]
    fn classify_twofactor_invalid_variants() {
        for description in [
            "twofactor_invalid",
            "Invalid otp_code parameter value.",
            "Invalid twofactor token.",
            "Invalid backup_code parameter value.",
        ] {
            let err = classify_token_error(400, Some(description), None);
            assert!(matches!(err, AuthError::TwoFactorInvalidCode), "description {description} misclassified: {err:?}");
        }
    }

    #[test]
    fn classify_account_not_found() {
        let err = classify_token_error(400, Some("account_not_found"), None);
        assert!(matches!(err, AuthError::AccountNotFound));
    }

    #[test]
    fn classify_invalid_credentials_variants() {
        for description in [
            "Invalid username parameter value.",
            "Invalid password parameter value.",
            "Request is missing password parameter.",
        ] {
            let err = classify_token_error(400, Some(description), None);
            assert!(matches!(err, AuthError::InvalidCredentials), "description {description} misclassified: {err:?}");
        }
    }

    #[test]
    fn classify_twofactor_login_locked() {
        let err = classify_token_error(400, Some("twofactor_login_locked"), None);
        assert!(matches!(err, AuthError::TwoFactorLoginLocked));
    }

    #[test]
    fn classify_twofactor_banned() {
        let err = classify_token_error(400, Some("twofactor_banned"), None);
        assert!(matches!(err, AuthError::TwoFactorBanned));
    }

    #[test]
    fn classify_unknown_description_falls_to_rejected() {
        let err = classify_token_error(400, Some("some_new_wg_error_we_have_not_seen"), None);
        assert!(
            matches!(err, AuthError::Rejected { description } if description == "some_new_wg_error_we_have_not_seen")
        );
    }

    #[test]
    fn classify_missing_description_is_bad_response() {
        let err = classify_token_error(500, None, None);
        assert!(matches!(err, AuthError::BadResponse { .. }));
    }
}
