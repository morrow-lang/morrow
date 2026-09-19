//! Independent source oracles for context-sensitive supervisor helpers.
use morrow_compiler::{check, parse, Type};

#[test]
fn transitive_helpers_and_closures_specialize_for_root_and_each_mailbox() {
    let source = include_str!("supervisors/dual_context.mr");
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    for name in ["leaf", "middle", "launch", "stop_leaf", "stop_middle"] {
        let mut mailboxes: Vec<_> = program.functions.iter().filter(|f| f.name == name)
            .map(|f| f.mailbox.clone()).collect();
        mailboxes.sort_by_key(|m| format!("{m:?}"));
        assert_eq!(mailboxes.len(), 3, "one root and two mailbox instances for {name}: {mailboxes:?}");
        assert!(mailboxes.contains(&None));
        assert!(mailboxes.contains(&Some(Type::Int)));
        assert!(mailboxes.contains(&Some(Type::String)));
    }
}

#[test]
fn contextual_closures_cannot_escape_into_actor_or_ordinary_callback() {
    for (source, required) in [
        (include_str!("supervisors/reject_root_closure_escape.mr"), "root"),
        (include_str!("supervisors/reject_ordinary_callback.mr"), "function"),
    ] {
        let error = check::check_library(&parse::parse(source).unwrap()).unwrap_err();
        assert!(error.message.contains(required), "wrong rejection: {error:?}");
    }
}

#[test]
fn startup_terminal_helper_does_not_handle_a_prior_suspended_result() {
    let source = include_str!("supervisors/reject_terminal_result.mr");
    let error = check::check_library(&parse::parse(source).unwrap()).unwrap_err();
    assert!(error.message.contains("Result"), "wrong rejection: {error:?}");
}

#[test]
fn child_keys_preserve_mailbox_constraints_and_start_link_rejects_root() {
    for (source, required) in [
        (include_str!("supervisors/reject_wrong_mailbox.mr"), "type"),
        (include_str!("supervisors/reject_root_start_link.mr"), "actor context"),
    ] {
        let error = check::check_library(&parse::parse(source).unwrap()).unwrap_err();
        assert!(error.message.contains(required), "wrong rejection: {error:?}");
    }
}

#[test]
fn context_specialized_requests_emit_registered_resumes_and_root_adapters() {
    let source = include_str!("supervisors/dual_context.mr");
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    let machine = morrow_compiler::lowering::lower(&program).unwrap();
    morrow_compiler::cranelift::emit_object(&machine).unwrap();
    let text = format!("{machine:?}");
    for name in ["morrow_supervisor_register", "morrow_supervisor_request", "morrow_supervisor_take_reply", "morrow_supervisor_root_request"] {
        assert!(text.contains(name), "missing {name}");
    }
}
