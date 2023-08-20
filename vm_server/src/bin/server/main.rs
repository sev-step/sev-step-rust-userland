use std::sync::{Arc, Mutex};

use axum::{routing::post, Router};
use vm_server::handlers::{self, ServerState};

#[tokio::main]
async fn main() {
    env_logger::init();

    let shared_state = Arc::new(Mutex::new(ServerState {
        target_programm: None,
    }));
    // build our application with a single route
    let app = Router::new()
        .route(
            "/assembly-target/new",
            post(handlers::init_assembly_target_handler),
        )
        .route("/run-target", post(handlers::run_target_handler))
        .route(
            "page-ping-ponger/new",
            post(handlers::init_page_ping_ponger_handler),
        )
        .with_state(shared_state);

    let listen_str = "0.0.0.0:8080".to_string();
    eprintln!("listening on {}", listen_str);
    axum::Server::bind(&listen_str.parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
