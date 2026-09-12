//! Bounded plain-text source types and function signatures for documentation and editors.
//! Consumers own markup escaping. No compiler-generated identity is displayed implicitly.
use crate::{Constructor, Diagnostic, Span, Type, ast, parse};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Per-render structured depth, aggregate node and UTF-8 output byte limits.
/// Limits may be lowered, never raised above the defaults. Source parsing separately
/// retains the parser's source/token/depth limits; source bodies are not rendered.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub depth: usize,
    pub nodes: usize,
    pub bytes: usize,
}

impl Default for Limits {
    /// Return conservative limits for one signature, independently of open-buffer count.
    fn default() -> Self {
        Self {
            depth: 128,
            nodes: 4096,
            bytes: 65_536,
        }
    }
}

/// Render a validated semantic source type; internal variables and Never are errors.
/// Caller-created trees need no prior validation. Output is plain text, not markup.
pub fn render_type(ty: &Type, limits: Limits) -> Result<String> {
    render_type_with_names(ty, &[], limits)
}

/// Render `ty`, renaming explicitly registered `$`-prefixed inferred quantifiers.
/// Names are assigned by first occurrence, avoiding every explicit source generic.
/// Duplicate or source-spelled registrations are errors; unknown internal names fail.
pub fn render_type_with_names(ty: &Type, generated: &[String], limits: Limits) -> Result<String> {
    let mut writer = Writer::new(limits, generated)?;
    writer.collect(ty, 0)?;
    writer.ty(ty, 0)?;
    Ok(writer.output)
}

/// Render a selected type using its enclosing scheme's inferred display-name assignment.
/// Explicit variables in both types are reserved first. All work shares one render budget.
/// The enclosing text is discarded; only the selected type is returned as plain text.
pub fn render_type_in_context(
    ty: &Type,
    context: &Type,
    generated: &[String],
    limits: Limits,
) -> Result<String> {
    let mut writer = Writer::new(limits, generated)?;
    writer.collect(context, 0)?;
    writer.collect(ty, 0)?;
    writer.ty(context, 0)?;
    writer.output.clear();
    writer.ty(ty, 0)?;
    Ok(writer.output)
}

/// Extract a current source header at its exact `fn` byte anchor after parsing.
/// Includes `pub` and the body separator, preserving omissions, patterns and guards.
/// Trailing comments/layout before the body are preserved; no body is included.
/// Invalid source/anchors or excessive output return a diagnostic, never stale text.
pub fn source_signature(source: &str, anchor: usize, limits: Limits) -> Result<String> {
    let mut writer = Writer::new(limits, &[])?;
    let program = parse::parse(source)?;
    let function = program
        .functions
        .iter()
        .find(|f| f.span.start == anchor)
        .ok_or_else(|| error("no function begins at this source anchor"))?;
    writer.source_shape(function)?;
    let header = source
        .get(anchor..function.body.span.start)
        .ok_or_else(|| error("invalid source signature range"))?
        .trim_end();
    if function.public {
        writer.push("pub ")?;
    }
    writer.push(header)?;
    Ok(writer.output)
}

/// Render finalized parameter/result types with the original source parameter patterns.
/// Guards/body are intentionally excluded: this presents a reusable group signature.
/// The caller supplies source AST, not normalized dispatch AST. Types/patterns/names
/// are bounded and validated; unused body/guard AST is never traversed or cloned.
pub fn resolved_signature(
    function: &ast::Function,
    parameters: &[Type],
    result: &Type,
    generated: &[String],
    limits: Limits,
) -> Result<String> {
    let mut writer = Writer::new(limits, generated)?;
    if parameters.len() != function.params.len() || parameters.len() > 255 {
        return Err(error(
            "resolved parameter count does not match the source signature",
        ));
    }
    for ty in parameters.iter().chain([result]) {
        writer.collect(ty, 0)?;
    }
    for param in &function.params {
        if let Some(ty) = &param.annotation {
            writer.collect(ty, 0)?;
        }
    }
    if let Some(ty) = &function.return_type {
        writer.collect(ty, 0)?;
    }
    if function.public {
        writer.push("pub ")?;
    }
    writer.push("fn ")?;
    writer.name(&function.name, true)?;
    writer.push("(")?;
    for (index, (param, ty)) in function.params.iter().zip(parameters).enumerate() {
        if index > 0 {
            writer.push(", ")?;
        }
        if let Some(label) = &param.label {
            writer.name(&label.name, false)?;
            writer.push(" ")?;
        }
        writer.pattern(&param.pattern, 0)?;
        writer.push(": ")?;
        writer.ty(ty, 0)?;
    }
    writer.push(") -> ")?;
    writer.ty(result, 0)?;
    Ok(writer.output)
}

