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

#[allow(non_snake_case)]
#[derive(Deserialize)]
struct FormData {
    timestamp: String,
    pv2012_kWh: String,
    gas: String,
    water: String,
}

fn render_form(
    timestamp_error: &str,
    pv2012: &Result<Option<f64>, (String, &'static str)>,
    gas: &Result<Option<f64>, (String, &'static str)>,
    water: &Result<Option<f64>, (String, &'static str)>,
    state_data_len: usize,
    general_error_msg: &str,
) -> String {
    let form = format!(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Meter Form</title>
        </head>
        <body>
            {}
            <form action="/form" method="POST">
                <label for="timestamp">timestamp</label>
                <input type="text" id="timestamp" name="timestamp" value="{}">{}<br>
                <label for="pv2012_kWh">PV2012 (kWh)</label>
                <input type="number" id="pv2012_kWh" name="pv2012_kWh" value="{}">{}<br>
                <label for="gas">gas (m³)</label>
                <input type="number" id="gas" name="gas" value="{}">{}<br>
                <label for="water">water (m³)</label>
                <input type="number" id="water" name="water" value="{}">{}<br>
                <button type="submit">Submit</button>
            </form>
            <br>
            {} input measurements
        </body>
        </html>"#,
        general_error_msg,
        // RFC3339-like format, floor-ed to previous full minute
        chrono::Local::now().format("%Y-%m-%dT%H:%M:00%:z"),
        timestamp_error,
        pv2012.as_ref().map_or_else(
            |x| x.0.to_string(),
            |op_f64| op_f64.map_or("".to_string(), |f| format!("{}", &f))
        ),
        if let Err((_, msg)) = pv2012 { msg } else { "" },
        gas.as_ref().map_or_else(
            |x| x.0.to_string(),
            |op_f64| op_f64.map_or("".to_string(), |f| format!("{}", f))
        ),
        if let Err((_, msg)) = gas { msg } else { "" },
        water.as_ref().map_or_else(
            |x| x.0.to_string(),
            |op_f64| op_f64.map_or("".to_string(), |f| format!("{}", f))
        ),
        if let Err((_, msg)) = water { msg } else { "" },
        state_data_len,
    );
    return form;
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
// let form = format!(
//         r#"<!DOCTYPE html>
//         <html lang="en">
//         <head>
//             <meta charset="UTF-8">
//             <meta name="viewport" content="width=device-width, initial-scale=1.0">
//             <title>Meter Form</title>
//         </head>
//         <body>
//             <form action="/form" method="POST">
//                 <label for="timestamp">timestamp</label>
//                 <input type="text" id="timestamp" name="timestamp" value="{}"><br>
//                 <label for="pv2012_kWh">PV2012 (kWh)</label>
//                 <input type="number" id="pv2012_kWh" name="pv2012_kWh"><br>
//                 <label for="gas">gas (m³)</label>
//                 <input type="number" id="gas" name="gas"><br>
//                 <label for="water">water (m³)</label>
//                 <input type="number" id="water" name="water"><br>
//                 <button type="submit">Submit</button>
//             </form>
//             <br>
//             {} input measurements
//         </body>
//         </html>"#,
//         // RFC3339-like format, floor-ed to previous full minute
//         chrono::Local::now().format("%Y-%m-%dT%H:%M:00%:z"),
//         state.data.len(),
//     );
//     Html(form.to_string())
// }

fn parse_opt_float(s: &str) -> Result<Option<f64>, &'static str> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }

    match s.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(Some(value)),
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
        parse_opt_float(&form_data.pv2012_kWh),
        parse_opt_float(&form_data.gas),
        parse_opt_float(&form_data.water),
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
            return Ok((StatusCode::SEE_OTHER, Redirect::to("/form")));
        }
        (e_timestamp, e_pv2012, e_gas, e_water) => {
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
                        <input type="text" id="timestamp" name="timestamp" value="{}">{}<br>
                        <label for="pv2012_kWh">PV2012 (kWh)</label>
                        <input type="number" id="pv2012_kWh" name="pv2012_kWh" value="{}">{}<br>
                        <label for="gas">gas (m³)</label>
                        <input type="number" id="gas" name="gas" value="{}">{}<br>
                        <label for="water">water (m³)</label>
                        <input type="number" id="water" name="water" value="{}">{}<br>
                        <button type="submit">Submit</button>
                    </form>
                    <br>
                    {} input measurements
                </body>
                </html>"#,
                // RFC3339-like format, floor-ed to previous full minute
                chrono::Local::now().format("%Y-%m-%dT%H:%M:00%:z"),
                if let Err(e) = e_timestamp {
                    format!("{}", e)
                } else {
                    "".to_string()
                },
                form_data.pv2012_kWh,
                if let Err(e) = e_pv2012 { e } else { "" },
                form_data.gas,
                if let Err(e) = e_gas { e } else { "" },
                form_data.water,
                if let Err(e) = e_water { e } else { "" },
                state.data.len(),
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
    let blocking_ref = Arc::clone(&shared_state);
    let polling_period = Duration::from_secs(10);
    let _res = task::spawn_blocking(move || {
        println!("AXUM_METER_READINGS_P1_DATA_CMD='{}'", p1_data_cmd);
        println!("AXUM_METER_READINGS_PV_2022_CMD='{}'", pv_2022_cmd);
        println!("AXUM_METER_READINGS_SQL_CMD='{}'", sql_cmd);
        println!("AXUM_METER_READINGS_DUMP_INTERVAL='{}'", dump_interval);
        loop {
            let start = Instant::now();
            let (p1, pv_2022) = poll_automated_measurements(&p1_data_cmd, &pv_2022_cmd);
            save_data(&blocking_ref, p1, pv_2022, &sql_cmd, dump_interval);
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
