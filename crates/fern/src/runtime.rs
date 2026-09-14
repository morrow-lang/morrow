//! Audited source signatures and C ABI descriptions; lookup does not enable an API.
//! Sources: docs/STDLIB_API_REFERENCE.md, lib/checker.c, lib/codegen.c and runtime headers.
//! Packed Options and FernStringList require explicit adapters before Rust can call them.
use crate::Type;

/// Opaque runtime-owned handles; callers cannot inspect or construct their C fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeType {
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
    /// Heap Result whose Ok payload must bridge native StringList to Fern List(String).
    HeapStringListResult,
    /// Rust Options use Result allocations; never pass these to fern_option_*.
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
    /// Convert a source border name into the audited native FernBoxStyle enum.
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
        "fern_str_compare",
    ),
    entry(&["String.len", "str_len"], &[String], Int, "fern_str_len"),
    entry(
        &["String.concat", "str_concat"],
        &[String, String],
        String,
        "fern_str_concat",
    ),
    entry(
        &["String.eq", "str_eq"],
        &[String, String],
        Bool,
        "fern_str_eq",
    ),
    entry(
        &["String.starts_with", "str_starts_with"],
        &[String, String],
        Bool,
        "fern_str_starts_with",
    ),
    entry(
        &["String.ends_with", "str_ends_with"],
        &[String, String],
        Bool,
        "fern_str_ends_with",
    ),
    entry(
        &["String.contains", "str_contains"],
        &[String, String],
        Bool,
        "fern_str_contains",
    ),
    entry(
        &["String.slice", "str_slice"],
        &[String, Int, Int],
        String,
        "fern_str_slice",
    ),
    entry(
        &["String.trim", "str_trim"],
        &[String],
        String,
        "fern_str_trim",
    ),
    entry(
        &["String.trim_start", "str_trim_start"],
        &[String],
        String,
        "fern_str_trim_start",
    ),
    entry(
        &["String.trim_end", "str_trim_end"],
        &[String],
        String,
        "fern_str_trim_end",
    ),
    entry(
        &["String.to_upper", "str_to_upper"],
        &[String],
        String,
        "fern_str_to_upper",
    ),
    entry(&["String.quote"], &[String], String, "fern_str_quote"),
    entry(
        &["String.to_lower", "str_to_lower"],
        &[String],
        String,
        "fern_str_to_lower",
    ),
    entry(
        &["String.replace", "str_replace"],
        &[String, String, String],
        String,
        "fern_str_replace",
    ),
    entry(
        &["String.repeat", "str_repeat"],
        &[String, Int],
        String,
        "fern_str_repeat",
    ),
    entry(
        &["String.is_empty", "str_is_empty"],
        &[String],
        Bool,
        "fern_str_is_empty",
    ),
    entry(&["Int.parse"], &[String], OptionInt, "fern_int_parse"),
    entry(
        &["Int.checked_add"],
        &[Int, Int],
        OptionInt,
        "fern_int_checked_add",
    ),
    entry(
        &["Int.checked_sub"],
        &[Int, Int],
        OptionInt,
        "fern_int_checked_sub",
    ),
    entry(
        &["Int.checked_mul"],
        &[Int, Int],
        OptionInt,
        "fern_int_checked_mul",
    ),
    entry(
        &["Int.checked_div"],
        &[Int, Int],
        OptionInt,
        "fern_int_checked_div",
    ),
    entry(
        &["Int.checked_rem"],
        &[Int, Int],
        OptionInt,
        "fern_int_checked_rem",
    ),
    entry(
        &["Int.checked_neg"],
        &[Int],
        OptionInt,
        "fern_int_checked_neg",
    ),
    returned(
        entry(
            &["String.index_of"],
            &[String, String],
            OptionInt,
            "fern_str_index_of",
        ),
        ValueAbi::PackedOption,
    ),
    returned(
        entry(
            &["String.char_at"],
            &[String, Int],
            OptionInt,
            "fern_str_char_at",
        ),
        ValueAbi::PackedOption,
    ),
    returned(
        entry(
            &["String.split"],
            &[String, String],
            ListString,
            "fern_str_split",
        ),
        ValueAbi::StringList,
    ),
    returned(
        entry(&["String.lines"], &[String], ListString, "fern_str_lines"),
        ValueAbi::StringList,
    ),
    arguments(
        entry(
            &["String.join"],
            &[ListString, String],
            String,
            "fern_str_join",
        ),
        &[(0, ValueAbi::StringList)],
    ),
    entry(&["List.len", "list_len"], &[ListA], Int, "fern_list_len"),
    entry(&["List.get", "list_get"], &[ListA, Int], A, "fern_list_get"),
    entry(&["List.head", "list_head"], &[ListA], A, "fern_list_head"),
    entry(&["List.at"], &[ListA, Int], OptionA, "fern_list_at"),
    entry(&["List.first"], &[ListA], OptionA, "fern_list_first"),
    entry(&["List.last"], &[ListA], OptionA, "fern_list_last"),
    entry(&["List.take"], &[ListA, Int], ListA, "fern_list_take"),
    entry(&["List.drop"], &[ListA, Int], ListA, "fern_list_drop"),
    entry(&["List.sum"], &[ListInt], Int, "fern_list_sum"),
    entry(&["List.range"], &[Int, Int], ListInt, "fern_list_range"),
    entry(&["List.zip"], &[ListA, ListB], ListPairAB, "fern_list_zip"),
    operation(
        entry(&["List.sort"], &[ListA], ListA, "fern_list_sort"),
        Operation::ScalarSort,
    ),
    entry(
        &["List.tail", "list_tail"],
        &[ListA],
        ListA,
        "fern_list_tail",
    ),
    entry(
        &["List.push", "list_push"],
        &[ListA, A],
        ListA,
        "fern_list_push",
    ),
    entry(
        &["List.reverse", "list_reverse"],
        &[ListA],
        ListA,
        "fern_list_reverse",
    ),
    entry(
        &["List.concat", "list_concat"],
        &[ListA, ListA],
        ListA,
        "fern_list_concat",
    ),
    entry(
        &["List.is_empty", "list_is_empty"],
        &[ListA],
        Bool,
        "fern_list_is_empty",
    ),
    operation(
        entry(&["List.contains"], &[ListA, A], Bool, "fern_list_contains"),
        Operation::ScalarContains,
    ),
    entry(&["Option.is_some"], &[OptionA], Bool, "fern_result_is_ok"),
    operation(
        entry(&["Option.is_none"], &[OptionA], Bool, "fern_result_is_ok"),
        Operation::InvertBool,
    ),
    entry(
        &["Option.unwrap_or"],
        &[OptionA, A],
        A,
        "fern_result_unwrap_or",
    ),
    entry(&["Result.is_ok"], &[ResultAE], Bool, "fern_result_is_ok"),
    operation(
        entry(&["Result.is_err"], &[ResultAE], Bool, "fern_result_is_ok"),
        Operation::InvertBool,
    ),
    entry(
        &["Result.unwrap_or"],
        &[ResultAE, A],
        A,
        "fern_result_unwrap_or",
    ),
    entry(
        &["fs.read", "File.read", "read_file"],
        &[String],
        ResultSI,
        "fern_read_file",
    ),
    entry(
        &["fs.write", "File.write", "write_file"],
        &[String, String],
        ResultII,
        "fern_write_file",
    ),
    entry(
        &["fs.append", "File.append", "append_file"],
        &[String, String],
        ResultII,
        "fern_append_file",
    ),
    entry(
        &["fs.exists", "File.exists", "file_exists"],
        &[String],
        Bool,
        "fern_file_exists",
    ),
    entry(
        &["fs.delete", "File.delete", "delete_file"],
        &[String],
        ResultII,
        "fern_delete_file",
    ),
    entry(
        &["fs.size", "File.size", "file_size"],
        &[String],
        ResultII,
        "fern_file_size",
    ),
    entry(
        &["fs.is_dir", "File.is_dir"],
        &[String],
        Bool,
        "fern_is_dir",
    ),
    returned(
        entry(
            &["fs.list_dir", "File.list_dir"],
            &[String],
            DirectoryResult,
            "fern_read_dir_result",
        ),
        ValueAbi::HeapStringListResult,
    ),
    entry(
        &["json.parse", "Json.parse"],
        &[String],
        Json(JsonShape::ResultValue),
        "fern_json_value_parse",
    ),
    entry(
        &["json.stringify", "Json.stringify"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultString),
        "fern_json_value_stringify",
    ),
    entry(
        &["http.get", "Http.get"],
        &[String],
        ResultSI,
        "fern_http_get",
    ),
    entry(
        &["http.post", "Http.post"],
        &[String, String],
        ResultSI,
        "fern_http_post",
    ),
    entry(
        &["sql.open", "Sql.open"],
        &[String],
        ResultII,
        "fern_sql_open",
    ),
    entry(
        &["sql.close", "Sql.close"],
        &[Int],
        ResultII,
        "fern_sql_close",
    ),
    entry(
        &["sql.execute", "Sql.execute"],
        &[Int, String],
        ResultII,
        "fern_sql_execute",
    ),
    entry(
        &["actors.start", "Actors.start"],
        &[String],
        Int,
        "fern_actor_start",
    ),
    entry(
        &["actors.post", "Actors.post"],
        &[Int, String],
        ResultII,
        "fern_actor_post",
    ),
    entry(
        &["actors.next", "Actors.next"],
        &[Int],
        ResultSI,
        "fern_actor_next",
    ),
    entry(
        &["actors.monitor", "Actors.monitor"],
        &[Int, Int],
        ResultII,
        "fern_actor_monitor",
    ),
    entry(
        &["actors.demonitor", "Actors.demonitor"],
        &[Int, Int],
        ResultII,
        "fern_actor_demonitor",
    ),
    entry(
        &["actors.restart", "Actors.restart"],
        &[Int],
        ResultII,
        "fern_actor_restart",
    ),
    entry(
        &["actors.supervise", "Actors.supervise"],
        &[Int, Int, Int, Int],
        ResultII,
        "fern_actor_supervise",
    ),
    entry(
        &[
            "actors.supervise_one_for_all",
            "Actors.supervise_one_for_all",
        ],
        &[Int, Int, Int, Int],
        ResultII,
        "fern_actor_supervise_one_for_all",
    ),
    entry(
        &[
            "actors.supervise_rest_for_one",
            "Actors.supervise_rest_for_one",
        ],
        &[Int, Int, Int, Int],
        ResultII,
        "fern_actor_supervise_rest_for_one",
    ),
    entry(&["System.args_count"], &[], Int, "fern_args_count"),
    entry(&["System.arg"], &[Int], String, "fern_arg"),
    returned(
        entry(&["System.args"], &[], ListString, "fern_args"),
        ValueAbi::StringList,
    ),
    entry(&["System.exit"], &[Int], Unit, "fern_exit"),
    entry(&["System.getenv"], &[String], String, "fern_getenv"),
    entry(&["System.setenv"], &[String, String], Int, "fern_setenv"),
    entry(&["System.cwd"], &[], String, "fern_cwd"),
    entry(&["System.chdir"], &[String], Int, "fern_chdir"),
    entry(&["System.hostname"], &[], String, "fern_hostname"),
    entry(&["System.user"], &[], String, "fern_user"),
    entry(&["System.home"], &[], String, "fern_home"),
    entry(
        &["Regex.is_match"],
        &[String, String],
        Bool,
        "fern_regex_is_match",
    ),
    returned(
        entry(
            &["Regex.find_all"],
            &[String, String],
            ListString,
            "fern_regex_find_all",
        ),
        ValueAbi::StringList,
    ),
    returned(
        entry(
            &["Regex.split"],
            &[String, String],
            ListString,
            "fern_regex_split",
        ),
        ValueAbi::StringList,
    ),
    entry(
        &["Regex.replace"],
        &[String, String, String],
        String,
        "fern_regex_replace",
    ),
    entry(
        &["Regex.replace_all"],
        &[String, String, String],
        String,
        "fern_regex_replace_all",
    ),
    entry(&["Tui.Style.black"], &[String], String, "fern_style_black"),
    entry(&["Tui.Style.red"], &[String], String, "fern_style_red"),
    entry(&["Tui.Style.green"], &[String], String, "fern_style_green"),
    entry(
        &["Tui.Style.yellow"],
        &[String],
        String,
        "fern_style_yellow",
    ),
    entry(&["Tui.Style.blue"], &[String], String, "fern_style_blue"),
    entry(
        &["Tui.Style.magenta"],
        &[String],
        String,
        "fern_style_magenta",
    ),
    entry(&["Tui.Style.cyan"], &[String], String, "fern_style_cyan"),
    entry(&["Tui.Style.white"], &[String], String, "fern_style_white"),
    entry(
        &["Tui.Style.bright_black"],
        &[String],
        String,
        "fern_style_bright_black",
    ),
    entry(
        &["Tui.Style.bright_red"],
        &[String],
        String,
        "fern_style_bright_red",
    ),
    entry(
        &["Tui.Style.bright_green"],
        &[String],
        String,
        "fern_style_bright_green",
    ),
    entry(
        &["Tui.Style.bright_yellow"],
        &[String],
        String,
        "fern_style_bright_yellow",
    ),
    entry(
        &["Tui.Style.bright_blue"],
        &[String],
        String,
        "fern_style_bright_blue",
    ),
    entry(
        &["Tui.Style.bright_magenta"],
        &[String],
        String,
        "fern_style_bright_magenta",
    ),
    entry(
        &["Tui.Style.bright_cyan"],
        &[String],
        String,
        "fern_style_bright_cyan",
    ),
    entry(
        &["Tui.Style.bright_white"],
        &[String],
        String,
        "fern_style_bright_white",
    ),
    entry(
        &["Tui.Style.on_black"],
        &[String],
        String,
        "fern_style_on_black",
    ),
    entry(
        &["Tui.Style.on_red"],
        &[String],
        String,
        "fern_style_on_red",
    ),
    entry(
        &["Tui.Style.on_green"],
        &[String],
        String,
        "fern_style_on_green",
    ),
    entry(
        &["Tui.Style.on_yellow"],
        &[String],
        String,
        "fern_style_on_yellow",
    ),
    entry(
        &["Tui.Style.on_blue"],
        &[String],
        String,
        "fern_style_on_blue",
    ),
    entry(
        &["Tui.Style.on_magenta"],
        &[String],
        String,
        "fern_style_on_magenta",
    ),
    entry(
        &["Tui.Style.on_cyan"],
        &[String],
        String,
        "fern_style_on_cyan",
    ),
    entry(
        &["Tui.Style.on_white"],
        &[String],
        String,
        "fern_style_on_white",
    ),
    entry(&["Tui.Style.bold"], &[String], String, "fern_style_bold"),
    entry(&["Tui.Style.dim"], &[String], String, "fern_style_dim"),
    entry(
        &["Tui.Style.italic"],
        &[String],
        String,
        "fern_style_italic",
    ),
    entry(
        &["Tui.Style.underline"],
        &[String],
        String,
        "fern_style_underline",
    ),
    entry(&["Tui.Style.blink"], &[String], String, "fern_style_blink"),
    entry(
        &["Tui.Style.reverse"],
        &[String],
        String,
        "fern_style_reverse",
    ),
    entry(
        &["Tui.Style.strikethrough"],
        &[String],
        String,
        "fern_style_strikethrough",
    ),
    entry(
        &["Tui.Style.color"],
        &[String, Int],
        String,
        "fern_style_color",
    ),
    entry(
        &["Tui.Style.on_color"],
        &[String, Int],
        String,
        "fern_style_on_color",
    ),
    entry(
        &["Tui.Style.rgb"],
        &[String, Int, Int, Int],
        String,
        "fern_style_rgb",
    ),
    entry(
        &["Tui.Style.on_rgb"],
        &[String, Int, Int, Int],
        String,
        "fern_style_on_rgb",
    ),
    entry(
        &["Tui.Style.hex"],
        &[String, String],
        String,
        "fern_style_hex",
    ),
    entry(
        &["Tui.Style.on_hex"],
        &[String, String],
        String,
        "fern_style_on_hex",
    ),
    entry(&["Tui.Style.reset"], &[String], String, "fern_style_reset"),
    entry(&["Tui.Status.warn"], &[String], String, "fern_status_warn"),
    entry(&["Tui.Status.ok"], &[String], String, "fern_status_ok"),
    entry(&["Tui.Status.info"], &[String], String, "fern_status_info"),
    entry(
        &["Tui.Status.error"],
        &[String],
        String,
        "fern_status_error",
    ),
    entry(
        &["Tui.Status.debug"],
        &[String],
        String,
        "fern_status_debug",
    ),
    entry(&["Tui.Log.debug"], &[String], String, "fern_log_debug"),
    entry(&["Tui.Log.info"], &[String], String, "fern_log_info"),
    entry(&["Tui.Log.warn"], &[String], String, "fern_log_warn"),
    entry(&["Tui.Log.error"], &[String], String, "fern_log_error"),
    entry(&["Tui.Live.print"], &[String], Unit, "fern_live_print"),
    entry(&["Tui.Live.clear_line"], &[], Unit, "fern_live_clear_line"),
    entry(&["Tui.Live.update"], &[String], Unit, "fern_live_update"),
    entry(&["Tui.Live.done"], &[], Unit, "fern_live_done"),
    entry(&["Tui.Live.sleep"], &[Int], Unit, "fern_sleep_ms"),
    entry(
        &["Tui.Term.move_to"],
        &[Int, Int],
        Unit,
        "fern_term_move_to",
    ),
    entry(&["Tui.Term.up"], &[Int], Unit, "fern_term_up"),
    entry(&["Tui.Term.down"], &[Int], Unit, "fern_term_down"),
    entry(&["Tui.Term.left"], &[Int], Unit, "fern_term_left"),
    entry(&["Tui.Term.right"], &[Int], Unit, "fern_term_right"),
    entry(&["Tui.Term.clear"], &[], Unit, "fern_term_clear"),
    entry(
        &["Tui.Term.hide_cursor"],
        &[],
        Unit,
        "fern_term_hide_cursor",
    ),
    entry(
        &["Tui.Term.show_cursor"],
        &[],
        Unit,
        "fern_term_show_cursor",
    ),
    entry(
        &["Tui.Term.save_cursor"],
        &[],
        Unit,
        "fern_term_save_cursor",
    ),
    entry(
        &["Tui.Term.restore_cursor"],
        &[],
        Unit,
        "fern_term_restore_cursor",
    ),
    entry(&["Tui.Term.is_tty"], &[], Bool, "fern_term_is_tty"),
    entry(
        &["Tui.Term.color_support"],
        &[],
        Int,
        "fern_term_color_support",
    ),
    entry(
        &["Tui.Prompt.input"],
        &[String],
        String,
        "fern_prompt_input",
    ),
    returned(
        entry(
            &["Tui.Prompt.confirm"],
            &[String],
            Bool,
            "fern_prompt_confirm",
        ),
        ValueAbi::Word32,
    ),
    arguments(
        returned(
            entry(
                &["Tui.Prompt.select"],
                &[String, ListString],
                Int,
                "fern_prompt_select",
            ),
            ValueAbi::Word32,
        ),
        &[(1, ValueAbi::StringList)],
    ),
    entry(
        &["Tui.Prompt.password"],
        &[String],
        String,
        "fern_prompt_password",
    ),
    entry(
        &["Tui.Prompt.int"],
        &[String, Int, Int],
        Int,
        "fern_prompt_int",
    ),
    entry(
        &["Tui.Panel.new"],
        &[String],
        Native(NativeType::Panel),
        "fern_panel_new",
    ),
    entry(
        &["Tui.Panel.title"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "fern_panel_title",
    ),
    entry(
        &["Tui.Panel.subtitle"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "fern_panel_subtitle",
    ),
    entry(
        &["Tui.Panel.border"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "fern_panel_border_str",
    ),
    entry(
        &["Tui.Panel.width"],
        &[Native(NativeType::Panel), Int],
        Native(NativeType::Panel),
        "fern_panel_width",
    ),
    operation(
        entry(
            &["Tui.Panel.padding"],
            &[Native(NativeType::Panel), Int],
            Native(NativeType::Panel),
            "fern_panel_padding",
        ),
        Operation::UniformPadding,
    ),
    entry(
        &["Tui.Panel.border_color"],
        &[Native(NativeType::Panel), String],
        Native(NativeType::Panel),
        "fern_panel_border_color",
    ),
    entry(
        &["Tui.Panel.render"],
        &[Native(NativeType::Panel)],
        String,
        "fern_panel_render",
    ),
    entry(
        &["Tui.Table.new"],
        &[],
        Native(NativeType::Table),
        "fern_table_new",
    ),
    entry(
        &["Tui.Table.add_column"],
        &[Native(NativeType::Table), String],
        Native(NativeType::Table),
        "fern_table_add_column",
    ),
    arguments(
        entry(
            &["Tui.Table.add_row"],
            &[Native(NativeType::Table), ListString],
            Native(NativeType::Table),
            "fern_table_add_row",
        ),
        &[(1, ValueAbi::StringList)],
    ),
    entry(
        &["Tui.Table.title"],
        &[Native(NativeType::Table), String],
        Native(NativeType::Table),
        "fern_table_title",
    ),
    operation(
        entry(
            &["Tui.Table.border"],
            &[Native(NativeType::Table), String],
            Native(NativeType::Table),
            "fern_table_border",
        ),
        Operation::TableBorder,
    ),
    entry(
        &["Tui.Table.show_header"],
        &[Native(NativeType::Table), Int],
        Native(NativeType::Table),
        "fern_table_show_header",
    ),
    entry(
        &["Tui.Table.render"],
        &[Native(NativeType::Table)],
        String,
        "fern_table_render",
    ),
    entry(
        &["Tui.Tree.new"],
        &[String],
        Native(NativeType::Tree),
        "fern_tree_new",
    ),
    entry(
        &["Tui.Tree.add"],
        &[Native(NativeType::Tree), Native(NativeType::Tree)],
        Native(NativeType::Tree),
        "fern_tree_add",
    ),
    entry(
        &["Tui.Tree.render"],
        &[Native(NativeType::Tree)],
        String,
        "fern_tree_render",
    ),
    entry(
        &["Tui.Progress.new"],
        &[Int],
        Native(NativeType::Progress),
        "fern_progress_new",
    ),
    entry(
        &["Tui.Progress.description"],
        &[Native(NativeType::Progress), String],
        Native(NativeType::Progress),
        "fern_progress_description",
    ),
    entry(
        &["Tui.Progress.width"],
        &[Native(NativeType::Progress), Int],
        Native(NativeType::Progress),
        "fern_progress_width",
    ),
    entry(
        &["Tui.Progress.advance"],
        &[Native(NativeType::Progress)],
        Native(NativeType::Progress),
        "fern_progress_advance",
    ),
    entry(
        &["Tui.Progress.set"],
        &[Native(NativeType::Progress), Int],
        Native(NativeType::Progress),
        "fern_progress_set",
    ),
    entry(
        &["Tui.Progress.render"],
        &[Native(NativeType::Progress)],
        String,
        "fern_progress_render",
    ),
    entry(
        &["Tui.Spinner.new"],
        &[],
        Native(NativeType::Spinner),
        "fern_spinner_new",
    ),
    entry(
        &["Tui.Spinner.message"],
        &[Native(NativeType::Spinner), String],
        Native(NativeType::Spinner),
        "fern_spinner_message",
    ),
    entry(
        &["Tui.Spinner.style"],
        &[Native(NativeType::Spinner), String],
        Native(NativeType::Spinner),
        "fern_spinner_style",
    ),
    entry(
        &["Tui.Spinner.tick"],
        &[Native(NativeType::Spinner)],
        Native(NativeType::Spinner),
        "fern_spinner_tick",
    ),
    entry(
        &["Tui.Spinner.render"],
        &[Native(NativeType::Spinner)],
        String,
        "fern_spinner_render",
    ),
    returned(
        entry(&["System.exec"], &[String], ExecTuple, "fern_exec"),
        ValueAbi::ExecResult,
    ),
    returned(
        arguments(
            entry(
                &["System.exec_args"],
                &[ListString],
                ExecTuple,
                "fern_exec_args",
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
            "fern_regex_find",
        ),
        ValueAbi::RegexMatch,
    ),
    returned(
        entry(
            &["Regex.captures"],
            &[String, String],
            CapturesList,
            "fern_regex_captures",
        ),
        ValueAbi::RegexCaptures,
    ),
    returned(
        entry(&["Tui.Term.size"], &[], TermTuple, "fern_term_size"),
        ValueAbi::TermSize,
    ),
    entry(
        &["json.is_null", "Json.is_null"],
        &[Json(JsonShape::Value)],
        Bool,
        "fern_json_value_is_null",
    ),
    entry(
        &["json.get", "Json.get"],
        &[Json(JsonShape::Value), String],
        Json(JsonShape::ResultValue),
        "fern_json_value_get",
    ),
    entry(
        &["json.at", "Json.at"],
        &[Json(JsonShape::Value), Int],
        Json(JsonShape::ResultValue),
        "fern_json_value_at",
    ),
    entry(
        &["json.length", "Json.length"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultInt),
        "fern_json_value_length",
    ),
    entry(
        &["json.as_bool", "Json.as_bool"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultBool),
        "fern_json_value_as_bool",
    ),
    entry(
        &["json.as_int", "Json.as_int"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultInt),
        "fern_json_value_as_int",
    ),
    entry(
        &["json.as_float", "Json.as_float"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultFloat),
        "fern_json_value_as_float",
    ),
    entry(
        &["json.as_string", "Json.as_string"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultString),
        "fern_json_value_as_string",
    ),
    entry(
        &["json.number_text", "Json.number_text"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultString),
        "fern_json_value_number_text",
    ),
    entry(
        &["json.error_code", "Json.error_code"],
        &[Json(JsonShape::Error)],
        Int,
        "fern_json_value_error_code",
    ),
    entry(
        &["json.error_offset", "Json.error_offset"],
        &[Json(JsonShape::Error)],
        Int,
        "fern_json_value_error_offset",
    ),
    entry(
        &["json.error_message", "Json.error_message"],
        &[Json(JsonShape::Error)],
        String,
        "fern_json_value_error_message",
    ),
    entry(
        &["json.null", "Json.null"],
        &[],
        Json(JsonShape::Value),
        "fern_json_value_null",
    ),
    entry(
        &["json.from_bool", "Json.from_bool"],
        &[Bool],
        Json(JsonShape::Value),
        "fern_json_value_from_bool",
    ),
    entry(
        &["json.from_int", "Json.from_int"],
        &[Int],
        Json(JsonShape::Value),
        "fern_json_value_from_int",
    ),
    entry(
        &["json.from_float", "Json.from_float"],
        &[Float],
        Json(JsonShape::ResultValue),
        "fern_json_value_from_float",
    ),
    entry(
        &["json.from_string", "Json.from_string"],
        &[String],
        Json(JsonShape::ResultValue),
        "fern_json_value_from_string",
    ),
    entry(
        &["json.from_number_text", "Json.from_number_text"],
        &[String],
        Json(JsonShape::ResultValue),
        "fern_json_value_from_number_text",
    ),
    entry(
        &["json.from_array", "Json.from_array"],
        &[Json(JsonShape::List)],
        Json(JsonShape::ResultValue),
        "fern_json_value_from_array",
    ),
    entry(
        &["json.elements", "Json.elements"],
        &[Json(JsonShape::Value)],
        Json(JsonShape::ResultList),
        "fern_json_value_elements",
    ),
    returned(
        entry(
            &["json.members", "Json.members"],
            &[Json(JsonShape::Value)],
            Json(JsonShape::ResultMembers),
            "fern_json_value_members",
        ),
        ValueAbi::HeapJsonMembers,
    ),
    operation(
        entry(
            &["json.from_object", "Json.from_object"],
            &[Json(JsonShape::Map)],
            Json(JsonShape::ResultValue),
            "fern_json_value_from_object",
        ),
        Operation::JsonObject,
    ),
    returned(
        arguments(
            entry(
                &["System.exec_args_bounded"],
                &[ListString, Int, Int],
                BoundedExecResult,
                "fern_exec_args_bounded",
            ),
            &[(0, ValueAbi::StringList)],
        ),
        ValueAbi::HeapExecResult,
    ),
    entry(
        &["System.write_stderr"],
        &[String],
        ResultUnitInt,
        "fern_write_stderr",
    ),
    operation(
        entry(
            &["String.is_decimal", "str_is_decimal"],
            &[String],
            Bool,
            "fern_str_is_decimal",
        ),
        Operation::DecimalPredicate,
    ),
    entry(
        &["json.error_path", "Json.error_path"],
        &[Json(JsonShape::Error)],
        String,
        "fern_json_value_error_path",
    ),
    entry(
        &["$ffi.float32"],
        &[Float],
        ResultCFloat32,
        "fern_ffi_float32",
    ),
    entry(
        &["$ffi.borrow_string"],
        &[String],
        PtrByte,
        "fern_ffi_borrow_string",
    ),
    entry(
        &["$ffi.read_string"],
        &[Int, Int],
        ResultSS,
        "fern_ffi_read_string",
    ),
];

const OMISSIONS: &[Omission] = &[
    Omission {
        names: &[
            "fern_ffi_float32",
            "fern_ffi_borrow_string",
            "fern_ffi_read_string",
        ],
        reason: "Compiler-owned checked foreign adapters use private runtime identities, not directly callable source signatures.",
    },
    Omission {
        names: &["fern_json_codec_encode", "fern_json_codec_decode"],
        reason: "Typed compiler-owned JSON codec operations require a validated concrete descriptor, not a source runtime signature.",
    },
    Omission {
        names: &["fern_json_value_limit_error"],
        reason: "Internal checked JSON adapter preflight; not a source API.",
    },
    Omission {
        names: &[
            "fern_str_slice_is_valid",
            "fern_str_split_is_valid",
            "fern_str_decimal_size_is_valid",
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
            "fern_bool_to_str",
            "fern_int_to_str",
            "fern_float_to_str",
            "fern_print_float",
            "fern_println_float",
            "fern_print_bool",
            "fern_print_int",
            "fern_print_str",
            "fern_println_bool",
            "fern_println_int",
            "fern_println_str",
            "fern_result_err",
            "fern_result_ok",
            "fern_result_unwrap",
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
            "fern_list_all",
            "fern_list_any",
            "fern_list_filter",
            "fern_list_find",
            "fern_list_fold",
            "fern_list_map",
            "fern_option_map",
            "fern_result_and_then",
            "fern_result_map",
            "fern_result_unwrap_or_else",
        ],
        reason: "Source calls use compiler-owned typed closure lowering. The legacy C callback ABI lacks closure environments and is deliberately not invoked.",
    },
    Omission {
        names: &["fern_regex_captures_free", "fern_regex_match_free"],
        reason: "Internal native regex allocation lifecycle; source results are translated into managed tuples and lists.",
    },
    Omission {
        names: &[
            "fern_option_is_some",
            "fern_option_none",
            "fern_option_some",
            "fern_option_unwrap",
            "fern_option_unwrap_or",
        ],
        reason: "Legacy packed Option helper truncates payloads; Rust Options use heap Result helpers instead. No direct calls are safe.",
    },
    Omission {
        names: &[
            "spawn",
            "spawn_link",
            "receive",
            "fern_actor_clock_advance",
            "fern_actor_clock_now",
            "fern_actor_clock_set",
            "fern_actor_exit",
            "fern_actor_mailbox_len",
            "fern_actor_receive",
            "fern_actor_scheduler_next",
            "fern_actor_self",
            "fern_actor_send",
            "fern_actor_set_current",
            "fern_actor_spawn",
            "fern_actor_spawn_link",
        ],
        reason: "Internal actor/scheduler entry points or actor execution syntax; registry mailbox APIs do not implement autonomous actor execution.",
    },
    Omission {
        names: &[
            "fern_alloc",
            "fern_drop",
            "fern_dup",
            "fern_free",
            "fern_list_free",
            "fern_list_new",
            "fern_list_push_mut",
            "fern_list_with_capacity",
            "fern_rc_alloc",
            "fern_rc_drop",
            "fern_rc_dup",
            "fern_rc_flags",
            "fern_rc_refcount",
            "fern_rc_set_flags",
            "fern_rc_type_tag",
            "fern_set_args",
            "fern_sort_begin",
            "fern_sort_finish",
            "fern_sort_next",
            "fern_sort_report",
            "fern_str_list_free",
        ],
        reason: "Compiler-owned allocation, reference bookkeeping, mutation, sort driving, or process initialization; not public source-callable stdlib APIs.",
    },
    Omission {
        names: &["fern_list_dir"],
        reason: "Legacy nullable directory ABI remains available to C callers; source fs.list_dir uses the explicit Result helper.",
    },
    Omission {
        names: &[
            "fern_panel_free",
            "fern_progress_free",
            "fern_spinner_free",
            "fern_table_free",
            "fern_panel_border",
        ],
        reason: "Runtime-owned TUI lifecycle or internal enum helper; source objects expose builders, not native destruction or raw layouts.",
    },
    Omission {
        names: &["fern_list_contains_str"],
        reason: "Selected only by type-directed List.contains String dispatch; not an independent public source API.",
    },
    Omission {
        names: &["fern_list_sort_float", "fern_list_sort_str"],
        reason: "Selected only by type-directed List.sort Float/String dispatch; not independent public source APIs.",
    },
];
