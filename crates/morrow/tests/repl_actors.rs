use morrow_compiler::repl::Session;

#[test]
fn waiting_actors_persist_across_entries_without_replaying_effects() {
    let mut session = Session::default();
    session
        .evaluate("fn worker():\n    receive:\n        message -> println(message)")
        .unwrap();
    assert_eq!(
        session
            .evaluate("let pid: Pid(Int) = spawn(worker)")
            .unwrap(),
        ""
    );
    assert_eq!(
        session
            .evaluate("match send(pid, 42):\n    Ok(()) -> ()\n    Err(_) -> ()")
            .unwrap(),
        "42\n"
    );
    assert_eq!(
        session.evaluate("Result.is_err(send(pid, 9))").unwrap(),
        "true : Bool\n"
    );
}

#[test]
fn selective_receive_keeps_unmatched_messages_in_order() {
    let mut session = Session::default();
    session.evaluate("fn worker():\n    receive:\n        2 -> println(2)\n    receive:\n        value -> println(value)").unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    assert_eq!(
        session.evaluate("Result.is_ok(send(pid, 1))").unwrap(),
        "true : Bool\n"
    );
    assert_eq!(
        session
            .evaluate("match send(pid, 2):\n    Ok(()) -> ()\n    Err(_) -> ()")
            .unwrap(),
        "2\n1\n"
    );
}

#[test]
fn receive_timeouts_use_virtual_time_and_do_not_sleep() {
    let mut session = Session::default();
    session.evaluate("fn worker():\n    receive:\n        1 -> println(1)\n        _ after 1000000 -> println(9)").unwrap();
    assert_eq!(
        session
            .evaluate("let pid: Pid(Int) = spawn(worker)")
            .unwrap(),
        "9\n"
    );
}

