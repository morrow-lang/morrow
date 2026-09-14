//! Derivations produce ordinary checked Fern methods, never unchecked runtime shortcuts.
use super::*;
use std::borrow::Cow;

const BUILTINS: &str = r#"
trait Show(a):
    fn show(value: a) -> String
trait Eq(a):
    fn eq(left: a, right: a) -> Bool
    fn neq(left: a, right: a) -> Bool:
        not eq(left: left, right: right)
trait Ord(a) with Eq(a):
    fn compare(left: a, right: a) -> Ordering
trait Clone(a):
    fn clone(value: a) -> a
trait Json(a):
    fn to_json(value: a) -> Result(json.Value, json.Error)
    fn from_json(value: json.Value) -> Result(a, json.Error)
type Ordering:
    Less
    Equal
    Greater
"#;

/// Names the prelude introduces; a program declaring any of them cannot receive a forced prelude.
const PRELUDE_NAMES: &[&str] = &[
    "show",
    "eq",
    "neq",
    "compare",
    "clone",
    "to_json",
    "from_json",
    "Show",
    "Eq",
    "Ord",
    "Clone",
    "Json",
    "Ordering",
    "Less",
    "Equal",
    "Greater",
];

/// Whether forcing the prelude would collide with user declarations instead of helping.
pub(in crate::check) fn prelude_conflicts(source: &ast::Program) -> bool {
    source
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .chain(source.traits.iter().map(|t| t.name.as_str()))
        .chain(source.types.iter().map(|t| t.name.as_str()))
        .chain(source.newtypes.iter().map(|t| t.name.as_str()))
        .chain(
            source
                .types
                .iter()
                .flat_map(|t| t.variants.iter().map(|v| v.name.as_str())),
        )
        .any(|name| PRELUDE_NAMES.contains(&name))
}

