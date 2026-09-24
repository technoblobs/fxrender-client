//! Rust client for the FXRender public API.
//!
//! Shared by the `fxr` CLI and the desktop app so both speak to
//! `api.fxrender.com` (or a local `public_api` dev server) through one
//! implementation. See `public_api/docs/API.md` in the main FXRender repo
//! for the full narrative reference this crate follows.
//!
//! ```no_run
//! use fxrender_core::{Client, models::JobCreate};
//!
//! # async fn run() -> fxrender_core::Result<()> {
//! let client = Client::new(fxrender_core::DEFAULT_BASE_URL, "fxr_live_...")?;
//! let me = client.me().await?;
//! println!("{} has {}s of render time left", me.email, me.remaining_seconds);
//! # Ok(())
//! # }
//! ```

mod client;
mod error;
pub mod models;
pub mod store;
pub mod watch;

pub use client::{Client, LogEvent, UploadedAsset, DEFAULT_BASE_URL};
pub use error::{Error, Result};