struct Writer {
    limits: Limits,
    nodes: usize,
    output: String,
    generated: BTreeSet<String>,
    explicit: BTreeSet<String>,
    names: BTreeMap<String, String>,
    name_bytes: usize,
    next_name: usize,
}

impl Writer {
    /// Validate caller limits and register bounded internal quantifier identities.
    fn new(limits: Limits, generated: &[String]) -> Result<Self> {
        let maximum = Limits::default();
        if limits.depth == 0
            || limits.depth > maximum.depth
            || limits.nodes == 0
            || limits.nodes > maximum.nodes
            || limits.bytes == 0
            || limits.bytes > maximum.bytes
        {
            return Err(error("invalid presentation limits"));
        }
        if generated.len() > limits.nodes {
            return Err(error("presentation node limit exceeded"));
        }
        let mut result = Self {
            limits,
            nodes: generated.len(),
            output: String::new(),
            generated: BTreeSet::new(),
            explicit: BTreeSet::new(),
            names: BTreeMap::new(),
            name_bytes: 0,
            next_name: 0,
        };
        for name in generated {
            result.charge_name(name)?;
            if !name.starts_with('$') || name.len() == 1 || !result.generated.insert(name.clone()) {
                return Err(error(
                    "generated quantifiers require unique internal $-prefixed names",
                ));
            }
        }
        Ok(result)
    }

    /// Apply requested structured limits to the parsed header without rendering its body.
    fn source_shape(&mut self, function: &ast::Function) -> Result<()> {
        if function.params.len() > 255 {
            return Err(error("function parameter limit exceeded"));
        }
        for param in &function.params {
            if let Some(label) = &param.label {
                self.name(&label.name, false)?;
                self.push(" ")?;
            }
            self.pattern(&param.pattern, 0)?;
            if let Some(ty) = &param.annotation {
                self.collect(ty, 0)?;
            }
        }
        if let Some(ty) = &function.return_type {
            self.collect(ty, 0)?;
        }
        self.output.clear();
        Ok(())
    }

    /// Account for one visited node before recursion or child iteration.
    fn node(&mut self, depth: usize) -> Result<()> {
        if depth >= self.limits.depth || self.nodes >= self.limits.nodes {
            return Err(error("presentation depth or node limit exceeded"));
        }
        self.nodes += 1;
        Ok(())
    }

    /// Append only when the entire piece fits; never publish a silently truncated type.
    fn push(&mut self, value: &str) -> Result<()> {
        if value.len() > self.limits.bytes.saturating_sub(self.output.len()) {
            return Err(error("presentation byte limit exceeded"));
        }
        self.output.push_str(value);
        Ok(())
    }

    /// Bound retained name storage independently of output and before cloning strings.
    fn charge_name(&mut self, name: &str) -> Result<()> {
        if name.len() > self.limits.bytes.saturating_sub(self.name_bytes) {
            return Err(error("presentation name byte limit exceeded"));
        }
        self.name_bytes += name.len();
        Ok(())
    }

