//! Keccak-512 hashcash proof-of-work solver.

use std::time::Duration;
use std::time::Instant;

use sha3::Digest;
use sha3::Keccak512;
use thiserror::Error;

/// A WGNI PoW challenge, parsed into its clean form.
///
/// The wire field for `resource` is literally spelled "resourse" (a WG
/// typo); the serde rename for that wire shape lives on the client's wire
/// model, not this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowChallenge {
    pub algorithm_version: String,
    pub complexity: u8,
    pub timestamp: String,
    pub resource: String,
    pub extension: String,
    pub random_string: String,
}

/// A solved `pow_number` satisfying a `PowChallenge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowSolution(u64);

impl PowSolution {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Error)]
pub enum PowError {
    #[error("proof-of-work challenge could not be solved before the timeout")]
    Timeout,
}

fn candidate(challenge: &PowChallenge, pow_number: u64) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}",
        challenge.algorithm_version,
        challenge.complexity,
        challenge.timestamp,
        challenge.resource,
        challenge.extension,
        challenge.random_string,
        pow_number
    )
}

fn leading_zero_hex_chars(digest: &[u8]) -> u8 {
    let mut count: u8 = 0;
    for byte in digest {
        if *byte == 0 {
            count += 2;
            continue;
        }
        if byte & 0xf0 == 0 {
            count += 1;
        }
        break;
    }
    count
}

/// Brute-forces `pow_number` from 0 until the Keccak-512 hex digest of the
/// candidate string's Keccak-512 hex digest has at least `complexity`
/// leading zero hex characters, or `timeout` elapses.
///
/// Single-threaded for this phase; multi-worker search is a documented
/// follow-up.
pub fn solve(challenge: &PowChallenge, timeout: Duration) -> Result<PowSolution, PowError> {
    let deadline = Instant::now() + timeout;
    let mut pow_number: u64 = 0;
    loop {
        let digest = Keccak512::digest(candidate(challenge, pow_number).as_bytes());
        if leading_zero_hex_chars(&digest) >= challenge.complexity {
            return Ok(PowSolution(pow_number));
        }
        if Instant::now() >= deadline {
            return Err(PowError::Timeout);
        }
        pow_number += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_hex(challenge: &PowChallenge, pow_number: u64) -> String {
        let digest = Keccak512::digest(candidate(challenge, pow_number).as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn small_challenge(complexity: u8) -> PowChallenge {
        PowChallenge {
            algorithm_version: "1".to_string(),
            complexity,
            timestamp: "1700000000".to_string(),
            resource: "id/api/v3/account/credentials/create/oauth/token/".to_string(),
            extension: "".to_string(),
            random_string: "abc123".to_string(),
        }
    }

    #[test]
    fn solves_complexity_1_and_verifies_independently() {
        let challenge = small_challenge(1);
        let solution = solve(&challenge, Duration::from_secs(5)).expect("solves quickly");
        let hex = digest_hex(&challenge, solution.get());
        assert!(hex.starts_with('0'), "digest {hex} does not have 1 leading zero hex char");
    }

    #[test]
    fn solves_complexity_2_and_verifies_independently() {
        let challenge = small_challenge(2);
        let solution = solve(&challenge, Duration::from_secs(5)).expect("solves quickly");
        let hex = digest_hex(&challenge, solution.get());
        assert!(hex.starts_with("00"), "digest {hex} does not have 2 leading zero hex chars");
    }

    #[test]
    fn solves_complexity_3_and_verifies_independently() {
        let challenge = small_challenge(3);
        let solution = solve(&challenge, Duration::from_secs(5)).expect("solves quickly");
        let hex = digest_hex(&challenge, solution.get());
        assert!(hex.starts_with("000"), "digest {hex} does not have 3 leading zero hex chars");
    }

    #[test]
    fn solving_is_deterministic() {
        let challenge = small_challenge(2);
        let first = solve(&challenge, Duration::from_secs(5)).expect("solves quickly");
        let second = solve(&challenge, Duration::from_secs(5)).expect("solves quickly");
        assert_eq!(first, second);
    }

    #[test]
    fn high_complexity_times_out() {
        let challenge = small_challenge(8);
        let result = solve(&challenge, Duration::from_millis(1));
        assert!(matches!(result, Err(PowError::Timeout)));
    }
}
