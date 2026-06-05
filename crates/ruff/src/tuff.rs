//! Tuff-specific CLI integration.
//!
//! Keep product-specific configuration, option adaptation, and formatter entrypoints in this
//! module so `commands` can stay close to upstream Ruff orchestration.

pub(crate) mod config;
pub(crate) mod format;
