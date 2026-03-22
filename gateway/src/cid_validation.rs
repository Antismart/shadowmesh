//! CID validation — re-exports from the protocol crate.
//!
//! The canonical implementation lives in `protocol::cid_validation`.
//! This module re-exports everything so gateway code can use
//! `crate::cid_validation::validate_cid(...)` without reaching into protocol.

pub use protocol::cid_validation::*;