    /// Reserve all explicit quantifiers before assigning any inferred display name.
    fn collect(&mut self, ty: &Type, depth: usize) -> Result<()> {
        self.node(depth)?;
        self.type_head(ty)?;
        match ty {
            Type::Generic(name) if !self.generated.contains(name) => {
                if !generic_name(name) {
                    return Err(error("unresolved internal generic name"));
                }
                if !self.explicit.contains(name) {
                    self.charge_name(name)?;
                    self.explicit.insert(name.clone());
                }
            }
            Type::Union(fields) | Type::Tuple(fields) | Type::Named(_, fields) => {
                for field in fields {
                    self.collect(field, depth + 1)?;
                }
            }
            Type::Function(params, result) => {
                for param in params {
                    self.collect(param, depth + 1)?;
                }
                self.collect(result, depth + 1)?;
            }
            Type::List(a) | Type::Option(a) => self.collect(a, depth + 1)?,
            Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                self.collect(a, depth + 1)?;
                self.collect(b, depth + 1)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Reject impossible public type heads before scanning or allocating their names.
    fn type_head(&self, ty: &Type) -> Result<()> {
        match ty {
            Type::Never | Type::Infer(_) => {
                return Err(error("cannot present an unresolved internal type"));
            }
            Type::Generic(name) => {
                if name.len() > self.limits.bytes {
                    return Err(error("presentation name byte limit exceeded"));
                }
                if !self.generated.contains(name) && !generic_name(name) {
                    return Err(error("unresolved internal generic name"));
                }
            }
            Type::Named(name, fields) => {
                if name.len() > self.limits.bytes
                    || !identifier(name, true)
                    || builtin_type(name)
                    || (fields.is_empty() && generic_name(name))
                {
                    return Err(error("invalid nominal source type"));
                }
            }
            Type::Union(members) if !(2..=128).contains(&members.len()) => {
                return Err(error("union presentation requires 2 to 128 members"));
            }
            Type::Tuple(fields) if fields.is_empty() => {
                return Err(error("empty tuple type must use Unit"));
            }
            Type::Function(params, _) if params.len() > 255 => {
                return Err(error("function type parameter limit exceeded"));
            }
            _ => {}
        }
        Ok(())
    }

    /// Render source type grammar, rejecting impossible/internal representations.
    fn ty(&mut self, ty: &Type, depth: usize) -> Result<()> {
        self.node(depth)?;
        self.type_head(ty)?;
        match ty {
            Type::Never | Type::Infer(_) => {
                Err(error("cannot present an unresolved internal type"))
            }
            Type::Int => self.push("Int"),
            Type::Float => self.push("Float"),
            Type::Bool => self.push("Bool"),
            Type::String => self.push("String"),
            Type::Range => self.push("Range"),
            Type::Unit => self.push("()"),
            Type::Native(native) => self.push(native.name()),
            Type::Generic(name) => self.generic(name),
            Type::Union(members) => self.union(members, depth),
            Type::Tuple(fields) => {
                if fields.is_empty() {
                    return Err(error("empty tuple type must use Unit"));
                }
                self.types("(", fields, ")", depth, fields.len() == 1)
            }
            Type::Function(params, result) => {
                if params.len() > 255 {
                    return Err(error("function type parameter limit exceeded"));
                }
                self.types("(", params, ") -> ", depth, false)?;
                self.ty(result, depth + 1)
            }
            Type::Pid(a) => self.container("Pid", &[a], depth),
            Type::ActorFunction(_, _) => {
                Err(error("cannot present an internal actor function type"))
            }
            Type::List(a) => self.container("List", &[a], depth),
            Type::Option(a) => self.container("Option", &[a], depth),
            Type::Map(a, b) => self.container("Map", &[a, b], depth),
            Type::Result(a, b) => self.container("Result", &[a, b], depth),
            Type::Named(name, args) => {
                if builtin_type(name) {
                    return Err(error("builtin type requires its semantic type variant"));
                }
                self.name(name, true)?;
                if !args.is_empty() {
                    self.types("(", args, ")", depth, false)?;
                }
                Ok(())
            }
        }
    }

    /// Parenthesize only alternatives whose arrow or nested union needs an explicit boundary.
    fn union(&mut self, members: &[Type], depth: usize) -> Result<()> {
        for (index, member) in members.iter().enumerate() {
            if index != 0 {
                self.push(" | ")?;
            }
            let grouped = matches!(member, Type::Function(..) | Type::Union(_));
            if grouped {
                self.push("(")?;
            }
            self.ty(member, depth + 1)?;
            if grouped {
                self.push(")")?;
            }
        }
        Ok(())
    }

    /// Render a fixed-arity compound without cloning its children.
    fn container(&mut self, name: &str, children: &[&Type], depth: usize) -> Result<()> {
        self.push(name)?;
        self.push("(")?;
        for (index, child) in children.iter().enumerate() {
            if index > 0 {
                self.push(", ")?;
            }
            self.ty(child, depth + 1)?;
        }
        self.push(")")
    }

    /// Render a bounded child sequence and preserve singleton tuple identity.
    fn types(
        &mut self,
        open: &str,
        fields: &[Type],
        close: &str,
        depth: usize,
        comma: bool,
    ) -> Result<()> {
        self.push(open)?;
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                self.push(", ")?;
            }
            self.ty(field, depth + 1)?;
        }
        if comma {
            self.push(",")?;
        }
        self.push(close)
    }

