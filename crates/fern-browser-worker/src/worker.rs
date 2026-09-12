//! Synchronous installation of listeners lets the generated worker boot offline.
use js_sys::Array;
use std::cell::RefCell;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::{JsFuture, future_to_promise};
use web_sys::{
    Cache, ExtendableEvent, FetchEvent, Request, RequestCache, RequestInit,
    ServiceWorkerGlobalScope, Url,
};

const PREFIX: &str = "fern-public-assets-v1-";
thread_local! {
    // The worker global owns these callbacks for its entire lifetime. No forgotten
    // closure or subscription captures a browser application's mutable state.
    static LISTENERS: RefCell<Option<Listeners>> = const { RefCell::new(None) };
}
struct Listeners {
    scope: ServiceWorkerGlobalScope,
    install: Closure<dyn FnMut(ExtendableEvent)>,
    activate: Closure<dyn FnMut(ExtendableEvent)>,
    fetch: Closure<dyn FnMut(FetchEvent)>,
}
impl Drop for Listeners {
    fn drop(&mut self) {
        self.scope.set_oninstall(None);
        self.scope.set_onactivate(None);
        self.scope.set_onfetch(None);
    }
}

fn cache_name() -> String {
    format!(
        "{PREFIX}{}",
        option_env!("FERN_WEB_CACHE_VERSION").unwrap_or("development")
    )
}
fn scope() -> Result<ServiceWorkerGlobalScope, JsValue> {
    js_sys::global()
        .dyn_into::<ServiceWorkerGlobalScope>()
        .map_err(|_| JsValue::from_str("Fern worker requires a service worker global"))
}
async fn cache(scope: &ServiceWorkerGlobalScope) -> Result<Cache, JsValue> {
    JsFuture::from(scope.caches()?.open(&cache_name()))
        .await?
        .dyn_into()
}
async fn install() -> Result<JsValue, JsValue> {
    let scope = scope()?;
    let assets = Array::new();
    let manifest = option_env!("FERN_WEB_ASSET_INTEGRITIES").unwrap_or("");
    for (path, integrity) in crate::asset_integrities(manifest).map_err(JsValue::from_str)? {
        let options = RequestInit::new();
        options.set_integrity(integrity);
        options.set_cache(RequestCache::Reload);
        assets.push(&Request::new_with_str_and_init(path, &options)?.into());
    }
    // Cache.addAll rejects the batch when any response cannot be stored. The
    // previous active worker/cache remains intact if installation fails.
    JsFuture::from(cache(&scope).await?.add_all_with_request_sequence(&assets)).await?;
    Ok(JsValue::UNDEFINED)
}
async fn activate() -> Result<JsValue, JsValue> {
    let scope = scope()?;
    let storage = scope.caches()?;
    let keys: Array = JsFuture::from(storage.keys()).await?.dyn_into()?;
    let current = cache_name();
    for key in keys.iter() {
        if let Some(name) = key.as_string()
            && name.starts_with(PREFIX)
            && name != current
        {
            JsFuture::from(storage.delete(&name)).await?;
        }
    }
    // Do not skip waiting: old pages must keep their matching asset revision.
    JsFuture::from(scope.clients().claim()).await?;
    Ok(JsValue::UNDEFINED)
}

async fn cached(path: &'static str) -> Result<JsValue, JsValue> {
    let value = JsFuture::from(cache(&scope()?).await?.match_with_str(path)).await?;
    if value.is_undefined() {
        // A cleared cache must not mix old boot code with a new deployed ABI.
        return Err(JsValue::from_str(
            "Fern offline assets were cleared; reload online to update",
        ));
    }
    Ok(value)
}

/// Called synchronously by generated initSync, before lifecycle events dispatch.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let scope = scope()?;
    let on_install = Closure::wrap(Box::new(move |event: ExtendableEvent| {
        let _ = event.wait_until(&future_to_promise(install()));
    }) as Box<dyn FnMut(ExtendableEvent)>);
    let on_activate = Closure::wrap(Box::new(move |event: ExtendableEvent| {
        let _ = event.wait_until(&future_to_promise(activate()));
    }) as Box<dyn FnMut(ExtendableEvent)>);
    let origin = scope.location().origin();
    let on_fetch = Closure::wrap(Box::new(move |event: FetchEvent| {
        let request = event.request();
        let Ok(url) = Url::new(&request.url()) else {
            return;
        };
        if let Some(path) = crate::cache_path(
            &request.method(),
            url.origin() == origin,
            &url.pathname(),
            &url.search(),
        ) {
            let _ = event.respond_with(&future_to_promise(cached(path)));
        }
    }) as Box<dyn FnMut(FetchEvent)>);
    let listeners = Listeners {
        scope: scope.clone(),
        install: on_install,
        activate: on_activate,
        fetch: on_fetch,
    };
    LISTENERS.with(|stored| {
        // Remove previous callbacks before installing their replacements.
        stored.borrow_mut().take();
        scope.set_oninstall(Some(listeners.install.as_ref().unchecked_ref()));
        scope.set_onactivate(Some(listeners.activate.as_ref().unchecked_ref()));
        scope.set_onfetch(Some(listeners.fetch.as_ref().unchecked_ref()));
        *stored.borrow_mut() = Some(listeners);
    });
    Ok(())
}
