//! Nginx Config Modification Tool
//! ==============================
//!
//! Note: we currently working on command-line tool, so internals/lib are
//! poorly documented. We plan to improve on it later.
//!
//! [Docs](https://docs.rs/nginx-config-mod/) |
//! [Github](https://github.com/tailhook/nginx-config-mod/) |
//! [Crate](https://crates.io/crates/nginx-config-mod)
//!
extern crate nginx_config;
#[macro_use] extern crate failure;

mod config;
mod errors;

pub use errors::ReadError;
pub use config::{Config, EntryPoint};
