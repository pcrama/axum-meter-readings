use axum::{
    Router,
    extract::{Form, State},
    handler::Handler,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::get_service,
};
use chrono::{self, DateTime};
use serde::Deserialize;
use std::{
    env,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tokio::task;

mod blocking_task;
use blocking_task::{SharedState, poll_automated_measurements, save_data, save_manual_inputs};

const FORM_PATH: &str = "/axum-meter-readings/form";

#[allow(non_snake_case)]
#[derive(Deserialize)]
struct FormData {
    timestamp: String,
    pv2012_kWh: String,
    gas: String,
    water: String,
}

fn render_form_field(
    label: &str,
    name: &str,
    unit: &str,
    value: &Result<Option<f64>, (String, &'static str)>,
) -> String {
    let empty_string = String::new();
    let input_value = match value {
        Ok(Some(f)) => &(f.to_string()),
        Ok(None) => &empty_string,
        Err((raw, _)) => raw,
    };

    let error_msg = match value {
        Err((_, msg)) => &format!(r#"<div class="error">{msg}</div>"#),
        _ => &empty_string,
    };

    format!(
        r#"<div>
            <label for="{name}">{label} {unit}</label>
            <input type="number" id="{name}" name="{name}" value="{input_value}" step="0.001" min="0">
            {error_msg}
        </div>"#
    )
}

fn render_form(
    timestamp_error: &str,
    pv2012: &Result<Option<f64>, (String, &'static str)>,
    gas: &Result<Option<f64>, (String, &'static str)>,
    water: &Result<Option<f64>, (String, &'static str)>,
    state_data_len: usize,
    general_error_msg: &str,
) -> String {
    let empty_string = String::new();
    let general_error = if general_error_msg.is_empty() {
        &empty_string
    } else {
        &format!(r#"<div class="general-error">{}</div>"#, general_error_msg)
    };

    let timestamp_err = if timestamp_error.is_empty() {
        &empty_string
    } else {
        &format!(r#"<div class="error">{}</div>"#, timestamp_error)
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Meter Form</title>
    <style>
        body {{
            font-family: sans-serif;
            margin: 1em;
            padding: 0;
            line-height: 1.4;
        }}
        form {{
            display: flex;
            flex-direction: column;
            gap: 1em;
        }}
        label {{
            font-weight: bold;
            margin-bottom: 0.3em;
            display: block;
        }}
        input {{
            width: 100%;
            max-width: 100%;
            padding: 0.6em;
            font-size: 1em;
            border: 1px solid #ccc;
            border-radius: 6px;
            box-sizing: border-box;
        }}
        .error {{
            color: #b00020;
            font-size: 0.9em;
            margin-top: 0.2em;
        }}
        button {{
            padding: 0.8em;
            font-size: 1em;
            border: none;
            border-radius: 6px;
            background-color: #333;
            color: white;
            cursor: pointer;
        }}
        button:hover {{
            background-color: #555;
        }}
        .summary {{
            margin-top: 1.5em;
            font-size: 0.95em;
            color: #555;
        }}
        .general-error {{
            margin-bottom: 1em;
            color: #b00020;
            font-weight: bold;
        }}
    </style>
</head>
<body>
    {general_error}
    <form action="{form_path}" method="POST">
        <div>
            <label for="timestamp">Timestamp</label>
            <input type="text" id="timestamp" name="timestamp" value="{timestamp}">
            {timestamp_err}
        </div>

        {pv2012_field}
        {gas_field}
        {water_field}

        <button type="submit">Submit</button>
    </form>

    <div class="summary">
        {state_data_len} input measurements
    </div>
</body>
</html>"#,
        general_error = general_error,
        form_path = FORM_PATH,
        timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:00%:z"),
        timestamp_err = timestamp_err,
        pv2012_field = render_form_field("PV2012", "pv2012_kWh", "(kWh)", pv2012),
        gas_field = render_form_field("Gas", "gas", "(m³)", gas),
        water_field = render_form_field("Water", "water", "(m³)", water),
        state_data_len = state_data_len,
    )
}

async fn get_form(State(state): State<SharedState>) -> Html<String> {
    let state = state.read().unwrap();
    Html(render_form(
        "",
        &Ok(None),
        &Ok(None),
        &Ok(None),
        state.data.len(),
        "",
    ))
}

fn parse_opt_positive_float(s: &str) -> Result<Option<f64>, &'static str> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }

    match s.parse::<f64>() {
        Ok(value) if value.is_finite() => {
            if value > 0.0 {
                Ok(Some(value))
            } else {
                Err("Value must be strictly positive")
            }
        }
        Ok(_) => Err("Invalid float: NaN or infinity"),
        Err(_) => Err("Unable to parse as floating point value"),
    }
}

async fn post_form(
    State(state): State<SharedState>,
    Form(form_data): Form<FormData>,
) -> Result<(StatusCode, impl IntoResponse), Html<String>> {
    println!(
        "Form submitted with: timestamp={}, pv2012_kWh={}, gas={}, water={}",
        form_data.timestamp, form_data.pv2012_kWh, form_data.gas, form_data.water
    );
    match (
        DateTime::parse_from_rfc3339(&form_data.timestamp),
        parse_opt_positive_float(&form_data.pv2012_kWh),
        parse_opt_positive_float(&form_data.gas),
        parse_opt_positive_float(&form_data.water),
    ) {
        (Ok(_), Ok(None), Ok(None), Ok(None)) => {
            let state = state.read().unwrap();
            return Err(Html(render_form(
                "",
                &Ok(None),
                &Ok(None),
                &Ok(None),
                state.data.len(),
                &format!(
                    "Nothing to do for timestamp={}, pv2012_kWh={}, gas={}, water={}",
                    form_data.timestamp, form_data.pv2012_kWh, form_data.gas, form_data.water
                ),
            )));
        }
        (Ok(timestamp), Ok(pv2012), Ok(gas), Ok(water)) => {
            let mut state = state.write().unwrap();
            save_manual_inputs(&mut state, timestamp, pv2012, gas, water);
            return Ok((StatusCode::SEE_OTHER, Redirect::to(FORM_PATH)));
        }
        (e_timestamp, e_pv2012, e_gas, e_water) => {
            let state = state.read().unwrap();
            let timestamp_error = if let Err(e) = e_timestamp {
                format!("{}", e)
            } else {
                String::new()
            };
            let form = render_form(
                &timestamp_error,
                &(e_pv2012.map_err(|e| (form_data.pv2012_kWh, e))),
                &(e_gas.map_err(|e| (form_data.gas, e))),
                &(e_water.map_err(|e| (form_data.water, e))),
                state.data.len(),
                "",
            );
            return Err(Html(form));
        }
    }
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
    let dump_interval = env::var("AXUM_METER_READINGS_DUMP_INTERVAL")
        .map_or(None, |s| s.parse::<i64>().ok())
        .unwrap_or(3600);
    let verbose = env::var("AXUM_METER_READINGS_VERBOSE").map_or(true, |s| {
        s.to_uppercase() != "FALSE" && s.to_uppercase() != "NO" && s != "0"
    });
    let blocking_ref = Arc::clone(&shared_state);
    let polling_period = Duration::from_secs(15);
    let _res = task::spawn_blocking(move || {
        println!("AXUM_METER_READINGS_P1_DATA_CMD='{}'", p1_data_cmd);
        println!("AXUM_METER_READINGS_PV_2022_CMD='{}'", pv_2022_cmd);
        println!("AXUM_METER_READINGS_SQL_CMD='{}'", sql_cmd);
        println!("AXUM_METER_READINGS_DUMP_INTERVAL='{}'", dump_interval);
        println!("AXUM_METER_READINGS_VERBOSE={}", verbose);
        loop {
            let start = Instant::now();
            let (p1, pv_2022) = poll_automated_measurements(&p1_data_cmd, &pv_2022_cmd, verbose);
            save_data(&blocking_ref, p1, pv_2022, &sql_cmd, dump_interval, verbose);
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
            FORM_PATH,
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
