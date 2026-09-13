//! HTTP, SQLite and POSIX regular expressions behind the stable native ABI.
#[cfg(feature = "http")]
mod http;
mod regex;
#[cfg(feature = "sqlite")]
mod sql;
#[cfg(feature = "http")]
pub use http::*;
pub use regex::*;
#[cfg(feature = "sqlite")]
pub use sql::*;

#[cfg(all(test, any(feature = "http", feature = "sqlite")))]
fn decode(value: i64) -> Result<i64, i64> {
    let value = unsafe { &*(value as *const crate::abi::ResultValue) };
    if value.tag == 0 {
        Ok(value.value)
    } else {
        Err(value.value)
    }
}
