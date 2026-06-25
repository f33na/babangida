//! SSR-сервер babangida web (leptos_axum). Запускается через cargo-leptos из flake.

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use babangida_web::app::{App, shell};
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};

    let conf = get_configuration(None).expect("leptos config");
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    let app = Router::new()
        .leptos_routes(&leptos_options, routes, {
            let opts = leptos_options.clone();
            move || shell(opts.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    log!("babangida web (SSR) на http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(listener, app.into_make_service())
        .await
        .expect("serve");
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // В режиме hydrate бинарь не используется — точка входа в lib::hydrate().
}
