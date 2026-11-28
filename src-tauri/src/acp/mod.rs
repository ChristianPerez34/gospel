//! Agent Client Protocol integration built on the official
//! `agent-client-protocol` SDK.
//!
//! - `client`: typed connection glue over the SDK transport
//! - `dialect`: Factory Droid payload dialect (config options, extensions)
//! - `droid`: binary discovery, auth, model catalog
//! - `turn`: prompt execution, streaming, cancellation, fork
//! - `workspace`: containment, filesystem policy, approval classification

pub mod client;
pub mod dialect;
pub mod droid;
pub mod turn;
pub mod workspace;
