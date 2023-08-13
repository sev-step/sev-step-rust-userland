use std::sync::{Arc, Mutex};

use axum::{routing::post, Router};
use vm_server_v2::handlers::{self, ServerState};

#[tokio::main]
async fn main() {
    env_logger::init();

    let shared_state = Arc::new(Mutex::new(ServerState {
        assembly_target: None,
    }));
    // build our application with a single route
    let app = Router::new()
        .route(
            "/assembly-target/new",
            post(handlers::init_assembly_target_handler),
        )
        .route(
            "/assembly-target/run",
            post(handlers::run_assembly_target_handler),
        )
        .with_state(shared_state);

    // run it with hyper on localhost:3000
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
