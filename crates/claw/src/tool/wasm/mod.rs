//! WASM tool execution system.
//!
//! This module provides a secure sandbox for executing WASM-based tools.
//! It implements an opt-in capability system where tools must explicitly
//! request and be granted permissions for operations like HTTP requests,
//! file access, and tool invocation.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                              WASM Tool Execution                             │
//! │                                                                              │
//! │   WASM Tool ──▶ Host Function ──▶ Allowlist ──▶ Credential ──▶ Execute     │
//! │   (untrusted)   (boundary)        Validator     Injector       Request      │
//! │                                                                    │        │
//! │                                                                    ▼        │
//! │                              ◀────── Leak Detector ◀────── Response        │
//! │                          (sanitized, no secrets)                            │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Design Patterns
//!
//! ## Compile Once, Instantiate Fresh
//! - Tools are validated and compiled once when registered
//! - Each execution creates a fresh instance for isolation
//!
//! ## Opt-in Capabilities
//! - All capabilities are denied by default
//! - Explicit declaration and authorization required for:
//!   - HTTP requests (with allowlist)
//!   - File reading (with path restrictions)
//!   - Tool invocation
//!   - Secret access
//!
//! ## Resource Limits
//! - Memory: 10 MB default
//! - CPU: 10M instructions (fuel metering)
//! - Time: 60 seconds timeout
//!
//! # Usage
//!
//! ```ignore
//! use claw::tool::wasm::{WasmToolRuntime, WasmToolLoader};
//!
//! // Create the runtime
//! let runtime = WasmToolRuntime::new()?;
//!
//! // Load tools from the default directory
//! let loader = WasmToolLoader::with_default_dir(runtime, tool_manager)?;
//! loader.load_all();
//! ```

pub mod allowlist;
pub mod capabilities;
pub mod capabilities_schema;
pub mod error;
pub mod host;
pub mod limits;
pub mod loader;
pub mod runtime;
pub mod wrapper;

// Re-export main types for convenience
pub use allowlist::AllowlistValidator;
pub use capabilities::Capabilities;
pub use capabilities_schema::ToolMetadata;
pub use error::WasmError;
pub use host::{HostState, LogEntry, LogLevel};
pub use limits::{ResourceLimits, WasmResourceLimiter};
pub use loader::WasmToolLoader;
pub use runtime::WIT_VERSION;
pub use runtime::WasmToolRuntime;
pub use wrapper::WasmToolWrapper;
