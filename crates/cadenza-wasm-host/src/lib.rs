use serde::{Deserialize, Serialize};

pub const WIT_PACKAGE: &str = "cadenza:runtime@0.1.0";
pub const WIT_WORLD: &str = "tool-runtime";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmRuntimeLimits {
    pub max_memory_bytes: usize,
    pub max_tables: usize,
    pub max_instances: usize,
    pub epoch_timeout_ms: u64,
    pub max_http_body_bytes: usize,
}

impl Default for WasmRuntimeLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            max_tables: 64,
            max_instances: 16,
            epoch_timeout_ms: 5_000,
            max_http_body_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmComponentRef {
    pub name: String,
    pub path: String,
    pub wit_package: String,
    pub wit_world: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WasmHostError {
    #[error("component WIT package mismatch: expected {expected}, actual {actual}")]
    WitPackageMismatch { expected: String, actual: String },
    #[error("component WIT world mismatch: expected {expected}, actual {actual}")]
    WitWorldMismatch { expected: String, actual: String },
    #[error("component denied by capability policy: {0}")]
    CapabilityDenied(String),
}

/// Placeholder for the Wasmtime host runtime.
/// Implement concrete component loading only after WIT ABI snapshots are frozen.
pub struct ComponentRuntime {
    pub limits: WasmRuntimeLimits,
}

impl ComponentRuntime {
    pub fn new(limits: WasmRuntimeLimits) -> Self {
        Self { limits }
    }
}
