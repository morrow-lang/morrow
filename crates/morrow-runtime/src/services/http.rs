//! Bounded HTTP text requests through Rust HTTP/TLS implementations.
use crate::{abi, io};
use std::ffi::c_char;
use std::io::Read;
use std::time::Duration;

unsafe fn request(url: *const c_char, body: Option<*const c_char>) -> Result<i64, i64> {
    let url = unsafe { io::bounded_bytes(url, 1024 * 1024) }.map_err(|_| 3)?;
    let url = std::str::from_utf8(url).map_err(|_| 3)?;
    let authority = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or(3)?
        .split(['/', '?', '#'])
        .next()
        .ok_or(3)?;
    if authority.is_empty() || authority.len() >= 640 || authority.contains('@') {
        return Err(3);
    }
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .ok_or(3)?
            .trim_start_matches('[')
    } else {
        authority.split(':').next().ok_or(3)?
    };
    if host.is_empty() || host.len() >= 512 {
        return Err(3);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut response = match body {
        Some(body) => {
            let bytes = unsafe { io::bounded_bytes(body, io::TEXT_LIMIT) }.map_err(|_| 3)?;
            if !io::valid_text(bytes) {
                return Err(3);
            }
            agent
                .post(url)
                .header("Content-Type", "text/plain")
                .header("Connection", "close")
                .send(bytes)
                .map_err(|_| 3)?
        }
        None => agent
            .get(url)
            .header("Connection", "close")
            .call()
            .map_err(|_| 3)?,
    };
    if !response.status().is_success() {
        return Err(3);
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(io::TEXT_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| 3)?;
    if bytes.len() > io::TEXT_LIMIT || !io::valid_text(&bytes) {
        return Err(3);
    }
    Ok(abi::bytes(&bytes) as i64)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_http_get(url: *const c_char) -> i64 {
    match unsafe { request(url, None) } {
        Ok(value) => abi::result_ok(value),
        Err(error) => abi::result_err(error),
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_http_post(url: *const c_char, body: *const c_char) -> i64 {
    match unsafe { request(url, Some(body)) } {
        Ok(value) => abi::result_ok(value),
        Err(error) => abi::result_err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::decode;
    use std::io::{Read, Write};

    #[test]
    fn http_uses_real_request_and_preserves_status_and_body_failures() {
        for (reply, expected) in [
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nhey",
                Some("hey"),
            ),
            (
                "HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\n\r\n",
                None,
            ),
            ("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nshort", None),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = std::ffi::CString::new(format!(
                "http://{}/path?x=1",
                listener.local_addr().unwrap()
            ))
            .unwrap();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let size = stream.read(&mut request).unwrap();
                assert!(request[..size].starts_with(b"GET /path?x=1 HTTP/1.1\r\n"));
                stream.write_all(reply.as_bytes()).unwrap();
            });
            let value = decode(unsafe { morrow_http_get(url.as_ptr()) });
            match expected {
                Some(expected) => assert_eq!(
                    unsafe { crate::abi::text(value.unwrap() as *const std::ffi::c_char) },
                    expected
                ),
                None => assert_eq!(value, Err(3)),
            }
            thread.join().unwrap();
        }
    }
}
