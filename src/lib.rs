extern crate nginx_config;
#[macro_use] extern crate failure;

mod config;
mod errors;

pub use errors::ReadError;
pub use config::{Config, EntryPoint};
