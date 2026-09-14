//! Full-width Results and compatibility scalar Option helpers.
use crate::abi::{self, ResultValue};

type Unary = unsafe extern "C" fn(i64) -> i64;

/// Create a full-width successful result.
#[unsafe(no_mangle)]
pub extern "C" fn fern_result_ok(value: i64) -> i64 {
    abi::result_ok(value)
}
/// Create a full-width error result.
#[unsafe(no_mangle)]
pub extern "C" fn fern_result_err(value: i64) -> i64 {
    abi::result_err(value)
}
macro_rules! checked_binary {
    ($name:ident, $method:ident) => {
        #[doc = "Full-width checked integer operation; overflow or an invalid divisor is `None`."]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(left: i64, right: i64) -> i64 {
            abi::heap_option(left.$method(right))
        }
    };
}
checked_binary!(fern_int_checked_add, checked_add);
checked_binary!(fern_int_checked_sub, checked_sub);
checked_binary!(fern_int_checked_mul, checked_mul);
checked_binary!(fern_int_checked_div, checked_div);
checked_binary!(fern_int_checked_rem, checked_rem);
/// Negate without wrapping; the minimum value is `None`.
#[unsafe(no_mangle)]
pub extern "C" fn fern_int_checked_neg(value: i64) -> i64 {
    abi::heap_option(value.checked_neg())
}
/// Inspect a native Result tag.
/// # Safety
/// `value` must be a live ResultValue address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_result_is_ok(value: i64) -> i64 {
    i64::from(unsafe { (*(value as *const ResultValue)).tag } == 0)
}
/// Read the payload of either Result variant.
/// # Safety
/// `value` must be a live ResultValue address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_result_unwrap(value: i64) -> i64 {
    unsafe { (*(value as *const ResultValue)).value }
}
/// Map only a successful Result.
/// # Safety
/// Result is live; callback obeys the native scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_result_map(value: i64, callback: Unary) -> i64 {
    if unsafe { fern_result_is_ok(value) } == 0 {
        value
    } else {
        abi::result_ok(unsafe { callback(fern_result_unwrap(value)) })
    }
}
/// Chain a successful Result through a callback.
/// # Safety
/// Result is live; callback returns a live Result and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_result_and_then(value: i64, callback: Unary) -> i64 {
    if unsafe { fern_result_is_ok(value) } == 0 {
        value
    } else {
        unsafe { callback(fern_result_unwrap(value)) }
    }
}
/// Return a successful payload or the supplied default.
/// # Safety
/// Result is a live ResultValue address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_result_unwrap_or(value: i64, default: i64) -> i64 {
    if unsafe { fern_result_is_ok(value) } == 0 {
        default
    } else {
        unsafe { fern_result_unwrap(value) }
    }
}
/// Compute a fallback only for an error Result.
/// # Safety
/// Result is live; callback obeys the scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_result_unwrap_or_else(value: i64, callback: Unary) -> i64 {
    let payload = unsafe { fern_result_unwrap(value) };
    if unsafe { fern_result_is_ok(value) } == 0 {
        unsafe { callback(payload) }
    } else {
        payload
    }
}
/// Encode Some for native helpers that retain their scalar Option ABI.
#[unsafe(no_mangle)]
pub extern "C" fn fern_option_some(value: i64) -> i64 {
    abi::option_some(value)
}
/// Encode a missing native scalar.
#[unsafe(no_mangle)]
pub extern "C" fn fern_option_none() -> i64 {
    abi::option_none()
}
/// Test the native scalar Option tag.
#[unsafe(no_mangle)]
pub extern "C" fn fern_option_is_some(value: i64) -> i64 {
    i64::from(value as u32 == 1)
}
/// Unwrap a scalar Option, rejecting absence before reading its payload.
#[unsafe(no_mangle)]
pub extern "C" fn fern_option_unwrap(value: i64) -> i64 {
    if fern_option_is_some(value) == 0 {
        abi::fault("unwrap of None");
    }
    ((value as u64) >> 32) as i64
}
/// Map a present native scalar.
/// # Safety
/// Callback obeys the scalar ABI and does not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_option_map(value: i64, callback: Unary) -> i64 {
    if fern_option_is_some(value) == 0 {
        value
    } else {
        abi::option_some(unsafe { callback(fern_option_unwrap(value)) })
    }
}
/// Select a scalar Option payload or the supplied default.
#[unsafe(no_mangle)]
pub extern "C" fn fern_option_unwrap_or(value: i64, default: i64) -> i64 {
    if fern_option_is_some(value) == 0 {
        default
    } else {
        fern_option_unwrap(value)
    }
}