    /// Assign fresh display variables deterministically without conflating explicit ones.
    fn generic(&mut self, name: &str) -> Result<()> {
        if !self.generated.contains(name) {
            if !generic_name(name) {
                return Err(error("unresolved internal generic name"));
            }
            return self.push(name);
        }
        if let Some(display) = self.names.get(name).cloned() {
            return self.push(&display);
        }
        for _ in 0..=self.limits.nodes {
            self.node(0)?;
            let display = display_name(self.next_name);
            self.next_name += 1;
            if self.explicit.contains(&display) {
                continue;
            }
            self.charge_name(&display)?;
            self.explicit.insert(display.clone());
            self.names.insert(name.to_owned(), display.clone());
            return self.push(&display);
        }
        Err(error("presentation generic name limit exceeded"))
    }

    /// Validate identifiers before output so caller AST cannot inject source syntax.
    fn name(&mut self, name: &str, qualified: bool) -> Result<()> {
        if name.len() > self.limits.bytes || !identifier(name, qualified) {
            return Err(error("invalid source name in presentation"));
        }
        self.push(name)
    }

    /// Render caller-created source patterns with the same aggregate limits as types.
    fn pattern(&mut self, pattern: &ast::Pattern, depth: usize) -> Result<()> {
        self.node(depth)?;
        use ast::PatternKind::*;
        let prefix_len = match &pattern.kind {
            Tuple(fields) => fields.len(),
            List { prefix, .. } | TupleRest { prefix, .. } => prefix.len(),
            _ => 0,
        };
        if prefix_len > 128 {
            return Err(error("pattern prefix limit exceeded"));
        }
        match &pattern.kind {
            Typed {
                pattern,
                annotation,
            } => {
                self.pattern(pattern, depth + 1)?;
                self.push(": ")?;
                self.ty(annotation, depth + 1)
            }
            Wildcard => self.push("_"),
            Bind(name) => self.binding(name),
            Int(value) => self.push(&value.to_string()),
            Bool(value) => self.push(if *value { "true" } else { "false" }),
            String(value) => self.quote(value),
            Tuple(fields) => self.patterns("(", fields, None, ")", depth, fields.len() == 1),
            TupleRest { prefix, rest } => self.patterns("(", prefix, Some(rest), ")", depth, false),
            List { prefix, rest } => self.patterns("[", prefix, rest.as_deref(), "]", depth, false),
            NamedConstructor { name, fields } => {
                if name.len() > self.limits.bytes || !constructor_name(name) {
                    return Err(error(
                        "nullary constructor requires an uppercase source name",
                    ));
                }
                self.name(name, true)?;
                if !fields.is_empty() {
                    self.patterns("(", fields, None, ")", depth, false)?;
                }
                Ok(())
            }
            Constructor {
                constructor,
                binding,
            } => self.constructor(*constructor, binding.as_deref()),
        }
    }

    /// Bindings must retain their source role instead of turning into constructors/wildcards.
    fn binding(&mut self, name: &str) -> Result<()> {
        if name == "_" || name.starts_with(|c: char| c.is_uppercase()) {
            return Err(error("invalid source binding pattern"));
        }
        self.name(name, false)
    }

