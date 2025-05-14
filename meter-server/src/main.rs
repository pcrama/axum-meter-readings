use axum::{
    Router,
    body::Bytes,
    extract::{Form, Path, State},
    handler::Handler,
    http::StatusCode,
    response::Html,
    routing::{delete, get, get_service},
};
use serde::Deserialize;
use std::{
    env,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tokio::task;

mod blocking_task;
use blocking_task::{SharedState, poll_automated_measurements};

#[derive(Deserialize)]
struct FormData {
    number: i32,
}

async fn get_form(State(state): State<SharedState>) -> Html<String> {
    let state = state.read().unwrap();
    let form = format!(
        r#"<!DOCTYPE html>
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
                <input type="number" id="number" name="number" required value="{}">
                <button type="submit">Submit</button>
            </form>
            {} {} {}
        </body>
        </html>"#,
        state.get_counter(),
        state.data.len(),
        match state
            .data
            .peek_first(|r| { format!("first {}", r.timestamp) })
        {
            Some(x) => x,
            None => "".to_string(),
        },
        match state
            .data
            .peek_last(|r| { format!("last {}", r.timestamp) })
        {
            Some(x) => x,
            None => "".to_string(),
        },
    );
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
    let state = state.read().unwrap();
    let format_kwh = |x: Option<f64>, y: &str| {
        x.map(|z| format!("{}: {}kWh", y, z))
            .unwrap_or("".to_string())
    };
    let last_data = match state.get_last_data() {
        Some(last_data) => format!(
            "{} {} {} {} {}",
            format_kwh(last_data.peak_conso_kWh, "Peak consumption"),
            format_kwh(last_data.off_conso_kWh, "Off-hour consumption"),
            format_kwh(last_data.peak_inj_kWh, "Peak injection"),
            format_kwh(last_data.off_inj_kWh, "Off-hour injection"),
            format_kwh(last_data.pv2022_kWh, "PV 2022 production"),
        ),
        None => "".to_string(),
    };
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
            {}
        </body>
        </html>
    "#,
        form_data.number, last_data,
    );
    Html(response)
}

#[tokio::main]
async fn main() {
    let shared_state = SharedState::default();

    let p1_data_cmd = env::var("AXUM_METER_READINGS_P1_DATA_CMD")
        .unwrap_or_else(|_| "cat /tmp/p1_data.txt".to_string());
    let pv_2022_cmd = env::var("AXUM_METER_READINGS_PV_2022_CMD")
        .unwrap_or_else(|_| "cat /tmp/pv_2022.json".to_string());
    let blocking_ref = Arc::clone(&shared_state);
    let polling_period = Duration::from_secs(10);
    let _res = task::spawn_blocking(move || {
        println!("AXUM_METER_READINGS_P1_DATA_CMD='{}'", p1_data_cmd);
        println!("AXUM_METER_READINGS_PV_2022_CMD='{}'", pv_2022_cmd);
        loop {
            let start = Instant::now();
            let counter: i32;
            {
                let state = &mut blocking_ref.write().unwrap();
                counter = state.get_counter() + 1;
                state.set_counter(counter);
            }
            println!("counter={}", counter);
            let (p1, pv_2022) = poll_automated_measurements(&p1_data_cmd, &pv_2022_cmd);
            {
                let state = &mut blocking_ref.write().unwrap();
                match state.set_data(p1, pv_2022) {
                    Some(_) => {
                        state.halve_data();
                    }
                    None => {}
                }
            }
            let elapsed = start.elapsed();
            if elapsed < polling_period {
                thread::sleep(polling_period - elapsed);
            } else {
                println!(
                    "Warning: poll_automated_measurements took longer than {}s: {}s",
                    polling_period.as_secs(),
                    elapsed.as_secs()
                );
            }
        }
    });

    // Build our application by composing routes
    let app = Router::new()
        .route(
            "/form",
            get_service(get_form.with_state(Arc::clone(&shared_state)))
                .post_service(post_form.with_state(Arc::clone(&shared_state))),
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
