use serde_json::Value;
use std::{
    io::{BufReader, Read},
    process::{Command, Stdio},
};

/* {"result":{"0199-xxxxx9BD":{"6800_08822000":{"1":[{"validVals":[9401,9402,9403,9404,9405],"val":[{"tag":9404}]}]},"6800_10821E00":{"1":[{"val":"SN: xxxxxxx245"}]},"6800_08811F00":{"1":[{"validVals":[1129,1130],"val":[{"tag":1129}]}]},"6180_08214800":{"1":[{"val":[{"tag":307}]}]},"6180_08414900":{"1":[{"val":[{"tag":886}]}]},"6180_08522F00":{"1":[{"val":[{"tag":16777213}]}]},"6800_088A2900":{"1":[{"validVals":[302,9327,9375,9376,9437,19043],"val":[{"tag":302}]}]},"6100_40463600":{"1":[{"val":null}]},"6100_40463700":{"1":[{"val":null}]},"6100_40263F00":{"1":[{"val":null}]},"6400_00260100":{"1":[{"val":7459043}]},"6800_00832A00":{"1":[{"low":5000,"high":5000,"val":5000}]},"6800_008AA200":{"1":[{"low":0,"high":null,"val":0}]},"6400_00462500":{"1":[{"val":null}]},"6100_00418000":{"1":[{"val":null}]},"6800_08822B00":{"1":[{"validVals":[461],"val":[{"tag":461}]}]},"6100_0046C200":{"1":[{"val":null}]},"6400_0046C300":{"1":[{"val":7459043}]},"6802_08834500":{"1":[{"validVals":[303,1439],"val":[{"tag":1439}]}]},"6180_08412800":{"1":[{"val":[{"tag":16777213}]}]}}}}

curl --silent --connect-timeout 1 --max-time 2 --insecure https://sunnyboy50/dyn/getDashValues.json */
pub fn fetch_dashboard_value(
    pv_2022_cmd: &str,
    verbose: bool,
) -> core::result::Result<f64, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(pv_2022_cmd)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", pv_2022_cmd, e))?;
    let stdout = child
        .stdout
        .take()
        .ok_or(format!("Failed to get output of {}", pv_2022_cmd))?;

    let mut reader = BufReader::new(stdout);
    let mut response_bytes = Vec::new();
    reader
        .read_to_end(&mut response_bytes)
        .map_err(|e| format!("Failed to read stdout: {}", e))?;
    child
        .wait()
        .map_err(|e| format!("Unable to wait for '{}': {}", pv_2022_cmd, e))?;

    let response_text = std::str::from_utf8(&response_bytes)
        .map_err(|e| format!("Failed to parse curl response as UTF-8: {}", e))?;

    if verbose {
        println!("response_text={}", response_text)
    };
    let json: Value =
        serde_json::from_str(response_text).map_err(|e| format!("Unable to parse JSON: {}", e))?;
    let value = json["result"]["0199-xxxxx9BD"]["6400_00260100"]["1"][0]["val"]
        .as_f64()
        .ok_or("Invalid JSON response")?;

    Ok(value / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_with_example() {
        assert_eq!(
            fetch_dashboard_value(
                "echo '{\"result\":{\"0199-xxxxx9BD\":{\"6800_08822000\":{\"1\":[{\"validVals\":[9401,9402,9403,9404,9405],\"val\":[{\"tag\":9404}]}]},\"6800_10821E00\":{\"1\":[{\"val\":\"SN: xxxxxxx245\"}]},\"6800_08811F00\":{\"1\":[{\"validVals\":[1129,1130],\"val\":[{\"tag\":1129}]}]},\"6180_08214800\":{\"1\":[{\"val\":[{\"tag\":307}]}]},\"6180_08414900\":{\"1\":[{\"val\":[{\"tag\":886}]}]},\"6180_08522F00\":{\"1\":[{\"val\":[{\"tag\":16777213}]}]},\"6800_088A2900\":{\"1\":[{\"validVals\":[302,9327,9375,9376,9437,19043],\"val\":[{\"tag\":302}]}]},\"6100_40463600\":{\"1\":[{\"val\":null}]},\"6100_40463700\":{\"1\":[{\"val\":null}]},\"6100_40263F00\":{\"1\":[{\"val\":null}]},\"6400_00260100\":{\"1\":[{\"val\":7459043}]},\"6800_00832A00\":{\"1\":[{\"low\":5000,\"high\":5000,\"val\":5000}]},\"6800_008AA200\":{\"1\":[{\"low\":0,\"high\":null,\"val\":0}]},\"6400_00462500\":{\"1\":[{\"val\":null}]},\"6100_00418000\":{\"1\":[{\"val\":null}]},\"6800_08822B00\":{\"1\":[{\"validVals\":[461],\"val\":[{\"tag\":461}]}]},\"6100_0046C200\":{\"1\":[{\"val\":null}]},\"6400_0046C300\":{\"1\":[{\"val\":7459043}]},\"6802_08834500\":{\"1\":[{\"validVals\":[303,1439],\"val\":[{\"tag\":1439}]}]},\"6180_08412800\":{\"1\":[{\"val\":[{\"tag\":16777213}]}]}}}}'",
                true
            ),
            Ok(7459.043)
        );
    }

    #[test]
    fn handles_parse_error_without_panic() {
        assert!(fetch_dashboard_value("echo '{\"result\":'", true).is_err());
    }
}
