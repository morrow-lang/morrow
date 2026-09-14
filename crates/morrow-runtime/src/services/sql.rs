//! Real SQLite connections with bounded live slots and never-recycled handles.
use crate::abi;
use rusqlite::{Connection, ffi};
use std::collections::BTreeMap;
use std::ffi::{CStr, OsStr, c_char};
use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;

struct Connections {
    next: i64,
    live: BTreeMap<i64, Connection>,
}
static CONNECTIONS: Mutex<Connections> = Mutex::new(Connections {
    next: 1,
    live: BTreeMap::new(),
});

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_sql_open(path: *const c_char) -> i64 {
    if path.is_null() || unsafe { *path } == 0 {
        return abi::result_err(3);
    }
    let mut connections = CONNECTIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if connections.live.len() == 256 || connections.next == i64::MAX {
        return abi::result_err(4);
    }
    let path = OsStr::from_bytes(unsafe { CStr::from_ptr(path) }.to_bytes());
    let Ok(connection) = Connection::open(path) else {
        return abi::result_err(3);
    };
    if connection.busy_timeout(std::time::Duration::ZERO).is_err() {
        return abi::result_err(3);
    }
    let id = connections.next;
    connections.next += 1;
    connections.live.insert(id, connection);
    abi::result_ok(id)
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn morrow_sql_execute(handle: i64, query: *const c_char) -> i64 {
    if handle <= 0 || query.is_null() || unsafe { *query } == 0 {
        return abi::result_err(3);
    }
    let connections = CONNECTIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(connection) = connections.live.get(&handle) else {
        return abi::result_err(3);
    };
    // sqlite3_exec permits complete statement batches and ignores returned query rows.
    // rusqlite owns the connection; the lock excludes concurrent close/statement access.
    let status = unsafe {
        ffi::sqlite3_exec(
            connection.handle(),
            query,
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if status == ffi::SQLITE_OK {
        abi::result_ok(unsafe { ffi::sqlite3_changes(connection.handle()) } as i64)
    } else {
        abi::result_err(3)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn morrow_sql_close(handle: i64) -> i64 {
    let mut connections = CONNECTIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(connection) = connections.live.remove(&handle) else {
        return abi::result_err(3);
    };
    match connection.close() {
        Ok(()) => abi::result_ok(0),
        Err((connection, _)) => {
            connections.live.insert(handle, connection);
            abi::result_err(3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::decode;

    #[test]
    fn sqlite_lifecycle_quota_stale_ids_and_arbitrary_statements() {
        unsafe {
            let first = decode(morrow_sql_open(c":memory:".as_ptr())).unwrap();
            assert_eq!(
                decode(morrow_sql_execute(
                    first,
                    c"CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(1),(2); SELECT * FROM t;"
                        .as_ptr()
                )),
                Ok(2)
            );
            assert_eq!(
                decode(morrow_sql_execute(first, c"UPDATE t SET x=x+1".as_ptr())),
                Ok(2)
            );
            assert_eq!(decode(morrow_sql_close(first)), Ok(0));
            assert_eq!(decode(morrow_sql_close(first)), Err(3));
            assert_eq!(
                decode(morrow_sql_execute(first, c"SELECT 1".as_ptr())),
                Err(3)
            );
            let mut handles = Vec::new();
            for _ in 0..256 {
                handles.push(decode(morrow_sql_open(c":memory:".as_ptr())).unwrap());
            }
            assert_eq!(decode(morrow_sql_open(c":memory:".as_ptr())), Err(4));
            let old = handles.pop().unwrap();
            assert_eq!(decode(morrow_sql_close(old)), Ok(0));
            let new = decode(morrow_sql_open(c":memory:".as_ptr())).unwrap();
            assert!(new > old);
            assert_eq!(
                decode(morrow_sql_execute(old, c"SELECT 1".as_ptr())),
                Err(3)
            );
            handles.push(new);
            for handle in handles {
                assert_eq!(decode(morrow_sql_close(handle)), Ok(0));
            }
        }
    }
}
