#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

use std::borrow::Cow;

use json_patch::Patch;
use leptos::{create_signal, ReadSignal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsValue;

cfg_if::cfg_if! {
    if #[cfg(all(feature = "actix", feature = "ssr"))] {
        mod actix;
        pub use crate::actix::*;
    }
}

cfg_if::cfg_if! {
    if #[cfg(all(feature = "axum", feature = "ssr"))] {
        mod axum;
        pub use crate::axum::*;
    }
}

/// A server signal update containing the signal type name and json patch.
///
/// This is whats sent over the SSE, and is used to patch the signal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSignalUpdate {
    name: Cow<'static, str>,
    patch: Patch,
}

impl ServerSignalUpdate {
    /// Creates a new [`ServerSignalUpdate`] from an old and new instance of `T`.
    pub fn new<T>(
        name: impl Into<Cow<'static, str>>,
        old: &T,
        new: &T,
    ) -> Result<Self, serde_json::Error>
    where
        T: Serialize,
    {
        let left = serde_json::to_value(old)?;
        let right = serde_json::to_value(new)?;
        let patch = json_patch::diff(&left, &right);
        Ok(ServerSignalUpdate {
            name: name.into(),
            patch,
        })
    }

    /// Creates a new [`ServerSignalUpdate`] from two json values.
    pub fn new_from_json<T>(name: impl Into<Cow<'static, str>>, old: &Value, new: &Value) -> Self {
        let patch = json_patch::diff(old, new);
        ServerSignalUpdate {
            name: name.into(),
            patch,
        }
    }
}

/// Provides a SSE url for server signals, if there is not already one provided.
/// This ensures that you can provide it at the highest possible level, without overwriting a SSE
/// that has already been provided (for example, by a server-rendering integration.)
///
/// Note, the server should have a route to handle this SSE.
///
/// # Example
///
/// ```ignore
/// #[component]
/// pub fn App() -> impl IntoView {
///     // Provide SSE connection
///     leptos_sse::provide_sse("http://localhost:3000/sse").unwrap();
///
///     // ...
/// }
/// ```
#[allow(unused_variables)]
pub fn provide_sse(url: &str) -> Result<(), JsValue> {
    provide_sse_inner(url)
}

/// Creates a signal which is controlled by the server.
///
/// This signal is initialized as T::default, is read-only on the client, and is updated through json patches
/// sent through a SSE connection.
///
/// # Example
///
/// ```
/// #[derive(Clone, Default, Serialize, Deserialize)]
/// pub struct Count {
///     pub value: i32,
/// }
///
/// #[component]
/// pub fn App() -> impl IntoView {
///     // Create server signal
///     let count = create_sse_signal::<Count>("counter");
///
///     view! {
///         <h1>"Count: " {move || count().value.to_string()}</h1>
///     }
/// }
/// ```
#[allow(unused_variables)]
pub fn create_sse_signal<T>(name: impl Into<Cow<'static, str>>) -> ReadSignal<T>
where
    T: Default + Serialize + for<'de> Deserialize<'de>,
{
    let name = name.into();
    let (get, set) = create_signal(T::default());

    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            use leptos::{use_context, create_effect, create_rw_signal, SignalSet, SignalGet};

            let signal = create_rw_signal(serde_json::to_value(T::default()).unwrap());
            if let Some(ServerSignalEventSource { state_signals, .. }) = use_context::<ServerSignalEventSource>() {
                let name: Cow<'static, str> = name.into();
                state_signals.borrow_mut().insert(name.clone(), signal);

                // Note: The leptos docs advise against doing this. It seems to work
                // well in testing, and the primary caveats are around unnecessary
                // updates firing, but our state synchronization already prevents
                // that on the server side
                create_effect(move |_| {
                    let name = name.clone();
                    let new_value = serde_json::from_value(signal.get()).unwrap();
                    set.set(new_value);
                });

            } else {
                leptos::logging::error!(
                    r#"server signal was used without a SSE being provided.

Ensure you call `leptos_sse::provide_sse("http://localhost:3000/sse")` at the highest level in your app."#
                );
            }
        }
    }

    get
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::rc::Rc;

        use web_sys::EventSource;
        use leptos::{provide_context, RwSignal};

        #[derive(Clone, Debug, PartialEq, Eq)]
        struct ServerSignalEventSource {
            inner: EventSource,
            // References to these are kept by the closure for the callback
            // onmessage callback on the event source
            state_signals: Rc<RefCell<HashMap<Cow<'static, str>, RwSignal<Value>>>>,
            // When the event source is first established, leptos may not have
            // completed the traversal that sets up all of the state signals.
            // Without that, we don't have a base state to apply the patches to,
            // and therefore we must keep a record of the patches to apply after
            // the state has been set up.
            delayed_updates: Rc<RefCell<HashMap<Cow<'static, str>, Vec<Patch>>>>,
        }

        #[inline]
        fn provide_sse_inner(url: &str) -> Result<(), JsValue> {
            use web_sys::MessageEvent;
            use wasm_bindgen::{prelude::Closure, JsCast};
            use leptos::{use_context, SignalUpdate};
            use js_sys::{Function, JsString};

            if use_context::<ServerSignalEventSource>().is_none() {
                let es = EventSource::new(url)?;
                provide_context(ServerSignalEventSource { inner: es, state_signals: Default::default(), delayed_updates: Default::default() });
            }

            let es = use_context::<ServerSignalEventSource>().unwrap();
            let handlers = es.state_signals.clone();
            let delayed_updates = es.delayed_updates.clone();
            let callback = Closure::wrap(Box::new(move |event: MessageEvent| {
                let ws_string = event.data().dyn_into::<JsString>().unwrap().as_string().unwrap();
                if let Ok(update_signal) = serde_json::from_str::<ServerSignalUpdate>(&ws_string) {
                    let handler_map = (*handlers).borrow();
                    let name = &update_signal.name;
                    let mut delayed_map = (*delayed_updates).borrow_mut();
                    if let Some(signal) = handler_map.get(name) {
                        if let Some(delayed_patches) = delayed_map.remove(name) {
                            signal.update(|doc| {
                                for patch in delayed_patches {
                                    json_patch::patch(doc, &patch).unwrap();
                                }
                            });
                        }
                        signal.update(|doc| {
                            json_patch::patch(doc, &update_signal.patch).unwrap();
                        });
                    } else {
                        leptos::logging::warn!("No local state for update to {}. Queuing patch.", name);
                        delayed_map.entry(name.clone()).or_default().push(update_signal.patch.clone());
                    }
                }
            }) as Box<dyn FnMut(_)>);
            let function: &Function = callback.as_ref().unchecked_ref();
            es.inner.set_onmessage(Some(function));

            // Keep the closure alive for the lifetime of the program
            callback.forget();

            Ok(())
        }
    } else {
        #[inline]
        fn provide_sse_inner(_url: &str) -> Result<(), JsValue> {
            Ok(())
        }
    }
}
