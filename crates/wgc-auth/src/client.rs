//! Async HTTP client for the WGNI login flow and game token minting.

use std::time::Duration;
use std::time::SystemTime;

use rootcause::prelude::*;
use wgc_core::TrackingId;
use zeroize::Zeroize;

use crate::error::AuthError;
use crate::error::TokenRejection;
use crate::game_token::GameTitle;
use crate::game_token::GameToken1;
use crate::identity::AccountId;
use crate::identity::Nickname;
use crate::models;
use crate::pow;
use crate::pow::PowChallenge;
use crate::pow::PowError;
use crate::pow::PowSolution;
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
/// token; a login still pending after this many polls is awaiting an
/// out-of-band second factor.
const LOGIN_POLL: PollPolicy = PollPolicy { attempts: 15, interval: Duration::from_secs(2) };

/// WGC receives the token1 on its first poll, about 0.2 s after the POST.
/// The game waits on the pipe meanwhile, so a stalled mint fails fast.
const GAME_TOKEN1_TIMING: Token1Timing = Token1Timing {
    poll: PollPolicy { attempts: 20, interval: Duration::from_millis(500) },
    deadline: Duration::from_secs(20),
};

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

    /// `https://<realm host>`; tests point it at a local server.
    origin: String,
    game_token1: Token1Timing,
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
        let origin = format!("https://{}", realm.config().domain_wgnet);
        AuthClient { http, realm, tracking_id, origin, game_token1: GAME_TOKEN1_TIMING }
    }

    /// Full password-grant login: challenge -> solve PoW -> token -> exchange
    /// -> account info. Returns `TwoFactorRequired`/`CaptchaRequired` as Err
    /// variants rather than a partial success.
    pub async fn login(&self, credentials: AccountCredentials) -> rootcause::Result<StoredSession, AuthError> {
        self.login_inner(&credentials, None).await
    }

    /// Resubmit after `AuthError::TwoFactorRequired`, with either an OTP or
    /// a backup code.
    pub async fn login_with_two_factor(
        &self,
        credentials: AccountCredentials,
        twofactor_token: String,
        code: TwoFactorCode,
    ) -> rootcause::Result<StoredSession, AuthError> {
        self.login_inner(&credentials, Some((twofactor_token, code))).await
    }

    async fn login_inner(
        &self,
        credentials: &AccountCredentials,
        twofactor: Option<(String, TwoFactorCode)>,
    ) -> rootcause::Result<StoredSession, AuthError> {
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
            return Err(report!(AuthError::BadResponse {
                detail: format!("account/info sub {} does not match token user {}", account_id.get(), user_id),
            }));
        }
        Ok(StoredSession { realm: self.realm, account_id, nickname, tokens, expiry })
    }

    /// Lightweight authenticated probe (`account/info`) to check whether a
    /// stored `TokenPair` is still valid, without a full re-login.
    pub async fn verify(&self, tokens: &TokenPair) -> rootcause::Result<(AccountId, Nickname), AuthError> {
        self.request_account_info(tokens).await
    }

    /// Mints a one-time game login token for `session`'s account, as WGC does
    /// when a game sends `QuerySpaToken1`. Mint it only when the game asks:
    /// the game spends it within about a second.
    ///
    /// `SessionExpired` means the session token was rejected and the account
    /// must sign in again.
    pub async fn create_game_token1(
        &self,
        session: &StoredSession,
        title: &GameTitle,
    ) -> rootcause::Result<GameToken1, AuthError> {
        for requested in [session.realm, title.realm()] {
            if requested != self.realm {
                return Err(report!(AuthError::RealmMismatch { client: self.realm, requested }));
            }
        }

        let deadline = self.game_token1.deadline;
        match tokio::time::timeout(deadline, self.request_game_token1(&session.tokens, title)).await {
            Ok(result) => result,
            Err(_elapsed) => Err(report!(AuthError::GameTokenTimeout { limit: deadline })),
        }
    }

    /// Best-effort server-side revoke of `account/credentials/revoke/oauth/token/`.
    /// Under the current `WGNI_API_VERSION` this call may simply 404; callers
    /// must delete the local keyring entry regardless of whether it returns
    /// `Err`.
    pub async fn logout(&self, tokens: &TokenPair) -> rootcause::Result<(), AuthError> {
        let url = self.endpoint("account/credentials/revoke/oauth/token/");
        let bearer = bearer_header(tokens);
        let response =
            self.http.post(&url).header(reqwest::header::AUTHORIZATION, bearer).send().await.map_err(|source| {
                report!(AuthError::Request { url: url.clone(), source }).attach("revoking the session token")
            })?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        // Best-effort: a revoke failure body is diagnostic only, and a body
        // read can itself fail (same precedent as wgc-catalog's client).
        let body = response.text().await.unwrap_or_default();
        Err(report!(AuthError::BadResponse {
            detail: format!("revoke endpoint returned HTTP {}: {}", status.as_u16(), body),
        }))
    }

    async fn request_challenge(&self) -> rootcause::Result<models::PowChallengeWire, AuthError> {
        let url = self.endpoint("account/credentials/create/oauth/token/challenge/");
        let response = self.http.post(&url).send().await.map_err(|source| {
            report!(AuthError::Request { url: url.clone(), source }).attach("requesting the WGNI PoW challenge")
        })?;
        let status = response.status();
        // HTTP 409 on the challenge endpoint means banned, not a generic
        // failure.
        if status.as_u16() == 409 {
            return Err(report!(AuthError::AccountBanned));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(report!(AuthError::BadResponse {
                detail: format!("challenge endpoint returned HTTP {}: {}", status.as_u16(), body),
            }));
        }
        let body: models::ChallengeResponse = response.json().await.map_err(|source| {
            report!(AuthError::Request { url: url.clone(), source }).attach("parsing the WGNI challenge response body")
        })?;
        if body.captcha.is_some() || body.captcha_challenges.is_some() {
            // Fail closed: this crate never renders/solves a CAPTCHA.
            return Err(report!(AuthError::CaptchaRequired));
        }
        let pow = body.pow.ok_or_else(|| {
            report!(AuthError::BadResponse { detail: "challenge response missing pow object".to_string() })
        })?;
        // pow::solve only implements the hashcash/Keccak-512 algorithm; a
        // different algorithm here would silently compute a doomed solution
        // instead of failing clearly.
        if pow.algorithm.name != "hashcash" {
            return Err(report!(AuthError::BadResponse {
                detail: format!("unsupported proof-of-work algorithm {:?}", pow.algorithm.name),
            }));
        }
        Ok(pow)
    }

    async fn request_token_by_password(
        &self,
        credentials: &AccountCredentials,
        pow: PowSolution,
        twofactor: Option<&(String, TwoFactorCode)>,
    ) -> rootcause::Result<models::TokenSuccessResponse, AuthError> {
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

    async fn request_token_exchange(&self, access_token: &AccessToken) -> rootcause::Result<ExchangeCode, AuthError> {
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

    async fn request_account_info(&self, tokens: &TokenPair) -> rootcause::Result<(AccountId, Nickname), AuthError> {
        let url = self.endpoint("account/info/");
        let bearer = bearer_header(tokens);
        let form = [("fields", "nickname")];
        let response =
            self.http.post(&url).header(reqwest::header::AUTHORIZATION, bearer).form(&form).send().await.map_err(
                |source| report!(AuthError::Request { url: url.clone(), source }).attach("requesting account/info"),
            )?;
        let (status, body_text) = self.resolve_login(response, &url).await?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(report!(AuthError::SessionExpired));
        }
        if !status.is_success() {
            return Err(report!(AuthError::BadResponse {
                detail: format!("account/info returned HTTP {}: {}", status.as_u16(), body_text),
            }));
        }
        let info: models::AccountInfoResponse = serde_json::from_str(&body_text).map_err(|source| {
            report!(AuthError::MalformedResponse { url: url.clone(), source }).attach("parsing account/info response")
        })?;
        Ok((AccountId::new(info.sub), Nickname::new(info.nickname)))
    }

    /// POST a form body to `url`, parsing a 2xx JSON body as a success token
    /// response and classifying a non-2xx body via `classify_token_error`.
    async fn post_form_expect_token(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> rootcause::Result<models::TokenSuccessResponse, AuthError> {
        let response = self.http.post(url).form(form).send().await.map_err(|source| {
            report!(AuthError::Request { url: url.to_string(), source }).attach("posting to the WGNI token endpoint")
        })?;
        let (status, body_text) = self.resolve_login(response, url).await?;
        if status.is_success() {
            return serde_json::from_str(&body_text).map_err(|source| {
                report!(AuthError::MalformedResponse { url: url.to_string(), source }).attach("parsing token response")
            });
        }
        // The error body may not even be JSON (proxy/5xx page); a failed
        // parse here just means no structured error/error_description to
        // classify on, which classify_token_error already handles via `None`.
        let error: Option<models::TokenErrorResponse> = serde_json::from_str(&body_text).ok();
        let (error_code, description, twofactor_token) = match error {
            Some(e) => (e.error, e.error_description, e.twofactor_token),
            None => (None, None, None),
        };
        Err(classify_token_error(status.as_u16(), error_code.as_deref(), description.as_deref(), twofactor_token))
    }

    async fn request_game_token1(
        &self,
        tokens: &TokenPair,
        title: &GameTitle,
    ) -> rootcause::Result<GameToken1, AuthError> {
        let url = self.endpoint("account/credentials/create/token1/");
        let bearer = bearer_header(tokens);
        let body =
            models::Token1Request { requested_for: title.name().as_str(), title_code: title.title_code().as_str() }
                .form_body();
        let response = self
            .http
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, &bearer)
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|source| {
                report!(AuthError::Request { url: url.clone(), source }).attach("requesting a game token1")
            })?;

        let poll = self.game_token1.poll;
        let (status, mut body_text) = match self.resolve_async(response, &url, poll, Some(&bearer)).await? {
            Settled::Final { status, body } => (status, body),
            Settled::StillPending => {
                return Err(report!(AuthError::GameTokenTimeout { limit: poll.interval * poll.attempts as u32 }));
            }
        };

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(report!(AuthError::SessionExpired));
        }
        if !status.is_success() {
            // A non-JSON error body only means there is nothing structured to
            // classify on.
            let error: Option<models::TokenErrorResponse> = serde_json::from_str(&body_text).ok();
            let (error_code, description) = match error {
                Some(e) => (e.error, e.error_description),
                None => (None, None),
            };
            return Err(classify_token_error(status.as_u16(), error_code.as_deref(), description.as_deref(), None));
        }

        let parsed: Result<models::Token1Response, _> = serde_json::from_str(&body_text);
        body_text.zeroize();
        let mut parsed = parsed.map_err(|source| {
            report!(AuthError::MalformedResponse { url: url.clone(), source }).attach("parsing the token1 response")
        })?;

        Ok(GameToken1::new(std::mem::take(&mut parsed.token)))
    }

    /// `resolve_async` with the login budget. A login still pending after it
    /// is waiting on an out-of-band second factor (an authenticator approval
    /// or emailed code).
    async fn resolve_login(
        &self,
        response: reqwest::Response,
        context_url: &str,
    ) -> rootcause::Result<(reqwest::StatusCode, String), AuthError> {
        match self.resolve_async(response, context_url, LOGIN_POLL, None).await? {
            Settled::Final { status, body } => Ok((status, body)),
            Settled::StillPending => Err(report!(AuthError::VerificationPending)),
        }
    }

    /// WGNI answers token grants, account/info and token1 with `202 Accepted`
    /// plus a `Location` that materializes the real result; follow it until it
    /// settles to a final status or `policy` runs out. `context_url` names the
    /// originating request for error reporting; `authorization` is sent on
    /// every poll when present, and only to the originating origin.
    async fn resolve_async(
        &self,
        response: reqwest::Response,
        context_url: &str,
        policy: PollPolicy,
        authorization: Option<&str>,
    ) -> rootcause::Result<Settled, AuthError> {
        let status = response.status();
        // The URL this crate requested, not `response.url()`: a redirect must
        // not make another origin trusted with the bearer.
        let origin = url::Url::parse(context_url).ok().map(|url| url.origin());
        let next = location_of(&response);
        let body = response.text().await.map_err(|source| {
            report!(AuthError::Request { url: context_url.to_string(), source }).attach("reading response body")
        })?;
        if status != reqwest::StatusCode::ACCEPTED {
            return Ok(Settled::Final { status, body });
        }

        let mut location = next.ok_or_else(|| {
            report!(AuthError::BadResponse { detail: format!("{context_url} returned 202 without a Location") })
        })?;
        for _ in 0..policy.attempts {
            let mut request = self.http.get(location.as_str());
            if let Some(authorization) = authorization {
                // The bearer must never reach a host WGNI redirected to.
                if origin.as_ref() != Some(&location.origin()) {
                    return Err(report!(AuthError::BadResponse {
                        detail: format!("{context_url} pointed its result at foreign origin {location}"),
                    }));
                }
                request = request.header(reqwest::header::AUTHORIZATION, authorization);
            }
            let polled = request.send().await.map_err(|source| {
                report!(AuthError::Request { url: location.to_string(), source })
                    .attach("polling the async result Location")
            })?;
            let status = polled.status();
            let chained = location_of(&polled);
            let body = polled.text().await.map_err(|source| {
                report!(AuthError::Request { url: location.to_string(), source })
                    .attach(format!("reading polled body from {location}"))
            })?;
            if status != reqwest::StatusCode::ACCEPTED {
                return Ok(Settled::Final { status, body });
            }

            if let Some(chained) = chained {
                location = chained;
            }
            tokio::time::sleep(policy.interval).await;
        }

        Ok(Settled::StillPending)
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/id/api/{}/{}", self.origin, WGNI_API_VERSION, path)
    }
}

