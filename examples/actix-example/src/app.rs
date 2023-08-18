use leptos::*;
use leptos_sse::create_sse_signal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Count {
    pub value: i32,
}

#[component]
pub fn App(cx: Scope) -> impl IntoView {
    // Provide sse connection
    leptos_sse::provide_sse(cx, "http://localhost:3000/sse").unwrap();

    // Create server signal
    let count = create_sse_signal::<Count>(cx, "counter");

    view! { cx,
        <h1>"Count: " {move || count().value.to_string()}</h1>
    }
}
