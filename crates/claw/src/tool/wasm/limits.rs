//! Resource limits for WASM tool execution.
//!
//! This module provides resource limiting to ensure WASM tools cannot
//! consume excessive memory, CPU, or time.

use std::sync::Arc;
use wasmtime::ResourceLimiter;

/// Default memory limit for WASM modules (10 MB).
pub const DEFAULT_MEMORY_LIMIT: usize = 10 * 1024 * 1024;

/// Default table element limit for WASM modules.
pub const DEFAULT_TABLE_LIMIT: u32 = 10000;

/// Default fuel (CPU instruction) limit (10 million).
pub const DEFAULT_FUEL_LIMIT: u64 = 10_000_000;

/// Default execution timeout in milliseconds (60 seconds).
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;

/// Resource limits configuration for WASM execution.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum memory the WASM module can allocate.
    pub memory_limit: usize,

    /// Maximum number of table elements.
    pub table_limit: u32,

    /// Maximum CPU fuel (instructions).
    pub fuel_limit: u64,

    /// Maximum execution time in milliseconds.
    pub timeout_ms: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_limit: DEFAULT_MEMORY_LIMIT,
            table_limit: DEFAULT_TABLE_LIMIT,
            fuel_limit: DEFAULT_FUEL_LIMIT,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

impl ResourceLimits {
    /// Create new resource limits with custom values.
    #[must_use]
    pub fn new(memory_limit: usize, table_limit: u32, fuel_limit: u64, timeout_ms: u64) -> Self {
        Self {
            memory_limit,
            table_limit,
            fuel_limit,
            timeout_ms,
        }
    }

    /// Create resource limits with a custom memory limit.
    #[must_use]
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = limit;
        self
    }

    /// Create resource limits with a custom fuel limit.
    #[must_use]
    pub fn with_fuel_limit(mut self, limit: u64) -> Self {
        self.fuel_limit = limit;
        self
    }

    /// Create resource limits with a custom timeout.
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

/// WASM resource limiter implementation.
///
/// This struct implements `wasmtime::ResourceLimiter` to enforce
/// memory and table limits on WASM modules.
#[derive(Debug)]
pub struct WasmResourceLimiter {
    limits: Arc<ResourceLimits>,
    /// Current memory usage (for tracking).
    memory_used: usize,
}

impl WasmResourceLimiter {
    /// Create a new resource limiter with the given limits.
    #[must_use]
    pub fn new(limits: Arc<ResourceLimits>) -> Self {
        Self {
            limits,
            memory_used: 0,
        }
    }

    /// Get the configured limits.
    #[must_use]
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Get current memory usage.
    #[must_use]
    pub fn memory_used(&self) -> usize {
        self.memory_used
    }
}

impl ResourceLimiter for WasmResourceLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        // Allow growth if within limits
        if desired <= self.limits.memory_limit {
            self.memory_used = desired;
            Ok(true)
        } else {
            tracing::warn!(
                current = current,
                desired = desired,
                limit = self.limits.memory_limit,
                "Memory growth rejected"
            );
            Ok(false)
        }
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        // Allow growth if within limits
        let table_limit = self.limits.table_limit as usize;
        if desired <= table_limit {
            Ok(true)
        } else {
            tracing::warn!(
                current = current,
                desired = desired,
                limit = table_limit,
                "Table growth rejected"
            );
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.memory_limit, DEFAULT_MEMORY_LIMIT);
        assert_eq!(limits.table_limit, DEFAULT_TABLE_LIMIT);
        assert_eq!(limits.fuel_limit, DEFAULT_FUEL_LIMIT);
        assert_eq!(limits.timeout_ms, DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn custom_limits() {
        let limits = ResourceLimits::new(5 * 1024 * 1024, 5000, 5_000_000, 30_000);
        assert_eq!(limits.memory_limit, 5 * 1024 * 1024);
        assert_eq!(limits.table_limit, 5000);
        assert_eq!(limits.fuel_limit, 5_000_000);
        assert_eq!(limits.timeout_ms, 30_000);
    }

    #[test]
    fn builder_pattern() {
        let limits = ResourceLimits::default()
            .with_memory_limit(20 * 1024 * 1024)
            .with_fuel_limit(20_000_000)
            .with_timeout_ms(120_000);

        assert_eq!(limits.memory_limit, 20 * 1024 * 1024);
        assert_eq!(limits.fuel_limit, 20_000_000);
        assert_eq!(limits.timeout_ms, 120_000);
        // Other values should remain default
        assert_eq!(limits.table_limit, DEFAULT_TABLE_LIMIT);
    }

    #[test]
    fn resource_limiter_allows_within_limit() -> anyhow::Result<()> {
        let limits = Arc::new(ResourceLimits::default());
        let mut limiter = WasmResourceLimiter::new(limits);

        // Should allow growth within limit
        assert!(limiter.memory_growing(0, 1024, None)?);
        assert_eq!(limiter.memory_used(), 1024);

        // Should allow more growth
        assert!(limiter.memory_growing(1024, 5 * 1024 * 1024, None)?);
        assert_eq!(limiter.memory_used(), 5 * 1024 * 1024);

        Ok(())
    }

    #[test]
    fn resource_limiter_rejects_over_limit() -> anyhow::Result<()> {
        let limits = Arc::new(ResourceLimits::default());
        let mut limiter = WasmResourceLimiter::new(limits);

        // Should reject growth over limit
        assert!(!limiter.memory_growing(0, DEFAULT_MEMORY_LIMIT + 1, None)?);

        Ok(())
    }

    #[test]
    fn resource_limiter_table_growth() -> anyhow::Result<()> {
        let limits = Arc::new(ResourceLimits::default());
        let mut limiter = WasmResourceLimiter::new(limits);

        // Should allow within limit
        assert!(limiter.table_growing(0, 5000, None)?);

        // Should reject over limit
        assert!(!limiter.table_growing(0, (DEFAULT_TABLE_LIMIT + 1) as usize, None)?);

        Ok(())
    }
}
