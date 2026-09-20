use super::*;
use serde::Deserialize;

pub(super) fn authenticate(weak: Weak<RefCell<App>>) {
    let Some(app) = weak.upgrade() else {
        return;
    };
    let request_generation = {
        let mut state = app.borrow_mut();
        if state.auth_pending {
            return;
        }
        state.auth_enabled = true;
        state.auth_pending = true;
        state.request_generation = state.request_generation.wrapping_add(1);
        state.request_generation
    };
    spawn_local(async move {
        let result = session().await;
        let Some(app) = weak.upgrade() else {
            return;
        };
        {
            let mut state = app.borrow_mut();
            if state.request_generation != request_generation {
                return;
            }
            state.auth_pending = false;
            state.timer.take();
        }
        match result {
            Ok(csrf) => {
                if let Err(error) = connect(&app, csrf) {
                    let mut state = app.borrow_mut();
                    state.offline(
                        &error
                            .as_string()
                            .unwrap_or_else(|| "Connection failed".into()),
                    );
                    let _ = state.render();
                    drop(state);
                    schedule(&app);
                }
            }
            Err((unauthorized, message)) => {
                let mut state = app.borrow_mut();
                state.offline(&message);
                if unauthorized {
                    state.auth_enabled = false;
                }
                let _ = state.render();
                drop(state);
                if !unauthorized {
                    schedule(&app);
                }
            }
        }
    });
}

async fn session() -> Result<String, (bool, String)> {
    async fn fetch_session() -> Result<(Response, Deadline), JsValue> {
        let options = RequestInit::new();
        let deadline = Deadline::new(&options)?;
        options.set_credentials(RequestCredentials::SameOrigin);
        let response = JsFuture::from(window()?.fetch_with_str_and_init("/session", &options))
            .await?
            .dyn_into()?;
        Ok((response, deadline))
    }
    let (response, _deadline) = fetch_session().await.map_err(|_| {
        (
            false,
            "Offline · your draft remains editable on this device".into(),
        )
    })?;
    if response.status() == 401 || response.status() == 403 {
        return Err((true, "Couldn't start a session · tap Reconnect".into()));
    }
    if !response.ok() {
        return Err((
            false,
            "Server unavailable · reconnecting with backoff".into(),
        ));
    }
    let text = JsFuture::from(
        response
            .text()
            .map_err(|_| (false, "Invalid session response".into()))?,
    )
    .await
    .map_err(|_| (false, "Invalid session response".into()))?
    .as_string()
    .ok_or((false, "Invalid session response".into()))?;
    #[derive(Deserialize)]
    struct Session {
        csrf: String,
    }
    let session: Session = morrow_web_protocol::decode(text.as_bytes())
        .map_err(|_| (false, "Invalid session response".into()))?;
    if session.csrf.is_empty()
        || session.csrf.len() > 128
        || !session
            .csrf
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err((false, "Invalid session token".into()));
    }
    Ok(session.csrf)
}

fn connect(app: &Rc<RefCell<App>>, csrf: String) -> Result<(), JsValue> {
    let location = window()?.location();
    let scheme = if location.protocol()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    let protocols = Array::new();
    protocols.push(&JsValue::from_str(morrow_web_protocol::binary::SUBPROTOCOL));
    protocols.push(&JsValue::from_str(&format!("morrow.csrf.{csrf}")));
    let socket = WebSocket::new_with_str_sequence(
        &format!("{scheme}://{}/ws", location.host()?),
        &protocols,
    )?;
    socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
    let generation = {
        let mut state = app.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        state.timer.take();
        state.handshake.take();
        state.socket.take();
        state.csrf = Some(csrf);
        state.offline("Connecting · local drafts stay available");
        state.generation
    };
    let mut listeners = Vec::new();
    for name in ["open", "message", "close", "error"] {
        let weak = Rc::downgrade(app);
        listeners.push(Listener::new(socket.clone().into(), name, move |event| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let mut state = app.borrow_mut();
            if state.generation != generation {
                return;
            }
            let mut reconnect = false;
            let result = match name {
                "open" => {
                    if state.socket.as_ref().is_none_or(|socket| {
                        socket.socket.protocol() != morrow_web_protocol::binary::SUBPROTOCOL
                    }) {
                        Err(js_error(
                            "Server did not negotiate the Morrow protobuf protocol",
                        ))
                    } else {
                        state.send(&ClientMessage::Join {
                            room: ROOM.into(),
                            resume_namespace: state.namespace.clone(),
                        })
                    }
                }
                "message" => {
                    let message = event
                        .dyn_into::<MessageEvent>()
                        .ok()
                        .and_then(|event| event.data().dyn_into::<js_sys::ArrayBuffer>().ok());
                    match message {
                        Some(buffer) if buffer.byte_length() as usize <= MAX_FRAME_BYTES => {
                            let bytes = Uint8Array::new(&buffer).to_vec();
                            match morrow_web_protocol::binary::decode_server(&bytes) {
                                Ok(message) => match state.receive(message) {
                                    Ok(retry) => {
                                        reconnect = retry;
                                        Ok(())
                                    }
                                    Err(error) => Err(error),
                                },
                                Err(error) => Err(js_error(error)),
                            }
                        }
                        Some(_) => Err(js_error("Server frame exceeds the protocol limit")),
                        None => Err(js_error("Expected a protobuf binary frame")),
                    }
                }
                _ => {
                    state.offline("Disconnected · pending changes remain visible; reconnecting");
                    reconnect = true;
                    Ok(())
                }
            };
            if let Err(error) = result {
                state.offline(
                    &error
                        .as_string()
                        .unwrap_or_else(|| "Connection failed".into()),
                );
                if let Some(socket) = &state.socket {
                    let _ = socket.socket.close();
                }
                reconnect = true;
            }
            let _ = state.render();
            drop(state);
            if reconnect {
                schedule(&app);
            }
        })?);
    }
    app.borrow_mut().socket = Some(Socket {
        socket,
        _listeners: listeners,
    });
    let weak = Rc::downgrade(app);
    let callback = Closure::wrap(Box::new(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let mut state = app.borrow_mut();
        if state.generation != generation || state.online {
            return;
        }
        state.offline("Connection handshake timed out · reconnecting");
        if let Some(socket) = &state.socket {
            let _ = socket.socket.close();
        }
        let _ = state.render();
        drop(state);
        schedule(&app);
    }) as Box<dyn FnMut()>);
    let id = window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        10_000,
    )?;
    app.borrow_mut().handshake = Some(Timer {
        id,
        _callback: callback,
    });
    app.borrow_mut().render()
}

