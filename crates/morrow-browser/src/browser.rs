use crate::*;
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use morrow_web_protocol::{ClientMessage, Mutation, ServerMessage, Status};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Document, Element, Event, EventTarget, HtmlButtonElement, HtmlInputElement, MessageEvent,
    RequestCredentials, RequestInit, Response, WebSocket, Window,
};

mod application;
mod renderer;
mod transport;

thread_local! { static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) }; }
const STORAGE_KEY: &str = "morrow.checklist.v1";
const ROOM: &str = "garden";

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
fn window() -> Result<Window, JsValue> {
    web_sys::window().ok_or_else(|| js_error("browser window unavailable"))
}
fn element(document: &Document, id: &str) -> Result<Element, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| js_error(format!("missing #{id}")))
}

/// Load real compiled Morrow application exports, restore bounded offline state and
/// mount the Rust host. Generated JavaScript only calls init() and this function.
#[wasm_bindgen]
pub async fn mount() -> Result<(), JsValue> {
    unmount();
    let document = window()?
        .document()
        .ok_or_else(|| js_error("document unavailable"))?;
    let result = mount_inner(document.clone()).await;
    if let Err(error) = &result
        && let Ok(status) = element(&document, "status")
    {
        status.set_text_content(Some(&format!(
            "Could not start Morrow: {}",
            error
                .as_string()
                .unwrap_or_else(|| "browser initialization failed".into())
        )));
    }
    result
}

async fn mount_inner(document: Document) -> Result<(), JsValue> {
    let response: Response = JsFuture::from(window()?.fetch_with_str("/morrow_app.wasm"))
        .await?
        .dyn_into()?;
    if !response.ok() {
        return Err(js_error("compiled Morrow application could not be loaded"));
    }
    if response
        .headers()
        .get("content-type")?
        .as_deref()
        .is_none_or(|mime| !mime.starts_with("application/wasm"))
    {
        return Err(js_error(
            "compiled application must be served as application/wasm",
        ));
    }
    let bytes = Uint8Array::new(&JsFuture::from(response.array_buffer()?).await?);
    if bytes.length() > 4_194_304 {
        return Err(js_error("application module exceeds preview limit"));
    }
    let instance = JsFuture::from(js_sys::WebAssembly::instantiate_buffer(
        &bytes.to_vec(),
        &Object::new(),
    ))
    .await?;
    let instance = Reflect::get(&instance, &JsValue::from_str("instance"))?;
    let exports = Reflect::get(&instance, &JsValue::from_str("exports"))?;
    let saved = window()?
        .local_storage()?
        .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
        .and_then(|text| Saved::decode(&text).ok())
        .unwrap_or(Saved {
            draft: String::new(),
            snapshot: None,
            had_pending: false,
        });
    let policy = application::Application::new(exports, &saved.draft)?;
    let draft: HtmlInputElement = element(&document, "draft")?.dyn_into()?;
    draft.set_value(&saved.draft);
    let app = Rc::new(RefCell::new(App {
        document,
        policy,
        client: None,
        saved,
        renderer: renderer::Renderer::default(),
        status: "Offline · your draft stays on this device".into(),
        online: false,
        namespace: None,
        socket: None,
        listeners: Vec::new(),
        timer: None,
        handshake: None,
        attempts: 0,
        generation: 0,
        request_generation: 0,
        auth_pending: false,
        auth_enabled: true,
        csrf: None,
    }));
    App::install(&app)?;
    app.borrow_mut().render()?;
    APP.with(|root| *root.borrow_mut() = Some(app.clone()));
    transport::authenticate(Rc::downgrade(&app), None);
    let registration = window()?
        .navigator()
        .service_worker()
        .register("/worker.js");
    spawn_local(async move {
        let _ = JsFuture::from(registration).await;
    });
    Ok(())
}

/// Unmount releases subscriptions, timers, socket callbacks and DOM handles.
#[wasm_bindgen]
pub fn unmount() {
    APP.with(|root| {
        root.borrow_mut().take();
    });
}

struct Listener {
    target: EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}
