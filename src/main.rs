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
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};
use tokio::task;

pub mod p1_meter;

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
        .unwrap_or_else(|_| "cat /tmp/p1_data.txt".to_string());
    let pv_2022_cmd = env::var("AXUM_METER_READINGS_PV_2022_CMD")
        .unwrap_or_else(|_| "cat /tmp/pv_2022.json".to_string());
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

type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
struct AppState {
    db: HashMap<String, Bytes>,
    counter: i32,
}

impl AppState {
    fn set_counter(&mut self, val: i32) {
        self.counter = val;
    }

    fn get_counter(&self) -> i32 {
        self.counter
    }
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

fn blocking_task_loop_body(
    blocking_ref: &Arc<RwLock<AppState>>,
    p1_data_cmd: &str,
    pv_2022_cmd: &str,
) {
    let counter: i32;
    {
        let state = &mut blocking_ref.write().unwrap();
        counter = state.get_counter() + 1;
        state.set_counter(counter);
    }
    println!("counter={}", counter);
    thread::sleep(Duration::from_secs(4));

    let stdout = Command::new("sh")
        .arg("-c")
        .arg(p1_data_cmd)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
        .stdout
        .unwrap();
    let reader = BufReader::new(stdout);
    match p1_meter::parse_lines(reader.lines().map(|x| x.unwrap())) {
        Ok(Some(complete)) => println!("complete = {:?}", complete),
        Ok(None) => println!("nothing parsed"),
        Err(_) => panic!("Error"),
    }

    match fetch_dashboard_value(pv_2022_cmd) {
        Ok(pv_2022) => println!("PV2022={}", pv_2022),
        Err(s) => println!("PV2022 err: {}", s),
    }
}

/* {"result":{"0199-xxxxx9BD":{"6800_08822000":{"1":[{"validVals":[9401,9402,9403,9404,9405],"val":[{"tag":9404}]}]},"6800_10821E00":{"1":[{"val":"SN: xxxxxxx245"}]},"6800_08811F00":{"1":[{"validVals":[1129,1130],"val":[{"tag":1129}]}]},"6180_08214800":{"1":[{"val":[{"tag":307}]}]},"6180_08414900":{"1":[{"val":[{"tag":886}]}]},"6180_08522F00":{"1":[{"val":[{"tag":16777213}]}]},"6800_088A2900":{"1":[{"validVals":[302,9327,9375,9376,9437,19043],"val":[{"tag":302}]}]},"6100_40463600":{"1":[{"val":null}]},"6100_40463700":{"1":[{"val":null}]},"6100_40263F00":{"1":[{"val":null}]},"6400_00260100":{"1":[{"val":7459043}]},"6800_00832A00":{"1":[{"low":5000,"high":5000,"val":5000}]},"6800_008AA200":{"1":[{"low":0,"high":null,"val":0}]},"6400_00462500":{"1":[{"val":null}]},"6100_00418000":{"1":[{"val":null}]},"6800_08822B00":{"1":[{"validVals":[461],"val":[{"tag":461}]}]},"6100_0046C200":{"1":[{"val":null}]},"6400_0046C300":{"1":[{"val":7459043}]},"6802_08834500":{"1":[{"validVals":[303,1439],"val":[{"tag":1439}]}]},"6180_08412800":{"1":[{"val":[{"tag":16777213}]}]}}}}

curl --silent --connect-timeout 1 --max-time 2 --insecure https://sunnyboy50/dyn/getDashValues.json */
fn fetch_dashboard_value(pv_2022_cmd: &str) -> core::result::Result<f64, String> {
    let stdout = Command::new("sh")
        .arg("-c")
        .arg(pv_2022_cmd)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", pv_2022_cmd, e))?
        .stdout
        .ok_or(format!("Failed to get output of {}", pv_2022_cmd))?;

    let mut reader = BufReader::new(stdout);
    let mut response_bytes = Vec::new();
    reader
        .read_to_end(&mut response_bytes)
        .map_err(|e| format!("Failed to read stdout: {}", e))?;

    let response_text = std::str::from_utf8(&response_bytes)
        .map_err(|e| format!("Failed to parse curl response as UTF-8: {}", e))?;

    print!("response_text={}", response_text);
    let json: Value =
        serde_json::from_str(response_text).map_err(|e| format!("Unable to parse JSON: {}", e))?;
    let value = json["result"]["0199-xxxxx9BD"]["6400_00260100"]["1"][0]["val"]
        .as_f64()
        .ok_or("Invalid JSON response")?;

    Ok(value)
}
