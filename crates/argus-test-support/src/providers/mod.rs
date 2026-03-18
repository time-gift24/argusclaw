//! Test providers for simulating various failure scenarios.

mod always_fail;
mod intermittent;

pub use always_fail::AlwaysFailProvider;
pub use intermittent::IntermittentFailureProvider;
