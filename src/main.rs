use axum::{
    Router,
    body::Bytes,
    extract::{Form, Path, State},
    handler::Handler,
    http::StatusCode,
    response::Html,
    routing::{delete, get},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    env,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};
use tokio::task;

// pub mod blocking_task;
// pub mod p1_meter;
use crate::blocking_task::blocking_task_loop_body;

#[derive(Deserialize)]
struct FormData {
    number: i32,
}

async fn get_form() -> Html<String> {
    let form = r#"
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Number Form</title>
        </head>
        <body>
            <h1>Enter an Integer</h1>
            <form action="/form" method="POST">
                <label for="number">Number:</label>
                <input type="number" id="number" name="number" required>
                <button type="submit">Submit</button>
            </form>
        </body>
        </html>
    "#;
    Html(form.to_string())
}

async fn post_form(
    State(state): State<SharedState>,
    Form(form_data): Form<FormData>,
) -> Html<String> {
    {
        let state = &mut state.write().unwrap();
        state.set_counter(form_data.number);
    }
    println!("Form submitted with number: {}", form_data.number);
    let response = format!(
        r#"
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Number Submission</title>
        </head>
        <body>
            <h1>An integer was submitted</h1>
            <p>It's value was {}.</p>
        </body>
        </html>
    "#,
        form_data.number
    );
    Html(response)
}

#[tokio::main]
async fn main() {
    let shared_state = SharedState::default();

    let p1_data_cmd = env::var("AXUM_METER_READINGS_P1_DATA_CMD")
        .unwrap_or_else(|_| "sudo stty -F /dev/ttyUSB0 115200 cs8 -parenb; head -n 30 /dev/ttyUSB0".to_string());
    let pv_2022_cmd = env::var("AXUM_METER_READINGS_PV_2022_CMD")
        .unwrap_or_else(|_| "curl --silent --insecure --connect-timeout 1 --max-time 2 https://sunnyboy50/dyn/getDashValues.json".to_string());

    let blocking_ref = Arc::clone(&shared_state);
    let _res = task::spawn_blocking(move || {
        println!("AXUM_METER_READINGS_P1_DATA_CMD='{}'", p1_data_cmd);
        println!("AXUM_METER_READINGS_PV_2022_CMD='{}'", pv_2022_cmd);
        loop {
            blocking_task_loop_body(&blocking_ref, &p1_data_cmd, &pv_2022_cmd);
        }
    });

    // Build our application by composing routes
    let app = Router::new()
        .route(
            "/form",
            get(get_form).post_service(post_form.with_state(Arc::clone(&shared_state))),
        )
        .route(
            "/access/{key}",
            get(kv_get).post_service(kv_set.with_state(Arc::clone(&shared_state))),
        )
        .route("/keys", get(list_keys))
        // Nest our admin routes under `/admin`
        .nest("/admin", admin_routes())
        .with_state(Arc::clone(&shared_state));

    // Run our app with hyper
    let bind_addr =
        env::var("AXUM_METER_READINGS_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn kv_get(
    Path(key): Path<String>,
    State(state): State<SharedState>,
) -> Result<Bytes, StatusCode> {
    let state = &state.read().unwrap();

    if key == "counter" {
        let counter_string = state.get_counter().to_string();
        let bytes = counter_string.as_bytes();
        println!("Getting counter {}", state.get_counter());
        return Ok(bytes.to_vec().into());
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
