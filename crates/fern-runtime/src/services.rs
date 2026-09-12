//! HTTP, SQLite and POSIX regular expressions behind the stable native ABI.
mod http;
mod regex;
mod sql;
pub use http::*;
pub use regex::*;
pub use sql::*;

#[cfg(test)]
fn decode(value: i64) -> Result<i64, i64> {
    let value = unsafe { &*(value as *const crate::abi::ResultValue) };
    if value.tag == 0 {
        Ok(value.value)
    } else {
        Err(value.value)
    }
}
