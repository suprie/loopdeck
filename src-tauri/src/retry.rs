//! Retry policy for transient gateway errors (e.g. `529 overloaded`).
//!
//! LoopDeck doesn't call the LLM gateway directly — it spawns the `claude` CLI
//! subprocess, and a gateway failure surfaces as an
//! `AgentResponse { is_error: true, result: "<human-readable message>" }`. A
//! `529` from `api.z.ai` (or any Anthropic-compatible gateway) arrives as text
//! like `API Error: 529 [...] overloaded [...]`, so retry eligibility is a
//! substring match on that result text rather than a structured status code.
//!
//! Backoff is exponential with a cap, using `tokio::time::sleep` (the `time`
//! feature is already enabled). Tunable via the constants below.

/// Maximum number of send attempts (1 initial + retries).
pub const MAX_ATTEMPTS: u32 = 4;

/// Base backoff for the first retry. Subsequent waits double, capped by
/// [`BACKOFF_CAP`]. With the defaults this yields ~2s, 4s, 8s before giving up.
pub const BACKOFF_BASE_MS: u64 = 2_000;

/// Multiplier applied between successive backoffs.
pub const BACKOFF_FACTOR: u32 = 2;

/// Upper bound on a single backoff sleep.
pub const BACKOFF_CAP_MS: u64 = 30_000;

/// Decide whether a provider error result is transient and worth retrying.
///
/// Matches the gateway-overload signal in the `claude` CLI's error text:
/// `529` (Anthropic's "overloaded" status) or the word `overloaded`. Case-
/// insensitive so it survives minor wording changes across CLI/gateway
/// versions. Returns `false` for non-transient failures (401 auth, 400 bad
/// request) — those won't fix themselves on retry and should surface
/// immediately.
///
/// # Examples
///
/// - `"API Error: 529 [1305][The service may be temporarily overloaded]"` → `true`
/// - `"the service is overloaded, try later"` → `true`
/// - `"Not logged in"` → `false` (auth failure, non-transient)
/// - `"API Error: 401 unauthorized"` → `false`
///
/// Behavior is covered exhaustively by the `retry::tests` module.
pub fn is_overloaded(result: &str) -> bool {
    let lower = result.to_ascii_lowercase();
    lower.contains("529") || lower.contains("overloaded")
}

/// Backoff duration in milliseconds for a given 0-based attempt index.
///
/// `attempt` is the number of the attempt that just failed (0 for the first),
/// so the wait *before* the next try grows exponentially: base, base*2, …,
/// capped. Returns `None` when there are no attempts left to retry into
/// (i.e. `attempt + 1 >= MAX_ATTEMPTS`).
pub fn backoff_ms(attempt: u32) -> Option<u64> {
    if attempt + 1 >= MAX_ATTEMPTS {
        return None;
    }
    // attempt=0 → base; each subsequent attempt multiplies by BACKOFF_FACTOR,
    // saturating at BACKOFF_CAP_MS. `saturating_mul` guards the 32-bit multiply
    // long before the cap would bind, but the min() is what actually enforces
    // it.
    let raw = BACKOFF_BASE_MS.saturating_mul(u64::from(BACKOFF_FACTOR.saturating_pow(attempt)));
    Some(raw.min(BACKOFF_CAP_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_real_529_message() {
        // The exact error string that prompted this feature.
        let err = "API Error: 529 [1305][The service may be temporarily \
                   overloaded, please try again later]\
                   [20260710144048bbd3fcea3b5f4d0f]. This is a server-side \
                   issue, usually temporary — try again in a moment.";
        assert!(is_overloaded(err));
    }

    #[test]
    fn matches_variants() {
        assert!(is_overloaded("529 overloaded"));
        assert!(is_overloaded("Overloaded"));
        assert!(is_overloaded("error code: 529"));
        assert!(is_overloaded("API Error: 529"));
    }

    #[test]
    fn ignores_non_transient_errors() {
        // Auth/config/usage failures must NOT be retried — they won't recover.
        assert!(!is_overloaded("Not logged in · Please run /login"));
        assert!(!is_overloaded("API Error: 401 unauthorized"));
        assert!(!is_overloaded("API Error: 400 bad request"));
        assert!(!is_overloaded(""));
    }

    #[test]
    fn backoff_progression_and_cap() {
        // 2s, 4s, 8s; the 4th attempt (index 3) is terminal → None.
        assert_eq!(backoff_ms(0), Some(2_000));
        assert_eq!(backoff_ms(1), Some(4_000));
        assert_eq!(backoff_ms(2), Some(8_000));
        assert_eq!(backoff_ms(3), None);
    }

    #[test]
    fn backoff_caps_at_limit() {
        // With a tiny cap, the growth saturates immediately.
        let raw = BACKOFF_BASE_MS.saturating_mul(u64::from(BACKOFF_FACTOR.saturating_pow(10)));
        assert!(raw >= BACKOFF_CAP_MS); // would overshoot…
        assert_eq!(raw.min(BACKOFF_CAP_MS), BACKOFF_CAP_MS); // …but is capped.
    }
}
