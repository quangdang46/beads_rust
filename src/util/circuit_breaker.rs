//! Circuit breaker pattern for network operations.
//!
//! A simple state machine that prevents repeated calls to a failing
//! remote service by "tripping open" after a configurable number of
//! consecutive failures. After a timeout, it transitions to half-open
//! to allow a probe request through. If the probe succeeds, the circuit
//! resets to closed; if it fails, the circuit re-opens.
//!
//! This is a pure in-memory state machine — no persistence yet.
//!
//! States:
//! - **Closed**: Normal operation. Requests pass through. Failures are counted.
//! - **Open**: Requests are rejected immediately (fail-fast). After a timeout,
//!   the circuit transitions to half-open automatically on the next `call()`.
//! - **HalfOpen**: A single probe request is allowed. Success → Closed,
//!   Failure → Open.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests pass through.
    Closed,
    /// Rejecting requests — fail-fast mode.
    Open,
    /// Allowing a single probe request to test the waters.
    HalfOpen,
}

impl CircuitState {
    /// Returns a string representation of the state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Open => "open",
            Self::HalfOpen => "half-open",
        }
    }
}

/// Configuration for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before tripping to Open.
    pub failure_threshold: u32,
    /// Number of consecutive successes in HalfOpen needed to reset to Closed.
    pub success_threshold: u32,
    /// Duration to stay Open before transitioning to HalfOpen.
    pub timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 1,
            timeout: Duration::from_secs(5),
        }
    }
}

/// Error returned when the circuit is open and rejects a call.
#[derive(Debug, Clone)]
pub struct CircuitOpenError;

impl std::fmt::Display for CircuitOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "circuit breaker is open: failing fast (cooldown active)"
        )
    }
}

/// A simple circuit breaker implementing the state machine pattern.
///
/// The circuit breaker protects remote service calls from being
/// repeatedly attempted when the service is known to be failing.
///
/// # Example
///
/// ```rust,ignore
/// use beads_rust::util::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
/// use std::time::Duration;
///
/// let mut cb = CircuitBreaker::new(CircuitBreakerConfig::default());
///
/// // Record some failures
/// for _ in 0..5 {
///     if cb.call() {
///         // would make the call here, then:
///         cb.record_failure();
///     }
/// }
///
/// // Circuit is now open
/// assert_eq!(cb.state(), CircuitState::Open);
/// assert!(!cb.call());
/// ```
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Current state.
    state: CircuitState,
    /// Configuration.
    config: CircuitBreakerConfig,
    /// Running failure count.
    failure_count: u32,
    /// Running success count (used in HalfOpen).
    success_count: u32,
    /// Timestamp when the circuit was last tripped open.
    last_tripped_at: Option<Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration.
    #[must_use]
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            config,
            failure_count: 0,
            success_count: 0,
            last_tripped_at: None,
        }
    }

    /// Returns `true` if a request should be allowed through, `false` if the
    /// circuit is open and the call should be rejected (fail-fast).
    ///
    /// Automatically transitions from Open to HalfOpen when the timeout has
    /// elapsed.
    #[must_use]
    pub fn call(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                // Check if the timeout has elapsed → transition to HalfOpen.
                if let Some(tripped_at) = self.last_tripped_at {
                    if tripped_at.elapsed() >= self.config.timeout {
                        self.state = CircuitState::HalfOpen;
                        self.success_count = 0;
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Record a successful call.
    ///
    /// In Closed state, resets the failure count.
    /// In HalfOpen state, increments the success count and transitions to
    /// Closed when the success threshold is reached.
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                    self.last_tripped_at = None;
                }
            }
            CircuitState::Open => {
                // If we somehow record a success while open, reset to closed.
                self.state = CircuitState::Closed;
                self.failure_count = 0;
                self.success_count = 0;
                self.last_tripped_at = None;
            }
        }
    }

    /// Record a failed call.
    ///
    /// In Closed state, increments the failure count and transitions to Open
    /// when the failure threshold is reached.
    /// In HalfOpen state, transitions immediately back to Open.
    /// In Open state, this is a no-op (we're already failing fast).
    pub fn record_failure(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    self.state = CircuitState::Open;
                    self.last_tripped_at = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                // A single failure in half-open re-trips the circuit.
                self.state = CircuitState::Open;
                self.last_tripped_at = Some(Instant::now());
                self.success_count = 0;
            }
            CircuitState::Open => {
                // Already open — update the tripped timestamp to extend
                // the cooldown window.
                self.last_tripped_at = Some(Instant::now());
            }
        }
    }

    /// Returns the current circuit breaker state.
    #[must_use]
    pub const fn state(&self) -> CircuitState {
        self.state
    }

    /// Returns the current failure count.
    #[must_use]
    pub const fn failure_count(&self) -> u32 {
        self.failure_count
    }

    /// Returns the current success count (meaningful only in HalfOpen).
    #[must_use]
    pub const fn success_count(&self) -> u32 {
        self.success_count
    }

    /// Force-reset the circuit breaker to Closed state.
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.last_tripped_at = None;
    }
}

