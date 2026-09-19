//! Audited source signatures and C ABI descriptions; lookup does not enable an API.
//! Sources: docs/STDLIB_API_REFERENCE.md, lib/checker.c, lib/codegen.c and runtime headers.
//! Packed Options and MorrowStringList require explicit adapters before Rust can call them.
use crate::Type;

/// Opaque runtime-owned handles; callers cannot inspect or construct their C fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeType {
    ProcessId,
    MonitorRef,
    Panel,
    Table,
    Tree,
    Progress,
    Spinner,
    JsonValue,
    JsonError,
}

impl NativeType {
    /// Qualified source annotation spelling, preserving ordinary user-defined type names.
    pub fn name(self) -> &'static str {
        match self {
            Self::ProcessId => "ProcessId",
            Self::MonitorRef => "MonitorRef",
            Self::Panel => "Tui.Panel",
            Self::Table => "Tui.Table",
            Self::Tree => "Tui.Tree",
            Self::Progress => "Tui.Progress",
            Self::Spinner => "Tui.Spinner",
            Self::JsonValue => "json.Value",
            Self::JsonError => "json.Error",
        }
    }
}

/// Recognize only canonical native object annotations, never user-defined layout aliases.
pub fn native_type(name: &str) -> Option<NativeType> {
    let name = match name {
        "Json.Value" => "json.Value",
        "Json.Error" => "json.Error",
        name => name,
    };
    [
        NativeType::ProcessId,
        NativeType::MonitorRef,
        NativeType::Panel,
        NativeType::Table,
        NativeType::Tree,
        NativeType::Progress,
        NativeType::Spinner,
        NativeType::JsonValue,
        NativeType::JsonError,
    ]
    .into_iter()
    .find(|ty| ty.name() == name)
}

/// Physical transport at one native argument/result boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueAbi {
    Word64,
    /// Native C double argument, never an integer payload transport.
    Double64,
    /// Heap Result(List(native member records)) requires tagged tuple conversion.
    HeapJsonMembers,
    Word32,
    Void,
    /// Existing Result allocations already carry full-width tagged payloads.
    HeapResult,
    /// Heap Result whose Ok payload must bridge native StringList to Morrow List(String).
    HeapStringListResult,
    /// Rust Options use Result allocations; never pass these to morrow_option_*.
    HeapOption,
    /// Legacy C Option packs a truncated payload into the high 32 bits (low tag: Some=1, None=0).
    PackedOption,
    /// Distinct C StringList representation; use an explicitly audited bridge.
    StringList,
    /// Directory listing additionally uses NULL for failure, absent from its source type.
    NullableStringList,
    /// Native full-width process triple without the Rust tuple tag.
    ExecResult,
    /// Heap Result whose successful native process triple requires a tuple tag.
    HeapExecResult,
    /// Native match record uses negative start and a NULL text for absence.
    RegexMatch,
    /// Native count and contiguous match-record array.
    RegexCaptures,
    /// Native pair of full-width dimensions without the Rust tuple tag.
    TermSize,
}

/// Extra type-directed lowering required beyond an ordinary symbol call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Direct,
    /// Check the decimal classifier input ceiling through the caller fault context.
    DecimalPredicate,
    /// Adapt one compiler-owned Map into checked parallel native lists.
    JsonObject,
    InvertBool,
    /// Accept Int/Float/Bool/String; Float uses a typed helper and String uses semantic comparison.
    ScalarContains,
    /// Sort Int/Bool words, Float bit patterns or String pointers with element-directed helpers.
    ScalarSort,
    /// Source padding applies equally to vertical and horizontal native arguments.
    UniformPadding,
    /// Convert a source border name into the audited native MorrowBoxStyle enum.
    TableBorder,
}

/// A type scheme plus the exact runtime contract, independent of checker internals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub parameters: Vec<Type>,
    pub return_type: Type,
    pub symbol: &'static str,
    pub parameter_abi: Vec<ValueAbi>,
    pub return_abi: ValueAbi,
    pub operation: Operation,
}

impl Signature {
    /// Whether this signature needs a representation bridge or custom dispatch before use.
    pub fn requires_adapter(&self) -> bool {
        self.operation != Operation::Direct
            || self
                .parameter_abi
                .iter()
                .chain(std::iter::once(&self.return_abi))
                .any(|abi| {
                    matches!(
                        abi,
                        ValueAbi::PackedOption
                            | ValueAbi::HeapJsonMembers
                            | ValueAbi::StringList
                            | ValueAbi::NullableStringList
                            | ValueAbi::HeapStringListResult
                            | ValueAbi::HeapExecResult
                            | ValueAbi::ExecResult
                            | ValueAbi::RegexMatch
                            | ValueAbi::RegexCaptures
                            | ValueAbi::TermSize
                    )
                })
    }
}

/// A deliberately unavailable source API or internal native helper, with an explicit reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Omission {
    pub names: &'static [&'static str],
    pub reason: &'static str,
}

/// Look up only existing source spellings; type variables are schemes, never emitted IR.
pub fn lookup(name: &str) -> Option<Signature> {
    signature(resolve(name)?)
}

/// Resolve aliases to one stable append-only table identity within this compiler version.
pub fn resolve(name: &str) -> Option<usize> {
    ENTRIES.iter().position(|entry| entry.names.contains(&name))
}

/// Retrieve a signature by its checked runtime identity; invalid IDs are ordinary errors.
pub fn signature(id: usize) -> Option<Signature> {
    ENTRIES.get(id).map(|entry| {
        let mut parameter_abi: Vec<_> = entry.parameters.iter().map(|shape| shape.abi()).collect();
        for &(index, abi) in entry.parameter_overrides {
            parameter_abi[index] = abi;
        }
        Signature {
            parameters: entry.parameters.iter().map(|shape| shape.ty()).collect(),
            return_type: entry.result.ty(),
            symbol: entry.symbol,
            parameter_abi,
            return_abi: entry.return_override.unwrap_or_else(|| entry.result.abi()),
            operation: entry.operation,
        }
    })
}

