//! Cereal's configuration comes from `CEREAL_*` environment variables, all
//! read through here.
use std::{env::VarError, ffi::OsString};

pub fn var(name: &str) -> Result<String, VarError> {
    std::env::var(name)
}
pub fn var_os(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}
