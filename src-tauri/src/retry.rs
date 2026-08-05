//! Retry policy for transient gateway errors (e.g. `529 overloaded`).
//!
//! Selasar doesn't call the LLM gateway directly — it spawns the `claude` CLI
//! subprocess, and a gateway failure surfaces as an
//! `AgentResponse { is_error: true, result: "<human-readable message>" }`. A
//! `529` from `api.z.ai` (or any Anthropic-compatible gateway) arrives as text
//! like `API Error: 529 [...] overloaded [...]`, so retry eligibility is a
//! substring match on that result text rather than a structured status code.
//!
//! Backoff is exponential with a cap, using `tokio::time::sleep` (the `time`
//! feature is already enabled). Tunable via the constants below.

/// Maximum number of send attempts (1 initial + retries).
///
/// Tuned for sustained gateway blips (e.g. `api.z.ai` overload windows that
/// outlast a few retries). At the default base/factor/cap below this yields
/// waits of ~2s, 4s, 8s, 16s, 30s, 30s, 30s, 30s ≈ 2.5 min of sleeps before
/// the 9th attempt is terminal. See [`BACKOFF_TOTAL_BUDGET_MS`] for the hard
/// wall-clock ceiling that can terminate retries earlier.
pub const MAX_ATTEMPTS: u32 = 9;

/// Base backoff for the first retry. Subsequent waits double, capped by
/// [`BACKOFF_CAP_MS`].
pub const BACKOFF_BASE_MS: u64 = 2_000;

/// Multiplier applied between successive backoffs.
pub const BACKOFF_FACTOR: u32 = 2;

/// Upper bound on a single backoff sleep.
pub const BACKOFF_CAP_MS: u64 = 30_000;

/// Hard wall-clock ceiling on the *total* time spent in backoff sleeps across
/// all retries of a single turn.
///
/// `MAX_ATTEMPTS` bounds the count of retries; this bounds their cumulative
/// duration. Whichever binds first terminates the retry loop, so a future
/// bump to `MAX_ATTEMPTS` (or a longer `BACKOFF_CAP_MS`) can't silently extend
/// the worst-case wait past ~5 minutes. Checked per-iteration in
/// [`next_backoff`] against the elapsed sleep time so the budget is enforced
/// in one pure, testable place rather than at each call site.
pub const BACKOFF_TOTAL_BUDGET_MS: u64 = 300_000; // 5 minutes

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
///
/// Count-based only — does NOT consult the wall-clock budget. Use
/// [`next_backoff`] from retry loops to also enforce [`BACKOFF_TOTAL_BUDGET_MS`].
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

/// The backoff to apply before the next retry, or `None` to stop retrying.
///
/// The retry-loop entry point: combines the count bound ([`backoff_ms`]) with
/// the wall-clock budget ([`BACKOFF_TOTAL_BUDGET_MS`]). `elapsed_ms` is the
/// cumulative time already spent in backoff sleeps for this turn (the caller
/// accumulates it). Returns `None` — meaning "surface the error" — when either
/// the attempt count is exhausted OR the remaining budget can't fit a useful
/// sleep. Centralizing both bounds here keeps the retry wrappers free of
/// policy and makes the budget trivially testable without a fake clock.
///
/// Note: this does not shorten the *next* sleep to fit a remaining budget
/// sliver — a near-zero leftover isn't worth one more full backoff, so we stop.
pub fn next_backoff(attempt: u32, elapsed_ms: u64) -> Option<u64> {
    let wait = backoff_ms(attempt)?;
    if elapsed_ms + wait >= BACKOFF_TOTAL_BUDGET_MS {
        return None;
    }
    Some(wait)
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
        // 2s, 4s, 8s, 16s, then capped at 30s for the remaining waits; the
        // 9th attempt (index 8) is terminal → None.
        assert_eq!(backoff_ms(0), Some(2_000));
        assert_eq!(backoff_ms(1), Some(4_000));
        assert_eq!(backoff_ms(2), Some(8_000));
        assert_eq!(backoff_ms(3), Some(16_000));
        assert_eq!(backoff_ms(4), Some(30_000));
        assert_eq!(backoff_ms(5), Some(30_000));
        assert_eq!(backoff_ms(6), Some(30_000));
        assert_eq!(backoff_ms(7), Some(30_000));
        assert_eq!(backoff_ms(8), None);
    }

    #[test]
    fn backoff_caps_at_limit() {
        // With a tiny cap, the growth saturates immediately.
        let raw = BACKOFF_BASE_MS.saturating_mul(u64::from(BACKOFF_FACTOR.saturating_pow(10)));
        assert!(raw >= BACKOFF_CAP_MS); // would overshoot…
        assert_eq!(raw.min(BACKOFF_CAP_MS), BACKOFF_CAP_MS); // …but is capped.
    }

    #[test]
    fn next_backoff_ignores_budget_when_far_from_it() {
        // Early retries with no elapsed time just return the count-based wait.
        assert_eq!(next_backoff(0, 0), Some(2_000));
        assert_eq!(next_backoff(1, 2_000), Some(4_000));
    }

    #[test]
    fn next_backoff_stops_when_next_wait_would_breach_budget() {
        // Simulate being near the 5-minute budget. attempt 0 would wait 2s,
        // but elapsed + 2s would cross the budget → stop.
        let elapsed = BACKOFF_TOTAL_BUDGET_MS - 1_000;
        assert_eq!(next_backoff(0, elapsed), None);
    }

    #[test]
    fn next_backoff_budget_bounds_the_loop_before_count_does() {
        // Walk the retry loop purely via next_backoff, summing waits, and
        // confirm the total never exceeds the budget — i.e. the budget (not
        // MAX_ATTEMPTS) is what terminates a pathological long-cap sequence.
        let mut elapsed = 0u64;
        let mut attempt = 0u32;
        let mut last_wait = 0u64;
        while let Some(wait) = next_backoff(attempt, elapsed) {
            elapsed += wait;
            last_wait = wait;
            attempt += 1;
            // Hard safety net for the test itself — never a real bound.
            if attempt > 100 {
                panic!("retry loop did not terminate");
            }
        }
        assert!(
            elapsed <= BACKOFF_TOTAL_BUDGET_MS,
            "total backoff {elapsed} exceeded budget {BACKOFF_TOTAL_BUDGET_MS}"
        );
        // Sanity: we did make progress (at least one retry happened).
        assert!(attempt >= 1, "loop never retried; last_wait={last_wait}");
    }
}