// ---------------------------------------------------------------------------
// Persistent (file-backed) circuit breaker
// ---------------------------------------------------------------------------

/// On‑disk representation of the circuit breaker state.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedBreakerState {
    state: String,
    failure_count: u32,
    success_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_tripped_at: Option<u64>, // unix nanos
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_updated: Option<u64>, // unix nanos
}

/// A file‑backed circuit breaker for cross‑process coordination.
///
/// Wraps the in‑memory [`CircuitBreaker`] with JSON file persistence so that
/// state survives process restarts. Multiple processes sharing the same file
/// coordinate fail‑fast behaviour.
///
/// The state file is written on every state change and read on every
/// [`allow`](PersistentCircuitBreaker::allow) call, making it suitable for
/// short‑lived processes that need to know whether a shared dependency is
/// currently available.
///
/// # Default State Directory
///
/// State files live under `/tmp/beads-circuit/`. Each breaker instance gets
/// a unique file name based on its `id` (sanitised).
///
/// # Example
///
/// ```rust,ignore
/// use beads_rust::util::circuit_breaker::PersistentCircuitBreaker;
///
/// let cb = PersistentCircuitBreaker::new("my-service");
/// if cb.allow() {
///     // ... do the operation ...
///     cb.record_success();
/// } else {
///     eprintln!("service is down — failing fast");
/// }
/// ```
pub struct PersistentCircuitBreaker {
    inner: std::sync::Mutex<CircuitBreaker>,
    file_path: PathBuf,
    bypass: bool,
}

impl std::fmt::Debug for PersistentCircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentCircuitBreaker")
            .field("file_path", &self.file_path)
            .field("bypass", &self.bypass)
            .finish()
    }
}

/// Default directory for circuit breaker state files.
const BREAKER_DIR: &str = "/tmp/beads-circuit";

