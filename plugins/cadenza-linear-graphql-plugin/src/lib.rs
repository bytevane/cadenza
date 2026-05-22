//! Minimal placeholder plugin crate.
//!
//! TODO: Replace this with WIT-generated bindings after `wit/runtime.wit` is frozen.
//! This crate exists so the workspace has a concrete `wasm32-wasip2` build target.

/// Stable ABI marker for early smoke tests.
#[unsafe(no_mangle)]
pub extern "C" fn cadenza_plugin_abi_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn abi_version_is_one() {
        assert_eq!(super::cadenza_plugin_abi_version(), 1);
    }
}
