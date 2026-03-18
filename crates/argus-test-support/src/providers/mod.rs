//! Test providers for simulating various failure scenarios.

mod intermittent;
mod always_fail;

pub use intermittent::IntermittentFailureProvider;
pub use always_fail::AlwaysFailProvider;
