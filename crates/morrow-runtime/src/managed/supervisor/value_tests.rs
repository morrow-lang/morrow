use super::*;

fn leaf(kind: i64) -> Type {
    Type {
        kind,
        count: 0,
        children: null(),
        arities: null(),
    }
}
unsafe extern "C" fn done(_: *mut Exec, _: *mut c_void) -> i64 {
    2
}

#[test]
fn copied_key_keeps_exact_authority_and_owns_its_name_after_source_gc() {
    unsafe {
        let scalar = leaf(0);
        let string = leaf(1);
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        let exec = lifecycle::new_local(&mut fault, functions.as_ptr(), 1);
        let mut roots = Box::new([exec as usize, 0, 0]);
        let root = memory::root_range(roots.as_ptr(), roots.len());
        let name = abi::string("worker 🌿");
        let key = morrow_managed_supervisor_child_key(exec, name, &scalar).cast::<ChildKey>();
        assert!(!key.is_null());
        roots[1] = key as usize;
        let children = [&scalar as *const Type];
        let key_type = Type {
            count: 1,
            children: children.as_ptr(),
            ..leaf(16)
        };
        assert_eq!(
            cost::value((*exec).session, &key_type, key as i64),
            Some(16 + 24 + "worker 🌿".len() + 1)
        );
        let copied = copy::value((*exec).session, &key_type, key as i64);
        roots[2] = copied.value as usize;
        let copy = copied.value as *const ChildKey;
        assert_ne!(copy, key);
        assert_ne!((*copy).name, (*key).name);
        assert_eq!((*copy).token, (*key).token);
        assert!(valid_key((*exec).session, copy, &scalar));
        assert!(!valid_key((*exec).session, copy, &string));
        let weak = control::observe((*key).token);
        drop(copied);
        roots[1] = 0;
        memory::morrow_gc_collect_precise();
        assert!(!memory::heap_owns(0, key.cast()));
        assert_eq!(
            std::ffi::CStr::from_ptr((*copy).name).to_str().unwrap(),
            "worker 🌿"
        );
        assert!(weak.upgrade().is_some());
        morrow_managed_close(exec);
        roots[0] = 0;
        memory::morrow_gc_collect_precise();
        assert!(
            weak.upgrade().is_some(),
            "key token outlives closed invocation without Session ownership"
        );
        roots[2] = 0;
        drop(root);
        memory::morrow_gc_collect_precise();
        assert!(weak.upgrade().is_none());
        assert_eq!(fault, 0);
    }
}

