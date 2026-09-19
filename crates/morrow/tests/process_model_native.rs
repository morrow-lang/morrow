//! Native Process lifecycle oracles execute compiler output against the managed runtime.
#[path = "support/process_native.rs"]
mod support;
use support::execute_source;

#[test]
fn isolated_fault_monitors_and_copied_identities_preserve_typed_messages() {
    let source = include_str!("process_model/monitors.mr");
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_library_string_port(exec: usize) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn morrow_gc_frame_leave(token: usize);
    fn morrow_gc_collect_precise();
}
fn main() {
    let mut fault = 0;
    unsafe {
        let exec = morrow_library_open(&mut fault);
        assert_ne!(exec, 0);
        let port = morrow_library_string_port(exec);
        let root = morrow_gc_frame_enter(&port, 1);
        morrow_export_start(&mut fault, exec, port);
        let mut received = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for step in 0..5000 {
            let status = morrow_managed_poll(exec, 1);
            // This host thread collects only its own domain. Worker heaps stay
            // owned by their schedulers; the native port remains explicitly rooted.
            morrow_gc_collect_precise();
            assert_eq!(fault, 0, "isolated actor fault escaped at poll {step}, status {status}");
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if len >= 0 { received.push(String::from_utf8(bytes[..len as usize].to_vec()).unwrap()); }
            if received.len() == 5 { break; }
            assert!(std::time::Instant::now() < deadline, "message deadline: {received:?}");
            if len < 0 { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        assert_eq!(received.len(), 5, "{received:?}");
        // The observer's reports are causally ordered. The independent sibling
        // may appear anywhere; requiring a global cross-actor order would race.
        assert_eq!(received.iter().filter(|message| message.as_str() == "sibling").count(), 1);
        let observer: Vec<_> = received.iter().filter(|message| message.as_str() != "sibling").map(String::as_str).collect();
        assert_eq!(observer, ["identities", "wide=9007199254740993", "first", "second"]);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
    }
    println!("isolated, copied, ordered and rooted");
}
"#;
    execute_source(source, harness, b"isolated, copied, ordered and rooted\n");
}

#[test]
fn demonitor_flush_and_info_dead_processes_and_event_message_view() {
    // Pinned OTP oracle docs/process-model/monitor_reference.*: monitoring self
    // returns an inert reference, so demonitor with info reports false.
    let harness = r#"
unsafe extern "C" {
    fn morrow_library_open(fault: *mut i64) -> usize;
    fn morrow_library_string_port(exec: usize) -> usize;
    fn morrow_export_start(fault: *mut i64, exec: usize, port: usize) -> i32;
    fn morrow_managed_poll(exec: usize, steps: i64) -> i64;
    fn morrow_managed_port_read(exec: usize, port: usize, output: *mut u8, capacity: usize) -> i64;
    fn morrow_managed_close(exec: usize);
    fn morrow_gc_frame_enter(slots: *const usize, words: usize) -> usize;
    fn morrow_gc_frame_leave(token: usize);
    fn morrow_gc_collect_precise();
}
fn main() {
    let mut fault = 0;
    unsafe {
        let exec = morrow_library_open(&mut fault);
        assert_ne!(exec, 0);
        let port = morrow_library_string_port(exec);
        let root = morrow_gc_frame_enter(&port, 1);
        morrow_export_start(&mut fault, exec, port);
        let mut received = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for step in 0..5000 {
            let status = morrow_managed_poll(exec, 1);
            // This host thread collects only its own domain. Worker heaps stay
            // owned by their schedulers; the native port remains explicitly rooted.
            morrow_gc_collect_precise();
            assert_eq!(fault, 0, "actor fault at poll {step}, status {status}");
            let mut bytes = [0u8; 128];
            let len = morrow_managed_port_read(exec, port, bytes.as_mut_ptr(), bytes.len());
            if len >= 0 { received.push(String::from_utf8(bytes[..len as usize].to_vec()).unwrap()); }
            if received.len() == 7 { break; }
            assert!(std::time::Instant::now() < deadline, "message deadline: {received:?}");
            if len < 0 { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        assert_eq!(received, ["cancelled", "inactive", "normal", "flushed", "empty", "dead", "message"]);
        morrow_managed_close(exec);
        morrow_gc_frame_leave(root);
    }
    println!("demonitor and event views");
}
"#;
    execute_source(
        include_str!("process_model/demonitor.mr"),
        harness,
        b"demonitor and event views\n",
    );
}
