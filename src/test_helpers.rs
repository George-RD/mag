//! Shared test utilities for modules that need HOME directory isolation.
//!
//! The `with_temp_home` helper serializes access to the `HOME` environment
//! variable so that tool detection, config writer, and setup tests can each
//! manipulate HOME without racing each other.

use std::path::Path;
use std::sync::Mutex;

/// Mutex that serializes all tests which mutate the `HOME` environment variable.
///
/// Rust's test runner uses threads, so concurrent tests that call `set_var("HOME", ...)`
/// would race. Every test that needs a fake HOME must hold this lock.
pub static HOME_MUTEX: Mutex<()> = Mutex::new(());

/// Creates a temporary directory, sets `HOME` to it, runs the closure, then restores `HOME`.
///
/// The closure receives a reference to the temporary home directory path.
/// The temporary directory is cleaned up after the closure returns.
///
/// # Panics
///
/// Panics if the temporary directory cannot be created (test infrastructure failure).
pub fn with_temp_home<F, R>(f: F) -> R
where
    F: FnOnce(&Path) -> R,
{
    let _guard = HOME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("mag-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("failed to create temp home directory for test");
    let prev_home = std::env::var_os("HOME");
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");

    // SAFETY: Tests are serialized by HOME_MUTEX, ensuring no concurrent
    // access to these environment variables. This is required because
    // tool detection resolves paths relative to HOME and XDG_CONFIG_HOME.
    unsafe {
        std::env::set_var("HOME", &dir);
        // Unset XDG_CONFIG_HOME so xdg_config_home() falls back to $HOME/.config,
        // giving tests a predictable path within the temp directory.
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    let result = f(&dir);

    // SAFETY: Restoring original values under the same mutex guard.
    unsafe {
        match prev_home {
            Some(val) => std::env::set_var("HOME", val),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    result
}
