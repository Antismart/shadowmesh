//! ShadowMesh Edge Function SDK
//!
//! Write serverless functions that run on the ShadowMesh CDN edge.
//!
//! # Example
//! ```rust,ignore
//! use shadowmesh_edge::{Request, Response};
//!
//! fn main() {
//!     shadowmesh_edge::run(|req| {
//!         Response::ok().json(&serde_json::json!({
//!             "hello": "world",
//!             "path": req.path(),
//!         }))
//!     });
//! }
//! ```

mod request;
mod response;
pub mod runtime;

pub use request::Request;
pub use response::Response;
pub use runtime::run;
