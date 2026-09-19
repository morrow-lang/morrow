//! Independent native descriptor and payload oracles for opaque supervision values.
//! The numeric kinds and word layout deliberately do not import production
//! opaque structs, so missing traversal cannot make these expectations pass.
use super::*;

fn ty(kind: i64) -> Type {
    Type {
        kind,
        count: 0,
        children: null(),
        arities: null(),
    }
}

fn branch(name: *const u8, children: *const i64, len: usize) -> [i64; 14] {
    // Branch, no worker key/initializer, permanent restart, infinite shutdown,
    // nonsignificant, one-for-one, intensity1/period1, no auto shutdown.
    [
        1,
        0,
        name as i64,
        0,
        children as i64,
        len as i64,
        0,
        1,
        0,
        0,
        0,
        1,
        1,
        0,
    ]
}

#[test]
fn supervisor_descriptor_children_are_constraints_not_payload_fields() {
    let scalar = ty(0);
    let children = [&scalar as *const Type];
    unsafe {
        for kind in [15, 17] {
            let opaque = ty(kind);
            let mut work = 0;
            assert!(
                descriptor(&opaque, false, &mut work),
                "kind {kind} is a zero-child opaque value"
            );
            let malformed = Type {
                count: 1,
                children: children.as_ptr(),
                ..ty(kind)
            };
            assert!(!descriptor(&malformed, false, &mut 0));
        }
        let key = Type {
            count: 1,
            children: children.as_ptr(),
            ..ty(16)
        };
        assert!(
            descriptor(&key, false, &mut 0),
            "ChildKey retains its mailbox constraint"
        );
        assert!(!descriptor(&ty(16), false, &mut 0));
        let invalid = ty(99);
        let invalid_children = [&invalid as *const Type];
        let key = Type {
            children: invalid_children.as_ptr(),
            ..key
        };
        assert!(
            !descriptor(&key, false, &mut 0),
            "opaque keys do not waive descriptor validation"
        );
    }
}

#[test]
fn opaque_child_spec_accounts_its_hidden_name_and_branch_graph() {
    let spec_type = ty(17);
    let leaf = branch(c"leaf".as_ptr().cast(), null(), 0);
    let children = [leaf.as_ptr() as i64];
    let parent = branch(c"root".as_ptr().cast(), children.as_ptr(), 1);
    let mut session = Session::default();
    unsafe {
        assert_eq!(
            value(&mut session, &spec_type, leaf.as_ptr() as i64),
            Some(112 + 5)
        );
        assert_eq!(
            value(&mut session, &spec_type, parent.as_ptr() as i64),
            Some(112 + 5 + 8 + 112 + 5)
        );
    }
}

#[test]
fn repeated_opaque_value_charges_one_owned_graph_and_rejects_a_cycle() {
    let spec_type = ty(17);
    let leaf = branch(c"leaf".as_ptr().cast(), null(), 0);
    let data = [leaf.as_ptr() as i64; 2];
    let list = abi::List {
        len: 2,
        cap: 2,
        data: data.as_ptr().cast_mut(),
    };
    let children = [&spec_type as *const Type];
    let list_type = Type {
        kind: 2,
        count: 1,
        children: children.as_ptr(),
        arities: null(),
    };
    let mut session = Session::default();
    unsafe {
        assert_eq!(
            value(&mut session, &list_type, &list as *const _ as i64),
            Some(24 + 16 + 112 + 5)
        );
        let mut cyclic = branch(c"cycle".as_ptr().cast(), null(), 1);
        let cyclic_children = [cyclic.as_ptr() as i64];
        cyclic[4] = cyclic_children.as_ptr() as i64;
        assert_eq!(
            value(&mut session, &spec_type, cyclic.as_ptr() as i64),
            None
        );
    }
}

#[test]
fn opaque_branch_fragment_owns_names_and_children_after_sender_collection() {
    let fragment = std::thread::spawn(|| unsafe {
        let descriptor = ty(17);
        let name = abi::string("leaf 🌿");
        let leaf = branch(name.cast(), null(), 0);
        let children = [leaf.as_ptr() as i64];
        let parent_name = abi::string("root");
        let parent = branch(parent_name.cast(), children.as_ptr(), 1);
        assert!(value(null_mut(), &descriptor, parent.as_ptr() as i64).is_some());
        let fragment = copy::value_fragment(null_mut(), &descriptor, parent.as_ptr() as i64);
        assert_ne!(fragment.value, parent.as_ptr() as i64);
        memory::morrow_gc_collect_precise();
        assert!(!memory::heap_owns(0, name.cast()));
        assert!(!memory::heap_owns(0, parent_name.cast()));
        fragment
    })
    .join()
    .unwrap();
    std::thread::spawn(move || unsafe {
        let address = fragment.value;
        let adopted = fragment.adopt();
        assert_eq!(
            adopted, address,
            "adoption preserves native block addresses"
        );
        let root_word = adopted as usize;
        let root = memory::root_range(&root_word, 1);
        memory::morrow_gc_collect_precise();
        let parent = std::slice::from_raw_parts(adopted as *const i64, 14);
        assert_eq!(parent[0], 1);
        assert_eq!(parent[5], 1);
        assert_eq!(
            std::ffi::CStr::from_ptr(parent[2] as *const _)
                .to_str()
                .unwrap(),
            "root"
        );
        let child = *(parent[4] as *const i64) as *const i64;
        let child = std::slice::from_raw_parts(child, 14);
        assert_eq!(child[0], 1);
        assert_eq!(child[5], 0);
        assert_eq!(
            std::ffi::CStr::from_ptr(child[2] as *const _)
                .to_str()
                .unwrap(),
            "leaf 🌿"
        );
        assert!(memory::heap_owns(0, child.as_ptr().cast()));
        assert!(memory::heap_owns(0, (child[2] as *const u8).cast()));
        drop(root);
        memory::morrow_gc_collect_precise();
        assert_eq!(
            memory::stats().bytes,
            0,
            "every adopted payload is released once"
        );
    })
    .join()
    .unwrap();
}
