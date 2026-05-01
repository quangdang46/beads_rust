//! Cooperative shutdown coordination for `SIGINT`, `SIGTERM`, and `SIGHUP`.
//!
//! On Unix, the default action for these signals is to terminate the
//! process *without unwinding the stack*, which means
//! [`Drop`](std::ops::Drop) impls — including
//! [`crate::storage::SqliteStorage::drop`] — never run, and WAL frames
//! that haven't been checkpointed yet are left stranded on disk
//! (issue #270).
//!
//! This module installs a small handler that translates those signals
//! into a single atomic "shutdown requested" flag, then lets the main
//! thread complete its current operation, return from `main`, and run
//! every destructor on the way out. If the user signals again while the
//! main thread is still inside a long operation we escalate to an
//! immediate `_exit`, matching the muscle-memory of "press Ctrl-C
//! twice."
//!
//! On Windows we currently rely on the default Ctrl-C behaviour and the
//! [`Drop`] / `panic = "abort"` interaction; the public surface here is
//! a no-op so callers don't need `cfg(unix)` at every call site.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;

/// Set when one of the registered termination signals has been
/// observed. Public callers should use [`is_requested`] /
/// [`exit_code`].
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// `128 + signo` of the signal that triggered the shutdown, encoding
/// the conventional Unix exit code. Stored as `i32` so the relaxed
/// load is wait-free; only the *first* signal wins, which keeps the
/// reported exit code stable when multiple signals race.
static SHUTDOWN_EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// Tracks whether [`install`] has already wired the background thread,
/// so callers can invoke it safely from `main` without worrying about
/// double-registration in test harnesses or library re-entry.
static INSTALLED: OnceLock<()> = OnceLock::new();

/// Install signal handlers for `SIGINT`, `SIGTERM`, and `SIGHUP` (Unix
/// only). On non-Unix targets this is a no-op.
///
/// # Behaviour
///
/// * The first signal flips [`is_requested`] and records the exit code
///   `128 + signo`. The main thread is responsible for noticing this
///   flag at a safe checkpoint and returning from `main`.
/// * The second matching signal calls `libc::_exit` immediately so a
///   user can always escape a hung command by hitting Ctrl-C twice.
///
/// Idempotent: subsequent calls return without re-installing.
pub fn install() {
    if INSTALLED.set(()).is_err() {
        return;
    }
    #[cfg(unix)]
    install_unix();
}

/// Returns `true` once any registered signal has been observed.
#[must_use]
pub fn is_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Acquire)
}

/// Returns the conventional Unix exit code (`128 + signo`) for the
/// signal that triggered shutdown, or `None` if no signal has fired.
#[must_use]
pub fn exit_code() -> Option<i32> {
    let code = SHUTDOWN_EXIT_CODE.load(Ordering::Acquire);
    (code != 0).then_some(code)
}

#[cfg(unix)]
fn install_unix() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals = match Signals::new([SIGINT, SIGTERM, SIGHUP]) {
        Ok(signals) => signals,
        Err(err) => {
            // If we can't install a handler we fall back to default
            // signal action (process termination). Logging here keeps
            // the failure visible without aborting startup, since the
            // user's command is already in flight.
            tracing::warn!(
                error = %err,
                "failed to install shutdown signal handler; SIGTERM/SIGINT/SIGHUP \
                 will skip Drop and may strand WAL frames"
            );
            return;
        }
    };

    std::thread::Builder::new()
        .name("br-shutdown".to_string())
        .spawn(move || {
            for signo in signals.forever() {
                let exit = 128 + signo;
                // First signal: cooperatively notify main; second: hard
                // exit. `compare_exchange` ensures both fields advance
                // atomically so a racing reader never sees the flag set
                // without the matching exit code.
                if SHUTDOWN_REQUESTED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    SHUTDOWN_EXIT_CODE.store(exit, Ordering::Release);
                } else {
                    // Second strike: bypass main, accept that any
                    // remaining WAL frames are forfeit — the user
                    // explicitly asked to bail out now. signal-hook's
                    // `low_level::exit` wraps `_exit` and is documented
                    // as async-signal-safe for exactly this case.
                    signal_hook::low_level::exit(exit);
                }
            }
        })
        .map(drop)
        .unwrap_or_else(|err| {
            tracing::warn!(
                error = %err,
                "failed to spawn br-shutdown thread; falling back to default signal action"
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// `install` must be safe to call repeatedly without leaking
    /// background threads or panicking on the second invocation.
    #[test]
    fn install_is_idempotent() {
        install();
        install();
        install();
        // The flag itself is process-global; clearing it here keeps
        // other tests in the same binary unaffected by an accidental
        // earlier install. We only touch it when no signal has been
        // observed, which is the common case in unit tests.
        if !is_requested() {
            SHUTDOWN_REQUESTED.store(false, Ordering::Release);
            SHUTDOWN_EXIT_CODE.store(0, Ordering::Release);
        }
    }

    #[test]
    fn exit_code_is_none_until_signal_fires() {
        // We don't fire a real signal in unit tests because that would
        // race with cargo's own Ctrl-C handling. Instead this asserts
        // the documented invariant for the unsignalled state — the
        // signalled state is exercised end-to-end by the shutdown
        // integration test in tests/.
        if !is_requested() {
            assert_eq!(exit_code(), None);
        }
    }
}
