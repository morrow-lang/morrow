use fern_compiler::{Type, runtime};
use runtime::{Operation, ValueAbi, lookup};

#[test]
fn gate_c_service_signatures_use_heap_results_and_exact_symbols() {
    let cases = [
        (
            "fs.read",
            vec![Type::String],
            Type::Result(Box::new(Type::String), Box::new(Type::Int)),
            "fern_read_file",
        ),
        (
            "fs.write",
            vec![Type::String, Type::String],
            Type::Result(Box::new(Type::Int), Box::new(Type::Int)),
            "fern_write_file",
        ),
        (
            "http.post",
            vec![Type::String, Type::String],
            Type::Result(Box::new(Type::String), Box::new(Type::Int)),
            "fern_http_post",
        ),
        (
            "sql.close",
            vec![Type::Int],
            Type::Result(Box::new(Type::Int), Box::new(Type::Int)),
            "fern_sql_close",
        ),
        (
            "sql.execute",
            vec![Type::Int, Type::String],
            Type::Result(Box::new(Type::Int), Box::new(Type::Int)),
            "fern_sql_execute",
        ),
        (
            "actors.next",
            vec![Type::Int],
            Type::Result(Box::new(Type::String), Box::new(Type::Int)),
            "fern_actor_next",
        ),
    ];
    for (name, parameters, result, symbol) in cases {
        let signature = lookup(name).unwrap();
        assert_eq!(signature.parameters, parameters, "{name}");
        assert_eq!(signature.return_type, result, "{name}");
        assert_eq!(signature.symbol, symbol);
        assert_eq!(signature.return_abi, ValueAbi::HeapResult);
        assert!(!signature.requires_adapter());
    }
    assert_eq!(lookup("fs.read"), lookup("File.read"));
    assert_eq!(lookup("fs.read"), lookup("read_file"));
}

#[test]
fn packed_options_and_string_lists_require_explicit_conversion() {
    let index = lookup("String.index_of").unwrap();
    assert_eq!(index.return_type, Type::Option(Box::new(Type::Int)));
    assert_eq!(index.return_abi, ValueAbi::PackedOption);
    assert!(index.requires_adapter());
    for name in [
        "String.split",
        "String.lines",
        "System.args",
        "Regex.find_all",
        "Regex.split",
    ] {
        let signature = lookup(name).unwrap();
        assert_eq!(signature.return_abi, ValueAbi::StringList, "{name}");
        assert!(signature.requires_adapter());
    }
    let join = lookup("String.join").unwrap();
    assert_eq!(join.parameter_abi[0], ValueAbi::StringList);
    assert!(join.requires_adapter());
    let directory = lookup("fs.list_dir").unwrap();
    assert_eq!(
        directory.return_type,
        Type::Result(
            Box::new(Type::List(Box::new(Type::String))),
            Box::new(Type::Int)
        )
    );
    assert_eq!(directory.return_abi, ValueAbi::HeapStringListResult);
    assert!(directory.requires_adapter());
}

#[test]
fn integer_boolean_and_void_transports_follow_c_declarations() {
    assert_eq!(
        lookup("System.args_count").unwrap().return_abi,
        ValueAbi::Word64
    );
    assert_eq!(
        lookup("Regex.is_match").unwrap().return_abi,
        ValueAbi::Word64
    );
    assert_eq!(
        lookup("Tui.Prompt.confirm").unwrap().return_abi,
        ValueAbi::Word32
    );
    assert_eq!(
        lookup("Tui.Prompt.select").unwrap().return_abi,
        ValueAbi::Word32
    );
    assert_eq!(lookup("Tui.Term.clear").unwrap().return_abi, ValueAbi::Void);
    assert_eq!(
        lookup("Tui.Term.move_to").unwrap().parameter_abi,
        vec![ValueAbi::Word64, ValueAbi::Word64]
    );
}

#[test]
fn generic_list_and_heap_sum_operations_record_dynamic_dispatch() {
    let get = lookup("List.get").unwrap();
    assert_eq!(get.return_type, Type::Generic("a".into()));
    let contains = lookup("List.contains").unwrap();
    assert_eq!(contains.operation, Operation::ScalarContains);
    assert!(contains.requires_adapter());
    let none = lookup("Option.is_none").unwrap();
    assert_eq!(none.operation, Operation::InvertBool);
    assert_eq!(none.symbol, "fern_result_is_ok");
    assert_eq!(none.parameter_abi, vec![ValueAbi::HeapOption]);
    let unwrap = lookup("Result.unwrap_or").unwrap();
    assert_eq!(unwrap.parameter_abi[0], ValueAbi::HeapResult);
    assert_eq!(
        lookup("Option.unwrap_or").unwrap().symbol,
        "fern_result_unwrap_or"
    );
}