#[test]
fn invalid_actor_code_is_rejected_before_prior_file_effects() {
    let directory = std::env::temp_dir().join(format!("morrow-actors-repl-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("must-not-exist");
    let source = format!(
        "match File.write(\"{}\", \"bad\"):\n    Ok(_) -> ()\n    Err(_) -> ()\nlet pid: Pid(()) = spawn(42)",
        path.display()
    );
    assert!(Session::default().evaluate(&source).is_err());
    let side_effect = path.exists();
    if side_effect {
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::remove_dir(&directory).unwrap();
    assert!(!side_effect, "type failure must precede external effects");
}

#[test]
fn supervision_cleans_up_restarts_and_preserves_sibling_fairness() {
    let definitions = "fn broken():\n    defer println(\"cleanup\")\n    println(\"attempt\")\n    let empty: List(Int) = []\n    println(List.head(empty))\nfn sibling(): println(\"sibling\")";
    let body =
        "let failed: Pid(()) = supervise(broken, 2)\nlet other: Pid(()) = spawn(sibling)\n()";
    let replay = morrow_compiler::repl::simulate_actors(&[definitions, body]).unwrap();
    assert_eq!(
        replay.outcomes[1].as_deref().unwrap(),
        "attempt\ncleanup\nsibling\nattempt\ncleanup\nattempt\ncleanup\n"
    );
    assert_eq!(replay.actors.live, 0);
    assert_eq!(replay.actors.spawned, 4);
}

#[test]
fn seeded_mailbox_workload_matches_independent_fifo_round_robin_model() {
    use std::collections::VecDeque;
    for seed in [1u64, 17, 0x1234abcd] {
        let mut state = seed;
        let mut queues = vec![VecDeque::new(); 5];
        let mut source = String::new();
        for i in 0..5 {
            source.push_str(&format!("let p{i}: Pid(Int) = spawn(() -> worker(16))\n"));
        }
        // Generate independently permuted per-actor payloads before running any actor.
        for n in 0..16 {
            for (i, queue) in queues.iter_mut().enumerate() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let value = (state as i64).wrapping_add(n);
                queue.push_back(value);
                source.push_str(&format!("match send(p{i}, {value}):\n    Ok(()) -> ()\n    Err(_) -> println(\"send failed\")\n"));
            }
        }
        source.push_str("()\n");
        let mut ready: VecDeque<_> = (0..5).collect();
        let mut expected = String::new();
        while let Some(id) = ready.pop_front() {
            if let Some(value) = queues[id].pop_front() {
                expected.push_str(&format!("{value}\n"));
                if !queues[id].is_empty() {
                    ready.push_back(id);
                }
            }
        }
        let definition = "fn worker(n: Int):\n    if n > 0:\n        receive:\n            message -> println(message)\n        worker(n - 1)";
        let replay = morrow_compiler::repl::simulate_actors(&[definition, &source]).unwrap();
        assert_eq!(
            replay.outcomes[1].as_deref().unwrap(),
            expected,
            "seed={seed}"
        );
        assert_eq!(replay.actors.live, 0);
        assert_eq!(replay.actors.queued_messages, 0);
        assert_eq!(
            morrow_compiler::repl::simulate_actors(&[definition, &source]).unwrap(),
            replay
        );
    }
}

#[test]
fn virtual_deadlines_progress_while_other_actors_remain_runnable() {
    let definitions = "fn busy(n: Int):\n    if n > 0:\n        println(n)\n        busy(n - 1)\nfn timer():\n    receive:\n        () -> ()\n        _ after 3 -> println(\"timer\")";
    let body =
        "let worker: Pid(()) = spawn(() -> busy(50))\nlet timeout: Pid(()) = spawn(timer)\n()";
    let replay = morrow_compiler::repl::simulate_actors(&[definitions, body]).unwrap();
    let output = replay.outcomes[1].as_deref().unwrap();
    assert!(
        output.find("timer\n").unwrap() < output.rfind("1\n").unwrap(),
        "{output}"
    );
    assert_eq!(replay.actors.live, 0);
}

#[test]
fn actor_results_are_checked_and_simulation_never_touches_host_files() {
    let mut session = Session::default();
    session
        .evaluate("fn worker():\n    receive:\n        value -> ()")
        .unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    assert!(session.evaluate("send(pid, 42)\n()").is_err());
    assert_eq!(session.actor_report().queued_messages, 0);
    let replay = morrow_compiler::repl::simulate_actors(&[
        "Result.is_err(File.read(\"/tmp/morrow-simulation-must-not-read\"))",
    ])
    .unwrap();
    assert!(
        replay.outcomes[0]
            .as_ref()
            .unwrap_err()
            .contains("host APIs"),
        "{:?}",
        replay.outcomes
    );
}

#[test]
fn failed_entry_keeps_executed_actor_effects_but_does_not_install_bindings() {
    let mut session = Session::default();
    session
        .evaluate("fn worker():\n    receive:\n        value -> println(value)")
        .unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    session.evaluate("fn failing(pid: Pid(Int)) -> Int:\n    match send(pid, 9):\n        Ok(()) -> ()\n        Err(_) -> ()\n    let empty: List(Int) = []\n    List.head(empty)").unwrap();
    let failed = "let lost = failing(pid)";
    let failure = session.evaluate(failed).unwrap_err();
    assert!(failure.contains("head of empty list"), "{failure}");
    assert!(session.expression_type("lost").is_err());
    assert_eq!(session.evaluate("()").unwrap(), "9\n");
}

#[test]
fn persistent_actor_limits_recover_capacity_without_reviving_old_pids() {
    let mut session = Session::default();
    session
        .evaluate("fn worker():\n    receive:\n        value -> ()")
        .unwrap();
    session
        .evaluate("let first: Pid(Int) = spawn(worker)")
        .unwrap();
    session
        .evaluate("for n in 0..255:\n    let pid: Pid(Int) = spawn(worker)\n    ()\n()")
        .unwrap();
    assert_eq!(session.actor_report().live, 256);
    assert!(
        session
            .evaluate("let overflow: Pid(Int) = spawn(worker)")
            .unwrap_err()
            .contains("actor limit")
    );
    assert_eq!(session.actor_report().live, 256);
    session
        .evaluate("match send(first, 1):\n    Ok(()) -> ()\n    Err(_) -> ()")
        .unwrap();
    session
        .evaluate("let replacement: Pid(Int) = spawn(worker)")
        .unwrap();
    assert_eq!(session.actor_report().live, 256);
    assert_eq!(
        session.evaluate("Result.is_err(send(first, 2))").unwrap(),
        "true : Bool\n"
    );
}

#[test]
fn mailbox_budget_is_aggregate_across_persistent_entries() {
    let mut session = Session::default();
    session
        .evaluate("fn worker():\n    receive:\n        999999 -> ()")
        .unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    let batch = "for n in 0..1024:\n    match send(pid, n):\n        Ok(()) -> ()\n        Err(_) -> println(\"unexpected full\")\n()";
    for _ in 0..4 {
        assert_eq!(session.evaluate(batch).unwrap(), "");
    }
    assert_eq!(session.actor_report().queued_messages, 4096);
    assert_eq!(
        session.evaluate("Result.is_err(send(pid, 7))").unwrap(),
        "true : Bool\n"
    );
    assert_eq!(session.actor_report().queued_messages, 4096);
}

#[test]
fn suspended_actor_cleanup_waits_until_logical_completion() {
    let mut session = Session::default();
    session.evaluate("fn helper():\n    defer println(\"helper cleanup\")\n    println(\"helper body\")\nfn worker():\n    defer println(\"actor cleanup\")\n    receive:\n        1 -> helper()").unwrap();
    assert_eq!(
        session
            .evaluate("let pid: Pid(Int) = spawn(worker)")
            .unwrap(),
        ""
    );
    assert_eq!(
        session
            .evaluate("match send(pid, 1):\n    Ok(()) -> ()\n    Err(_) -> ()")
            .unwrap(),
        "helper body\nhelper cleanup\nactor cleanup\n"
    );
}

#[test]
fn cancellation_drains_waiting_actors_and_does_not_reuse_their_pids() {
    let mut session = Session::default();
    session
        .evaluate("fn worker():\n    defer println(\"cleanup\")\n    receive:\n        1 -> ()")
        .unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    assert_eq!(session.stop_actors().unwrap(), "cleanup\n");
    assert_eq!(session.actor_report().live, 0);
    assert_eq!(session.stop_actors().unwrap(), "");
    session
        .evaluate("let replacement: Pid(Int) = spawn(worker)")
        .unwrap();
    assert_eq!(
        session.evaluate("Result.is_err(send(pid, 1))").unwrap(),
        "true : Bool\n"
    );
    assert_eq!(session.stop_actors().unwrap(), "cleanup\n");
}

#[test]
fn actor_cleanup_faults_preserve_original_failure_and_continue_other_callbacks() {
    let definitions = "fn bad_cleanup():\n    let empty: List(Int) = []\n    println(List.get(empty, 4))\nfn worker():\n    defer println(\"first\")\n    defer bad_cleanup()\n    defer println(\"last\")\n    receive:\n        1 ->\n            let empty: List(Int) = []\n            println(List.head(empty))";
    let mut session = Session::default();
    session.evaluate(definitions).unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    let failure = session
        .evaluate("match send(pid, 1):\n    Ok(()) -> ()\n    Err(_) -> ()")
        .unwrap_err();
    assert!(failure.contains("head of empty list"), "{failure}");
    assert_eq!(session.actor_report().live, 0);
    let replay = morrow_compiler::repl::simulate_actors(&[
        definitions,
        "let pid: Pid(Int) = supervise(worker, 0)",
        "match send(pid, 1):\n    Ok(()) -> ()\n    Err(_) -> ()",
    ])
    .unwrap();
    assert_eq!(replay.outcomes[2].as_deref().unwrap(), "last\nfirst\n");
    assert_eq!(replay.actors.live, 0);
}

#[test]
fn immutable_unicode_sets_cross_mailboxes_without_changing_captured_aliases() {
    let mut session = Session::default();
    session.evaluate("type Message:\n    Values(Set(String))\nfn worker(original: Set(String)):\n    receive:\n        Values(message) ->\n            println(Set.len(original))\n            println(Set.len(message))\n            println(Set.contains(original, \"こんにちは\"))\n            println(Set.contains(message, \"🌿\"))").unwrap();
    session
        .evaluate("let original = Set.from_list([\"🌿\", \"café\"])")
        .unwrap();
    session
        .evaluate("let pid: Pid(Message) = spawn(() -> worker(original))")
        .unwrap();
    session
        .evaluate("let changed = Set.insert(original, \"こんにちは\")")
        .unwrap();
    assert_eq!(
        session
            .evaluate("match send(pid, Values(changed)):\n    Ok(()) -> ()\n    Err(_) -> ()")
            .unwrap(),
        "2\n3\nfalse\ntrue\n"
    );
    assert_eq!(session.evaluate("Set.len(original)").unwrap(), "2 : Int\n");
}

#[test]
fn supervised_current_distinguishes_live_replacements_from_stale_identities() {
    let definitions = "fn broken():\n    let empty: List(Int) = []\n    println(List.head(empty))\nfn observe(original: Pid(())):\n    match send(original, ()):\n        Ok(()) -> println(\"wrong stale\")\n        Err(_) -> println(\"stale\")\n    match supervised_current(original):\n        Ok(current) ->\n            match send(current, ()):\n                Ok(()) -> println(\"fresh\")\n                Err(_) -> println(\"wrong fresh\")\n        Err(_) -> println(\"missing\")";
    let body = "let original: Pid(()) = supervise(broken, 1)\nlet observer: Pid(()) = spawn(() -> observe(original))\n()";
    let replay = morrow_compiler::repl::simulate_actors(&[definitions, body]).unwrap();
    assert_eq!(replay.outcomes[1].as_deref().unwrap(), "stale\nfresh\n");
    assert_eq!(replay.actors.live, 0);
}

#[test]
fn repl_quit_eof_and_reset_cancel_waiting_actor_scopes() {
    let setup = "fn worker():\n    defer println(\"cancelled\")\n    receive:\n        1 -> ()\n\nlet pid: Pid(Int) = spawn(worker)\n";
    for ending in ["", ":quit\n", ":reset\n:quit\n"] {
        let mut output = Vec::new();
        morrow_compiler::repl::serve(format!("{setup}{ending}").as_bytes(), &mut output, false)
            .unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "cancelled\n",
            "ending={ending:?}"
        );
    }
}

#[test]
fn deterministic_transcripts_can_replay_explicit_cancellation() {
    let entries = [
        "fn worker():\n    defer println(\"cancelled\")\n    receive:\n        1 -> ()",
        "let pid: Pid(Int) = spawn(worker)",
        ":stop",
        "Result.is_err(send(pid, 1))",
    ];
    let expected = morrow_compiler::repl::simulate_actors(&entries).unwrap();
    assert_eq!(expected.outcomes[2].as_deref().unwrap(), "cancelled\n");
    assert_eq!(expected.outcomes[3].as_deref().unwrap(), "true : Bool\n");
    assert_eq!(expected.actors.pending_cleanups, 0);
    assert_eq!(expected.actors.scope_frames, 0);
    assert_eq!(
        expected,
        morrow_compiler::repl::simulate_actors(&entries).unwrap()
    );
}

#[test]
fn result_sequencing_actor_campaign_matches_independent_rust_model() {
    let definitions = include_str!("actors/result_cps.fn");
    for mut seed in [1_u64, 0x4645524e, u64::MAX] {
        let mut session = Session::default();
        session.evaluate(definitions).unwrap();
        for turn in 0..18 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mode = (seed % 3) as i64;
            let n = ((seed >> 8) % 4) as i64 - 1;
            session
                .evaluate(&format!(
                    "let pid{turn}: Pid(Int) = spawn(() -> worker(mode: {mode}, n: {n}))"
                ))
                .unwrap();
            let first = session
                .evaluate(&format!(
                    "match send(pid{turn}, 1):\n    Ok(()) -> ()\n    Err(_) -> ()"
                ))
                .unwrap();
            let second = session
                .evaluate(&format!(
                    "match send(pid{turn}, 1):\n    Ok(()) -> ()\n    Err(_) -> ()"
                ))
                .unwrap();
            // Each Result short-circuits before the second receive after an initial error.
            let value: Result<(i64, &str), String> = if n < 0 {
                Err(format!("bad {n}"))
            } else {
                Ok((n, "🌿"))
            };
            let result = value
                .and_then(|(value, text)| {
                    if mode == 0 {
                        Ok(format!("{text} {value}"))
                    } else if mode == 2 && value == 0 {
                        Err("bad -9".to_owned())
                    } else {
                        Ok(format!("{text} {}", value + 1))
                    }
                })
                .unwrap_or_else(|error| error);
            let kind = ["try", "with", "handled"][mode as usize];
            let expected = if mode == 2 {
                format!("{kind} {n}: {result}\n{kind} cleanup {n}\n")
            } else {
                format!("{kind} cleanup {n}\n{kind} {n}: {result}\n")
            };
            assert_eq!(
                format!("{first}{second}"),
                expected,
                "seed {seed}, mode {mode}, n {n}"
            );
            assert_eq!(session.actor_report().live, 0);
        }
    }
}

