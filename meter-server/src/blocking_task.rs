use axum::body::Bytes;
use meter_core::{
    data::{Data202303, clone_data202303},
    p1_meter,
    p1_meter::CompleteP1Measurement,
    pv2022, ringbuffer,
    ringbuffer::RingBuffer,
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use time::{Date, Month, UtcOffset};

pub type SharedState = Arc<RwLock<AppState>>;

pub struct AppState {
    pub db: HashMap<String, Bytes>,
    counter: i32,
    pub data: RingBuffer<Data202303>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            db: Default::default(),
            counter: 0,
            data: ringbuffer::new::<Data202303>(1440),
        }
    }
}

impl AppState {
    pub fn set_counter(&mut self, val: i32) {
        self.counter = val;
    }

    pub fn get_counter(&self) -> i32 {
        self.counter
    }

    pub fn set_data(
        &mut self,
        p1: Option<CompleteP1Measurement>,
        pv_2022: Option<f64>,
    ) -> Option<Data202303> {
        if p1 == None && pv_2022 == None {
            return None;
        }

        let timestamp = match &p1 {
            Some(p1) => p1.timestamp.unix_timestamp(),
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };
        let time_since_last_update = match self.data.peek_last(|r| r.timestamp) {
            Some(last_update) => timestamp - last_update,
            None => 999,
        };
        if time_since_last_update < 60 {
            println!("time_since_last_update={}", time_since_last_update);
            return None;
        }

        self.data.push(match p1 {
            Some(p1) => Data202303 {
                timestamp,
                pv2012_kWh: None,
                pv2022_kWh: pv_2022,
                peak_conso_kWh: Some(p1.peak_hour_consumption),
                off_conso_kWh: Some(p1.off_hour_consumption),
                peak_inj_kWh: Some(p1.peak_hour_injection),
                off_inj_kWh: Some(p1.off_hour_injection),
                gas_m3: None,
                water_m3: None,
            },
            None => Data202303 {
                timestamp,
                pv2012_kWh: None,
                pv2022_kWh: pv_2022,
                peak_conso_kWh: None,
                off_conso_kWh: None,
                peak_inj_kWh: None,
                off_inj_kWh: None,
                gas_m3: None,
                water_m3: None,
            },
        })
    }

    pub fn get_first_data(&self) -> Option<Data202303> {
        self.data.peek_first(clone_data202303)
    }

    pub fn get_last_data(&self) -> Option<Data202303> {
        self.data.peek_last(clone_data202303)
    }

    pub fn halve_data(&mut self) {
        self.data.halve_data();
    }
}

pub fn poll_automated_measurements(
    p1_data_cmd: &str,
    pv_2022_cmd: &str,
) -> (Option<CompleteP1Measurement>, Option<f64>) {
    let stdout = Command::new("sh")
        .arg("-c")
        .arg(p1_data_cmd)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
        .stdout
        .unwrap();
    let reader = BufReader::new(stdout);
    let p1 = match p1_meter::parse_lines(reader.lines().map(|x| x.unwrap())) {
        Ok(Some(complete)) => {
            println!("complete = {:?}", complete);
            Some(complete)
        }
        Ok(None) => {
            println!("nothing parsed");
            None
        }
        Err(_) => panic!("Error"),
    };
    let pv_2022 = match pv2022::fetch_dashboard_value(pv_2022_cmd) {
        Ok(pv_2022) => {
            println!("PV2022={}", pv_2022);
            Some(pv_2022)
        }
        Err(s) => {
            println!("PV2022 err: {}", s);
            None
        }
    };
    (p1, pv_2022)
}