#[test]
fn worker_template_copies_wide_owned_captures_and_never_reuses_live_state() {
    unsafe extern "C" fn idle(_: *mut Exec, _: *mut c_void) -> i64 {
        2
    }
    unsafe {
        let scalar = leaf(0);
        let string = leaf(1);
        let json = leaf(12);
        let pid_children = [&scalar as *const Type];
        let pid_type = Type {
            count: 1,
            children: pid_children.as_ptr(),
            ..leaf(6)
        };
        let identity_type = leaf(13);
        let captures = [
            &scalar as *const Type,
            &scalar,
            &string,
            &string,
            &json,
            &pid_type,
            &identity_type,
        ];
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: captures.len() as i64,
            captures: captures.as_ptr(),
            mailbox: &scalar,
        };
        let idle_function = Function {
            identity: idle as *const c_void,
            step: Some(idle),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function, &idle_function];
        let mut fault = 0;
        let exec = lifecycle::new_local(&mut fault, functions.as_ptr(), 2);
        let mut roots = Box::new([exec as usize, 0, 0, 0, 0, 0, 0, 0]);
        let root = memory::root_range(roots.as_ptr(), roots.len());
        let key = morrow_managed_supervisor_child_key(exec, c"worker".as_ptr(), &scalar);
        assert!(!key.is_null());
        roots[1] = key as usize;
        let mut idle_entry = [idle as *const () as i64];
        let spawned = process::morrow_process_spawn(exec, idle_entry.as_mut_ptr().cast(), &scalar)
            as *const abi::ResultValue;
        assert_eq!((*spawned).tag, 0);
        let pid = (*spawned).value as *mut Pid;
        roots[4] = pid as usize;
        let identity = process::identity((*exec).session, (*pid).actor);
        roots[5] = identity as usize;
        let text = abi::string("immutable 🌿");
        roots[6] = text as usize;
        let json_value = crate::json::morrow_json_value_from_int(i64::MIN);
        roots[7] = json_value as usize;
        let node = crate::json::node(json_value);
        let json_weak = std::rc::Rc::downgrade(&node);
        drop(node);
        let entry = memory::alloc(64, false).cast::<i64>();
        let words = [
            done as *const () as i64,
            i64::MAX,
            (-0.0_f64).to_bits() as i64,
            text as i64,
            text as i64,
            json_value as i64,
            pid as i64,
            identity as i64,
        ];
        std::ptr::copy_nonoverlapping(words.as_ptr(), entry, words.len());
        roots[2] = entry as usize;
        let permanent = [0_i64];
        let infinity = [1_i64];
        let policy = [0, permanent.as_ptr() as i64, infinity.as_ptr() as i64, 0];
        process::COLLECT_CONSTRUCTION.with(|v| v.set(true));
        let result = morrow_managed_supervisor_worker(exec, key, entry.cast(), policy.as_ptr())
            as *const abi::ResultValue;
        process::COLLECT_CONSTRUCTION.with(|v| v.set(false));
        assert!(!result.is_null());
        assert_eq!((*result).tag, 0);
        let spec = (*result).value as *const ChildSpec;
        roots[3] = spec as usize;
        assert_eq!(std::mem::size_of::<ChildSpec>(), 14 * 8);
        assert_ne!((*spec).initializer, entry.cast());
        assert_ne!((*spec).key.cast::<c_void>(), key);
        roots[1] = 0;
        roots[2] = 0;
        roots[4..].fill(0);
        memory::morrow_gc_collect_precise();
        assert!(!memory::heap_owns(0, entry.cast()));
        assert!(!memory::heap_owns(0, text.cast()));
        assert!(
            json_weak.upgrade().is_none(),
            "copy must not keep source JSON Rc nodes"
        );
        let template = (*spec).initializer.cast::<i64>();
        assert_eq!(*template.add(1), i64::MAX);
        assert_eq!(*template.add(2) as u64, (-0.0_f64).to_bits());
        assert_eq!(
            *template.add(3),
            *template.add(4),
            "capture DAG sharing is preserved"
        );
        assert_eq!(
            std::ffi::CStr::from_ptr(*template.add(3) as *const _)
                .to_str()
                .unwrap(),
            "immutable 🌿"
        );
        let first = copy::frame((*exec).session, template.cast());
        *(first.value as *mut i64).add(1) = 42;
        let second = copy::frame((*exec).session, template.cast());
        assert_eq!(*(second.value as *const i64).add(1), i64::MAX);
        assert_ne!(first.value, second.value);
        assert_eq!(done(exec, second.value as *mut c_void), 2);
        drop(first);
        drop(second);
        let spec_type = leaf(17);
        let fragment = copy::value_fragment((*exec).session, &spec_type, spec as i64);
        morrow_managed_close(exec);
        drop(root);
        memory::morrow_gc_collect_precise();
        std::thread::spawn(move || {
            let value = fragment.adopt();
            let word = value as usize;
            let root = memory::root_range(&word, 1);
            memory::morrow_gc_collect_precise();
            let spec = &*(value as *const ChildSpec);
            let frame = spec.initializer.cast::<i64>();
            assert_eq!(*frame.add(1), i64::MAX);
            assert_eq!(*frame.add(2) as u64, (-0.0_f64).to_bits());
            let pid = &*(*frame.add(6) as *const Pid);
            let identity = &*(*frame.add(7) as *const process::Identity);
            assert_eq!(pid.actor, identity.actor);
            assert_eq!((*pid.actor).identity.id, identity.generation);
            assert!(!(*pid.actor).identity.alive.load(Ordering::Acquire));
            assert_eq!(
                std::ffi::CStr::from_ptr((*spec.key).name).to_str().unwrap(),
                "worker"
            );
            assert!(memory::verify_heap_edges().is_ok());
            drop(root);
            memory::morrow_gc_collect_precise();
            assert_eq!(memory::stats().bytes, 0);
        })
        .join()
        .unwrap();
        assert_eq!(fault, 0);
    }
}

#[test]
fn constructor_quota_and_options_fail_without_retaining_a_partial_key_or_template() {
    unsafe {
        let scalar = leaf(0);
        let function = Function {
            identity: done as *const c_void,
            step: Some(done),
            select: None,
            capture_count: 0,
            captures: null(),
            mailbox: &scalar,
        };
        let functions = [&function as *const Function];
        let mut fault = 0;
        let exec = lifecycle::new_local(&mut fault, functions.as_ptr(), 1);
        let exec_word = exec as usize;
        let root = memory::root_range(&exec_word, 1);
        let s = (*exec).session;
        let original = (*s).retained;
        let pressure = BYTES - original - 1;
        assert!(charge(s, pressure));
        let before = memory::stats().bytes;
        assert!(morrow_managed_supervisor_child_key(exec, c"key".as_ptr(), &scalar).is_null());
        assert_eq!(fault, 9);
        assert_eq!((*s).retained, original + pressure);
        assert_eq!(
            memory::stats().bytes,
            before,
            "declined key allocates no control or payload"
        );
        release(s, pressure);
        fault = 0;
        let oversized = vec![b'x'; 4097];
        assert!(
            morrow_managed_supervisor_child_key(exec, oversized.as_ptr().cast(), &scalar).is_null()
        );
        assert_eq!(fault, 9);
        assert_eq!((*s).retained, original);
        fault = 0;
        let permanent = [0_i64];
        let infinity = [1_i64];
        let bad_policy = [0, permanent.as_ptr() as i64, infinity.as_ptr() as i64, 1];
        let result = morrow_managed_supervisor_worker(exec, null(), null(), bad_policy.as_ptr())
            as *const abi::ResultValue;
        assert_eq!((*result).tag, 1);
        assert_eq!(
            *((*result).value as *const i64),
            0,
            "invalid options is typed Error tag0"
        );
        assert_eq!(fault, 0);
        assert_eq!((*s).retained, original);
        morrow_managed_close(exec);
        drop(root);
        memory::morrow_gc_collect_precise();
    }
}