#[test]
fn inventory_covers_current_utility_surface_without_inventing_aliases() {
    for name in [
        "String.starts_with",
        "String.ends_with",
        "String.slice",
        "String.trim_start",
        "String.trim_end",
        "String.to_upper",
        "String.to_lower",
        "String.replace",
        "String.repeat",
        "String.char_at",
        "fs.is_dir",
        "json.parse",
        "json.stringify",
        "http.get",
        "sql.open",
        "actors.monitor",
        "actors.demonitor",
        "actors.supervise",
        "actors.supervise_one_for_all",
        "actors.supervise_rest_for_one",
        "System.getenv",
        "System.setenv",
        "System.cwd",
        "System.chdir",
        "System.hostname",
        "System.user",
        "System.home",
        "Regex.replace",
        "Regex.replace_all",
        "Tui.Style.bright_red",
        "Tui.Style.on_white",
        "Tui.Style.strikethrough",
        "Tui.Style.rgb",
        "Tui.Style.hex",
        "Tui.Status.warn",
        "Tui.Log.error",
        "Tui.Live.sleep",
        "Tui.Prompt.password",
        "Tui.Prompt.int",
    ] {
        assert!(lookup(name).is_some(), "missing registry entry: {name}");
    }
    for name in [
        "String.from_int",
        "Int.to_string",
        "int_to_string",
        "system.cwd",
        "FS.read",
        "Progress.new",
        "made.up",
    ] {
        assert!(lookup(name).is_none(), "invented alias: {name}");
    }
    assert!(
        runtime::omissions()
            .iter()
            .any(|item| item.names.contains(&"fern_int_to_str"))
    );
    assert!(
        runtime::omissions()
            .iter()
            .any(|item| item.names.contains(&"fern_regex_captures_free"))
    );
    assert!(
        runtime::omissions()
            .iter()
            .any(|item| item.names.contains(&"List.any"))
    );
    assert!(
        runtime::omissions()
            .iter()
            .all(|item| !item.reason.is_empty())
    );
}

#[test]
fn every_registered_symbol_exists_and_name_and_abi_tables_are_consistent() {
    let symbols = include_str!("shipping-runtime-symbols.txt");
    let names = runtime::names();
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(names.len(), unique.len());
    for name in names {
        let signature = lookup(name).unwrap();
        assert!(
            symbols.lines().any(|symbol| symbol == signature.symbol),
            "undeclared runtime symbol for {name}: {}",
            signature.symbol
        );
        assert_eq!(
            signature.parameters.len(),
            signature.parameter_abi.len(),
            "{name}"
        );
        assert!(
            signature
                .symbol
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "unsafe backend symbol"
        );
    }
}

#[test]
fn every_declared_runtime_entry_point_is_registered_or_explicitly_inventoried() {
    let registered: std::collections::BTreeSet<_> = runtime::names()
        .into_iter()
        .map(|name| lookup(name).unwrap().symbol)
        .collect();
    let omitted: std::collections::BTreeSet<_> = runtime::omissions()
        .iter()
        .flat_map(|item| item.names.iter().copied())
        .collect();
    let symbols = include_str!("shipping-runtime-symbols.txt");
    let mut declarations = 0;
    for symbol in symbols.lines() {
        assert!(
            registered.contains(symbol) || omitted.contains(symbol),
            "uninventoried native ABI: {symbol}"
        );
        declarations += 1;
    }
    assert!(
        declarations == 283,
        "symbol audit must preserve the complete shipping runtime inventory"
    );
}

#[test]
fn stable_runtime_ids_share_aliases_and_reject_out_of_range_values() {
    let canonical = runtime::resolve("fs.read").unwrap();
    assert_eq!(runtime::resolve("File.read"), Some(canonical));
    assert_eq!(runtime::signature(canonical), lookup("fs.read"));
    assert!(runtime::signature(usize::MAX).is_none());
    assert!(runtime::resolve("not.a.runtime.api").is_none());
}

#[test]
fn opaque_tui_signatures_are_distinct_and_record_real_adapter_requirements() {
    let panel = lookup("Tui.Panel.new")
        .expect("Panel registry entry")
        .return_type;
    let table = lookup("Tui.Table.new")
        .expect("Table registry entry")
        .return_type;
    assert_ne!(panel, table);
    assert_ne!(panel, Type::Int);
    assert_eq!(lookup("Tui.Panel.render").unwrap().parameters, vec![panel]);
    assert_eq!(lookup("Tui.Table.render").unwrap().parameters, vec![table]);
    assert!(lookup("Tui.Panel.padding").unwrap().requires_adapter());
    assert!(lookup("Tui.Table.border").unwrap().requires_adapter());
    assert_eq!(
        lookup("Tui.Table.add_row").unwrap().parameter_abi[1],
        ValueAbi::StringList
    );
    for module in ["Tree", "Panel", "Table", "Progress", "Spinner"] {
        assert_eq!(
            lookup(&format!("Tui.{module}.render")).unwrap().return_type,
            Type::String
        );
    }
}

#[test]
fn native_annotations_use_qualified_names_without_reserving_user_types() {
    for name in ["Panel", "Table", "Tree", "Progress", "Spinner"] {
        assert!(
            runtime::native_type(name).is_none(),
            "user type name {name}"
        );
        let qualified = format!("Tui.{name}");
        let native = runtime::native_type(&qualified).expect("qualified native annotation");
        assert_eq!(native.name(), qualified);
    }
}

#[test]
fn tuple_result_signatures_require_audited_native_layout_adapters() {
    let exec = lookup("System.exec").expect("process tuple signature");
    assert_eq!(
        exec.return_type,
        Type::Tuple(vec![Type::Int, Type::String, Type::String])
    );
    assert!(exec.requires_adapter());
    assert_eq!(
        lookup("System.exec_args").unwrap().parameter_abi,
        vec![ValueAbi::StringList]
    );
    let matched = Type::Tuple(vec![Type::Int, Type::Int, Type::String]);
    assert_eq!(
        lookup("Regex.find").unwrap().return_type,
        Type::Option(Box::new(matched.clone()))
    );
    assert_eq!(
        lookup("Regex.captures").unwrap().return_type,
        Type::List(Box::new(matched))
    );
    assert_eq!(
        lookup("Tui.Term.size").unwrap().return_type,
        Type::Tuple(vec![Type::Int, Type::Int])
    );
}
