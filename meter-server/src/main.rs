use axum::{
    Router,
    extract::{Form, State},
    handler::Handler,
    response::Html,
    routing::get_service,
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
use blocking_task::{SharedState, poll_automated_measurements, save_data};

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
            <title>Meter Form</title>
        </head>
        <body>
            <form action="/form" method="POST">
                <label for="timestamp">timestamp</label>
                <input type="number" id="timestamp" name="timestamp" value="{}"><br>
                <label for="pv2012_kWh">PV2012 (kWh)</label>
                <input type="number" id="pv2012_kWh" name="pv2012_kWh"><br>
                <label for="gas">gas (m³)</label>
                <input type="number" id="gas" name="gas"><br>
                <label for="water">water (m³)</label>
                <input type="number" id="water" name="water"><br>
                <button type="submit">Submit</button>
            </form>
        </body>
        </html>"#,
        time::now().strftime("%Y-%m-%d][%H:%M:%S").unwrap(),
    );
    Html(form.to_string())
}

async fn post_form(
    State(state): State<SharedState>,
    Form(form_data): Form<FormData>,
) -> Html<String> {
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
    let sql_cmd = env::var("AXUM_METER_READINGS_SQL_CMD")
        .unwrap_or_else(|_| "cat /tmp/sql_cmd.log".to_string());
    let blocking_ref = Arc::clone(&shared_state);
    let polling_period = Duration::from_secs(10);
    let _res = task::spawn_blocking(move || {
        println!("AXUM_METER_READINGS_P1_DATA_CMD='{}'", p1_data_cmd);
        println!("AXUM_METER_READINGS_PV_2022_CMD='{}'", pv_2022_cmd);
        println!("AXUM_METER_READINGS_SQL_CMD='{}'", sql_cmd);
        loop {
            let start = Instant::now();
            let (p1, pv_2022) = poll_automated_measurements(&p1_data_cmd, &pv_2022_cmd);
            save_data(&blocking_ref, p1, pv_2022, &sql_cmd);
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
        .with_state(Arc::clone(&shared_state));

    // Run our app with hyper
    let bind_addr =
        env::var("AXUM_METER_READINGS_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