/// How often and how many times to poll an async WGNI `Location`.
#[derive(Debug, Clone, Copy)]
struct PollPolicy {
    attempts: usize,
    interval: Duration,
}

#[derive(Debug, Clone, Copy)]
struct Token1Timing {
    poll: PollPolicy,
    /// Bound on the whole mint, including stalled connections.
    deadline: Duration,
}

/// Outcome of following a WGNI `202 Accepted` chain.
enum Settled {
    Final { status: reqwest::StatusCode, body: String },
    StillPending,
}

/// The `Location` header resolved against the request URL, since WGNI may
/// send it relative. An unparsable header counts as absent.
fn location_of(response: &reqwest::Response) -> Option<url::Url> {
    let location = response.headers().get(reqwest::header::LOCATION)?.to_str().ok()?;
    response.url().join(location).ok()
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
async fn solve_pow(challenge: PowChallenge) -> rootcause::Result<PowSolution, AuthError> {
    let result = tokio::task::spawn_blocking(move || pow::solve(&challenge, POW_TIMEOUT))
        .await
        .map_err(|source| report!(AuthError::TaskJoin { source }).attach("proof-of-work solver task"))?;
    result.map_err(|PowError::Timeout| report!(AuthError::ProofOfWorkTimeout))
}

/// Client-minted 32-char hex-ish nonce: 32 characters
/// drawn from `[0-9A-F]`, not server-issued.
fn generate_exchange_code() -> String {
    (0..EXCHANGE_CODE_LEN)
        .map(|_| EXCHANGE_CODE_ALPHABET[rand::random_range(0..EXCHANGE_CODE_ALPHABET.len())] as char)
        .collect()
}

/// Tries `code` against the WG-specific machine codes this crate recognizes.
/// `rejection` is attached to the returned report either way, so the raw
/// HTTP status/error/error_description survive structurally even once
/// collapsed into an actionable `AuthError` variant.
fn classify_known_code(
    code: &str,
    rejection: &TokenRejection,
    twofactor_token: Option<String>,
) -> Option<rootcause::Report<AuthError>> {
    Some(match code {
        "twofactor_required" => match twofactor_token {
            Some(token) => report!(AuthError::TwoFactorRequired { twofactor_token: token }).attach(rejection.clone()),
            None => report!(AuthError::BadResponse {
                detail: "WGNI reported twofactor_required with no twofactor_token".to_string(),
            })
            .attach(rejection.clone()),
        },
        "twofactor_invalid" => report!(AuthError::TwoFactorInvalidCode).attach(rejection.clone()),
        "twofactor_login_locked" => report!(AuthError::TwoFactorLoginLocked).attach(rejection.clone()),
        "twofactor_banned" => report!(AuthError::TwoFactorBanned).attach(rejection.clone()),
        "account_not_found" => report!(AuthError::AccountNotFound).attach(rejection.clone()),
        _ => return None,
    })
}

/// Classifies a WGNI token-endpoint error response into a structured
/// `AuthError` report. WGNI's OAuth `error` field is the machine-readable key
/// per RFC 6749, so it is tried first; the only live capture on hand carries a
/// generic bucket there ("invalid_grant") and puts WG's specific reason in
/// `error_description` instead, so `error_description` is tried next against
/// the same known-code table. A last-resort fallback matches a short list of
/// `error_description` human sentences observed on the live endpoint (not
/// machine codes -- WG has not been seen to expose a distinct `error` value
/// for these). `rejection` (built here, once) is always attached so the raw
/// HTTP status/error/error_description are recoverable structurally even
/// from an unrecognized response.
fn classify_token_error(
    http_status: u16,
    error: Option<&str>,
    error_description: Option<&str>,
    twofactor_token: Option<String>,
) -> rootcause::Report<AuthError> {
    let rejection = TokenRejection {
        status: http_status,
        error: error.map(str::to_string),
        error_description: error_description.map(str::to_string),
    };

    if let Some(error) = error
        && let Some(report) = classify_known_code(error, &rejection, twofactor_token.clone())
    {
        return report;
    }
    if let Some(description) = error_description
        && let Some(report) = classify_known_code(description, &rejection, twofactor_token)
    {
        return report;
    }
    if let Some(description) = error_description {
        match description {
            "Invalid otp_code parameter value."
            | "Invalid twofactor token."
            | "Invalid backup_code parameter value." => {
                return report!(AuthError::TwoFactorInvalidCode).attach(rejection);
            }
            "Invalid username parameter value."
            | "Invalid password parameter value."
            | "Request is missing password parameter." => {
                return report!(AuthError::InvalidCredentials).attach(rejection);
            }
            _ => {}
        }
    }
    if error.is_none() && error_description.is_none() {
        return report!(AuthError::BadResponse {
            detail: format!("WGNI token endpoint returned HTTP {http_status} with no error body"),
        })
        .attach(rejection);
    }
    report!(AuthError::Rejected(rejection))
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
    fn classify_twofactor_required_carries_token_via_error_field() {
        // The `error` field itself is the known code: primary classification path.
        let report = classify_token_error(400, Some("twofactor_required"), None, Some("tft-123".to_string()));
        assert!(
            matches!(report.current_context(), AuthError::TwoFactorRequired { twofactor_token } if twofactor_token == "tft-123")
        );
    }

    #[test]
    fn classify_twofactor_required_carries_token_via_description_fallback() {
        // Live capture: `error` is the generic OAuth bucket, `error_description`
        // carries WG's specific reason. Exercises the fallback path.
        let report =
            classify_token_error(400, Some("invalid_grant"), Some("twofactor_required"), Some("tft-123".to_string()));
        assert!(
            matches!(report.current_context(), AuthError::TwoFactorRequired { twofactor_token } if twofactor_token == "tft-123")
        );
    }

    #[test]
    fn classify_twofactor_required_without_token_is_bad_response() {
        let report = classify_token_error(400, Some("twofactor_required"), None, None);
        assert!(matches!(report.current_context(), AuthError::BadResponse { .. }));
    }

    #[test]
    fn classify_twofactor_invalid_variants() {
        for description in [
            "twofactor_invalid",
            "Invalid otp_code parameter value.",
            "Invalid twofactor token.",
            "Invalid backup_code parameter value.",
        ] {
            let report = classify_token_error(400, None, Some(description), None);
            assert!(
                matches!(report.current_context(), AuthError::TwoFactorInvalidCode),
                "description {description} misclassified: {:?}",
                report.current_context()
            );
        }
    }

    #[test]
    fn classify_account_not_found() {
        let report = classify_token_error(400, Some("account_not_found"), None, None);
        assert!(matches!(report.current_context(), AuthError::AccountNotFound));
    }

    #[test]
    fn classify_invalid_credentials_variants() {
        for description in [
            "Invalid username parameter value.",
            "Invalid password parameter value.",
            "Request is missing password parameter.",
        ] {
            let report = classify_token_error(400, None, Some(description), None);
            assert!(
                matches!(report.current_context(), AuthError::InvalidCredentials),
                "description {description} misclassified: {:?}",
                report.current_context()
            );
        }
    }

    #[test]
    fn classify_twofactor_login_locked() {
        let report = classify_token_error(400, Some("twofactor_login_locked"), None, None);
        assert!(matches!(report.current_context(), AuthError::TwoFactorLoginLocked));
    }

    #[test]
    fn classify_twofactor_banned() {
        let report = classify_token_error(400, Some("twofactor_banned"), None, None);
        assert!(matches!(report.current_context(), AuthError::TwoFactorBanned));
    }

    #[test]
    fn classify_unknown_code_falls_to_rejected_with_rejection_preserved() {
        let report =
            classify_token_error(400, Some("some_new_wg_error_we_have_not_seen"), Some("New failure mode."), None);
        match report.current_context() {
            AuthError::Rejected(rejection) => {
                assert_eq!(rejection.status, 400);
                assert_eq!(rejection.error.as_deref(), Some("some_new_wg_error_we_have_not_seen"));
                assert_eq!(rejection.error_description.as_deref(), Some("New failure mode."));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn classify_missing_body_is_bad_response() {
        let report = classify_token_error(500, None, None, None);
        assert!(matches!(report.current_context(), AuthError::BadResponse { .. }));
    }

    #[test]
    fn classify_unrecognized_error_with_no_description_falls_to_rejected() {
        // `error` present but not one of the known codes, and no
        // `error_description` fallback available: still a normal rejection,
        // not a malformed-response BadResponse.
        let report = classify_token_error(400, Some("unsupported_grant_type"), None, None);
        match report.current_context() {
            AuthError::Rejected(rejection) => {
                assert_eq!(rejection.status, 400);
                assert_eq!(rejection.error.as_deref(), Some("unsupported_grant_type"));
                assert_eq!(rejection.error_description, None);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn classify_attaches_rejection_for_actionable_variants() {
        // Even a fully-classified, actionable variant must not lose the raw
        // status/error/error_description: they are recoverable from the report's
        // attachments, not just the enum shape.
        let report = classify_token_error(400, Some("account_not_found"), None, None);
        let rejection = report.attachments().iter().find_map(|a| a.downcast_inner::<TokenRejection>());
        let rejection = rejection.expect("TokenRejection attached to an actionable classification");
        assert_eq!(rejection.status, 400);
        assert_eq!(rejection.error.as_deref(), Some("account_not_found"));
    }

    mod game_token1 {
        use std::sync::Arc;
        use std::sync::Mutex;

        use tokio::io::AsyncReadExt;
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;
        use tokio::net::TcpStream;

        use super::*;
        use crate::game_token::GameTitle;
        use crate::identity::Nickname;

        #[derive(Clone, Copy)]
        enum Reply {
            Respond {
                status: u16,
                location: Option<&'static str>,
                body: &'static str,
            },
            /// Accept the connection and never answer.
            Hang,
        }

        /// A loopback HTTP/1.1 server that answers the n-th request with the
        /// n-th reply (the last one repeats) and records each request.
        struct ScriptedServer {
            origin: String,
            requests: Arc<Mutex<Vec<String>>>,
        }

        impl ScriptedServer {
            async fn start(script: Vec<Reply>) -> ScriptedServer {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
                let origin = format!("http://{}", listener.local_addr().expect("local addr"));
                let requests = Arc::new(Mutex::new(Vec::new()));

                let recorded = Arc::clone(&requests);
                tokio::spawn(async move {
                    let mut index = 0;
                    loop {
                        let Ok((stream, _)) = listener.accept().await else {
                            return;
                        };
                        let reply = script[index.min(script.len() - 1)];
                        index += 1;
                        tokio::spawn(answer(stream, reply, Arc::clone(&recorded)));
                    }
                });

                ScriptedServer { origin, requests }
            }

            fn requests(&self) -> Vec<String> {
                self.requests.lock().expect("request log lock").clone()
            }
        }

        async fn answer(mut stream: TcpStream, reply: Reply, recorded: Arc<Mutex<Vec<String>>>) {
            let request = read_request(&mut stream).await;
            recorded.lock().expect("request log lock").push(request);

            let Reply::Respond { status, location, body } = reply else {
                // Hold the connection open until the client gives up.
                tokio::time::sleep(Duration::from_secs(3600)).await;
                return;
            };
            let location = location.map(|l| format!("Location: {l}\r\n")).unwrap_or_default();
            let response = format!(
                "HTTP/1.1 {status} X\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.expect("write response");
        }

        /// The request line, the headers the token1 flow depends on, and the
        /// body.
        async fn read_request(stream: &mut TcpStream) -> String {
            let mut buffer = Vec::new();
            let head_end = loop {
                let mut chunk = [0u8; 1024];
                let read = stream.read(&mut chunk).await.expect("read request");
                assert_ne!(read, 0, "client closed before sending a full request head");
                buffer.extend_from_slice(&chunk[..read]);
                if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                    break position + 4;
                }
            };

            let head = String::from_utf8(buffer[..head_end].to_vec()).expect("ASCII request head");
            let mut lines = head.split("\r\n");
            let mut summary = vec![lines.next().expect("request line").to_string()];
            let mut content_length = 0;
            for line in lines {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                let name = name.to_ascii_lowercase();
                let value = value.trim();
                if name == "content-length" {
                    content_length = value.parse().expect("numeric content-length");
                }
                if ["authorization", "content-type"].contains(&name.as_str()) {
                    summary.push(format!("{name}: {value}"));
                }
            }

            while buffer.len() < head_end + content_length {
                let mut chunk = [0u8; 1024];
                let read = stream.read(&mut chunk).await.expect("read request body");
                assert_ne!(read, 0, "client closed before sending the full body");
                buffer.extend_from_slice(&chunk[..read]);
            }
            let body = String::from_utf8(buffer[head_end..head_end + content_length].to_vec()).expect("ASCII body");
            if !body.is_empty() {
                summary.push(body);
            }

            summary.join("\n")
        }

        fn client(server: &ScriptedServer, timing: Token1Timing) -> AuthClient {
            let tracking_id = TrackingId::parse("1700000000000000000").expect("valid");
            let mut client = AuthClient::new(AccountRealm::Na, tracking_id);
            client.origin = server.origin.clone();
            client.game_token1 = timing;
            client
        }

        fn fast_timing() -> Token1Timing {
            Token1Timing {
                poll: PollPolicy { attempts: 3, interval: Duration::from_millis(10) },
                deadline: Duration::from_secs(5),
            }
        }

        fn session(realm: AccountRealm) -> StoredSession {
            StoredSession {
                realm,
                account_id: AccountId::new(1_034_874_630),
                nickname: Nickname::new("TestPlayer"),
                tokens: TokenPair {
                    access_token: AccessToken::new("access-token".to_string()),
                    exchange_code: ExchangeCode::from_raw("00112233445566778899AABBCCDDEEFF".to_string()),
                },
                expiry: TokenExpiry::Unknown,
            }
        }

        fn wows_na() -> GameTitle {
            GameTitle::wows(AccountRealm::Na).expect("WoWs serves NA")
        }

        #[tokio::test]
        async fn mints_after_polling_the_location() {
            let location = "/id/api/v2/account/credentials/create/token1/0a4a140c6d5af4859d5d7d495a9cbc7a/";
            let server = ScriptedServer::start(vec![
                Reply::Respond { status: 202, location: Some(location), body: "{}" },
                Reply::Respond { status: 202, location: None, body: "{}" },
                Reply::Respond { status: 200, location: None, body: r#"{"token": 48213907}"# },
            ])
            .await;

            let token = client(&server, fast_timing())
                .create_game_token1(&session(AccountRealm::Na), &wows_na())
                .await
                .expect("token1 minted");

            assert_eq!(token.expose_secret(), "48213907");
            insta::assert_snapshot!(server.requests().join("\n\n"), @r"
            POST /id/api/v2/account/credentials/create/token1/ HTTP/1.1
            authorization: Bearer access-token:00112233445566778899AABBCCDDEEFF
            content-type: application/x-www-form-urlencoded
            requested_for=wows&title_code=us.wows

            GET /id/api/v2/account/credentials/create/token1/0a4a140c6d5af4859d5d7d495a9cbc7a/ HTTP/1.1
            authorization: Bearer access-token:00112233445566778899AABBCCDDEEFF

            GET /id/api/v2/account/credentials/create/token1/0a4a140c6d5af4859d5d7d495a9cbc7a/ HTTP/1.1
            authorization: Bearer access-token:00112233445566778899AABBCCDDEEFF
            ");
        }

        #[tokio::test]
        async fn pending_past_the_poll_budget_times_out() {
            let server = ScriptedServer::start(vec![Reply::Respond {
                status: 202,
                location: Some("/id/api/v2/account/credentials/create/token1/pending/"),
                body: "{}",
            }])
            .await;

            let report = client(&server, fast_timing())
                .create_game_token1(&session(AccountRealm::Na), &wows_na())
                .await
                .expect_err("never settles");

            assert!(matches!(report.current_context(), AuthError::GameTokenTimeout { .. }));
            // The POST plus every poll the budget allows.
            assert_eq!(server.requests().len(), 4);
        }

        #[tokio::test]
        async fn stalled_server_hits_the_deadline() {
            let server = ScriptedServer::start(vec![Reply::Hang]).await;
            let timing = Token1Timing { deadline: Duration::from_millis(200), ..fast_timing() };

            let report = client(&server, timing)
                .create_game_token1(&session(AccountRealm::Na), &wows_na())
                .await
                .expect_err("server never answers");

            assert!(matches!(
                report.current_context(),
                AuthError::GameTokenTimeout { limit } if *limit == Duration::from_millis(200)
            ));
        }

        #[tokio::test]
        async fn rejected_session_is_session_expired() {
            let server = ScriptedServer::start(vec![Reply::Respond { status: 401, location: None, body: "{}" }]).await;

            let report = client(&server, fast_timing())
                .create_game_token1(&session(AccountRealm::Na), &wows_na())
                .await
                .expect_err("401");

            assert!(matches!(report.current_context(), AuthError::SessionExpired));
        }

        #[tokio::test]
        async fn foreign_realm_is_refused_before_any_request() {
            let server = ScriptedServer::start(vec![Reply::Hang]).await;
            let client = client(&server, fast_timing());
            let wows_eu = GameTitle::wows(AccountRealm::Eu).expect("WoWs serves EU");

            let from_session =
                client.create_game_token1(&session(AccountRealm::Eu), &wows_na()).await.expect_err("session realm");
            let from_title =
                client.create_game_token1(&session(AccountRealm::Na), &wows_eu).await.expect_err("title realm");

            for report in [from_session, from_title] {
                assert!(matches!(
                    report.current_context(),
                    AuthError::RealmMismatch { client: AccountRealm::Na, requested: AccountRealm::Eu }
                ));
            }
            assert!(server.requests().is_empty());
        }

        #[tokio::test]
        async fn foreign_poll_origin_never_receives_the_bearer() {
            let server = ScriptedServer::start(vec![Reply::Respond {
                status: 202,
                location: Some("http://127.0.0.2:9/id/api/v2/account/credentials/create/token1/abc/"),
                body: "{}",
            }])
            .await;

            let report = client(&server, fast_timing())
                .create_game_token1(&session(AccountRealm::Na), &wows_na())
                .await
                .expect_err("foreign Location");

            assert!(matches!(report.current_context(), AuthError::BadResponse { .. }));
            assert_eq!(server.requests().len(), 1);
        }
    }
}