pub(in crate::check) fn expand(
    source: &ast::Program,
    force: bool,
) -> Checked<Cow<'_, ast::Program>> {
    if !force
        && source.traits.is_empty()
        && source.implementations.is_empty()
        && source.functions.iter().all(|f| f.constraints.is_empty())
        && source
            .types
            .iter()
            .flat_map(|d| &d.derives)
            .chain(source.newtypes.iter().flat_map(|d| &d.derives))
            .all(|d| d.name == "Json")
        && !dependencies::uses_trait_prelude(source)?
    {
        return Ok(Cow::Borrowed(source));
    }
    if let Some(declaration) = source.traits.iter().find(|t| t.name == "Json") {
        return Err(Diagnostic::new(
            declaration.span,
            "Json is the built-in codec trait; implement it for your nominal type",
        ));
    }
    let mut output = source.clone();
    let mut builtin = crate::parse::parse(BUILTINS)?;
    let existing: HashSet<_> = source.traits.iter().map(|t| t.name.as_str()).collect();
    let removed: HashSet<_> = builtin
        .traits
        .iter()
        .filter(|t| existing.contains(t.name.as_str()))
        .flat_map(|t| {
            t.methods
                .iter()
                .flat_map(|m| std::iter::once(m.function.clone()).chain(m.default.clone()))
        })
        .collect();
    builtin
        .traits
        .retain(|t| !existing.contains(t.name.as_str()));
    builtin.functions.retain(|f| !removed.contains(&f.name));
    append(&mut output, builtin, "prelude");
    let mut generated = String::from(
        "fn json_to(value: a) -> Result(json.Value,json.Error):\n    json.parse(json.encode(value)?)\nfn json_from(value: json.Value) -> Result(a,json.Error):\n    json.decode(json.stringify(value)?, a)\n",
    );
    for ty in ["Int", "Float", "Bool", "String", "()"] {
        if !existing.contains("Show") {
            // Strings show as literals so structural text stays unambiguous: `["a", ""]`.
            let body = match ty {
                "()" => "\"()\"".to_owned(),
                "String" => "String.quote(value)".to_owned(),
                _ => "\"{value}\"".to_owned(),
            };
            generated.push_str(&format!(
                "impl Show({ty}):\n    fn show(value: {ty}) -> String: {body}\n"
            ));
        }
        if !existing.contains("Eq") {
            let body = if ty == "()" { "true" } else { "left == right" };
            generated.push_str(&format!(
                "impl Eq({ty}):\n    fn eq(left: {ty}, right: {ty}) -> Bool: {body}\n"
            ));
        }
        if !existing.contains("Clone") {
            generated.push_str(&format!(
                "impl Clone({ty}):\n    fn clone(value: {ty}) -> {ty}: value\n"
            ));
        }
    }
    if !existing.contains("Ord") {
        generated.push_str("impl Ord(String):\n    fn compare(left: String, right: String) -> Ordering:\n        let order = String.compare(left, right)\n        if order < 0: Less else: if order > 0: Greater else: Equal\n");
        generated.push_str("impl Ord(Int):\n    fn compare(left: Int, right: Int) -> Ordering:\n        if left < right: Less else: if left > right: Greater else: Equal\n");
        generated.push_str("impl Ord(Bool):\n    fn compare(left: Bool, right: Bool) -> Ordering:\n        if left == right: Equal else: if right: Less else: Greater\n");
        // NaN is unordered: it compares Equal to everything rather than faulting.
        generated.push_str("impl Ord(Float):\n    fn compare(left: Float, right: Float) -> Ordering:\n        if left < right: Less else: if left > right: Greater else: Equal\n");
        generated
            .push_str("impl Ord(()):\n    fn compare(left: (), right: ()) -> Ordering: Equal\n");
    }
    for trait_name in ["Show", "Eq", "Ord", "Clone"] {
        if existing.contains(trait_name) {
            continue;
        }
        generated.push_str(containers(trait_name));
        generated.push_str(maps(trait_name));
        for arity in dependencies::trait_tuple_arities(source)? {
            generated.push_str(&tuple(arity, trait_name));
        }
        for (name, parameters, variants) in [
            (
                "Option",
                vec!["a"],
                vec![("Some", vec![Type::Generic("a".into())]), ("None", vec![])],
            ),
            (
                "Result",
                vec!["a", "b"],
                vec![
                    ("Ok", vec![Type::Generic("a".into())]),
                    ("Err", vec![Type::Generic("b".into())]),
                ],
            ),
        ] {
            let declaration = ast::TypeDecl {
                name: name.into(),
                parameters: parameters.into_iter().map(str::to_owned).collect(),
                public: false,
                derives: vec![],
                record: false,
                span: Span::default(),
                variants: variants
                    .into_iter()
                    .map(|(name, fields)| ast::Variant {
                        name: name.into(),
                        span: Span::default(),
                        fields: fields
                            .into_iter()
                            .map(|ty| ast::Field {
                                name: None,
                                ty,
                                span: Span::default(),
                            })
                            .collect(),
                    })
                    .collect(),
            };
            generated.push_str(&nominal(&declaration, trait_name)?);
        }
    }
    for declaration in &source.types {
        for derive in &declaration.derives {
            if derive.name == "Json" {
                continue;
            }
            generated.push_str(&nominal(declaration, &derive.name)?);
            if generated.len() > 512 * 1024 {
                return Err(Diagnostic::new(
                    derive.span,
                    "trait derivation output limit exceeded",
                ));
            }
        }
    }
    for declaration in &source.newtypes {
        let decl = ast::TypeDecl {
            derives: declaration.derives.clone(),
            public: declaration.public,
            name: declaration.name.clone(),
            parameters: declaration.parameters.clone(),
            record: false,
            span: declaration.span,
            variants: vec![ast::Variant {
                name: declaration.constructor.clone(),
                span: declaration.span,
                fields: vec![ast::Field {
                    name: None,
                    ty: declaration.inner.clone(),
                    span: declaration.inner_span,
                }],
            }],
        };
        for derive in &declaration.derives {
            if derive.name != "Json" {
                generated.push_str(&nominal(&decl, &derive.name)?);
            }
        }
    }
    append(&mut output, crate::parse::parse(&generated)?, "derived");
    Ok(Cow::Owned(output))
}

fn append(output: &mut ast::Program, mut generated: ast::Program, namespace: &str) {
    let rename = |name: &mut String| {
        if name.starts_with('$') || matches!(name.as_str(), "json_to" | "json_from") {
            *name = format!("$traits_{namespace}_{name}");
        }
    };
    for function in &mut generated.functions {
        rename(&mut function.name);
    }
    for declaration in &mut generated.traits {
        for method in &mut declaration.methods {
            if let Some(default) = &mut method.default {
                rename(default);
            }
        }
    }
    for implementation in &mut generated.implementations {
        for (_, function) in &mut implementation.methods {
            rename(function);
        }
    }
    output.functions.extend(generated.functions);
    output.types.extend(generated.types);
    output.traits.extend(generated.traits);
    output.implementations.extend(generated.implementations);
}