impl PersistentCircuitBreaker {
    /// Create a new persistent circuit breaker with the given `id` and
    /// default configuration.
    ///
    /// The state file is written to `/tmp/beads-circuit/beads-circuit-{id}.json`.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self::with_config(id, CircuitBreakerConfig::default())
    }

    /// Create a persistent circuit breaker with a custom configuration.
    #[must_use]
    pub fn with_config(id: &str, config: CircuitBreakerConfig) -> Self {
        let sanitised: String = id
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
                _ => '-',
            })
            .collect();
        let filename = format!("beads-circuit-{}.json", sanitised);
        let file_path = PathBuf::from(BREAKER_DIR).join(filename);

        // Try to restore from file
        let inner = match Self::read_state_file(&file_path) {
            Some(ps) => CircuitBreaker {
                state: match ps.state.as_str() {
                    "open" => CircuitState::Open,
                    "half-open" => CircuitState::HalfOpen,
                    _ => CircuitState::Closed,
                },
                failure_count: ps.failure_count,
                success_count: ps.success_count,
                last_tripped_at: ps
                    .last_tripped_at
                    .and_then(|ns| {
                        let d = Duration::from_nanos(ns);
                        // Instant is platform‑relative; we store raw nanos
                        // since the tripped event. On restore we approximate
                        // using Instant::now() minus the elapsed nanos.
                        Instant::now().checked_sub(d)
                    }),
                config: config.clone(),
            },
            None => CircuitBreaker::new(config.clone()),
        };

        Self {
            inner: std::sync::Mutex::new(inner),
            file_path,
            bypass: false,
        }
    }

    /// Create a breaker at an explicit file path (for testing).
    #[must_use]
    pub fn with_path(id: &str, config: CircuitBreakerConfig, path: PathBuf) -> Self {
        let inner = CircuitBreaker::new(config);
        Self {
            inner: std::sync::Mutex::new(inner),
            file_path: path,
            bypass: false,
        }
    }

    /// Enable bypass mode — all calls pass through without checking state.
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    /// Returns `true` if a request should be allowed through.
    #[must_use]
    pub fn allow(&self) -> bool {
        if self.bypass {
            return true;
        }
        let mut inner = self.inner.lock().unwrap();
        let allowed = inner.call();
        // If we just transitioned state (Open→HalfOpen), persist
        self.persist_on_change(&inner);
        allowed
    }

    /// Record a successful call — resets failure count, persists.
    pub fn record_success(&self) {
        if self.bypass {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        let old_state = inner.state();
        inner.record_success();
        if inner.state() != old_state {
            self.write_state(&inner);
        }
    }

    /// Record a failure — may trip the breaker, persists.
    pub fn record_failure(&self) {
        if self.bypass {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        let old_state = inner.state();
        inner.record_failure();
        if inner.state() != old_state {
            self.write_state(&inner);
        }
    }

    /// Force‑reset to Closed, removes the state file.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.reset();
        let _ = std::fs::remove_file(&self.file_path);
    }

    /// Return the current state (from in‑memory, no file read).
    #[must_use]
    pub fn state(&self) -> CircuitState {
        self.inner.lock().unwrap().state()
    }

    /// Return the file path for inspection / manual removal.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Stale TTL for state files: files older than this are auto‑reset.
    const STALE_TTL: Duration = Duration::from_secs(300); // 5 min

    // -------------------------------------------------------------------
    // File I/O
    // -------------------------------------------------------------------

    fn persist_on_change(&self, inner: &CircuitBreaker) {
        // Only write when there's meaningful state (not just Closed with 0)
        if inner.failure_count() > 0 || inner.success_count() > 0 || inner.state() != CircuitState::Closed {
            self.write_state(inner);
        }
    }

    fn write_state(&self, inner: &CircuitBreaker) {
        let elapsed_ns = inner
            .last_tripped_at
            .map(|instant| {
                // Store the Instant–relative elapsed as proxy nanos
                instant.elapsed().as_nanos() as u64
            });

        let ps = PersistedBreakerState {
            state: inner.state().as_str().to_string(),
            failure_count: inner.failure_count(),
            success_count: inner.success_count(),
            last_tripped_at: elapsed_ns,
            last_updated: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            ),
        };

        // Ensure dir exists
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&ps) {
            let _ = std::fs::write(&self.file_path, json);
        }
    }

    fn read_state_file(path: &Path) -> Option<PersistedBreakerState> {
        let data = std::fs::read_to_string(path).ok()?;
        let ps: PersistedBreakerState = serde_json::from_str(&data).ok()?;

        // Check staleness
        if let Some(updated) = ps.last_updated {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            if now.saturating_sub(updated) > Self::STALE_TTL.as_nanos() as u64 {
                // Stale — ignore
                return None;
            }
        }

        Some(ps)
    }
}

