//! A Rust container is not itself traced by the native collector.
use morrow_runtime::{abi, memory};

#[inline(never)]
fn managed_sources() -> Vec<&'static str> {
    (0..256)
        .map(|index| {
            let pointer = abi::string(&format!("source-{index:03}-{}", "x".repeat(128)));
            // SAFETY: the allocation remains live until the next collection. The
            // caller immediately lends these sources to the copying helper, whose
            // contract must protect every source throughout its own allocations.
            unsafe { abi::text(pointer) }
        })
        .collect()
}

#[test]
fn string_copy_roots_all_sources_held_only_in_a_rust_vector() {
    // The first copy exceeds the initial collection threshold. Later managed
    // strings exist only as pointers inside the ordinary Rust-owned source Vec.
    let large = "z".repeat(2 * 1024 * 1024);
    let mut sources = managed_sources();
    sources.insert(0, &large);
    let before = memory::stats().collections;
    let copied = abi::strings(&sources);
    assert!(memory::stats().collections > before);
    // SAFETY: strings returns a live initialized StringList containing all copies.
    unsafe {
        assert_eq!((*copied).len, 257);
        assert_eq!(abi::text(*(*copied).data), large);
        for index in 0..256 {
            assert_eq!(
                abi::text(*(*copied).data.add(index + 1)),
                format!("source-{index:03}-{}", "x".repeat(128))
            );
        }
    }
}
