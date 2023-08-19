#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#![doc = include_str!("../README.md")]

use std::borrow::Cow;

use json_patch::Patch;
use leptos::{create_signal, ReadSignal, Scope};
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
/// This is whats sent over the SSE, and is used to patch the signal if the type name matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSignalUpdate {
    event: Cow<'static, str>,
    patch: Patch,
}

impl ServerSignalUpdate {
    /// Creates a new [`ServerSignalUpdate`] from an old and new instance of `T`.
    pub fn new<'s, 'e, T>(
        event: impl Into<Cow<'static, str>>,
        old: &'s T,
        new: &'e T,
    ) -> Result<Self, serde_json::Error>
    where
        T: Serialize,
    {
        let left = serde_json::to_value(old)?;
        let right = serde_json::to_value(new)?;
        let patch = json_patch::diff(&left, &right);
        Ok(ServerSignalUpdate {
            event: event.into(),
            patch,
        })
    }

    /// Creates a new [`ServerSignalUpdate`] from two json values.
    pub fn new_from_json<'s, 'e, T>(
        event: impl Into<Cow<'static, str>>,
        old: &Value,
        new: &Value,
    ) -> Self {
        let patch = json_patch::diff(old, new);
        ServerSignalUpdate {
            event: event.into(),
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
/// pub fn App(cx: Scope) -> impl IntoView {
///     // Provide SSE connection
///     leptos_sse::provide_sse(cx, "http://localhost:3000/sse").unwrap();
///     
///     // ...
/// }
/// ```
#[allow(unused_variables)]
pub fn provide_sse(cx: Scope, url: &str) -> Result<(), JsValue> {
    provide_sse_inner(cx, url)
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
/// pub fn App(cx: Scope) -> impl IntoView {
///     // Create server signal
///     let count = create_sse_signal::<Count>(cx, "counter");
///
///     view! { cx,
///         <h1>"Count: " {move || count().value.to_string()}</h1>
///     }
/// }
/// ```
#[allow(unused_variables)]
pub fn create_sse_signal<T>(cx: Scope, event: impl Into<Cow<'static, str>>) -> ReadSignal<T>
where
    T: Default + Serialize + for<'de> Deserialize<'de>,
{
    let event_name = event.into();
    let (get, set) = create_signal(cx, T::default());

    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            use web_sys::MessageEvent;
            use wasm_bindgen::{prelude::Closure, JsCast};
            use leptos::{use_context, create_effect, SignalGetUntracked, SignalSet, SignalUpdate};
            use js_sys::{Function, JsString};

            let (json_get, json_set) = create_signal(cx, serde_json::to_value(T::default()).unwrap());
            let ws = use_context::<ServerSignalEventSource>(cx);

            match ws {
                Some(ServerSignalEventSource(es)) => {
                    create_effect(cx, move |_| {
                        let event_name = event_name.clone();
                        let event_name_clone = event_name.clone();
                        let callback = Closure::wrap(Box::new(move |event: MessageEvent| {
                            let ws_string = event.data().dyn_into::<JsString>().unwrap().as_string().unwrap();
                            if let Ok(update_signal) = serde_json::from_str::<ServerSignalUpdate>(&ws_string) {
                                if update_signal.event == event_name {
                                    json_set.update(|doc| {
                                        json_patch::patch(doc, &update_signal.patch).unwrap();
                                    });
                                    let new_value = serde_json::from_value(json_get.get_untracked()).unwrap();
                                    set.set(new_value);
                                }
                            }
                        }) as Box<dyn FnMut(_)>);
                        let function: &Function = callback.as_ref().unchecked_ref();
                        es.add_event_listener_with_callback(&event_name_clone, function).unwrap();

                        // Keep the closure alive for the lifetime of the program
                        callback.forget();
                    });
                }
                None => {
                    leptos::error!(
                        r#"server signal was used without a SSE being provided.

Ensure you call `leptos_sse::provide_sse(cx, "http://localhost:3000/sse")` at the highest level in your app."#
                    );
                }
            }
        }
    }

    get
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use web_sys::EventSource;
        use leptos::{provide_context, use_context};

        #[derive(Clone, Debug, PartialEq, Eq)]
        struct ServerSignalEventSource(EventSource);

        #[inline]
        fn provide_sse_inner(cx: Scope, url: &str) -> Result<(), JsValue> {
            if use_context::<ServerSignalEventSource>(cx).is_none() {
                let ws = EventSource::new(url)?;
                provide_context(cx, ServerSignalEventSource(ws));
            }

            Ok(())
        }
    } else {
        #[inline]
        fn provide_sse_inner(_cx: Scope, _url: &str) -> Result<(), JsValue> {
            Ok(())
        }
    }
}