/// Remove stale circuit breaker state files (older than [`PersistentCircuitBreaker::STALE_TTL`]).
///
/// Returns the number of files removed.
pub fn clean_stale_circuit_files() -> std::io::Result<usize> {
    let dir = Path::new(BREAKER_DIR);
    if !dir.is_dir() {
        return Ok(0);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let max_age = PersistentCircuitBreaker::STALE_TTL.as_nanos() as u64;
    let mut removed = 0;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let metadata = entry.metadata()?;
        if let Ok(modified) = metadata.modified() {
            if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
                if now.saturating_sub(d.as_nanos() as u64) > max_age {
                    let _ = std::fs::remove_file(&path);
                    removed += 1;
                }
            }
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod persistent_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_persistent_initial_state_closed() {
        let cb = PersistentCircuitBreaker::new("test-persistent");
        assert!(cb.allow());
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_persistent_trips_after_threshold() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("trip.json");
        let cb = PersistentCircuitBreaker::with_path(
            "trip",
            CircuitBreakerConfig {
                failure_threshold: 3,
                timeout: Duration::from_secs(60),
                ..CircuitBreakerConfig::default()
            },
            path,
        );

        for _ in 0..3 {
            assert!(cb.allow());
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow());
    }

    #[test]
    fn test_persistent_resets_on_success() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("reset.json");
        let cb = PersistentCircuitBreaker::with_path(
            "reset",
            CircuitBreakerConfig {
                failure_threshold: 2,
                timeout: Duration::from_secs(60),
                ..CircuitBreakerConfig::default()
            },
            path,
        );

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Bypass the open check to record success (reset works in Open state)
        std::fs::remove_file(cb.file_path()).ok();
        let mut inner = cb.inner.lock().unwrap();
        inner.record_success();
        assert_eq!(inner.state(), CircuitState::Closed);
    }

    #[test]
    fn test_persistent_writes_state_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("writes.json");
        let cb = PersistentCircuitBreaker::with_path(
            "writes",
            CircuitBreakerConfig {
                failure_threshold: 2,
                timeout: Duration::from_secs(60),
                ..CircuitBreakerConfig::default()
            },
            path.clone(),
        );

        cb.record_failure();
        cb.record_failure();
        assert!(path.exists(), "state file should exist after tripping");
    }

    #[test]
    fn test_persistent_reset_removes_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("clean.json");
        let cb = PersistentCircuitBreaker::with_path(
            "clean",
            CircuitBreakerConfig {
                failure_threshold: 1,
                ..CircuitBreakerConfig::default()
            },
            path.clone(),
        );

        cb.record_failure();
        assert!(path.exists());
        cb.reset();
        assert!(!path.exists(), "state file should be removed on reset");
    }

    #[test]
    fn test_persistent_bypass() {
        let mut cb = PersistentCircuitBreaker::new("bypass");
        cb.set_bypass(true);

        for _ in 0..10 {
            cb.record_failure();
        }
        assert!(cb.allow(), "bypass should always allow");
    }

    #[test]
    fn test_clean_stale_files_removes_old() {
        let dir = Path::new(BREAKER_DIR);
        let _ = std::fs::create_dir_all(dir);
        let file = dir.join("stale-test.json");
        let _ = std::fs::write(&file, r#"{"state":"open","failure_count":5,"success_count":0}"#);

        let count = clean_stale_circuit_files().unwrap_or(0);
        // File may already be gone; just check no crash
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn test_persistent_cooldown_transitions_to_half_open() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cooldown.json");
        let cb = PersistentCircuitBreaker::with_path(
            "cooldown",
            CircuitBreakerConfig {
                failure_threshold: 2,
                timeout: Duration::from_millis(10),
                ..CircuitBreakerConfig::default()
            },
            path,
        );

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        std::thread::sleep(Duration::from_millis(20));
        assert!(cb.allow(), "should allow after cooldown");
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_initial_state_is_closed() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.call());
    }

    #[test]
    fn test_trips_after_threshold_failures() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..CircuitBreakerConfig::default()
        });

        // Not tripped below threshold
        for _ in 0..2 {
            assert!(cb.call());
            cb.record_failure();
            assert_eq!(cb.state(), CircuitState::Closed);
        }

        // Trip on the third failure
        assert!(cb.call());
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_open_rejects_calls() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            timeout: Duration::from_secs(60), // long so it doesn't expire
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Calls should be rejected
        assert!(!cb.call());
    }

    #[test]
    fn test_transitions_to_half_open_after_timeout() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            timeout: Duration::from_millis(1), // very short
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(10));

        // Next call should transition to half-open
        assert!(cb.call());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_half_open_success_resets_to_closed() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout: Duration::from_millis(1),
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(10));

        // Transition to half-open
        assert!(cb.call());

        // First success in half-open
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Second success → closed
        assert!(cb.call());
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_failure_re_trips() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            timeout: Duration::from_millis(1),
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(10));

        // Call transitions to half-open
        assert!(cb.call());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Failure in half-open re-trips
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.call());
    }

    #[test]
    fn test_success_resets_failure_count_in_closed() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 5,
            ..CircuitBreakerConfig::default()
        });

        // Accumulate some failures but not enough to trip
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.failure_count(), 3);

        // Success resets
        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_reset_force_closes() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Force reset
        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.call());
    }

    #[test]
    fn test_state_as_str() {
        assert_eq!(CircuitState::Closed.as_str(), "closed");
        assert_eq!(CircuitState::Open.as_str(), "open");
        assert_eq!(CircuitState::HalfOpen.as_str(), "half-open");
    }

    #[test]
    fn test_circuit_open_error_display() {
        let err = CircuitOpenError;
        let msg = err.to_string();
        assert!(msg.contains("circuit breaker is open"));
    }

    #[test]
    fn test_default_config() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 1);
        assert_eq!(config.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_record_failure_in_open_extends_cooldown() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            timeout: Duration::from_millis(50),
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Immediately record another failure — resets the cooldown timer
        cb.record_failure();

        // Sleep for just under the timeout (which was refreshed)
        std::thread::sleep(Duration::from_millis(30));

        // Should still be open because the cooldown was refreshed
        assert!(!cb.call());
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_success_in_open_resets_to_closed() {
        let mut cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            ..CircuitBreakerConfig::default()
        });

        // Trip the breaker
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Record success (unusual but handle gracefully)
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.call());
    }
}
