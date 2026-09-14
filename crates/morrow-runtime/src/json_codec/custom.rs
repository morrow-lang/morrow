//! Native custom codecs share the enclosing operation's resources and fault cell.
use super::*;

const SCOPE_LIMIT_FAULT: i64 = i64::MIN + 19;

/// Stop an infallible JSON constructor inside a resource-exhausted custom method.
/// # Safety
/// Fault addresses the live exclusive invocation word passed to generated code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn morrow_json_codec_scope_check(fault: *mut i64) {
    if morrow_json::scope::exhausted() && unsafe { *fault } == 0 {
        unsafe {
            *fault = SCOPE_LIMIT_FAULT;
        }
    }
}

/// Compiler-emitted table; each thunk adapts its checked source argument to bits.
#[repr(C)]
pub(super) struct Callbacks {
    encode: unsafe extern "C" fn(*mut i64, i64) -> i64,
    decode: unsafe extern "C" fn(*mut i64, i64) -> i64,
    pub(super) managed: i64,
}

impl Execution<'_> {
    /// Descriptors, callback signatures and values are validated by the compiler.
    pub(super) unsafe fn custom_encode(
        &mut self,
        plan: *const Codec,
        value: i64,
        depth: usize,
    ) -> Result<Json> {
        // SAFETY: the caller establishes the validated custom descriptor and the
        // selected callback returns a rooted Result(json.Value,json.Error).
        unsafe {
            let value = self.custom_call(plan, value, true)?;
            let value = json::node(value as *const json::NativeJson);
            self.budget.work(value.nodes)?;
            self.charge_custom_nodes(value.nodes)?;
            if value.height.saturating_add(depth) > DEPTH {
                return Err(error(4, -1));
            }
            Ok(value)
        }
    }

    pub(super) unsafe fn custom_decode(&mut self, plan: *const Codec, value: &Json) -> Result<i64> {
        self.budget.work(value.nodes)?;
        self.budget
            .allocate(std::mem::size_of::<json::NativeJson>())?;
        let input = json::wrap(value.clone());
        let _root = ConstructionRoot::new(input as usize);
        // SAFETY: the owned input handle is rooted until the callback returns.
        unsafe { self.custom_call(plan, input as i64, false) }
    }

    fn charge_custom_nodes(&mut self, nodes: usize) -> Result<()> {
        if nodes > NODES.saturating_sub(self.budget.nodes) {
            return Err(error(4, -1));
        }
        morrow_json::scope::charge_nodes(nodes)?;
        self.budget.nodes += nodes;
        Ok(())
    }

    unsafe fn custom_call(&mut self, plan: *const Codec, value: i64, encode: bool) -> Result<i64> {
        self.budget.work(1)?;
        let scope = morrow_json::scope::Scope::enter(
            self.budget.work.min(self.budget.limits.work),
            (morrow_json::ALLOC - self.budget.allocated).min(self.budget.limits.allocated),
            NODES - self.budget.nodes,
        )?;
        let mut local_fault = 0i64;
        let fault = if self.fault.is_null() {
            &mut local_fault
        } else {
            self.fault
        };
        // SAFETY: the compiler owns this two-function table. Its thunks accept
        // the invocation's live fault word and a full-width typed payload.
        let result = unsafe {
            let callbacks = &*(*plan).children.cast::<Callbacks>();
            let call = if encode {
                callbacks.encode
            } else {
                callbacks.decode
            };
            call(fault, value)
        };
        let spent = scope.spent();
        drop(scope);
        let quota_fault = unsafe { *fault } == SCOPE_LIMIT_FAULT;
        if quota_fault {
            unsafe {
                *fault = 0;
            }
        }
        // A language fault wins and its neutral return is never dereferenced.
        if unsafe { *fault } != 0 {
            return Err(error(4, -1));
        }
        self.budget.work(spent.work)?;
        if spent.allocated != 0 {
            self.budget.allocate(spent.allocated)?;
        }
        self.charge_custom_nodes(spent.nodes)?;
        if spent.exhausted || quota_fault {
            morrow_json::scope::mark_exhausted();
            return Err(error(4, -1));
        }
        let _root = ConstructionRoot::new(result as usize);
        // SAFETY: a successful checked callback returns a live Result object;
        // the precise root protects both its payload and error path while read.
        unsafe {
            let result = &*(result as *const abi::ResultValue);
            if result.tag == 0 {
                Ok(result.value)
            } else {
                Err(self.custom_error(&*(result.value as *const json::NativeError))?)
            }
        }
    }

    unsafe fn custom_error(&mut self, native: &json::NativeError) -> Result<Error> {
        let code = u8::try_from(native.code)
            .ok()
            .filter(|code| (1..=14).contains(code))
            .unwrap_or(4);
        let mut length = 0usize;
        // SAFETY: json.Error owns a live NUL-terminated path; scanning is bounded
        // before allocation and the caller keeps its Result root registered.
        unsafe {
            while length < OUTPUT && *native.path.add(length) != 0 {
                self.budget.work(1)?;
                length += 1;
            }
            if length == OUTPUT || length > OUTPUT.saturating_sub(self.path.len()) {
                return Err(error(4, -1));
            }
            self.budget.work(self.path.len() + length)?;
            self.budget.allocate(self.path.len() + length + 40)?;
            let path =
                std::str::from_utf8(std::slice::from_raw_parts(native.path.cast::<u8>(), length))
                    .map_err(|_| error(4, -1))?;
            Ok(Error {
                code,
                offset: native.offset,
                path: Some(Rc::new(format!("{}{path}", self.path))),
            })
        }
    }
}
