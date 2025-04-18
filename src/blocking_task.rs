use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use crate::AppState;
use crate::p1_meter;

/*

 curl --silent --insecure --connect-timeout 1 --max-time 2 https://sunnyboy50/dyn/getDashValues.json

 */

pub fn blocking_task_loop_body(blocking_ref: &Arc<RwLock<AppState>>, p1_data_cmd: &str, pv_2022_cmd: &str) {
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
        Ok(pv_2022) => println!("pv_2022={}", pv_2022),
        Err(e) => println!("Error: {}", e)
    }
}

fn fetch_dashboard_value(pv_2022_cmd: &str) -> core::result::Result<f64, String> {
    let stdout = Command::new("sh")
        .arg("-c")
        .arg(pv_2022_cmd)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", pv_2022_cmd, e))?
        .stdout
        .ok_or(format!("Failed to get output of {}", pv_2022_cmd))?;

    // Convert the output to a UTF-8 string
    let response_text = std::str::from_utf8(&stdout)
        .map_err(|e| format!("Failed to parse curl response as UTF-8: {}", e))?;

    print!("response_text={}", response_text);
    let json: Value =
        serde_json::from_str(response_text).map_err(|e| format!("Unable to parse JSON: {}", e))?;
    let value = json["result"]["0199-xxxxx9BD"]["6400_00260100"]["1"][0]["val"]
        .as_f64()
        .ok_or("Invalid JSON response")?;

    Ok(value)
}
