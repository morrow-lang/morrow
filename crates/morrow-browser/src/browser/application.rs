//! Managed Morrow values never become host pointers. Every temporary export handle
//! has one Rust owner, including during failed projections and renderer errors.
use super::*;
use morrow_web_protocol::Task;

struct Module {
    exports: JsValue,
    memory: js_sys::WebAssembly::Memory,
    functions: RefCell<BTreeMap<String, Function>>,
    offset: u32,
    capacity: u32,
}
impl Module {
    fn function(&self, name: &str) -> Result<Function, JsValue> {
        if let Some(function) = self.functions.borrow().get(name) {
            return Ok(function.clone());
        }
        let function: Function = Reflect::get(&self.exports, &JsValue::from_str(name))?
            .dyn_into()
            .map_err(|_| js_error(format!("compiled Morrow export {name} missing")))?;
        self.functions
            .borrow_mut()
            .insert(name.into(), function.clone());
        Ok(function)
    }
    fn call(&self, name: &str, arguments: &[JsValue]) -> Result<JsValue, JsValue> {
        let values = Array::new();
        for value in arguments {
            values.push(value);
        }
        self.function(name)?.apply(&JsValue::NULL, &values)
    }
    fn application(&self, name: &str, arguments: &[JsValue]) -> Result<JsValue, JsValue> {
        self.call(&format!("morrow::checklist.{name}"), arguments)
    }
    fn number(value: JsValue) -> Result<u32, JsValue> {
        value
            .as_f64()
            .filter(|value| {
                value.is_finite()
                    && *value >= 0.0
                    && *value <= u32::MAX as f64
                    && value.fract() == 0.0
            })
            .map(|value| value as u32)
            .ok_or_else(|| js_error("invalid Morrow ABI number"))
    }
    fn integer(value: JsValue) -> Result<i64, JsValue> {
        js_sys::BigInt::new(&value)?
            .to_string(10)?
            .as_string()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| js_error("invalid Morrow integer"))
    }
    fn handle(self: &Rc<Self>, name: &str, arguments: &[JsValue]) -> Result<Handle, JsValue> {
        let id = Self::integer(self.application(name, arguments)?)?;
        if id <= 0 {
            return Err(js_error("invalid Morrow managed handle"));
        }
        Ok(Handle {
            module: self.clone(),
            id,
        })
    }
    fn string(self: &Rc<Self>, text: &str) -> Result<Handle, JsValue> {
        if text.len() > self.capacity as usize {
            return Err(js_error("Morrow host string limit"));
        }
        let memory = Uint8Array::new(&self.memory.buffer());
        let end = self
            .offset
            .checked_add(text.len() as u32)
            .ok_or_else(|| js_error("Morrow memory overflow"))?;
        if end > memory.length() {
            return Err(js_error("Morrow memory boundary"));
        }
        memory.subarray(self.offset, end).copy_from(text.as_bytes());
        let id =
            Self::integer(self.call("morrow_string_new", &[JsValue::from(text.len() as u32)])?)?;
        if id <= 0 {
            return Err(js_error("Morrow host handle limit"));
        }
        Ok(Handle {
            module: self.clone(),
            id,
        })
    }
}
struct Handle {
    module: Rc<Module>,
    id: i64,
}
impl Handle {
    fn argument(&self) -> JsValue {
        JsValue::from(self.id)
    }
    fn text(&self) -> Result<String, JsValue> {
        let length = Module::number(self.module.call("morrow_string_read", &[self.argument()])?)?;
        if length > self.module.capacity {
            return Err(js_error("Morrow string output limit"));
        }
        let memory = Uint8Array::new(&self.module.memory.buffer());
        let end = self
            .module
            .offset
            .checked_add(length)
            .ok_or_else(|| js_error("Morrow string overflow"))?;
        if end > memory.length() {
            return Err(js_error("Morrow string memory boundary"));
        }
        String::from_utf8(memory.subarray(self.module.offset, end).to_vec()).map_err(js_error)
    }
    fn get_text(&self, name: &str) -> Result<String, JsValue> {
        self.module.handle(name, &[self.argument()])?.text()
    }
    fn get_int(&self, name: &str) -> Result<i64, JsValue> {
        Module::integer(self.module.application(name, &[self.argument()])?)
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.module.call("morrow_release", &[self.argument()]);
    }
}