/// List registered source spellings for audits; entries with adapters remain unavailable to direct wiring.
pub fn names() -> Vec<&'static str> {
    ENTRIES
        .iter()
        .flat_map(|entry| entry.names.iter().copied())
        .filter(|name| !name.starts_with('$'))
        .collect()
}

/// Reserve canonical namespaces, while preserving the preexisting bare Json user name.
/// Its qualified compatibility APIs and opaque annotations remain individually reserved.
pub fn reserved_namespace(name: &str) -> bool {
    name != "Json"
        && ENTRIES
            .iter()
            .flat_map(|entry| entry.names.iter())
            .any(|api| {
                api.strip_prefix(name)
                    .is_some_and(|suffix| suffix.starts_with('.'))
            })
}

/// Inventory runtime symbols handled outside this registry or awaiting an explicit ABI.
pub fn omissions() -> &'static [Omission] {
    OMISSIONS
}

#[derive(Clone, Copy)]
enum JsonShape {
    Value,
    Error,
    List,
    Map,
    ResultValue,
    ResultString,
    ResultBool,
    ResultInt,
    ResultFloat,
    ResultList,
    ResultMembers,
}
impl JsonShape {
    fn ty(self) -> Type {
        let value = Type::Native(NativeType::JsonValue);
        let error = Type::Native(NativeType::JsonError);
        let payload = match self {
            Self::Value => return value,
            Self::Error => return error,
            Self::List => return Type::List(Box::new(value)),
            Self::Map => return Type::Map(Box::new(Type::String), Box::new(value)),
            Self::ResultValue => value,
            Self::ResultString => Type::String,
            Self::ResultBool => Type::Bool,
            Self::ResultInt => Type::Int,
            Self::ResultFloat => Type::Float,
            Self::ResultList => Type::List(Box::new(value)),
            Self::ResultMembers => Type::List(Box::new(Type::Tuple(vec![value.clone(), value]))),
        };
        Type::Result(Box::new(payload), Box::new(error))
    }
    fn abi(self) -> ValueAbi {
        match self {
            Self::Value | Self::Error | Self::List | Self::Map => ValueAbi::Word64,
            _ => ValueAbi::HeapResult,
        }
    }
}

#[derive(Clone, Copy)]
enum Shape {
    PtrByte,
    ResultCFloat32,
    ResultSS,
    Float,
    Json(JsonShape),
    Int,
    Bool,
    String,
    Unit,
    A,
    ListA,
    ListB,
    ListInt,
    ListPairAB,
    ListString,
    OptionA,
    OptionInt,
    ResultAE,
    ResultSI,
    ResultII,
    ResultUnitInt,
    Native(NativeType),
    DirectoryResult,
    ExecTuple,
    BoundedExecResult,
    MatchOption,
    CapturesList,
    TermTuple,
}

impl Shape {
    /// Instantiate a signature shape using explicit generic parameter names.
    fn ty(self) -> Type {
        match self {
            Self::ResultCFloat32 => Type::Result(
                Box::new(Type::Named("CFloat32".into(), vec![])),
                Box::new(Type::String),
            ),
            Self::PtrByte => Type::Named("Ptr".into(), vec![Type::Named("CUInt8".into(), vec![])]),
            Self::ResultSS => Type::Result(Box::new(Type::String), Box::new(Type::String)),
            Self::Float => Type::Float,
            Self::Json(shape) => shape.ty(),
            Self::DirectoryResult => Type::Result(
                Box::new(Type::List(Box::new(Type::String))),
                Box::new(Type::Int),
            ),
            Self::BoundedExecResult => {
                Type::Result(Box::new(Self::ExecTuple.ty()), Box::new(Type::Int))
            }
            Self::ExecTuple => Type::Tuple(vec![Type::Int, Type::String, Type::String]),
            Self::MatchOption => Type::Option(Box::new(Type::Tuple(vec![
                Type::Int,
                Type::Int,
                Type::String,
            ]))),
            Self::CapturesList => Type::List(Box::new(Type::Tuple(vec![
                Type::Int,
                Type::Int,
                Type::String,
            ]))),
            Self::TermTuple => Type::Tuple(vec![Type::Int, Type::Int]),
            Self::Native(ty) => Type::Native(ty),
            Self::Int => Type::Int,
            Self::Bool => Type::Bool,
            Self::String => Type::String,
            Self::Unit => Type::Unit,
            Self::A => Type::Generic("a".into()),
            Self::ListA => Type::List(Box::new(Type::Generic("a".into()))),
            Self::ListB => Type::List(Box::new(Type::Generic("b".into()))),
            Self::ListInt => Type::List(Box::new(Type::Int)),
            Self::ListPairAB => Type::List(Box::new(Type::Tuple(vec![
                Type::Generic("a".into()),
                Type::Generic("b".into()),
            ]))),
            Self::ListString => Type::List(Box::new(Type::String)),
            Self::OptionA => Type::Option(Box::new(Type::Generic("a".into()))),
            Self::OptionInt => Type::Option(Box::new(Type::Int)),
            Self::ResultAE => Type::Result(
                Box::new(Type::Generic("a".into())),
                Box::new(Type::Generic("e".into())),
            ),
            Self::ResultSI => Type::Result(Box::new(Type::String), Box::new(Type::Int)),
            Self::ResultII => Type::Result(Box::new(Type::Int), Box::new(Type::Int)),
            Self::ResultUnitInt => Type::Result(Box::new(Type::Unit), Box::new(Type::Int)),
        }
    }

    /// Default runtime transport; divergent representations require explicit table overrides.
    fn abi(self) -> ValueAbi {
        match self {
            Self::Float => ValueAbi::Double64,
            Self::Json(shape) => shape.abi(),
            Self::Unit => ValueAbi::Void,
            Self::OptionA | Self::OptionInt => ValueAbi::HeapOption,
            Self::ResultCFloat32
            | Self::ResultSS
            | Self::ResultAE
            | Self::ResultSI
            | Self::ResultII
            | Self::ResultUnitInt => ValueAbi::HeapResult,
            _ => ValueAbi::Word64,
        }
    }
}

