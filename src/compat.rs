// Tokio / Async-std compatibility layer

#[cfg_attr(feature = "async-std", path = "compat/async_std.rs")]
#[cfg_attr(feature = "tokio", path = "compat/tokio.rs")]
mod compat_feature;

pub use compat_feature::*;