fn nominal(decl: &ast::TypeDecl, trait_name: &str) -> Checked<String> {
    if !matches!(trait_name, "Show" | "Eq" | "Ord" | "Clone") {
        return Err(Diagnostic::new(
            decl.span,
            format!("unsupported derive trait '{trait_name}'"),
        ));
    }
    let owner = crate::format::type_text(&Type::Named(
        decl.name.clone(),
        decl.parameters.iter().cloned().map(Type::Generic).collect(),
    ))?;
    let used = nominal::generics(
        decl.variants
            .iter()
            .flat_map(|v| &v.fields)
            .map(|f| f.ty.clone()),
    );
    let parameters: Vec<_> = decl
        .parameters
        .iter()
        .filter(|p| used.contains(p))
        .collect();
    let bounds = if parameters.is_empty() {
        String::new()
    } else {
        format!(
            " where {}",
            parameters
                .iter()
                .map(|p| format!("{trait_name}({p})"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut text = format!("impl {trait_name}({owner}){bounds}:\n");
    let method = match trait_name {
        "Show" => "show",
        "Eq" => "eq",
        "Ord" => "compare",
        _ => "clone",
    };
    let result = match trait_name {
        "Show" => "String",
        "Eq" => "Bool",
        "Ord" => "Ordering",
        _ => &owner,
    };
    let binary = matches!(trait_name, "Eq" | "Ord");
    text.push_str(&format!(
        "    fn {method}({}) -> {result}:\n        match {}:\n",
        if binary {
            format!("left: {owner}, right: {owner}")
        } else {
            format!("value: {owner}")
        },
        if binary { "(left, right)" } else { "value" }
    ));
    for (tag, variant) in decl.variants.iter().enumerate() {
        let names: Vec<_> = (0..variant.fields.len())
            .map(|i| format!("field{i}"))
            .collect();
        let left = pattern(&variant.name, &names);
        let right_names: Vec<_> = names.iter().map(|n| format!("other_{n}")).collect();
        let right = pattern(&variant.name, &right_names);
        text.push_str(&format!(
            "            {} -> ",
            if binary {
                format!("({left}, {right})")
            } else {
                left
            }
        ));
        match trait_name {
            "Clone" => text.push_str(&pattern(
                &variant.name,
                &names
                    .iter()
                    .map(|n| format!("clone({n})"))
                    .collect::<Vec<_>>(),
            )),
            "Show" => {
                let name = variant.name.rsplit('.').next().unwrap_or(&variant.name);
                let parts: Vec<_> = names
                    .iter()
                    .zip(&variant.fields)
                    .map(|(n, f)| {
                        format!(
                            "{}{{show({n})}}",
                            if decl.record {
                                format!("{}: ", f.name.as_deref().unwrap_or(""))
                            } else {
                                String::new()
                            }
                        )
                    })
                    .collect();
                text.push_str(&format!(
                    "\"{name}{}\"",
                    if parts.is_empty() {
                        String::new()
                    } else {
                        format!("({})", parts.join(", "))
                    }
                ));
            }
            "Eq" => text.push_str(&if names.is_empty() {
                "true".into()
            } else {
                names
                    .iter()
                    .zip(&right_names)
                    .map(|(a, b)| format!("eq(left: {a}, right: {b})"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            }),
            "Ord" => {
                if names.is_empty() {
                    text.push_str("Equal");
                } else {
                    text.push('\n');
                    for (a, b) in names.iter().zip(&right_names) {
                        text.push_str(&format!("                let ordering = compare(left: {a}, right: {b})\n                match ordering:\n                    Equal -> ()\n                    _ -> return ordering\n"));
                    }
                    text.push_str("                Equal");
                }
            }
            _ => unreachable!(),
        }
        text.push('\n');
        if trait_name == "Ord" {
            for other in decl.variants.iter().skip(tag + 1) {
                let a = pattern(&variant.name, &vec!["_".into(); variant.fields.len()]);
                let b = pattern(&other.name, &vec!["_".into(); other.fields.len()]);
                text.push_str(&format!(
                    "            ({a}, {b}) -> Less\n            ({b}, {a}) -> Greater\n"
                ));
            }
        }
    }
    if trait_name == "Eq" && decl.variants.len() > 1 {
        text.push_str("            _ -> false\n");
    }
    Ok(text)
}

fn pattern(name: &str, fields: &[String]) -> String {
    if fields.is_empty() {
        name.to_owned()
    } else {
        format!("{name}({})", fields.join(", "))
    }
}

fn containers(trait_name: &str) -> &'static str {
    match trait_name {
        "Show" => {
            "impl Show(List(a)) where Show(a):\n    fn show(value: List(a)) -> String:\n        \"[\" + String.join(List.map(value, show), \", \") + \"]\"\n"
        }
        "Clone" => {
            "impl Clone(List(a)) where Clone(a):\n    fn clone(value: List(a)) -> List(a):\n        List.map(value, clone)\n"
        }
        "Eq" => {
            "impl Eq(List(a)) where Eq(a):\n    fn eq(left: List(a), right: List(a)) -> Bool:\n        match (left, right):\n            ([], []) -> true\n            ([a, ..xs], [b, ..ys]) -> eq(left: a, right: b) and eq(left: xs, right: ys)\n            _ -> false\n"
        }
        "Ord" => {
            "impl Ord(List(a)) where Ord(a):\n    fn compare(left: List(a), right: List(a)) -> Ordering:\n        match (left, right):\n            ([], []) -> Equal\n            ([], _) -> Less\n            (_, []) -> Greater\n            ([a, ..xs], [b, ..ys]) ->\n                match compare(left: a, right: b):\n                    Equal -> compare(left: xs, right: ys)\n                    order -> order\n"
        }
        _ => "",
    }
}

fn maps(trait_name: &str) -> &'static str {
    match trait_name {
        "Show" => {
            "impl Show(Map(k,v)) where Show(k), Show(v):\n    fn show(value:Map(k,v))->String:\n        let entries = List.map(Map.keys(value), (key:k)->\n            match Map.get(value,key):\n                Some(item) -> show(key) + \": \" + show(item)\n                None -> \"\"\n        )\n        \"%\\{\" + String.join(entries, \", \") + \"\\}\"\n"
        }
        "Eq" => {
            "impl Eq(Map(k,v)) where Eq(v):\n    fn eq(left:Map(k,v), right:Map(k,v))->Bool:\n        Map.len(left) == Map.len(right) and List.all(Map.keys(left), (key:k)->\n            match (Map.get(left,key), Map.get(right,key)):\n                (Some(a), Some(b)) -> eq(left:a, right:b)\n                _ -> false\n        )\n"
        }
        "Clone" => {
            "impl Clone(Map(k,v)) where Clone(k), Clone(v):\n    fn clone(value:Map(k,v))->Map(k,v):\n        List.fold(Map.keys(value), Map.new(), (acc:Map(k,v), key:k)->\n            match Map.get(value,key):\n                Some(item) -> Map.put(acc, clone(key), clone(item))\n                None -> acc\n        )\n"
        }
        _ => "",
    }
}

fn tuple(arity: usize, trait_name: &str) -> String {
    let params: Vec<_> = (0..arity).map(|i| format!("t{i}")).collect();
    let tuple = |items: Vec<String>| {
        format!(
            "({}{})",
            items.join(", "),
            if arity == 1 { "," } else { "" }
        )
    };
    let owner = tuple(params.clone());
    let bounds = params
        .iter()
        .map(|p| format!("{trait_name}({p})"))
        .collect::<Vec<_>>()
        .join(", ");
    let (method, result, binary) = match trait_name {
        "Show" => ("show", "String", false),
        "Eq" => ("eq", "Bool", true),
        "Ord" => ("compare", "Ordering", true),
        _ => ("clone", owner.as_str(), false),
    };
    let mut text = format!(
        "impl {trait_name}({owner}) where {bounds}:\n    fn {method}({})->{result}:\n",
        if binary {
            format!("left:{owner}, right:{owner}")
        } else {
            format!("value:{owner}")
        }
    );
    let fields: Vec<_> = (0..arity).map(|i| format!("value.{i}")).collect();
    match trait_name {
        "Show" => text.push_str(&format!(
            "        \"{}\"\n",
            tuple(fields.iter().map(|f| format!("{{show({f})}}")).collect())
        )),
        "Clone" => text.push_str(&format!(
            "        {}\n",
            tuple(fields.iter().map(|f| format!("clone({f})")).collect())
        )),
        "Eq" => text.push_str(&format!(
            "        {}\n",
            (0..arity)
                .map(|i| format!("eq(left:left.{i}, right:right.{i})"))
                .collect::<Vec<_>>()
                .join(" and ")
        )),
        "Ord" => {
            for i in 0..arity {
                text.push_str(&format!("        let order=compare(left:left.{i}, right:right.{i})\n        match order:\n            Equal -> ()\n            _ -> return order\n"));
            }
            text.push_str("        Equal\n");
        }
        _ => {}
    }
    text
}