pub fn save_data(blocking_ref: &SharedState, p1: Option<CompleteP1Measurement>, pv_2022: Option<f64>, sql_cmd: &str) {
    {
        let state = &mut blocking_ref.write().unwrap();
        match state.set_data(p1, pv_2022) {
            Some(_) => {
                state.halve_data();
            }
            None => {}
        }
        match (state.get_first_data(), state.get_last_data()) {
            (Some(first), Some(last)) => if last.timestamp - first.timestamp > 3600 {
                let mut inserted = 0;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FAKE_PV_2022: &str = "echo '{\"result\":{\"0199-xxxxx9BD\":{\"6800_08822000\":{\"1\":[{\"validVals\":[9401,9402,9403,9404,9405],\"val\":[{\"tag\":9404}]}]},\"6800_10821E00\":{\"1\":[{\"val\":\"SN: xxxxxxx245\"}]},\"6800_08811F00\":{\"1\":[{\"validVals\":[1129,1130],\"val\":[{\"tag\":1129}]}]},\"6180_08214800\":{\"1\":[{\"val\":[{\"tag\":307}]}]},\"6180_08414900\":{\"1\":[{\"val\":[{\"tag\":886}]}]},\"6180_08522F00\":{\"1\":[{\"val\":[{\"tag\":16777213}]}]},\"6800_088A2900\":{\"1\":[{\"validVals\":[302,9327,9375,9376,9437,19043],\"val\":[{\"tag\":302}]}]},\"6100_40463600\":{\"1\":[{\"val\":null}]},\"6100_40463700\":{\"1\":[{\"val\":null}]},\"6100_40263F00\":{\"1\":[{\"val\":null}]},\"6400_00260100\":{\"1\":[{\"val\":7439043}]},\"6800_00832A00\":{\"1\":[{\"low\":5000,\"high\":5000,\"val\":5000}]},\"6800_008AA200\":{\"1\":[{\"low\":0,\"high\":null,\"val\":0}]},\"6400_00462500\":{\"1\":[{\"val\":null}]},\"6100_00418000\":{\"1\":[{\"val\":null}]},\"6800_08822B00\":{\"1\":[{\"validVals\":[461],\"val\":[{\"tag\":461}]}]},\"6100_0046C200\":{\"1\":[{\"val\":null}]},\"6400_0046C300\":{\"1\":[{\"val\":7459043}]},\"6802_08834500\":{\"1\":[{\"validVals\":[303,1439],\"val\":[{\"tag\":1439}]}]},\"6180_08412800\":{\"1\":[{\"val\":[{\"tag\":16777213}]}]}}}}'";
    const FAKE_P1: &str = "echo '0-0:1.0.0(241025000000S)'; echo '1-0:1.8.1(002654.919*kWh)'; echo '1-0:1.8.2(002420.293*kWh)'; echo '1-0:2.8.1(006254.732*kWh)'; echo '1-0:2.8.2(002457.202*kWh)';";
    #[test]
    fn no_measurement() {
        assert_eq!(
            poll_automated_measurements("echo A", "echo B"),
            (None, None)
        )
    }

    #[test]
    fn only_pv_2022_measurement() {
        assert_eq!(
            poll_automated_measurements("echo A", FAKE_PV_2022),
            (None, Some(7439.043))
        )
    }

    #[test]
    fn only_p1_measurement() {
        assert_eq!(
            poll_automated_measurements(FAKE_P1, "echo B"),
            (
                Some(CompleteP1Measurement {
                    timestamp: Date::from_calendar_date(2024, Month::October, 25)
                        .unwrap()
                        .midnight()
                        .assume_offset(UtcOffset::from_hms(2, 0, 0).unwrap()),
                    peak_hour_consumption: 2654.919,
                    off_hour_consumption: 2420.293,
                    peak_hour_injection: 6254.732,
                    off_hour_injection: 2457.202
                }),
                None
            )
        )
    }

    #[test]
    fn both_measurements() {
        assert_eq!(
            poll_automated_measurements(FAKE_P1, FAKE_PV_2022),
            (
                Some(CompleteP1Measurement {
                    timestamp: Date::from_calendar_date(2024, Month::October, 25)
                        .unwrap()
                        .midnight()
                        .assume_offset(UtcOffset::from_hms(2, 0, 0).unwrap()),
                    peak_hour_consumption: 2654.919,
                    off_hour_consumption: 2420.293,
                    peak_hour_injection: 6254.732,
                    off_hour_injection: 2457.202
                }),
                Some(7439.043)
            )
        )
    }
}