    /// Preserve exact/prefix sequence patterns and reject nonbinding rest payloads.
    fn patterns(
        &mut self,
        open: &str,
        fields: &[ast::Pattern],
        rest: Option<&ast::Pattern>,
        close: &str,
        depth: usize,
        comma: bool,
    ) -> Result<()> {
        self.push(open)?;
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                self.push(", ")?;
            }
            self.pattern(field, depth + 1)?;
        }
        if let Some(rest) = rest {
            if !matches!(
                rest.kind,
                ast::PatternKind::Bind(_) | ast::PatternKind::Wildcard
            ) {
                return Err(error("pattern rest must bind a name or wildcard"));
            }
            if !fields.is_empty() {
                self.push(", ")?;
            }
            self.push("..")?;
            self.node(depth + 1)?;
            match &rest.kind {
                ast::PatternKind::Bind(name) if name != "_" => self.name(name, false)?,
                ast::PatternKind::Wildcard => self.push("_")?,
                _ => return Err(error("invalid rest binding")),
            }
        } else if comma {
            self.push(",")?;
        }
        self.push(close)
    }

    /// Render legacy flat constructors without accepting impossible None payloads.
    fn constructor(&mut self, constructor: Constructor, binding: Option<&str>) -> Result<()> {
        let name = match constructor {
            Constructor::Some => "Some",
            Constructor::None => "None",
            Constructor::Ok => "Ok",
            Constructor::Err => "Err",
        };
        self.push(name)?;
        if constructor == Constructor::None {
            return if binding.is_some() {
                Err(error("None cannot bind a payload"))
            } else {
                Ok(())
            };
        }
        self.push("(")?;
        match binding {
            Some(name) => self.binding(name)?,
            None => self.push("_")?,
        }
        self.push(")")
    }

    /// Quote exact Unicode string contents without reinterpreting interpolation braces.
    fn quote(&mut self, value: &str) -> Result<()> {
        if value.len() > self.limits.bytes {
            return Err(error("presentation byte limit exceeded"));
        }
        self.push("\"")?;
        for ch in value.chars() {
            match ch {
                '\n' => self.push("\\n")?,
                '\r' => self.push("\\r")?,
                '\t' => self.push("\\t")?,
                '"' => self.push("\\\"")?,
                '\\' => self.push("\\\\")?,
                '{' => self.push("\\{")?,
                '}' => self.push("\\}")?,
                '\0' => return Err(error("NUL is not a source string character")),
                _ => {
                    let mut buffer = [0; 4];
                    self.push(ch.encode_utf8(&mut buffer))?;
                }
            }
        }
        self.push("\"")
    }
}

/// Recognize source identifiers without treating arbitrary ASCII punctuation as names.
fn identifier(name: &str, qualified: bool) -> bool {
    !name.is_empty()
        && (qualified || !name.contains('.'))
        && name.split('.').all(|part| {
            let mut chars = part.chars();
            chars.next().is_some_and(|c| {
                c == '_' || c.is_ascii_alphabetic() || (!c.is_ascii() && !c.is_whitespace())
            }) && chars.all(|c| {
                c == '_' || c.is_ascii_alphanumeric() || (!c.is_ascii() && !c.is_whitespace())
            }) && !keyword(part)
        })
}

/// Reject reserved words that would alter a signature's parse rather than name a value.
fn keyword(name: &str) -> bool {
    matches!(
        name,
        "fn" | "pub"
            | "type"
            | "module"
            | "import"
            | "as"
            | "if"
            | "then"
            | "else"
            | "match"
            | "for"
            | "in"
            | "with"
            | "do"
            | "let"
            | "return"
            | "defer"
            | "break"
            | "continue"
            | "and"
            | "or"
            | "not"
            | "true"
            | "false"
    )
}

/// Match generic names the source parser treats as lowercase type variables.
fn generic_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_lowercase()) && identifier(name, false)
}

/// Bare uppercase final components are constructors rather than ordinary bindings.
fn constructor_name(name: &str) -> bool {
    name.rsplit('.')
        .next()
        .is_some_and(|part| part.starts_with(|c: char| c.is_uppercase()))
}

/// Prevent a caller-created nominal type from changing identity when parsed again.
fn builtin_type(name: &str) -> bool {
    matches!(
        name,
        "Int"
            | "Float"
            | "Bool"
            | "String"
            | "Unit"
            | "Range"
            | "List"
            | "Option"
            | "Result"
            | "Map"
    ) || crate::runtime::native_type(name).is_some()
}

/// Name inferred variables a..z, a1..z1, etc.; iteration is bounded by node limits.
fn display_name(index: usize) -> String {
    let letter = char::from(b'a' + (index % 26) as u8);
    if index < 26 {
        letter.to_string()
    } else {
        format!("{letter}{}", index / 26)
    }
}

/// Attach presentation failures to no fabricated source location.
fn error(message: &str) -> Diagnostic {
    Diagnostic::new(Span::default(), message)
}
