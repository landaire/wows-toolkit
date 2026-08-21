//! Wire-shape response models for WGNI's login flow
//! (WGNI-protocol.md 2.2-2.8). Field names follow the v2 OSS reference (the
//! only runnable source, see `client.rs`'s `WGNI_API_VERSION` doc comment).
//!
//! All fields are `Option` unless the report marks them CONFIRMED-present on
//! every response of that shape; nothing here is unwrapped-with-a-default.

use serde::Deserialize;

/// `.../oauth/token/challenge/` response.
#[derive(Debug, Deserialize)]
pub(crate) struct ChallengeResponse {
    pub pow: Option<PowChallengeWire>,
    /// Present (any shape) when WGNI additionally requires a CAPTCHA
    /// (WGNI-protocol.md 2.2). The payload shape is UNKNOWN (open question
    /// 2); this crate never tries to parse or solve it, only detects
    /// presence and fails closed.
    pub captcha: Option<serde_json::Value>,
    #[serde(rename = "captchaChallenges")]
    pub captcha_challenges: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PowChallengeWire {
    pub algorithm: PowAlgorithmWire,
    pub complexity: u8,
    pub timestamp: String,
    pub random_string: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PowAlgorithmWire {
    /// Checked against `"hashcash"` before solving (client.rs
    /// `request_challenge`): `pow::solve` only implements that algorithm.
    pub name: String,
    pub version: String,
    /// Wire field is literally spelled "resourse" (WG typo,
    /// WGNI-protocol.md 2.2).
    #[serde(rename = "resourse")]
    pub resource: String,
    pub extension: String,
}

/// `.../oauth/token/` success response (password grant or exchange grant).
#[derive(Debug, Deserialize)]
pub(crate) struct TokenSuccessResponse {
    pub access_token: String,
    /// Present on the password-grant response; the exchange-grant response
    /// only needs to confirm success (WGNI-protocol.md 2.5).
    pub user: Option<u64>,
    #[allow(dead_code)] // carried for completeness; not consumed yet
    pub token_type: Option<String>,
    pub expires_in: Option<u64>,
}

/// `.../oauth/token/` error response.
#[derive(Debug, Deserialize)]
pub(crate) struct TokenErrorResponse {
    #[allow(dead_code)] // classification uses error_description; error is diagnostic only
    pub error: Option<String>,
    pub error_description: Option<String>,
    pub twofactor_token: Option<String>,
}

/// `.../account/info/` response.
#[derive(Debug, Deserialize)]
pub(crate) struct AccountInfoResponse {
    pub sub: u64,
    pub nickname: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_response_parses_pow_with_resourse_typo() {
        let json = r#"{
            "pow": {
                "algorithm": {
                    "name": "hashcash",
                    "version": "1",
                    "resourse": "id/api/v2/account/credentials/create/oauth/token/",
                    "extension": ""
                },
                "complexity": 4,
                "timestamp": "1700000000",
                "random_string": "abc123"
            }
        }"#;
        let parsed: ChallengeResponse = serde_json::from_str(json).expect("parses");
        let pow = parsed.pow.expect("pow present");
        assert_eq!(pow.algorithm.resource, "id/api/v2/account/credentials/create/oauth/token/");
        assert_eq!(pow.algorithm.name, "hashcash");
        assert_eq!(pow.algorithm.version, "1");
        assert_eq!(pow.complexity, 4);
        assert_eq!(pow.timestamp, "1700000000");
        assert_eq!(pow.random_string, "abc123");
        assert!(parsed.captcha.is_none());
        assert!(parsed.captcha_challenges.is_none());
    }

    #[test]
    fn challenge_response_detects_captcha_presence() {
        let json = r#"{"pow": null, "captcha": {"site_key": "abc"}}"#;
        let parsed: ChallengeResponse = serde_json::from_str(json).expect("parses");
        assert!(parsed.captcha.is_some());
    }

    #[test]
    fn challenge_response_detects_captcha_challenges_presence() {
        let json = r#"{"pow": null, "captchaChallenges": [{"id": "1"}]}"#;
        let parsed: ChallengeResponse = serde_json::from_str(json).expect("parses");
        assert!(parsed.captcha_challenges.is_some());
    }

    #[test]
    fn token_success_parses_with_expires_in() {
        let json = r#"{"access_token": "tok", "user": 697769785, "token_type": "Bearer", "expires_in": 3600}"#;
        let parsed: TokenSuccessResponse = serde_json::from_str(json).expect("parses");
        assert_eq!(parsed.access_token, "tok");
        assert_eq!(parsed.user, Some(697769785));
        assert_eq!(parsed.expires_in, Some(3600));
    }

    #[test]
    fn token_success_parses_without_expires_in() {
        let json = r#"{"access_token": "tok", "user": 697769785}"#;
        let parsed: TokenSuccessResponse = serde_json::from_str(json).expect("parses");
        assert_eq!(parsed.access_token, "tok");
        assert_eq!(parsed.expires_in, None);
    }

    #[test]
    fn token_error_parses_twofactor_required() {
        let json =
            r#"{"error": "invalid_grant", "error_description": "twofactor_required", "twofactor_token": "tft-123"}"#;
        let parsed: TokenErrorResponse = serde_json::from_str(json).expect("parses");
        assert_eq!(parsed.error_description.as_deref(), Some("twofactor_required"));
        assert_eq!(parsed.twofactor_token.as_deref(), Some("tft-123"));
    }

    #[test]
    fn account_info_parses() {
        let json = r#"{"sub": 697769785, "nickname": "TestPlayer"}"#;
        let parsed: AccountInfoResponse = serde_json::from_str(json).expect("parses");
        assert_eq!(parsed.sub, 697769785);
        assert_eq!(parsed.nickname, "TestPlayer");
    }
}
