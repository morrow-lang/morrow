use morrow_sim::language;

#[test]
fn source_simulation_replays_selective_mailboxes_and_detects_corrupt_reports() {
    let definitions = "fn worker():\n    receive:\n        2 -> println(\"two\")\n    receive:\n        rest -> println(rest)";
    let spawn = "let pid: Pid(Int) = spawn(worker)";
    let unmatched = "Result.is_ok(send(pid, -9223372036854775808))";
    let matched = "Result.is_ok(send(pid, 2))";
    let entries = [definitions, spawn, unmatched, matched];
    let expected = language::run(&entries).unwrap();
    assert_eq!(expected.outcomes[2].as_deref().unwrap(), "true : Bool\n");
    assert_eq!(
        expected.outcomes[3].as_deref().unwrap(),
        "two\n-9223372036854775808\ntrue : Bool\n"
    );
    assert_eq!(expected.actors.live, 0);
    assert_eq!(expected.actors.queued_messages, 0);
    assert_eq!(language::replay(&entries, &expected).unwrap(), expected);
    let mut corrupt = expected;
    corrupt.actors.spawned += 1;
    assert!(language::replay(&entries, &corrupt).is_err());
}

#[test]
fn seeded_source_lifecycle_fault_and_cancellation_campaign_matches_independent_model() {
    #[derive(Clone)]
    struct ExpectedWorker {
        id: u64,
        alive: bool,
        remaining: u32,
        label: String,
    }
    let definition = "fn worker(label: String):\n    defer println(\"cleanup \" + label)\n    receive:\n        0 -> println(\"done \" + label)\n        1 ->\n            let empty: List(Int) = []\n            println(List.head(empty))";
    for seed in [1u64, 7, 17, 29, 101, 337, 0x1234abcd, u64::MAX] {
        let mut random = seed;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random
        };
        let count = 3 + (next() % 4) as usize;
        let mut workers = Vec::new();
        let mut initialize = String::new();
        for i in 0..count {
            let remaining = (next() % 3) as u32;
            let label = format!("🌿-{i}");
            workers.push(ExpectedWorker {
                id: i as u64 + 1,
                alive: true,
                remaining,
                label: label.clone(),
            });
            initialize.push_str(&format!(
                "let p{i}: Pid(Int) = supervise(() -> worker(\"{label}\"), {remaining})\n"
            ));
        }
        let mut issued = count as u64;
        let mut entries = vec![definition.to_owned(), initialize];
        let mut expected = vec![String::new(), String::new()];
        // Keep worker0 dormant deliberately so every seed exercises cancellation.
        for _ in 0..18 {
            let index = 1 + (next() % (count as u64 - 1)) as usize;
            let value = (next() % 2) as i64;
            entries.push(format!("match supervised_current(p{index}):\n    Ok(current) ->\n        match send(current, {value}):\n            Ok(()) -> ()\n            Err(_) -> println(\"unexpected send failure\")\n    Err(_) -> println(\"gone\")"));
            let worker = &mut workers[index];
            let output = if !worker.alive {
                "gone\n".to_owned()
            } else if value == 0 {
                worker.alive = false;
                format!("done {}\ncleanup {}\n", worker.label, worker.label)
            } else {
                if worker.remaining == 0 {
                    worker.alive = false;
                } else {
                    worker.remaining -= 1;
                    issued += 1;
                    worker.id = issued;
                }
                format!("cleanup {}\n", worker.label)
            };
            expected.push(output);
        }
        workers.sort_by_key(|worker| worker.id);
        let cancellation = workers
            .iter()
            .filter(|worker| worker.alive)
            .map(|worker| format!("cleanup {}\n", worker.label))
            .collect::<String>();
        entries.push(":stop".into());
        expected.push(cancellation);
        let source: Vec<_> = entries.iter().map(String::as_str).collect();
        let actual = language::run(&source).unwrap();
        assert_eq!(
            actual.outcomes,
            expected.into_iter().map(Ok).collect::<Vec<_>>(),
            "seed={seed}"
        );
        assert_eq!(actual.actors.spawned, issued, "seed={seed}");
        assert_eq!(actual.actors.live, 0);
        assert_eq!(actual.actors.queued_messages, 0);
        assert_eq!(actual.actors.pending_cleanups, 0);
        assert_eq!(actual.actors.scope_frames, 0);
        assert_eq!(language::replay(&source, &actual).unwrap(), actual);
    }
}
