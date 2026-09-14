//! A bounded keyed DOM renderer. Application decisions are supplied by Morrow nodes.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Node {
    pub key: String,
    pub parent: String,
    pub tag: String,
    pub text: String,
    pub value: String,
    pub flags: i64,
    pub attributes: BTreeMap<String, String>,
}
struct Mounted {
    element: Element,
    owned: bool,
    tag: String,
    text: bool,
    attributes: BTreeSet<String>,
}
#[derive(Default)]
pub(super) struct Renderer {
    nodes: BTreeMap<String, Mounted>,
}
impl Renderer {
    pub fn render(&mut self, document: &Document, nodes: Vec<Node>) -> Result<(), JsValue> {
        let mut keys = BTreeSet::new();
        for node in &nodes {
            if node.key.is_empty()
                || node.key.len() > 128
                || !keys.insert(node.key.clone())
                || !matches!(
                    node.tag.as_str(),
                    "" | "li" | "label" | "input" | "span" | "button" | "p" | "div"
                )
                || node.flags & !15 != 0
                || node.text.len() > 4096
                || node.value.len() > 4096
            {
                return Err(js_error("invalid Morrow view node"));
            }
            for (name, value) in &node.attributes {
                if !(matches!(name.as_str(), "class" | "type" | "title" | "role")
                    || name.starts_with("aria-")
                    || name.starts_with("data-"))
                    || name.len() > 64
                    || value.len() > 4096
                {
                    return Err(js_error("invalid Morrow view attribute"));
                }
            }
            if node.tag.is_empty() {
                element(document, &node.key)?;
            } else if node.parent == node.key
                || (!keys.contains(&node.parent)
                    && document.get_element_by_id(&node.parent).is_none())
            {
                return Err(js_error("Morrow view parent must precede its child"));
            }
            if let Some(current) = self.nodes.get(&node.key) {
                if current.tag != node.tag {
                    return Err(js_error("Morrow keyed node changed tag"));
                }
            } else if !node.tag.is_empty() && document.get_element_by_id(&node.key).is_some() {
                return Err(js_error("Morrow view key collides with host shell"));
            }
        }
        self.nodes.retain(|key, mounted| {
            if keys.contains(key) {
                true
            } else {
                if mounted.owned {
                    mounted.element.remove();
                }
                false
            }
        });
        for node in nodes {
            if !self.nodes.contains_key(&node.key) {
                let owned = !node.tag.is_empty();
                let target = if owned {
                    let target = document.create_element(&node.tag)?;
                    target.set_attribute("id", &node.key)?;
                    target
                } else {
                    element(document, &node.key)?
                };
                self.nodes.insert(
                    node.key.clone(),
                    Mounted {
                        element: target,
                        owned,
                        tag: node.tag.clone(),
                        text: false,
                        attributes: BTreeSet::new(),
                    },
                );
            }
            let parent = if !node.tag.is_empty() {
                Some(element(document, &node.parent).or_else(|_| {
                    self.nodes
                        .get(&node.parent)
                        .map(|node| node.element.clone())
                        .ok_or_else(|| js_error("missing Morrow parent"))
                })?)
            } else {
                None
            };
            let mounted = self
                .nodes
                .get_mut(&node.key)
                .ok_or_else(|| js_error("missing Morrow keyed node"))?;
            if let Some(parent) = parent {
                // Reparent only when necessary. Re-appending a focused input on every
                // update would unnecessarily disturb selection and accessibility state.
                if mounted.element.parent_element().as_ref() != Some(&parent) {
                    parent.append_child(&mounted.element)?;
                }
            }
            if !node.text.is_empty() || mounted.text {
                if mounted.element.text_content().as_deref() != Some(&node.text) {
                    mounted.element.set_text_content(Some(&node.text));
                }
                mounted.text = true;
            }
            if node.flags & 4 != 0 {
                mounted.element.set_attribute("hidden", "")?;
            } else {
                mounted.element.remove_attribute("hidden")?;
            }
            if let Some(input) = mounted.element.dyn_ref::<HtmlInputElement>() {
                input.set_disabled(node.flags & 1 != 0);
                input.set_checked(node.flags & 2 != 0);
                if node.flags & 8 != 0 && input.value() != node.value {
                    input.set_value(&node.value);
                }
            } else if let Some(button) = mounted.element.dyn_ref::<HtmlButtonElement>() {
                button.set_disabled(node.flags & 1 != 0);
            }
            for old in &mounted.attributes {
                if !node.attributes.contains_key(old) {
                    mounted.element.remove_attribute(old)?;
                }
            }
            for (name, value) in &node.attributes {
                if mounted.element.get_attribute(name).as_ref() != Some(value) {
                    mounted.element.set_attribute(name, value)?;
                }
            }
            mounted.attributes = node.attributes.into_keys().collect();
        }
        Ok(())
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        for node in self.nodes.values() {
            if node.owned {
                node.element.remove();
            }
        }
    }
}
