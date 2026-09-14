use fern_message_path::{Case, Codec, run};

#[test]
fn each_codec_applies_exact_transitions_and_recovers_durable_state() {
    for codec in [Codec::Json, Codec::Cbor, Codec::Protobuf] {
        for durable in [false, true] {
            let result = run(Case {
                codec,
                durable,
                tasks: 3,
                operations: 7,
            })
            .unwrap();
            assert_eq!(result.final_revision, 10);
            assert_eq!(result.final_tasks.len(), 3);
            for (index, task) in result.final_tasks.iter().enumerate() {
                assert_eq!(task.id.0, index as i64 + 1);
                assert_eq!(task.label, format!("Task {}", index + 1));
                assert_eq!(task.done, index == 0);
            }
            assert_eq!(result.restored, durable);
        }
    }
}

#[test]
fn invalid_workload_bounds_fail_before_starting_a_server() {
    for (tasks, operations) in [(0, 1), (101, 1), (1, 0), (1, 2001)] {
        assert!(
            run(Case {
                codec: Codec::Json,
                durable: false,
                tasks,
                operations
            })
            .is_err()
        );
    }
}