struct Entry {
    names: &'static [&'static str],
    parameters: &'static [Shape],
    result: Shape,
    symbol: &'static str,
    parameter_overrides: &'static [(usize, ValueAbi)],
    return_override: Option<ValueAbi>,
    operation: Operation,
}

/// Declare a source/runtime pair with ordinary transport until overrides explicitly apply.
const fn entry(
    names: &'static [&'static str],
    parameters: &'static [Shape],
    result: Shape,
    symbol: &'static str,
) -> Entry {
    Entry {
        names,
        parameters,
        result,
        symbol,
        parameter_overrides: &[],
        return_override: None,
        operation: Operation::Direct,
    }
}

/// Attach a return representation that differs from the semantic default.
const fn returned(mut entry: Entry, abi: ValueAbi) -> Entry {
    entry.return_override = Some(abi);
    entry
}

/// Attach parameter representation bridges by checked argument index.
const fn arguments(mut entry: Entry, overrides: &'static [(usize, ValueAbi)]) -> Entry {
    entry.parameter_overrides = overrides;
    entry
}

/// Require custom lowering, preventing dispatch-dependent APIs from masquerading as direct calls.
const fn operation(mut entry: Entry, op: Operation) -> Entry {
    entry.operation = op;
    entry
}

use Shape::*;

