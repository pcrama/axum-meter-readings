use axum::{
    body::Bytes,
    extract::{Path, State},
    handler::Handler,
    http::StatusCode,
    routing::{delete, get},
    Router,
};
use std::{
    collections::HashMap,
    thread,
    time::Duration,
    sync::{Arc, RwLock},
};
use tokio::task;

#[tokio::main]
async fn main() {
    let shared_state = SharedState::default();

    let blocking_ref = Arc::clone(&shared_state);
    let _res = task::spawn_blocking(move || {
        let mut counter: i32 = 0;
        loop {
            println!("counter={}", counter);
            {
                let state = &mut blocking_ref.write().unwrap();
                state.setCounter(counter)
            }
            thread::sleep(Duration::from_secs(4));
            counter += 1;
        }
    });

    let web_ref = Arc::clone(&shared_state);

    // Build our application by composing routes
    let app = Router::new()
        .route(
            "/{key}",
            get(kv_get)
                .post_service(
                    kv_set.with_state(web_ref),
                ),
        )
        .route("/keys", get(list_keys))
        // Nest our admin routes under `/admin`
        .nest("/admin", admin_routes())
        .with_state(Arc::clone(&shared_state));

    // Run our app with hyper
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
struct AppState {
    db: HashMap<String, Bytes>,
    counter: i32,
}

impl AppState {
    fn setCounter(&mut self, val: i32) {
        self.counter = val;
    }

    fn getCounter(&self) -> i32 {
        self.counter
    }
}

async fn kv_get(
    Path(key): Path<String>,
    State(state): State<SharedState>,
) -> Result<Bytes, StatusCode> {
    let state = &state.read().unwrap();

    if key == "counter" {
        let counter_string = state.getCounter().to_string();
        let bytes = counter_string.as_bytes();
        println!("Getting counter {}", state.getCounter());
        return Ok(bytes.to_vec().into())
    }

    if let Some(value) = state.db.get(&key) {
        Ok(value.clone())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn kv_set(Path(key): Path<String>, State(state): State<SharedState>, bytes: Bytes) {
    state.write().unwrap().db.insert(key, bytes);
}

async fn list_keys(State(state): State<SharedState>) -> String {
    let db = &state.read().unwrap().db;

    db.keys()
        .map(|key| key.to_string())
        .collect::<Vec<String>>()
        .join("\n")
}

fn admin_routes() -> Router<SharedState> {
    async fn delete_all_keys(State(state): State<SharedState>) {
        state.write().unwrap().db.clear();
    }

    async fn remove_key(Path(key): Path<String>, State(state): State<SharedState>) {
        state.write().unwrap().db.remove(&key);
    }

    Router::new()
        .route("/keys", delete(delete_all_keys))
        .route("/key/{key}", delete(remove_key))
}
