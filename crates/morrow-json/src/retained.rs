//! Conservative Rust-owned allocation accounting for immutable JSON graphs.
use super::*;

/// Count Rust allocation requests retained by one sealed JSON owner.
///
/// Includes each Rc control block, Node, String capacity, array/member capacity,
/// and object-index capacity. Shared DAG children are intentionally counted once
/// per incoming path, giving a conservative bound without an allocating identity
/// table. Traversal is bounded by the sealed depth/expanded-node contract; invalid
/// graphs or arithmetic overflow return usize::MAX and cannot undercharge an owner.
pub fn retained_bytes(value: &Node) -> usize {
    let mut remaining = NODES;
    count(value, 1, &mut remaining).unwrap_or(usize::MAX)
}

fn count(value: &Node, depth: usize, left: &mut usize) -> Option<usize> {
    if depth > DEPTH || *left == 0 {
        return None;
    }
    *left -= 1;
    let mut bytes = std::mem::size_of::<Node>() + 2 * std::mem::size_of::<usize>();
    match &value.kind {
        Kind::Number(text) | Kind::String(text) => bytes = bytes.checked_add(text.capacity())?,
        Kind::Array(children) => {
            bytes = bytes.checked_add(
                children
                    .capacity()
                    .checked_mul(std::mem::size_of::<Json>())?,
            )?;
            for child in children {
                bytes = bytes.checked_add(count(child, depth + 1, left)?)?;
            }
        }
        Kind::Object(members, index) => {
            bytes = bytes.checked_add(
                members
                    .capacity()
                    .checked_mul(std::mem::size_of::<(Json, Json)>())?,
            )?;
            bytes =
                bytes.checked_add(index.capacity().checked_mul(std::mem::size_of::<usize>())?)?;
            for (key, value) in members {
                bytes = bytes.checked_add(count(key, depth + 1, left)?)?;
                bytes = bytes.checked_add(count(value, depth + 1, left)?)?;
            }
        }
        Kind::Null | Kind::Bool(_) => (),
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charges_rc_headers_and_real_string_array_and_index_capacities() {
        let mut text = String::with_capacity(4096);
        text.push('x');
        let capacity = text.capacity();
        let leaf = text_node(text, 0);
        let mut children = Vec::with_capacity(512);
        children.push(leaf);
        let child_capacity = children.capacity();
        let array = Node {
            kind: Kind::Array(children),
            offset: 0,
            height: 2,
            nodes: 2,
            encoded: 5,
        };
        let node = std::mem::size_of::<Node>() + 2 * std::mem::size_of::<usize>();
        assert_eq!(
            retained_bytes(&array),
            2 * node + capacity + child_capacity * std::mem::size_of::<Json>()
        );
        let mut members = Vec::with_capacity(128);
        members.push((text_node("k".into(), 0), Rc::new(array)));
        let member_capacity = members.capacity();
        let index = Vec::<usize>::with_capacity(256);
        let index_capacity = index.capacity();
        let object = Node {
            kind: Kind::Object(members, index),
            offset: 0,
            height: 3,
            nodes: 4,
            encoded: 11,
        };
        assert_eq!(
            retained_bytes(&object),
            4 * node
                + 1
                + capacity
                + child_capacity * 8
                + member_capacity * 16
                + index_capacity * 8
        );
    }
    #[test]
    fn shared_dag_edges_are_conservatively_charged_per_owner_path() {
        let leaf = text_node("value".into(), 0);
        let children = vec![leaf.clone(), leaf.clone()];
        let bytes = children.capacity() * std::mem::size_of::<Json>();
        let array = Node {
            kind: Kind::Array(children),
            offset: 0,
            height: 2,
            nodes: 3,
            encoded: 17,
        };
        assert_eq!(
            retained_bytes(&array),
            std::mem::size_of::<Node>() + 16 + bytes + 2 * retained_bytes(&leaf)
        );
    }
}