pub(super) struct Application {
    module: Rc<Module>,
    model: Handle,
    confirmed: Vec<Task>,
}
impl Application {
    pub fn new(exports: JsValue, draft: &str) -> Result<Self, JsValue> {
        let call = |name: &str| -> Result<JsValue, JsValue> {
            Reflect::get(&exports, &JsValue::from_str(name))?
                .dyn_into::<Function>()?
                .call0(&JsValue::NULL)
        };
        if Module::number(call("morrow_abi_version")?)? != 1 {
            return Err(js_error("unsupported Morrow host ABI"));
        }
        let offset = Module::number(call("morrow_io_buffer")?)?;
        let capacity = Module::number(call("morrow_io_capacity")?)?;
        if capacity == 0 || capacity > 65_536 {
            return Err(js_error("invalid Morrow host buffer"));
        }
        let memory = Reflect::get(&exports, &JsValue::from_str("morrow_memory"))?.dyn_into()?;
        let module = Rc::new(Module {
            exports,
            memory,
            functions: RefCell::new(BTreeMap::new()),
            offset,
            capacity,
        });
        let text = module.string(draft)?;
        let model = module.handle("model_init", &[text.argument()])?;
        Ok(Self {
            module,
            model,
            confirmed: Vec::new(),
        })
    }
    fn update(&mut self, event: Handle) -> Result<Option<Mutation>, JsValue> {
        let change = self
            .module
            .handle("update", &[self.model.argument(), event.argument()])?;
        let next = self.module.handle("change_model", &[change.argument()])?;
        let effect = match change.get_int("change_effect_kind")? {
            0 => None,
            1 => Some(Mutation::Add {
                label: change.get_text("change_effect_label")?,
            }),
            2 => Some(Mutation::SetDone {
                id: Decimal(change.get_int("change_effect_id")?),
                done: Module::number(
                    self.module
                        .application("change_effect_done", &[change.argument()])?,
                )? == 1,
            }),
            3 => Some(Mutation::Remove {
                id: Decimal(change.get_int("change_effect_id")?),
            }),
            _ => return Err(js_error("unsupported Morrow effect")),
        };
        self.model = next;
        Ok(effect)
    }
    pub fn draft(&mut self, text: &str) -> Result<(), JsValue> {
        let text = self.module.string(text)?;
        let event = self.module.handle("event_draft", &[text.argument()])?;
        self.update(event).map(|_| ())
    }
    pub fn draft_text(&self) -> Result<String, JsValue> {
        self.model.get_text("model_draft")
    }
    pub fn status(&self) -> Result<String, JsValue> {
        self.model.get_text("model_status")
    }
    pub fn filter(&mut self, value: i64) -> Result<(), JsValue> {
        let event = self
            .module
            .handle("event_filter", &[JsValue::from(value)])?;
        self.update(event).map(|_| ())
    }
    pub fn action(&mut self, action: &str, id: Option<i64>) -> Result<Option<Mutation>, JsValue> {
        let arguments = id.map(JsValue::from).into_iter().collect::<Vec<_>>();
        let event = self.module.handle(action, &arguments)?;
        self.update(event)
    }
    pub fn connection(&mut self, online: bool, pending: bool, status: &str) -> Result<(), JsValue> {
        let status = self.module.string(status)?;
        let event = self.module.handle(
            "event_connection",
            &[
                JsValue::from(i32::from(online)),
                JsValue::from(i32::from(pending)),
                status.argument(),
            ],
        )?;
        self.update(event).map(|_| ())
    }
    pub fn snapshot(&mut self, tasks: &[Task]) -> Result<(), JsValue> {
        // Transport deduplication is host work. Rebuilding an unchanged snapshot
        // on each keystroke would allocate the entire list before local update.
        if tasks == self.confirmed {
            return Ok(());
        }
        let mut list = self.module.handle("tasks_empty", &[])?;
        for task in tasks {
            let text = self.module.string(&task.label)?;
            list = self.module.handle(
                "tasks_push",
                &[
                    list.argument(),
                    JsValue::from(task.id.0),
                    text.argument(),
                    JsValue::from(i32::from(task.done)),
                ],
            )?;
        }
        let event = self.module.handle("event_snapshot", &[list.argument()])?;
        self.update(event)?;
        self.confirmed = tasks.to_vec();
        Ok(())
    }
    pub fn view(&self) -> Result<Vec<super::renderer::Node>, JsValue> {
        let view = self.module.handle("view", &[self.model.argument()])?;
        let count = view.get_int("view_len")?;
        if !(0..=600).contains(&count) {
            return Err(js_error("Morrow view node limit"));
        }
        let mut nodes = Vec::with_capacity(count as usize);
        for index in 0..count {
            let node = self
                .module
                .handle("view_node", &[view.argument(), JsValue::from(index)])?;
            let count = node.get_int("node_attributes_len")?;
            if !(0..=16).contains(&count) {
                return Err(js_error("Morrow view attribute limit"));
            }
            let mut attributes = BTreeMap::new();
            for index in 0..count {
                let attribute = self
                    .module
                    .handle("node_attribute", &[node.argument(), JsValue::from(index)])?;
                if attributes
                    .insert(
                        attribute.get_text("attribute_name")?,
                        attribute.get_text("attribute_value")?,
                    )
                    .is_some()
                {
                    return Err(js_error("duplicate Morrow view attribute"));
                }
            }
            nodes.push(super::renderer::Node {
                key: node.get_text("node_key")?,
                parent: node.get_text("node_parent")?,
                tag: node.get_text("node_tag")?,
                text: node.get_text("node_text")?,
                value: node.get_text("node_value")?,
                flags: node.get_int("node_flags")?,
                attributes,
            });
        }
        Ok(nodes)
    }
}
