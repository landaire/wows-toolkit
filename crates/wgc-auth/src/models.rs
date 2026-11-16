//! Wire-shape response models for WGNI's login flow, derived from the live
//! endpoint's own responses (see `client.rs`'s `WGNI_API_VERSION`).
//!
//! Fields observed as always-present are required; anything not seen on every
//! response of that shape is `Option`. Nothing here is unwrapped with a
//! default.

use serde::Deserialize;
use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

/// WGNI sends `algorithm.version` and `timestamp` as JSON integers on the live
/// EU endpoint, but older/other responses may send strings. Accept either and
/// normalize to the string form the PoW candidate concatenation needs.
fn de_stringish<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrInt;
    impl serde::de::Visitor<'_> for StringOrInt {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or integer")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    deserializer.deserialize_any(StringOrInt)
}

/// `.../oauth/token/challenge/` response.
#[derive(Debug, Deserialize)]
pub(crate) struct ChallengeResponse {
    pub pow: Option<PowChallengeWire>,
    /// Present (any shape) when WGNI additionally requires a CAPTCHA. The
    /// payload shape is not modeled; this crate never tries to parse or solve
    /// it, only detects its presence and fails closed.
    pub captcha: Option<serde_json::Value>,
    #[serde(rename = "captchaChallenges")]
    pub captcha_challenges: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PowChallengeWire {
    pub algorithm: PowAlgorithmWire,
    pub complexity: u8,
    #[serde(deserialize_with = "de_stringish")]
    pub timestamp: String,
    pub random_string: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PowAlgorithmWire {
    /// Checked against `"hashcash"` before solving (client.rs
    /// `request_challenge`): `pow::solve` only implements that algorithm.
    pub name: String,
    #[serde(deserialize_with = "de_stringish")]
    pub version: String,
    /// The live endpoint spells this field "resourse" (a Wargaming typo).
    #[serde(rename = "resourse")]
    pub resource: String,
    pub extension: String,
}

/// `.../oauth/token/` success response (password grant or exchange grant).
#[derive(Debug, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct TokenSuccessResponse {
    pub access_token: String,
    /// Present on the password-grant response; the exchange-grant response
    /// only needs to confirm success.
    pub user: Option<u64>,
    #[allow(dead_code)] // carried for completeness; not consumed yet
    pub token_type: Option<String>,
    pub expires_in: Option<u64>,
}

/// `.../oauth/token/` error response.
#[derive(Debug, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct TokenErrorResponse {
    /// WG's OAuth machine code; classification tries this first (see
    /// `client.rs::classify_token_error`).
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

/// `.../create/token1/` request: a form body, as `Auth::QuerySPAToken1`
/// builds it in WGC's game_center.dll.
pub(crate) struct Token1Request<'a> {
    /// WGC's `WebEndpointData::gameName`; `wows` for WoWs.
    pub requested_for: &'a str,
    /// WGC's `WebEndpointData::titleId`. WGC omits the field when it is
    /// empty; this crate always has one.
    pub title_code: &'a str,
}

impl Token1Request<'_> {
    pub fn form_body(&self) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("requested_for", self.requested_for)
            .append_pair("title_code", self.title_code)
            .finish()
    }
}

/// `.../create/token1/<id>/` settled response. WGC reads `token` as a JSON
/// integer and formats it in decimal; a string is accepted too.
#[derive(Debug, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct Token1Response {
    #[serde(deserialize_with = "de_stringish")]
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_response_parses_live_pow_with_integer_fields() {
        // Shape captured from the live EU endpoint: version and timestamp are
        // JSON integers, the resource field is spelled "resourse", and a
        // trailing "type" field is present and ignored.
        let json = r#"{
            "pow": {
                "random_string": "sPRKpI4WyRa0oAsE",
                "complexity": 3,
                "timestamp": 1787286513,
                "algorithm": { "name": "hashcash", "version": 1, "resourse": "wgni", "extension": "" },
                "type": "pow"
            }
        }"#;
        let parsed: ChallengeResponse = serde_json::from_str(json).expect("parses");
        let pow = parsed.pow.expect("pow present");
        assert_eq!(pow.algorithm.resource, "wgni");
        assert_eq!(pow.algorithm.name, "hashcash");
        assert_eq!(pow.algorithm.version, "1");
        assert_eq!(pow.complexity, 3);
        assert_eq!(pow.timestamp, "1787286513");
        assert_eq!(pow.random_string, "sPRKpI4WyRa0oAsE");
        assert!(parsed.captcha.is_none());
        assert!(parsed.captcha_challenges.is_none());
    }

    #[test]
    fn challenge_response_also_accepts_string_pow_fields() {
        let json = r#"{"pow": {"algorithm": {"name": "hashcash", "version": "1", "resourse": "wgni",
            "extension": ""}, "complexity": 4, "timestamp": "1700000000", "random_string": "abc"}}"#;
        let pow = serde_json::from_str::<ChallengeResponse>(json).expect("parses").pow.expect("pow");
        assert_eq!(pow.algorithm.version, "1");
        assert_eq!(pow.timestamp, "1700000000");
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

    #[test]
    fn token1_request_form_body() {
        let request = Token1Request { requested_for: "wows", title_code: "us.wows" };
        insta::assert_snapshot!(request.form_body(), @"requested_for=wows&title_code=us.wows");
    }

    #[test]
    fn token1_response_parses_integer_and_string_tokens() {
        let integer: Token1Response = serde_json::from_str(r#"{"token": 48213907}"#).expect("parses");
        let string: Token1Response = serde_json::from_str(r#"{"token": "48213907"}"#).expect("parses");
        insta::assert_debug_snapshot!((integer, string), @r#"
        (
            Token1Response {
                token: "48213907",
            },
            Token1Response {
                token: "48213907",
            },
        )
        "#);
    }
}
