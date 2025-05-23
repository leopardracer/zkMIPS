//! # zkMIPS SDK Utilities
//!
//! A collection of utilities for the zkMIPS SDK.

use zkm_core_machine::io::ZKMStdin;
pub use zkm_core_machine::utils::setup_logger;

/// Dump the program and stdin to files for debugging if `ZKM_DUMP` is set.
pub(crate) fn zkm_dump(elf: &[u8], stdin: &ZKMStdin) {
    if std::env::var("ZKM_DUMP").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false) {
        std::fs::write("program.bin", elf).unwrap();
        let stdin = bincode::serialize(&stdin).unwrap();
        std::fs::write("stdin.bin", stdin.clone()).unwrap();
    }
}

/// Utility method for blocking on an async function.
///
/// If we're already in a tokio runtime, we'll block in place. Otherwise, we'll create a new
/// runtime.
#[cfg(feature = "network")]
pub(crate) fn block_on<T>(fut: impl std::future::Future<Output = T>) -> T {
    use tokio::task::block_in_place;

    // Handle case if we're already in an tokio runtime.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        block_in_place(|| handle.block_on(fut))
    } else {
        // Otherwise create a new runtime.
        let rt = tokio::runtime::Runtime::new().expect("Failed to create a new runtime");
        rt.block_on(fut)
    }
}
