//! Immutable node metadata, bounded indexing and exact JSON encoding.
use super::*;
/// Seal an already allocated decoded string with its exact escaped byte count and original offset.
pub fn text_node(text: String, offset: usize) -> Json {
    let encoded = 2 + text.bytes().map(escape_size).sum::<usize>();
    Rc::new(Node {
        kind: Kind::String(text),
        offset,
        height: 1,
        nodes: 1,
        encoded,
    })
}
/// Return the native encoded byte count for one decoded UTF-8 byte, including JSON controls.
fn escape_size(byte: u8) -> usize {
    match byte {
        b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 2,
        0..=31 => 6,
        _ => 1,
    }
}
/// Validate expanded child metadata, preserving order and building a checked object index before publication.
pub fn seal(
    children: Vec<Json>,
    object: bool,
    offset: usize,
    budget: &mut Budget<'_>,
) -> Result<Json> {
    let mut node = Node {
        kind: Kind::Null,
        offset,
        height: 1,
        nodes: 1,
        encoded: 2 + children.len().saturating_sub(1),
    };
    for child in &children {
        if child.encoded > OUTPUT - node.encoded
            || child.nodes > NODES - node.nodes
            || child.height >= DEPTH
        {
            return Err(error(4, budget.at as i64));
        }
        node.encoded += child.encoded;
        node.nodes += child.nodes;
        node.height = node.height.max(child.height + 1);
    }
    node.kind = if object {
        Limits::charge(
            &mut budget.limits.allocated,
            children.len() * std::mem::size_of::<Json>(),
        )?;
        let mut members = Vec::with_capacity(children.len() / 2);
        let mut children = children.into_iter();
        while let (Some(key), Some(value)) = (children.next(), children.next()) {
            members.push((key, value));
        }
        let index = sorted(&members, budget)?;
        Kind::Object(members, index)
    } else {
        Kind::Array(children)
    };
    Ok(Rc::new(node))
}
/// Read a validated object key String; non-key internal nodes produce an empty fallback.
fn text(node: &Node) -> &str {
    match &node.kind {
        Kind::String(text) => text,
        _ => "",
    }
}
/// Compare decoded key bytes lexically, charging each examined byte before comparison.
fn compare(a: &str, b: &str, budget: &mut Budget<'_>) -> Result<std::cmp::Ordering> {
    for (a, b) in a.bytes().zip(b.bytes()) {
        budget.work(1)?;
        if a != b {
            return Ok(a.cmp(&b));
        }
    }
    Ok(a.len().cmp(&b.len()))
}
/// Build a stable bounded index and reject the later key in the first sorted duplicate group.
fn sorted(members: &[(Json, Json)], budget: &mut Budget<'_>) -> Result<Vec<usize>> {
    let count = members.len();
    if count == 0 {
        return Ok(Vec::new());
    }
    budget.allocate(count * 8)?;
    budget.allocate(count * 8)?;
    let mut index: Vec<_> = (0..count).collect();
    let mut scratch = vec![0; count];
    let mut width = 1;
    while width < count {
        for base in (0..count).step_by(width * 2) {
            merge(members, &index, &mut scratch, base, width, budget)?;
        }
        std::mem::swap(&mut index, &mut scratch);
        width *= 2;
    }
    for pair in index.windows(2) {
        let a = &members[pair[0]].0;
        let b = &members[pair[1]].0;
        if compare(text(a), text(b), budget)?.is_eq() {
            return Err(error(3, b.offset as i64));
        }
    }
    Ok(index)
}
/// Merge two bounded index runs into scratch storage, charging comparisons and steps before writes.
fn merge(
    members: &[(Json, Json)],
    index: &[usize],
    scratch: &mut [usize],
    base: usize,
    width: usize,
    budget: &mut Budget<'_>,
) -> Result<()> {
    let middle = (base + width).min(index.len());
    let end = (middle + width).min(index.len());
    let (mut a, mut b) = (base, middle);
    for slot in &mut scratch[base..end] {
        budget.work(1)?;
        let left = b == end
            || (a < middle
                && !compare(
                    text(&members[index[a]].0),
                    text(&members[index[b]].0),
                    budget,
                )?
                .is_gt());
        *slot = index[if left {
            let i = a;
            a += 1;
            i
        } else {
            let i = b;
            b += 1;
            i
        }];
    }
    Ok(())
}

/// Binary-search a bounded decoded key using the stable index, distinguishing absence from profile failure.
pub fn get(
    members: &[(Json, Json)],
    index: &[usize],
    key: &str,
    limits: &mut Limits,
) -> Result<Json> {
    if key.len() > INPUT {
        return Err(error(4, -1));
    }
    let (mut low, mut high) = (0, index.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let slot = index[middle];
        Limits::charge(
            &mut limits.work,
            key.len().min(text(&members[slot].0).len()),
        )?;
        match key.as_bytes().cmp(text(&members[slot].0).as_bytes()) {
            std::cmp::Ordering::Equal => return Ok(members[slot].1.clone()),
            std::cmp::Ordering::Less => high = middle,
            std::cmp::Ordering::Greater => low = middle + 1,
        }
    }
    Err(error(6, -1))
}
/// Reserve validated output size and traversal work before encoding one immutable subtree.
pub fn stringify(node: &Node, limits: &mut Limits) -> Result<String> {
    if node.encoded > OUTPUT {
        return Err(error(4, -1));
    }
    Limits::charge(&mut limits.work, node.encoded + node.nodes)?;
    Limits::charge(&mut limits.allocated, node.encoded + 1)?;
    let mut out = String::with_capacity(node.encoded);
    encode(node, &mut out);
    debug_assert_eq!(out.len(), node.encoded);
    Ok(out)
}
/// Append a validated subtree into reserved output; sealed depth and expanded-node metadata bound recursion.
pub fn encode(node: &Node, out: &mut String) {
    match &node.kind {
        Kind::Null => out.push_str("null"),
        Kind::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Kind::Number(text) => out.push_str(text),
        Kind::String(text) => encode_string(text, out),
        Kind::Array(values) => {
            out.push('[');
            for (i, value) in values.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                encode(value, out);
            }
            out.push(']');
        }
        Kind::Object(values, _) => {
            out.push('{');
            for (i, (key, value)) in values.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                encode_string(text(key), out);
                out.push(':');
                encode(value, out);
            }
            out.push('}');
        }
    }
}
/// Append exact JSON escapes for decoded scalar text, preserving non-ASCII bytes and escaped NUL.
pub fn encode_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0'..='\u{1f}' => {
                out.push_str("\\u00");
                out.push(char::from_digit((ch as u32) >> 4, 16).unwrap());
                out.push(char::from_digit((ch as u32) & 15, 16).unwrap());
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
}
