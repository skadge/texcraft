//! Crash containment: every entry point (command, control request, frame, MCP call) runs inside
//! [`catch_panic`], so a bug in one command becomes an error message instead of a lost session.
//! On wasm32 a panic aborts; there the no-panic lints are the protection.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`, turning a panic into `Err` with its message.
pub fn catch_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_owned());
        log::error!("internal error caught: {message}");
        format!("Internal error: {message}")
    })
}