impl Listener {
    fn new(
        target: EventTarget,
        name: &'static str,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<Self, JsValue> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
        Ok(Self {
            target,
            name,
            callback,
        })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}
struct Socket {
    socket: WebSocket,
    _listeners: Vec<Listener>,
}
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = self.socket.close();
    }
}
struct Timer {
    id: i32,
    _callback: Closure<dyn FnMut()>,
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Ok(window) = window() {
            window.clear_timeout_with_handle(self.id);
        }
    }
}
struct App {
    document: Document,
    policy: application::Application,
    client: Option<Client>,
    saved: Saved,
    renderer: renderer::Renderer,
    status: String,
    online: bool,
    namespace: Option<String>,
    socket: Option<Socket>,
    listeners: Vec<Listener>,
    timer: Option<Timer>,
    handshake: Option<Timer>,
    attempts: u32,
    generation: u64,
    request_generation: u64,
    auth_pending: bool,
    auth_enabled: bool,
    csrf: Option<String>,
}
impl App {
    fn install(app: &Rc<RefCell<Self>>) -> Result<(), JsValue> {
        for (id, name) in [
            ("draft", "input"),
            ("add-form", "submit"),
            ("login-form", "submit"),
            ("tasks", "click"),
            ("filters", "click"),
            ("logout", "click"),
            ("retry", "click"),
        ] {
            let target: EventTarget = element(&app.borrow().document, id)?.into();
            let weak = Rc::downgrade(app);
            let listener = Listener::new(target, name, move |event| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if id == "login-form" {
                    event.prevent_default();
                    let key = element(&app.borrow().document, "access-key")
                        .ok()
                        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                        .map(|input| {
                            let key = input.value();
                            input.set_value("");
                            key
                        });
                    if let Some(key) = key {
                        transport::authenticate(Rc::downgrade(&app), Some(key));
                    }
                    return;
                }
                if id == "logout" {
                    transport::logout(Rc::downgrade(&app));
                    return;
                }
                if id == "retry" {
                    transport::authenticate(Rc::downgrade(&app), None);
                    return;
                }
                let mut state = app.borrow_mut();
                if let Err(error) = state.event(id, event) {
                    state.status = error.as_string().unwrap_or_else(|| "Action failed".into());
                }
                let _ = state.render();
            })?;
            app.borrow_mut().listeners.push(listener);
        }
        for name in ["online", "offline"] {
            let weak = Rc::downgrade(app);
            let listener = Listener::new(window()?.into(), name, move |_| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if name == "online" {
                    transport::authenticate(Rc::downgrade(&app), None);
                } else {
                    let mut state = app.borrow_mut();
                    state.offline("Offline · keep writing, reconnect to submit");
                    let _ = state.render();
                }
            })?;
            app.borrow_mut().listeners.push(listener);
        }
        Ok(())
    }

    fn event(&mut self, id: &str, event: Event) -> Result<(), JsValue> {
        match id {
            "draft" => {
                let input: HtmlInputElement = element(&self.document, "draft")?.dyn_into()?;
                self.policy.draft(&input.value())?;
                self.saved.draft = self.policy.draft_text()?;
                self.status = self.policy.status()?;
            }
            "add-form" => {
                event.prevent_default();
                if let Some(mutation) = self.policy.action("event_submit", None)? {
                    self.saved.draft = self.policy.draft_text()?;
                    self.submit(mutation)?;
                }
            }
            "filters" => {
                let target: Element = event
                    .target()
                    .ok_or_else(|| js_error("missing event target"))?
                    .dyn_into()?;
                if let Some(filter) = target
                    .get_attribute("data-filter")
                    .and_then(|s| s.parse::<i64>().ok())
                {
                    self.policy.filter(filter)?;
                }
            }
            "tasks" => {
                let target: Element = event
                    .target()
                    .ok_or_else(|| js_error("missing event target"))?
                    .dyn_into()?;
                if let Some(id) = target
                    .get_attribute("data-id")
                    .and_then(|s| s.parse::<i64>().ok())
                {
                    let action = if target.get_attribute("data-action").as_deref() == Some("remove")
                    {
                        "event_delete"
                    } else {
                        "event_toggle"
                    };
                    if let Some(mutation) = self.policy.action(action, Some(id))? {
                        self.submit(mutation)?;
                    }
                }
            }
            _ => {}
        }
        self.persist();
        Ok(())
    }

    fn submit(&mut self, mutation: Mutation) -> Result<(), JsValue> {
        let clears_draft = matches!(mutation, Mutation::Add { .. });
        let pending = self.client.as_ref().is_some_and(|c| c.pending().is_some());
        if !self.online || pending {
            return Err(js_error(
                "Reconnect or wait for the pending command before submitting",
            ));
        }
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| js_error("Sign in before submitting"))?;
        let command = client.submit(mutation).map_err(js_error)?;
        if clears_draft {
            self.policy.action("event_admitted", None)?;
            self.saved.draft = self.policy.draft_text()?;
        }
        self.saved.had_pending = true;
        self.persist();
        self.send(&ClientMessage::Command(command))?;
        self.status = "Saving · awaiting server confirmation".into();
        Ok(())
    }

    fn send(&self, message: &ClientMessage) -> Result<(), JsValue> {
        let socket = &self
            .socket
            .as_ref()
            .ok_or_else(|| js_error("connection unavailable"))?
            .socket;
        let bytes = morrow_web_protocol::binary::encode_client(message).map_err(js_error)?;
        if socket.ready_state() != WebSocket::OPEN
            || !can_send(socket.buffered_amount() as usize, bytes.len())
        {
            return Err(js_error(
                "Connection busy; pending command retained until reconnect",
            ));
        }
        socket.send_with_u8_array(&bytes)
    }

    fn receive(&mut self, message: ServerMessage) -> Result<bool, JsValue> {
        match message {
            ServerMessage::Connected(connected) => {
                self.handshake.take();
                self.namespace = Some(connected.namespace.clone());
                let retry = if let Some(client) = &mut self.client {
                    client.reconnect(connected.clone()).map_err(js_error)?
                } else {
                    self.client = Some(Client::new(connected.clone()).map_err(js_error)?);
                    None
                };
                self.saved.snapshot = Some(connected.snapshot);
                self.online = true;
                self.attempts = 0;
                self.status = if self.saved.had_pending && retry.is_none() {
                    "Connected · previous completion is uncertain; review refreshed state".into()
                } else {
                    "Connected · changes sync with everyone in this garden".into()
                };
                if let Some(command) = retry {
                    self.send(&ClientMessage::Command(command))?;
                }
            }
            ServerMessage::Snapshot(snapshot) => {
                if let Some(client) = &mut self.client {
                    client.accept_snapshot(snapshot, false).map_err(js_error)?;
                    self.saved.snapshot = Some(client.snapshot().clone());
                }
            }
            ServerMessage::Reset(snapshot) => {
                if let Some(client) = &mut self.client {
                    client
                        .accept_snapshot(snapshot.clone(), true)
                        .map_err(js_error)?;
                }
                self.saved.snapshot = Some(snapshot);
                self.status = "Server state reset · pending changes were not replayed".into();
                return Ok(true);
            }
            ServerMessage::Outcome(outcome) => {
                if let Some(client) = &mut self.client {
                    client.accept_outcome(&outcome).map_err(js_error)?;
                }
                self.saved.had_pending = false;
                self.status = match outcome.status {
                    Status::Applied => "Saved · confirmed by the server",
                    Status::Conflict => "Another client changed the list · review it and try again",
                    Status::NotFound => "That task no longer exists",
                    Status::Capacity => "This garden is full · remove a task first",
                    Status::Unknown => {
                        "Completion uncertain · review the refreshed list before retrying"
                    }
                }
                .into();
            }
            ServerMessage::Error(error) => {
                self.status = format!("Server: {error}");
                if matches!(error, Error::NamespaceExpired | Error::ConnectionExpired) {
                    self.namespace = None;
                    return Ok(true);
                }
                if matches!(error, Error::IncarnationMismatch | Error::SequenceGap) {
                    return Ok(true);
                }
                if error == Error::Unauthorized {
                    self.auth_enabled = false;
                    self.offline("Session revoked · sign in again");
                }
            }
        }
        self.persist();
        Ok(false)
    }

    fn offline(&mut self, status: &str) {
        self.online = false;
        if let Some(client) = &mut self.client {
            client.set_online(false);
        }
        self.status = status.into();
        self.persist();
    }

    fn persist(&mut self) {
        if let Ok(draft) = self.policy.draft_text() {
            self.saved.draft = draft;
        }
        let Ok(Some(storage)) = window().and_then(|w| w.local_storage()) else {
            return;
        };
        self.saved
            .keep_existing_draft(storage.get_item(STORAGE_KEY).ok().flatten().as_deref());
        if let Ok(text) = self.saved.encode() {
            let _ = storage.set_item(STORAGE_KEY, &text);
        }
    }

    fn render(&mut self) -> Result<(), JsValue> {
        let pending = self
            .client
            .as_ref()
            .is_some_and(|client| client.pending().is_some());
        self.policy.connection(self.online, pending, &self.status)?;
        let viewers = if self.online {
            self.saved
                .snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.viewers.0)
        } else {
            0
        };
        self.policy.presence(viewers)?;
        element(&self.document, "viewers")?
            .set_text_content(Some(&self.policy.presence_label(self.online, viewers)?));
        self.policy.snapshot(
            self.saved
                .snapshot
                .as_ref()
                .map_or(&[], |snapshot| snapshot.tasks.as_slice()),
        )?;
        let view = self.policy.view()?;
        self.renderer.render(&self.document, view)
    }
}