#[test]
fn receiving_helpers_preserve_nested_result_obligations() {
    let prefix = "fn nested() -> Result(Result(Int,String),String):\n    receive:\n        1 -> ()\n    Ok(Err(\"inner\"))\n";
    let handled = "fn worker():\n    with\n        inner <- nested()\n    do\n        match inner:\n            Ok(value) -> println(value)\n            Err(error) -> println(error)\n    else\n        Err(error) -> println(error)\n";
    let mut session = Session::default();
    session.evaluate(&format!("{prefix}{handled}")).unwrap();
    session
        .evaluate("let pid: Pid(Int) = spawn(worker)")
        .unwrap();
    assert_eq!(
        session
            .evaluate("match send(pid,1):\n    Ok(()) -> ()\n    Err(_) -> ()")
            .unwrap(),
        "inner\n"
    );
    for body in [
        "fn worker():\n    let ignored = nested()\n    ()\n",
        "fn worker():\n    with\n        ignored <- nested()\n    do\n        ()\n    else\n        Err(_) -> ()\n",
    ] {
        let error = Session::default()
            .evaluate(&format!("{prefix}{body}"))
            .unwrap_err();
        assert!(
            error.contains("Result") && !error.contains("does not yet prove"),
            "{error}"
        );
    }
}

#[test]
fn sum_callbacks_are_eager_to_create_selective_to_call_and_cooperative() {
    let mut session = Session::default();
    session
        .evaluate(&include_str!("actors/sums_cps.fn").replace("fn main():", "fn sum_campaign():"))
        .unwrap();
    assert_eq!(
        session.evaluate("sum_campaign()").unwrap(),
        include_str!("actors/sums_cps.stdout")
    );
}

#[test]
fn dynamic_actor_callbacks_keep_their_origin_across_new_definitions() {
    let mut session = Session::default();
    session
        .evaluate("fn apply(action: (Int) -> Int): println(action(4))")
        .unwrap();
    session
        .evaluate("let action = (value: Int) -> value + 100")
        .unwrap();
    session
        .evaluate("fn later(value: Int) -> Int: value + 999")
        .unwrap();
    session
        .evaluate(
            "fn new_worker(action: (Int) -> Int):\n    let ignored = later(0)\n    apply(action)",
        )
        .unwrap();
    assert_eq!(
        session
            .evaluate("let pid: Pid(()) = spawn(() -> new_worker(action))")
            .unwrap(),
        "104\n"
    );
}