struct Deadline {
    controller: web_sys::AbortController,
    _timer: Timer,
}
impl Deadline {
    fn new(options: &RequestInit) -> Result<Self, JsValue> {
        let controller = web_sys::AbortController::new()?;
        options.set_signal(Some(&controller.signal()));
        let cancel = controller.clone();
        let callback = Closure::wrap(Box::new(move || cancel.abort()) as Box<dyn FnMut()>);
        let id = window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            10_000,
        )?;
        Ok(Self {
            controller,
            _timer: Timer {
                id,
                _callback: callback,
            },
        })
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        self.controller.abort();
    }
}

fn schedule(app: &Rc<RefCell<App>>) {
    let mut state = app.borrow_mut();
    if state.timer.is_some() || !state.auth_enabled {
        return;
    }
    let delay = reconnect_delay_ms(state.attempts, (js_sys::Math::random() * 1000.0) as u32);
    state.attempts = state.attempts.saturating_add(1);
    let weak = Rc::downgrade(app);
    let callback = Closure::wrap(Box::new(move || {
        // The retained closure is replaced after the asynchronous session response,
        // never dropped while it is executing.
        authenticate(weak.clone());
    }) as Box<dyn FnMut()>);
    if let Ok(window) = window()
        && let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            delay as i32,
        )
    {
        state.timer = Some(Timer {
            id,
            _callback: callback,
        });
    }
}

pub(super) fn logout(weak: Weak<RefCell<App>>) {
    let Some(app) = weak.upgrade() else {
        return;
    };
    let csrf = {
        let mut state = app.borrow_mut();
        state.auth_enabled = false;
        state.auth_pending = false;
        state.request_generation = state.request_generation.wrapping_add(1);
        state.generation = state.generation.wrapping_add(1);
        state.socket.take();
        state.timer.take();
        state.handshake.take();
        state.namespace = None;
        state.client = None;
        state.saved.snapshot = None;
        state.saved.had_pending = false;
        state.offline("Signed out · local draft retained, server state cleared");
        let _ = state.render();
        state.csrf.take()
    };
    spawn_local(async move {
        let result = async {
            let options = RequestInit::new();
            let _deadline = Deadline::new(&options)?;
            options.set_method("POST");
            options.set_credentials(RequestCredentials::SameOrigin);
            let headers = web_sys::Headers::new()?;
            if let Some(csrf) = csrf {
                headers.set("X-Morrow-CSRF", &csrf)?;
            }
            options.set_headers(&headers);
            let response: Response =
                JsFuture::from(window()?.fetch_with_str_and_init("/logout", &options))
                    .await?
                    .dyn_into()?;
            if !response.ok() {
                return Err(js_error("Server logout failed"));
            }
            Ok::<(), JsValue>(())
        }
        .await;
        if result.is_err()
            && let Some(app) = weak.upgrade()
        {
            let mut state = app.borrow_mut();
            state.status = "Signed out locally; server revocation unconfirmed while offline".into();
            let _ = state.render();
        }
    });
}