const ENTRIES: &[Entry] = &[
    entry(
        &["String.compare"],
        &[String, String],
        Int,
        "morrow_str_compare",
    ),
    entry(&["String.len", "str_len"], &[String], Int, "morrow_str_len"),
    entry(
        &["String.concat", "str_concat"],
        &[String, String],
        String,
        "morrow_str_concat",
    ),
    entry(
        &["String.eq", "str_eq"],
        &[String, String],
        Bool,
        "morrow_str_eq",
    ),
    entry(
        &["String.starts_with", "str_starts_with"],
        &[String, String],
        Bool,
        "morrow_str_starts_with",
    ),
    entry(
        &["String.ends_with", "str_ends_with"],
        &[String, String],
        Bool,
        "morrow_str_ends_with",
    ),
    entry(
        &["String.contains", "str_contains"],
        &[String, String],
        Bool,
        "morrow_str_contains",
    ),
    entry(
        &["String.slice", "str_slice"],
        &[String, Int, Int],
        String,
        "morrow_str_slice",
    ),
    entry(
        &["String.trim", "str_trim"],
        &[String],
        String,
        "morrow_str_trim",
    ),
    entry(
        &["String.trim_start", "str_trim_start"],
        &[String],
        String,
        "morrow_str_trim_start",
    ),
    entry(
        &["String.trim_end", "str_trim_end"],
        &[String],
        String,
        "morrow_str_trim_end",
    ),
    entry(
        &["String.to_upper", "str_to_upper"],
        &[String],
        String,
        "morrow_str_to_upper",
    ),
    entry(&["String.quote"], &[String], String, "morrow_str_quote"),
    entry(
        &["String.to_lower", "str_to_lower"],
        &[String],
        String,
        "morrow_str_to_lower",
    ),
    entry(
        &["String.replace", "str_replace"],
        &[String, String, String],
        String,
        "morrow_str_replace",
    ),
    entry(
        &["String.repeat", "str_repeat"],
        &[String, Int],
        String,
        "morrow_str_repeat",
    ),
    entry(
        &["String.is_empty", "str_is_empty"],
        &[String],
        Bool,
        "morrow_str_is_empty",
    ),
    entry(&["Int.parse"], &[String], OptionInt, "morrow_int_parse"),
    entry(
        &["Int.checked_add"],
        &[Int, Int],
        OptionInt,
        "morrow_int_checked_add",
    ),
    entry(
        &["Int.checked_sub"],
        &[Int, Int],
        OptionInt,
        "morrow_int_checked_sub",
    ),
    entry(
        &["Int.checked_mul"],
        &[Int, Int],
        OptionInt,
        "morrow_int_checked_mul",
    ),
    entry(
        &["Int.checked_div"],
        &[Int, Int],
        OptionInt,
        "morrow_int_checked_div",
    ),
    entry(
        &["Int.checked_rem"],
        &[Int, Int],
        OptionInt,
        "morrow_int_checked_rem",
    ),
    entry(
        &["Int.checked_neg"],
        &[Int],
        OptionInt,
        "morrow_int_checked_neg",
    ),
    returned(
        entry(
            &["String.index_of"],
            &[String, String],
            OptionInt,
            "morrow_str_index_of",
        ),
        ValueAbi::PackedOption,
    ),
    returned(
        entry(
            &["String.char_at"],
            &[String, Int],
            OptionInt,
            "morrow_str_char_at",
        ),
        ValueAbi::PackedOption,
    ),
    returned(
        entry(
            &["String.split"],
            &[String, String],
            ListString,
            "morrow_str_split",
        ),
        ValueAbi::StringList,
    ),
    returned(
        entry(&["String.lines"], &[String], ListString, "morrow_str_lines"),
        ValueAbi::StringList,
    ),
    arguments(
        entry(
            &["String.join"],
            &[ListString, String],
            String,
            "morrow_str_join",
        ),
        &[(0, ValueAbi::StringList)],
    ),
    entry(&["List.len", "list_len"], &[ListA], Int, "morrow_list_len"),
    entry(
        &["List.get", "list_get"],
        &[ListA, Int],
        A,
        "morrow_list_get",
    ),
    entry(&["List.head", "list_head"], &[ListA], A, "morrow_list_head"),
    entry(&["List.at"], &[ListA, Int], OptionA, "morrow_list_at"),
    entry(&["List.first"], &[ListA], OptionA, "morrow_list_first"),
    entry(&["List.last"], &[ListA], OptionA, "morrow_list_last"),
    entry(&["List.take"], &[ListA, Int], ListA, "morrow_list_take"),
    entry(&["List.drop"], &[ListA, Int], ListA, "morrow_list_drop"),
    entry(&["List.sum"], &[ListInt], Int, "morrow_list_sum"),
    entry(&["List.range"], &[Int, Int], ListInt, "morrow_list_range"),
    entry(
        &["List.zip"],
        &[ListA, ListB],
        ListPairAB,
        "morrow_list_zip",
    ),
    operation(
        entry(&["List.sort"], &[ListA], ListA, "morrow_list_sort"),
        Operation::ScalarSort,
    ),
    entry(
        &["List.tail", "list_tail"],
        &[ListA],
        ListA,
        "morrow_list_tail",
    ),
    entry(
        &["List.push", "list_push"],
        &[ListA, A],
        ListA,
        "morrow_list_push",
    ),
    entry(
        &["List.reverse", "list_reverse"],
        &[ListA],
        ListA,
        "morrow_list_reverse",
    ),
    entry(
        &["List.concat", "list_concat"],
        &[ListA, ListA],
        ListA,
        "morrow_list_concat",
    ),
    entry(
        &["List.is_empty", "list_is_empty"],
        &[ListA],
        Bool,
        "morrow_list_is_empty",
    ),
    operation(
        entry(
            &["List.contains"],
            &[ListA, A],
            Bool,
            "morrow_list_contains",
        ),
        Operation::ScalarContains,
    ),
    entry(&["Option.is_some"], &[OptionA], Bool, "morrow_result_is_ok"),
    operation(
        entry(&["Option.is_none"], &[OptionA], Bool, "morrow_result_is_ok"),
        Operation::InvertBool,
    ),
    entry(
        &["Option.unwrap_or"],
        &[OptionA, A],
        A,
        "morrow_result_unwrap_or",
    ),
    entry(&["Result.is_ok"], &[ResultAE], Bool, "morrow_result_is_ok"),
    operation(
        entry(&["Result.is_err"], &[ResultAE], Bool, "morrow_result_is_ok"),
        Operation::InvertBool,
    ),
    entry(
        &["Result.unwrap_or"],
        &[ResultAE, A],
        A,
        "morrow_result_unwrap_or",
    ),
    entry(
        &["fs.read", "File.read", "read_file"],
        &[String],
        ResultSI,
        "morrow_read_file",
    ),
    entry(
        &["fs.write", "File.write", "write_file"],
        &[String, String],
        ResultII,
        "morrow_write_file",
    ),
    entry(
        &["fs.append", "File.append", "append_file"],
        &[String, String],
        ResultII,
        "morrow_append_file",
    ),
    entry(
        &["fs.exists", "File.exists", "file_exists"],
        &[String],
        Bool,
        "morrow_file_exists",
    ),
    entry(
        &["fs.delete", "File.delete", "delete_file"],
        &[String],
        ResultII,
        "morrow_delete_file",
    ),
    entry(
        &["fs.size", "File.size", "file_size"],
        &[String],
        ResultII,
        "morrow_file_size",
    ),
    entry(
        &["fs.is_dir", "File.is_dir"],
        &[String],
        Bool,
        "morrow_is_dir",
    ),
    returned(
        entry(
            &["fs.list_dir", "File.list_dir"],
            &[String],
            DirectoryResult,
            "morrow_read_dir_result",
        ),
        ValueAbi::HeapStringListResult,
    ),
    entry(
        &["json.parse", "Json.parse"],
        &[String],
        Json(JsonShape::ResultValue),
        "morrow_json_value_parse",
    ),
    entry(
        &["json.stringify", "Json.stringify"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultString),
        "morrow_json_value_stringify",
    ),
    entry(
        &["http.get", "Http.get"],
        &[String],
        ResultSI,
        "morrow_http_get",
    ),
    entry(
        &["http.post", "Http.post"],
        &[String, String],
        ResultSI,
        "morrow_http_post",
    ),
    entry(
        &["sql.open", "Sql.open"],
        &[String],
        ResultII,
        "morrow_sql_open",
    ),
    entry(
        &["sql.close", "Sql.close"],
        &[Int],
        ResultII,
        "morrow_sql_close",
    ),
    entry(
        &["sql.execute", "Sql.execute"],
        &[Int, String],
        ResultII,
        "morrow_sql_execute",
    ),
    entry(
        &["actors.start", "Actors.start"],
        &[String],
        Int,
        "morrow_actor_start",
    ),
    entry(
        &["actors.post", "Actors.post"],
        &[Int, String],
        ResultII,
        "morrow_actor_post",
    ),
    entry(
        &["actors.next", "Actors.next"],
        &[Int],
        ResultSI,
        "morrow_actor_next",
    ),
    entry(
        &["actors.monitor", "Actors.monitor"],
        &[Int, Int],
        ResultII,
        "morrow_actor_monitor",
    ),
    entry(
        &["actors.demonitor", "Actors.demonitor"],
        &[Int, Int],
        ResultII,
        "morrow_actor_demonitor",
    ),
    entry(
        &["actors.restart", "Actors.restart"],
        &[Int],
        ResultII,
        "morrow_actor_restart",
    ),
    entry(
        &["actors.supervise", "Actors.supervise"],
        &[Int, Int, Int, Int],
        ResultII,
        "morrow_actor_supervise",
    ),
    entry(
        &[
            "actors.supervise_one_for_all",
            "Actors.supervise_one_for_all",
        ],
        &[Int, Int, Int, Int],
        ResultII,
        "morrow_actor_supervise_one_for_all",
    ),
    entry(
        &[
            "actors.supervise_rest_for_one",
            "Actors.supervise_rest_for_one",
        ],
        &[Int, Int, Int, Int],
        ResultII,
        "morrow_actor_supervise_rest_for_one",
    ),
    entry(&["System.args_count"], &[], Int, "morrow_args_count"),
    entry(&["System.arg"], &[Int], String, "morrow_arg"),
    returned(
        entry(&["System.args"], &[], ListString, "morrow_args"),
        ValueAbi::StringList,
    ),
    entry(&["System.exit"], &[Int], Unit, "morrow_exit"),
    entry(&["System.getenv"], &[String], String, "morrow_getenv"),
    entry(&["System.setenv"], &[String, String], Int, "morrow_setenv"),
    entry(&["System.cwd"], &[], String, "morrow_cwd"),
    entry(&["System.chdir"], &[String], Int, "morrow_chdir"),
    entry(&["System.hostname"], &[], String, "morrow_hostname"),
    entry(&["System.user"], &[], String, "morrow_user"),
    entry(&["System.home"], &[], String, "morrow_home"),
    entry(
        &["Regex.is_match"],
        &[String, String],
        Bool,
        "morrow_regex_is_match",
    ),
    returned(
        entry(
            &["Regex.find_all"],
            &[String, String],
            ListString,
            "morrow_regex_find_all",
        ),
        ValueAbi::StringList,
    ),
    returned(
        entry(
            &["Regex.split"],
            &[String, String],
            ListString,
            "morrow_regex_split",
        ),
        ValueAbi::StringList,
    ),
    entry(
        &["Regex.replace"],
        &[String, String, String],
        String,
        "morrow_regex_replace",
    ),
    entry(
        &["Regex.replace_all"],
        &[String, String, String],
        String,
        "morrow_regex_replace_all",
    ),
    entry(
        &["Tui.Style.black"],
        &[String],
        String,
        "morrow_style_black",
    ),
    entry(&["Tui.Style.red"], &[String], String, "morrow_style_red"),
    entry(
        &["Tui.Style.green"],
        &[String],
        String,
        "morrow_style_green",
    ),
    entry(
        &["Tui.Style.yellow"],
        &[String],
        String,
        "morrow_style_yellow",
    ),
    entry(&["Tui.Style.blue"], &[String], String, "morrow_style_blue"),
    entry(
        &["Tui.Style.magenta"],
        &[String],
        String,
        "morrow_style_magenta",
    ),
    entry(&["Tui.Style.cyan"], &[String], String, "morrow_style_cyan"),
    entry(
        &["Tui.Style.white"],
        &[String],
        String,
        "morrow_style_white",
    ),
    entry(
        &["Tui.Style.bright_black"],
        &[String],
        String,
        "morrow_style_bright_black",
    ),
    entry(
        &["Tui.Style.bright_red"],
        &[String],
        String,
        "morrow_style_bright_red",
    ),
    entry(
        &["Tui.Style.bright_green"],
        &[String],
        String,
        "morrow_style_bright_green",
    ),
    entry(
        &["Tui.Style.bright_yellow"],
        &[String],
        String,
        "morrow_style_bright_yellow",
    ),
    entry(
        &["Tui.Style.bright_blue"],
        &[String],
        String,
        "morrow_style_bright_blue",
    ),
    entry(
        &["Tui.Style.bright_magenta"],
        &[String],
        String,
        "morrow_style_bright_magenta",
    ),
    entry(
        &["Tui.Style.bright_cyan"],
        &[String],
        String,
        "morrow_style_bright_cyan",
    ),
    entry(
        &["Tui.Style.bright_white"],
        &[String],
        String,
        "morrow_style_bright_white",
    ),
    entry(
        &["Tui.Style.on_black"],
        &[String],
        String,
        "morrow_style_on_black",
    ),
    entry(
        &["Tui.Style.on_red"],
        &[String],
        String,
        "morrow_style_on_red",
    ),
    entry(
        &["Tui.Style.on_green"],
        &[String],
        String,
        "morrow_style_on_green",
    ),
    entry(
        &["Tui.Style.on_yellow"],
        &[String],
        String,
        "morrow_style_on_yellow",
    ),
    entry(
        &["Tui.Style.on_blue"],
        &[String],
        String,
        "morrow_style_on_blue",
    ),
    entry(
        &["Tui.Style.on_magenta"],
        &[String],
        String,
        "morrow_style_on_magenta",
    ),
    entry(
        &["Tui.Style.on_cyan"],
        &[String],
        String,
        "morrow_style_on_cyan",
    ),
    entry(
        &["Tui.Style.on_white"],
        &[String],
        String,
        "morrow_style_on_white",
    ),
    entry(&["Tui.Style.bold"], &[String], String, "morrow_style_bold"),
    entry(&["Tui.Style.dim"], &[String], String, "morrow_style_dim"),
    entry(
        &["Tui.Style.italic"],
        &[String],
        String,
        "morrow_style_italic",
    ),
    entry(
        &["Tui.Style.underline"],
        &[String],
        String,
        "morrow_style_underline",
    ),
    entry(
        &["Tui.Style.blink"],
        &[String],
        String,
        "morrow_style_blink",
    ),
    entry(
        &["Tui.Style.reverse"],
        &[String],
        String,
        "morrow_style_reverse",
    ),
    entry(
        &["Tui.Style.strikethrough"],
        &[String],
        String,
        "morrow_style_strikethrough",
    ),
    entry(
        &["Tui.Style.color"],
        &[String, Int],
        String,
        "morrow_style_color",
    ),
    entry(
        &["Tui.Style.on_color"],
        &[String, Int],
        String,
        "morrow_style_on_color",
    ),
    entry(
        &["Tui.Style.rgb"],
        &[String, Int, Int, Int],
        String,
        "morrow_style_rgb",
    ),
    entry(
        &["Tui.Style.on_rgb"],
        &[String, Int, Int, Int],
        String,
        "morrow_style_on_rgb",
    ),
    entry(
        &["Tui.Style.hex"],
        &[String, String],
        String,
        "morrow_style_hex",
    ),
    entry(
        &["Tui.Style.on_hex"],
        &[String, String],
        String,
        "morrow_style_on_hex",
    ),
    entry(
        &["Tui.Style.reset"],
        &[String],
        String,
        "morrow_style_reset",
    ),
    entry(
        &["Tui.Status.warn"],
        &[String],
        String,
        "morrow_status_warn",
    ),
    entry(&["Tui.Status.ok"], &[String], String, "morrow_status_ok"),
    entry(
        &["Tui.Status.info"],
        &[String],
        String,
        "morrow_status_info",
    ),
    entry(
        &["Tui.Status.error"],
        &[String],
        String,
        "morrow_status_error",
    ),
    entry(
        &["Tui.Status.debug"],
        &[String],
        String,
        "morrow_status_debug",
    ),
    entry(&["Tui.Log.debug"], &[String], String, "morrow_log_debug"),
    entry(&["Tui.Log.info"], &[String], String, "morrow_log_info"),
    entry(&["Tui.Log.warn"], &[String], String, "morrow_log_warn"),
    entry(&["Tui.Log.error"], &[String], String, "morrow_log_error"),
    entry(&["Tui.Live.print"], &[String], Unit, "morrow_live_print"),
    entry(
        &["Tui.Live.clear_line"],
        &[],
        Unit,
        "morrow_live_clear_line",
    ),
    entry(&["Tui.Live.update"], &[String], Unit, "morrow_live_update"),
    entry(&["Tui.Live.done"], &[], Unit, "morrow_live_done"),
    entry(&["Tui.Live.sleep"], &[Int], Unit, "morrow_sleep_ms"),
    entry(
        &["Tui.Term.move_to"],
        &[Int, Int],
        Unit,
        "morrow_term_move_to",
    ),
    entry(&["Tui.Term.up"], &[Int], Unit, "morrow_term_up"),
    entry(&["Tui.Term.down"], &[Int], Unit, "morrow_term_down"),
    entry(&["Tui.Term.left"], &[Int], Unit, "morrow_term_left"),
    entry(&["Tui.Term.right"], &[Int], Unit, "morrow_term_right"),
    entry(&["Tui.Term.clear"], &[], Unit, "morrow_term_clear"),
    entry(
        &["Tui.Term.hide_cursor"],
        &[],
        Unit,
        "morrow_term_hide_cursor",
    ),
    entry(
        &["Tui.Term.show_cursor"],
        &[],
        Unit,
        "morrow_term_show_cursor",
    ),
    entry(
        &["Tui.Term.save_cursor"],
        &[],
        Unit,
        "morrow_term_save_cursor",
    ),
    entry(
        &["Tui.Term.restore_cursor"],
        &[],
        Unit,
        "morrow_term_restore_cursor",
    ),
    entry(&["Tui.Term.is_tty"], &[], Bool, "morrow_term_is_tty"),
    entry(
        &["Tui.Term.color_support"],
        &[],
        Int,
        "morrow_term_color_support",
    ),
    entry(
        &["Tui.Prompt.input"],
        &[String],
        String,
        "morrow_prompt_input",
    ),
    returned(
        entry(
            &["Tui.Prompt.confirm"],
            &[String],
            Bool,
            "morrow_prompt_confirm",
        ),
        ValueAbi::Word32,
    ),
    arguments(
        returned(
            entry(
                &["Tui.Prompt.select"],
                &[String, ListString],
                Int,
                "morrow_prompt_select",
            ),
            ValueAbi::Word32,
        ),
        &[(1, ValueAbi::StringList)],
    ),
    entry(
        &["Tui.Prompt.password"],
        &[String],
        String,
        "morrow_prompt_password",
    ),
    entry(
        &["Tui.Prompt.int"],
        &[String, Int, Int],
        Int,
        "morrow_prompt_int",
    ),
    entry(
        &["Tui.Panel.new"],
        &[String],
        Native(NativeType::Panel),
        "morrow_panel_new",
    ),
    entry(
        &["Tui.Panel.title"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "morrow_panel_title",
    ),
    entry(
        &["Tui.Panel.subtitle"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "morrow_panel_subtitle",
    ),
    entry(
        &["Tui.Panel.border"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "morrow_panel_border_str",
    ),
    entry(
        &["Tui.Panel.width"],
        &[Native(NativeType::Panel), Int],
        Native(NativeType::Panel),
        "morrow_panel_width",
    ),
    operation(
        entry(
            &["Tui.Panel.padding"],
            &[Native(NativeType::Panel), Int],
            Native(NativeType::Panel),
            "morrow_panel_padding",
        ),
        Operation::UniformPadding,
    ),
    entry(
        &["Tui.Panel.border_color"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "morrow_panel_border_color",
    ),
    entry(
        &["Tui.Panel.render"],
        &[Native(NativeType::Panel)],
        String,
        "morrow_panel_render",
    ),
    entry(
        &["Tui.Table.new"],
        &[],
        Native(NativeType::Table),
        "morrow_table_new",
    ),
    entry(
        &["Tui.Table.add_column"],
        &[Native(NativeType::Table), String],
        Native(NativeType::Table),
        "morrow_table_add_column",
    ),
    arguments(
        entry(
            &["Tui.Table.add_row"],
            &[Native(NativeType::Table), ListString],
            Native(NativeType::Table),
            "morrow_table_add_row",
        ),
        &[(1, ValueAbi::StringList)],
    ),
    entry(
        &["Tui.Table.title"],
        &[Native(NativeType::Table), String],
        Native(NativeType::Table),
        "morrow_table_title",
    ),
    operation(
        entry(
            &["Tui.Table.border"],
            &[Native(NativeType::Table), String],
            Native(NativeType::Table),
            "morrow_table_border",
        ),
        Operation::TableBorder,
    ),
    entry(
        &["Tui.Table.show_header"],
        &[Native(NativeType::Table), Int],
        Native(NativeType::Table),
        "morrow_table_show_header",
    ),
    entry(
        &["Tui.Table.render"],
        &[Native(NativeType::Table)],
        String,
        "morrow_table_render",
    ),
    entry(
        &["Tui.Tree.new"],
        &[String],
        Native(NativeType::Tree),
        "morrow_tree_new",
    ),
    entry(
        &["Tui.Tree.add"],
        &[Native(NativeType::Tree), Native(NativeType::Tree)],
        Native(NativeType::Tree),
        "morrow_tree_add",
    ),
    entry(
        &["Tui.Tree.render"],
        &[Native(NativeType::Tree)],
        String,
        "morrow_tree_render",
    ),
    entry(
        &["Tui.Progress.new"],
        &[Int],
        Native(NativeType::Progress),
        "morrow_progress_new",
    ),
    entry(
        &["Tui.Progress.description"],
        &[Native(NativeType::Progress), String],
        Native(NativeType::Progress),
        "morrow_progress_description",
    ),
    entry(
        &["Tui.Progress.width"],
        &[Native(NativeType::Progress), Int],
        Native(NativeType::Progress),
        "morrow_progress_width",
    ),
    entry(
        &["Tui.Progress.advance"],
        &[Native(NativeType::Progress)],
        Native(NativeType::Progress),
        "morrow_progress_advance",
    ),
    entry(
        &["Tui.Progress.set"],
        &[Native(NativeType::Progress), Int],
        Native(NativeType::Progress),
        "morrow_progress_set",
    ),
    entry(
        &["Tui.Progress.render"],
        &[Native(NativeType::Progress)],
        String,
        "morrow_progress_render",
    ),
    entry(
        &["Tui.Spinner.new"],
        &[],
        Native(NativeType::Spinner),
        "morrow_spinner_new",
    ),
    entry(
        &["Tui.Spinner.message"],
        &[Native(NativeType::Spinner), String],
        Native(NativeType::Spinner),
        "morrow_spinner_message",
    ),
    entry(
        &["Tui.Spinner.style"],
        &[Native(NativeType::Spinner), String],
        Native(NativeType::Spinner),
        "morrow_spinner_style",
    ),
    entry(
        &["Tui.Spinner.tick"],
        &[Native(NativeType::Spinner)],
        Native(NativeType::Spinner),
        "morrow_spinner_tick",
    ),
    entry(
        &["Tui.Spinner.render"],
        &[Native(NativeType::Spinner)],
        String,
        "morrow_spinner_render",
    ),
    returned(
        entry(&["System.exec"], &[String], ExecTuple, "morrow_exec"),
        ValueAbi::ExecResult,
    ),
    returned(
        arguments(
            entry(
                &["System.exec_args"],
                &[ListString],
                ExecTuple,
                "morrow_exec_args",
            ),
            &[(0, ValueAbi::StringList)],
        ),
        ValueAbi::ExecResult,
    ),
    returned(
        entry(
            &["Regex.find"],
            &[String, String],
            MatchOption,
            "morrow_regex_find",
        ),
        ValueAbi::RegexMatch,
    ),
    returned(
        entry(
            &["Regex.captures"],
            &[String, String],
            CapturesList,
            "morrow_regex_captures",
        ),
        ValueAbi::RegexCaptures,
    ),
    returned(
        entry(&["Tui.Term.size"], &[], TermTuple, "morrow_term_size"),
        ValueAbi::TermSize,
    ),
    entry(
        &["json.is_null", "Json.is_null"],
        &[Json(JsonShape::Value)],
        Bool,
        "morrow_json_value_is_null",
    ),
    entry(
        &["json.get", "Json.get"],
        &[Json(JsonShape::Value), String],
        Json(JsonShape::ResultValue),
        "morrow_json_value_get",
    ),
    entry(
        &["json.at", "Json.at"],
        &[Json(JsonShape::Value), Int],
        Json(JsonShape::ResultValue),
        "morrow_json_value_at",
    ),
    entry(
        &["json.length", "Json.length"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultInt),
        "morrow_json_value_length",
    ),
    entry(
        &["json.as_bool", "Json.as_bool"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultBool),
        "morrow_json_value_as_bool",
    ),
    entry(
        &["json.as_int", "Json.as_int"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultInt),
        "morrow_json_value_as_int",
    ),
    entry(
        &["json.as_float", "Json.as_float"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultFloat),
        "morrow_json_value_as_float",
    ),
    entry(
        &["json.as_string", "Json.as_string"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultString),
        "morrow_json_value_as_string",
    ),
    entry(
        &["json.number_text", "Json.number_text"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultString),
        "morrow_json_value_number_text",
    ),
    entry(
        &["json.error_code", "Json.error_code"],
        &[Json(JsonShape::Error)],
        Int,
        "morrow_json_value_error_code",
    ),
    entry(
        &["json.error_offset", "Json.error_offset"],
        &[Json(JsonShape::Error)],
        Int,
        "morrow_json_value_error_offset",
    ),
    entry(
        &["json.error_message", "Json.error_message"],
        &[Json(JsonShape::Error)],
        String,
        "morrow_json_value_error_message",
    ),
    entry(
        &["json.null", "Json.null"],
        &[],
        Json(JsonShape::Value),
        "morrow_json_value_null",
    ),
    entry(
        &["json.from_bool", "Json.from_bool"],
        &[Bool],
        Json(JsonShape::Value),
        "morrow_json_value_from_bool",
    ),
    entry(
        &["json.from_int", "Json.from_int"],
        &[Int],
        Json(JsonShape::Value),
        "morrow_json_value_from_int",
    ),
    entry(
        &["json.from_float", "Json.from_float"],
        &[Float],
        Json(JsonShape::ResultValue),
        "morrow_json_value_from_float",
    ),
    entry(
        &["json.from_string", "Json.from_string"],
        &[String],
        Json(JsonShape::ResultValue),
        "morrow_json_value_from_string",
    ),
    entry(
        &["json.from_number_text", "Json.from_number_text"],
        &[String],
        Json(JsonShape::ResultValue),
        "morrow_json_value_from_number_text",
    ),
    entry(
        &["json.from_array", "Json.from_array"],
        &[Json(JsonShape::List)],
        Json(JsonShape::ResultValue),
        "morrow_json_value_from_array",
    ),
    entry(
        &["json.elements", "Json.elements"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultList),
        "morrow_json_value_elements",
    ),
    returned(
        entry(
            &["json.members", "Json.members"],
            &[Json(JsonShape::Value)],
            Json(JsonShape::ResultMembers),
            "morrow_json_value_members",
        ),
        ValueAbi::HeapJsonMembers,
    ),
    operation(
        entry(
            &["json.from_object", "Json.from_object"],
            &[Json(JsonShape::Map)],
            Json(JsonShape::ResultValue),
            "morrow_json_value_from_object",
        ),
        Operation::JsonObject,
    ),
    returned(
        arguments(
            entry(
                &["System.exec_args_bounded"],
                &[ListString, Int, Int],
                BoundedExecResult,
                "morrow_exec_args_bounded",
            ),
            &[(0, ValueAbi::StringList)],
        ),
        ValueAbi::HeapExecResult,
    ),
    entry(
        &["System.write_stderr"],
        &[String],
        ResultUnitInt,
        "morrow_write_stderr",
    ),
    operation(
        entry(
            &["String.is_decimal", "str_is_decimal"],
            &[String],
            Bool,
            "morrow_str_is_decimal",
        ),
        Operation::DecimalPredicate,
    ),
    entry(
        &["json.error_path", "Json.error_path"],
        &[Json(JsonShape::Error)],
        String,
        "morrow_json_value_error_path",
    ),
    entry(
        &["$ffi.float32"],
        &[Float],
        ResultCFloat32,
        "morrow_ffi_float32",
    ),
    entry(
        &["$ffi.borrow_string"],
        &[String],
        PtrByte,
        "morrow_ffi_borrow_string",
    ),
    entry(
        &["$ffi.read_string"],
        &[Int, Int],
        ResultSS,
        "morrow_ffi_read_string",
    ),
];

const OMISSIONS: &[Omission] = &[
    Omission {
        names: &[
            "morrow_ffi_float32",
            "morrow_ffi_borrow_string",
            "morrow_ffi_read_string",
        ],
        reason: "Compiler-owned checked foreign adapters use private runtime identities, not directly callable source signatures.",
    },
    Omission {
        names: &["morrow_json_codec_encode", "morrow_json_codec_decode"],
        reason: "Typed compiler-owned JSON codec operations require a validated concrete descriptor, not a source runtime signature.",
    },
    Omission {
        names: &["morrow_json_value_limit_error"],
        reason: "Internal checked JSON adapter preflight; not a source API.",
    },
    Omission {
        names: &[
            "morrow_str_slice_is_valid",
            "morrow_str_split_is_valid",
            "morrow_str_decimal_size_is_valid",
        ],
        reason: "Internal nonallocating text preflight helpers used by compiler-generated fault guards, not source APIs",
    },
    Omission {
        names: &[
            "print",
            "println",
            "Some",
            "None",
            "Ok",
            "Err",
            "morrow_bool_to_str",
            "morrow_int_to_str",
            "morrow_float_to_str",
            "morrow_print_float",
            "morrow_println_float",
            "morrow_print_bool",
            "morrow_print_int",
            "morrow_print_str",
            "morrow_println_bool",
            "morrow_println_int",
            "morrow_println_str",
            "morrow_result_err",
            "morrow_result_ok",
            "morrow_result_unwrap",
        ],
        reason: "Type-directed compiler intrinsics or interpolation helpers; no single source signature/runtime symbol. Conversion helper names are not public source APIs.",
    },
    Omission {
        names: &[
            "List.any",
            "List.all",
            "List.map",
            "List.fold",
            "List.filter",
            "List.find",
            "Result.map",
            "Result.and_then",
            "Result.unwrap_or_else",
            "Option.map",
            "morrow_list_all",
            "morrow_list_any",
            "morrow_list_filter",
            "morrow_list_find",
            "morrow_list_fold",
            "morrow_list_map",
            "morrow_option_map",
            "morrow_result_and_then",
            "morrow_result_map",
            "morrow_result_unwrap_or_else",
        ],
        reason: "Source calls use compiler-owned typed closure lowering. The legacy C callback ABI lacks closure environments and is deliberately not invoked.",
    },
    Omission {
        names: &["morrow_regex_captures_free", "morrow_regex_match_free"],
        reason: "Internal native regex allocation lifecycle; source results are translated into managed tuples and lists.",
    },
    Omission {
        names: &[
            "morrow_option_is_some",
            "morrow_option_none",
            "morrow_option_some",
            "morrow_option_unwrap",
            "morrow_option_unwrap_or",
        ],
        reason: "Legacy packed Option helper truncates payloads; Rust Options use heap Result helpers instead. No direct calls are safe.",
    },
    Omission {
        names: &[
            "spawn",
            "spawn_link",
            "receive",
            "morrow_actor_clock_advance",
            "morrow_actor_clock_now",
            "morrow_actor_clock_set",
            "morrow_actor_exit",
            "morrow_actor_mailbox_len",
            "morrow_actor_receive",
            "morrow_actor_scheduler_next",
            "morrow_actor_self",
            "morrow_actor_send",
            "morrow_actor_set_current",
            "morrow_actor_spawn",
            "morrow_actor_spawn_link",
        ],
        reason: "Internal actor/scheduler entry points or actor execution syntax; registry mailbox APIs do not implement autonomous actor execution.",
    },
    Omission {
        names: &[
            "morrow_alloc",
            "morrow_drop",
            "morrow_dup",
            "morrow_free",
            "morrow_list_free",
            "morrow_list_new",
            "morrow_list_push_mut",
            "morrow_list_with_capacity",
            "morrow_rc_alloc",
            "morrow_rc_drop",
            "morrow_rc_dup",
            "morrow_rc_flags",
            "morrow_rc_refcount",
            "morrow_rc_set_flags",
            "morrow_rc_type_tag",
            "morrow_set_args",
            "morrow_sort_begin",
            "morrow_sort_finish",
            "morrow_sort_next",
            "morrow_sort_report",
            "morrow_str_list_free",
        ],
        reason: "Compiler-owned allocation, reference bookkeeping, mutation, sort driving, or process initialization; not public source-callable stdlib APIs.",
    },
    Omission {
        names: &["morrow_list_dir"],
        reason: "Legacy nullable directory ABI remains available to C callers; source fs.list_dir uses the explicit Result helper.",
    },
    Omission {
        names: &[
            "morrow_panel_free",
            "morrow_progress_free",
            "morrow_spinner_free",
            "morrow_table_free",
            "morrow_panel_border",
        ],
        reason: "Runtime-owned TUI lifecycle or internal enum helper; source objects expose builders, not native destruction or raw layouts.",
    },
    Omission {
        names: &["morrow_list_contains_str"],
        reason: "Selected only by type-directed List.contains String dispatch; not an independent public source API.",
    },
    Omission {
        names: &["morrow_list_sort_float", "morrow_list_sort_str"],
        reason: "Selected only by type-directed List.sort Float/String dispatch; not independent public source APIs.",
    },
];
