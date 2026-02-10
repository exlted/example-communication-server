use crate::handlers::handle_rejection;
use warp::Filter;
use crate::client::ClientMap;
use crate::handlers::{ensure_authentication, with_clients, ws_handler};

pub async fn webserver_loop(clients: ClientMap) {

    println!("Configuring websocket route");
    let ws_route = warp::path("ws")
        .and(ensure_authentication().await)
        .and(warp::ws())
        .and(with_clients(clients))
        .and_then(ws_handler);

    println!("Starting example-communication-server");
    let routes = ws_route.with(warp::cors().allow_any_origin()).recover(handle_rejection);
    let server = warp::serve(routes).run(([0, 0, 0, 0], 8080));

    server.await;
}