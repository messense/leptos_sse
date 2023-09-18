#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{
        routing::{get, post},
        Router,
    };
    use axum_example::app::*;
    use axum_example::fileserv::file_and_error_handler;
    use leptos::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};

    simple_logger::init_with_level(log::Level::Debug).expect("couldn't initialize logging");

    // Setting get_configuration(None) means we'll be using cargo-leptos's env values
    // For deployment these variables are:
    // <https://github.com/leptos-rs/start-axum#executing-a-server-on-a-remote-machine-without-the-toolchain>
    // Alternately a file can be specified such as Some("Cargo.toml")
    // The file would need to be included with the executable when moved to deployment
    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(|| view! { <App/> }).await;

    // build our application with a route
    let app = Router::new()
        .route("/api/*fn_name", post(leptos_axum::handle_server_fns))
        .route("/sse", get(handle_sse))
        .leptos_routes(&leptos_options, routes, || view! { <App/> })
        .fallback(file_and_error_handler)
        .with_state(leptos_options);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    leptos::logging::log!("listening on http://{}", &addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for a purely client-side app
    // see lib.rs for hydration function instead
}

#[cfg(feature = "ssr")]
use {
    axum::response::sse::{Event, KeepAlive, Sse},
    futures::stream::Stream,
};

#[cfg(feature = "ssr")]
async fn handle_sse() -> Sse<impl Stream<Item = Result<Event, axum::BoxError>>> {
    use axum_example::app::Count;
    use futures::stream;
    use leptos_sse::ServerSentEvents;
    use std::time::Duration;
    use tokio_stream::StreamExt as _;

    let mut value = 0;
    let stream = ServerSentEvents::new(
        "counter",
        stream::repeat_with(move || {
            let curr = value;
            value += 1;
            Ok(Count { value: curr })
        })
        .throttle(Duration::from_secs(1)),
    )
    .unwrap();
    Sse::new(stream).keep_alive(KeepAlive::default())
}
