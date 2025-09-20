use chrono::{DateTime, FixedOffset};
use meter_core::{
    data::{Data202303, clone_data202303, insert_many_data_202303},
    p1_meter::{self, CompleteP1Measurement},
    pv2022,
    ringbuffer::{self, RingBuffer, RingBufferView, freeze},
};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, RwLock, RwLockWriteGuard},
    time::{SystemTime, UNIX_EPOCH},
};

pub type SharedState = Arc<RwLock<AppState>>;

pub struct AppState {
    pub data: RingBuffer<Data202303>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            data: ringbuffer::new::<Data202303>(1440),
        }
    }
}

impl AppState {
    pub fn set_data(
        &mut self,
        p1: Option<CompleteP1Measurement>,
        pv_2022: Option<f64>,
    ) -> Option<Data202303> {
        if p1 == None && pv_2022 == None {
            return None;
        }

        let timestamp = match &p1 {
            Some(p1) => p1.timestamp.timestamp(),
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
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(p1_data_cmd)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let lines = BufReader::new(stdout).lines().map(|x| x.unwrap());
    let p1 = match p1_meter::parse_lines(lines) {
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
    child.wait().expect("unable to kill p1_data_cmd?");
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

pub fn save_data(
    blocking_ref: &SharedState,
    p1: Option<CompleteP1Measurement>,
    pv_2022: Option<f64>,
    sql_cmd: &str,
    dump_interval: i64,
) {
    let state = &mut blocking_ref.write().unwrap();
    if let Some(_) = state.set_data(p1, pv_2022) {
        state.halve_data();
    }
    if let (Some(first), Some(last)) = (state.get_first_data(), state.get_last_data()) {
        if last.timestamp - first.timestamp > dump_interval {
            match insert_many_data_202303(sql_cmd, freeze(&state.data).iter_limited(100)) {
                Ok(n) if n > 0 => state.data.drop_first(n),
                Ok(_) => println!("No error but no data saved either"),
                Err(e) => println!("Error saving data: {}", e),
            }
        }
    }
}

pub fn save_manual_inputs(
    state: &mut RwLockWriteGuard<'_, AppState>,
    timestamp: DateTime<FixedOffset>,
    #[allow(non_snake_case)] pv2012_kWh: Option<f64>,
    gas_m3: Option<f64>,
    water_m3: Option<f64>,
) {
    let len = state.data.len();
    let timestamp = timestamp.timestamp();
    match state.data.with_view(
        |vw: RingBufferView<'_, Data202303>| -> Result<(usize, Data202303), usize> {
            if len == 0 {
                return Err(0);
            }
            let mut left = 0;
            let mut right = vw.len();  // *NOT* len-1 because mid-point is biased towards left through integer division
            let mut best: Option<(usize, Data202303)> = None;
            let mut mid: usize;
            let mut old_mid = right;
            while {
                mid = (left + right) / 2;
                if let Some(elt) = vw.at(mid) {
                    let elt_diff = (elt.timestamp - timestamp).abs();
                    if elt_diff <= 60 {
                        if best.as_ref().map_or(
                            true,
                            |(
                                _,
                                Data202303 {
                                    timestamp: best_ts, ..
                                },
                            )| {
                                let best_diff = (best_ts - timestamp).abs();
                                best_diff > elt_diff
                                    // If same distance, prefer earlier (on account that the human took some time to fill it in manually)
                                    || ((best_diff == elt_diff) && (best_ts > &elt.timestamp))},
                        ) {
                            best = Some((mid, clone_data202303(elt)));
                        }
                    }
                    if elt.timestamp < timestamp {
                        left = mid;
                    } else {
                        right = mid;
                    }
                } else {
                    panic!("Not reached1: we should only look inside the correct range. left={} mid={} right={} len={}", left, mid, right, len);
                }
                left < right && mid != old_mid
            } {
                old_mid = mid;
            };
            if let Some(best) = best {
                return Ok(best);
            }
            if let Some(Data202303 { timestamp: ts, .. }) = vw.at(left) {
                if *ts < timestamp {
                    Err(left + 1)
                } else {
                    Err(left)
                }
            } else {
                panic!("Not reached2: we should only look inside the correct range. left={} mid={} right={} len={}", left, mid, right, len);
            }
        },
    ) {
        Ok((idx, existing_data)) => {
            state.data.replace(
                idx,
                Data202303 {
                    timestamp: existing_data.timestamp,
                    pv2012_kWh,
                    pv2022_kWh: existing_data.pv2022_kWh,
                    peak_conso_kWh: existing_data.peak_conso_kWh,
                    off_conso_kWh: existing_data.off_conso_kWh,
                    peak_inj_kWh: existing_data.peak_inj_kWh,
                    off_inj_kWh: existing_data.off_inj_kWh,
                    gas_m3,
                    water_m3,
                },
            );
        }
        Err(idx) => {
            state.data.insert_at(
                idx,
                Data202303 {
                    timestamp,
                    pv2012_kWh,
                    pv2022_kWh: None,
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3,
                    water_m3,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
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
                    timestamp: Utc.with_ymd_and_hms(2024, 10, 24, 22, 0, 0).unwrap(),
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
                    timestamp: Utc.with_ymd_and_hms(2024, 10, 24, 22, 0, 0).unwrap(),
                    peak_hour_consumption: 2654.919,
                    off_hour_consumption: 2420.293,
                    peak_hour_injection: 6254.732,
                    off_hour_injection: 2457.202
                }),
                Some(7439.043)
            )
        )
    }

    #[test]
    fn save_data_flushes_when_more_than_1h_of_data() {
        let state: SharedState = Arc::new(RwLock::new(AppState::default()));
        let mut timestamp = Utc.with_ymd_and_hms(2024, 10, 25, 2, 0, 0).unwrap();

        // Insert the first record
        save_data(
            &state,
            Some(CompleteP1Measurement {
                timestamp,
                peak_hour_consumption: 1.0,
                off_hour_consumption: 2.0,
                peak_hour_injection: 3.0,
                off_hour_injection: 4.0,
            }),
            Some(1234.0),
            "echo dontcallmenow; exit 123",
            3600,
        );

        assert_eq!(state.read().unwrap().data.len(), 1);

        // more entries, each 2 minutes later than the previous
        for i in 0..4 {
            timestamp += Duration::minutes(2);
            save_data(
                &state,
                Some(CompleteP1Measurement {
                    timestamp,
                    peak_hour_consumption: 1.0,
                    off_hour_consumption: 2.0,
                    peak_hour_injection: 3.0,
                    off_hour_injection: 4.0,
                }),
                Some(5678.0 + (i as f64)),
                &format!("echo dontcallmenow; exit 1{}4", i),
                3600,
            );
        }

        assert_eq!(state.read().unwrap().data.len(), 5);

        // "last" entry, 2h later to make sure that ringbuffer is "flushed"
        timestamp += Duration::hours(2);
        save_data(
            &state,
            Some(CompleteP1Measurement {
                timestamp,
                peak_hour_consumption: 11.0,
                off_hour_consumption: 12.0,
                peak_hour_injection: 13.0,
                off_hour_injection: 14.0,
            }),
            Some(6789.0),
            "echo 10; echo 14",
            3600,
        );

        // After flushing, the buffer should have dropped 14-10==4 entries
        let state_ref = state.read().unwrap();
        assert_eq!(state_ref.data.len(), 2);

        let first_opt = state_ref.get_first_data();
        let last_opt = state_ref.get_last_data();

        assert!(first_opt.is_some());
        assert!(last_opt.is_some());
        let first_opt = first_opt.unwrap();
        let last_opt = last_opt.unwrap();
        assert_eq!(first_opt.pv2022_kWh, Some(5681.0));
        assert_eq!(last_opt.timestamp, timestamp.timestamp());
        assert_eq!(last_opt.pv2022_kWh, Some(6789.0));
    }

    #[test]
    fn save_manual_inputs_enrich_existing_data() {
        let state: SharedState = Arc::new(RwLock::new(AppState::default()));
        let mut w = state.write().unwrap();
        w.data.push(Data202303 {
            timestamp: 1000,
            pv2012_kWh: None,
            pv2022_kWh: Some(123.45),
            peak_conso_kWh: Some(234.56),
            off_conso_kWh: Some(345.67),
            peak_inj_kWh: Some(456.78),
            off_inj_kWh: Some(567.89),
            gas_m3: None,
            water_m3: None,
        });
        w.data.push(Data202303 {
            timestamp: 1045,
            pv2012_kWh: None,
            pv2022_kWh: Some(23.45),
            peak_conso_kWh: Some(34.56),
            off_conso_kWh: Some(45.67),
            peak_inj_kWh: Some(56.78),
            off_inj_kWh: Some(67.89),
            gas_m3: None,
            water_m3: None,
        });
        w.data.push(Data202303 {
            timestamp: 1062,
            pv2012_kWh: None,
            pv2022_kWh: Some(234.5),
            peak_conso_kWh: Some(345.6),
            off_conso_kWh: Some(456.7),
            peak_inj_kWh: Some(567.8),
            off_inj_kWh: Some(678.9),
            gas_m3: None,
            water_m3: None,
        });
        w.data.push(Data202303 {
            timestamp: 1118,
            pv2012_kWh: None,
            pv2022_kWh: Some(2.345),
            peak_conso_kWh: Some(3.456),
            off_conso_kWh: Some(4.567),
            peak_inj_kWh: Some(5.678),
            off_inj_kWh: Some(6.789),
            gas_m3: None,
            water_m3: None,
        });
        w.data.push(Data202303 {
            timestamp: 2000,
            pv2012_kWh: Some(111.0),
            pv2022_kWh: None,
            peak_conso_kWh: None,
            off_conso_kWh: None,
            peak_inj_kWh: None,
            off_inj_kWh: None,
            gas_m3: None,
            water_m3: None,
        });
        save_manual_inputs(
            &mut w,
            DateTime::from_timestamp_nanos(1060 * 1_000_000_000).into(),
            Some(2.0),
            Some(3.0),
            Some(4.0),
        );
        w.data.with_view(|vw| {
            assert_eq!(
                vw.into_iter().map(clone_data202303).collect::<Vec<_>>(),
                vec![
                    Data202303 {
                        timestamp: 1000,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(123.45),
                        peak_conso_kWh: Some(234.56),
                        off_conso_kWh: Some(345.67),
                        peak_inj_kWh: Some(456.78),
                        off_inj_kWh: Some(567.89),
                        gas_m3: None,
                        water_m3: None
                    },
                    Data202303 {
                        timestamp: 1045,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(23.45),
                        peak_conso_kWh: Some(34.56),
                        off_conso_kWh: Some(45.67),
                        peak_inj_kWh: Some(56.78),
                        off_inj_kWh: Some(67.89),
                        gas_m3: None,
                        water_m3: None
                    },
                    Data202303 {
                        timestamp: 1062,
                        pv2012_kWh: Some(2.0),
                        pv2022_kWh: Some(234.5),
                        peak_conso_kWh: Some(345.6),
                        off_conso_kWh: Some(456.7),
                        peak_inj_kWh: Some(567.8),
                        off_inj_kWh: Some(678.9),
                        gas_m3: Some(3.0),
                        water_m3: Some(4.0)
                    },
                    Data202303 {
                        timestamp: 1118,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(2.345),
                        peak_conso_kWh: Some(3.456),
                        off_conso_kWh: Some(4.567),
                        peak_inj_kWh: Some(5.678),
                        off_inj_kWh: Some(6.789),
                        gas_m3: None,
                        water_m3: None
                    },
                    Data202303 {
                        timestamp: 2000,
                        pv2012_kWh: Some(111.0),
                        pv2022_kWh: None,
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None
                    },
                ]
            )
        })
    }

    #[test]
    fn save_manual_inputs_parameterized() {
        struct Case {
            name: &'static str,
            existing: Vec<Data202303>,
            input_ts: i64,
            input_pv2012: Option<f64>,
            input_gas: Option<f64>,
            input_water: Option<f64>,
            expected: Vec<Data202303>,
        }

        let cases: Vec<Case> = vec![
            Case {
                name: "insert_into_empty",
                existing: vec![],
                input_ts: 1000,
                input_pv2012: Some(1.0),
                input_gas: Some(2.0),
                input_water: Some(3.0),
                expected: vec![Data202303 {
                    timestamp: 1000,
                    pv2012_kWh: Some(1.0),
                    pv2022_kWh: None,
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: Some(2.0),
                    water_m3: Some(3.0),
                }],
            },
            Case {
                name: "insert_before_first",
                existing: vec![Data202303 {
                    timestamp: 2000,
                    pv2012_kWh: None,
                    pv2022_kWh: None,
                    peak_conso_kWh: None,
                    off_conso_kWh: Some(3.14),
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: None,
                    water_m3: None,
                }],
                input_ts: 1000,
                input_pv2012: Some(9.0),
                input_gas: None,
                input_water: None,
                expected: vec![
                    Data202303 {
                        timestamp: 1000,
                        pv2012_kWh: Some(9.0),
                        pv2022_kWh: None,
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 2000,
                        pv2012_kWh: None,
                        pv2022_kWh: None,
                        peak_conso_kWh: None,
                        off_conso_kWh: Some(3.14),
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                ],
            },
            Case {
                name: "insert_after_last",
                existing: vec![Data202303 {
                    timestamp: 1000,
                    pv2012_kWh: None,
                    pv2022_kWh: None,
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: None,
                    water_m3: None,
                }],
                input_ts: 2000,
                input_pv2012: None,
                input_gas: Some(7.0),
                input_water: Some(8.0),
                expected: vec![
                    Data202303 {
                        timestamp: 1000,
                        pv2012_kWh: None,
                        pv2022_kWh: None,
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 2000,
                        pv2012_kWh: None,
                        pv2022_kWh: None,
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: Some(7.0),
                        water_m3: Some(8.0),
                    },
                ],
            },
            Case {
                name: "update_exact_match (single pre-existing element)",
                existing: vec![Data202303 {
                    timestamp: 1500,
                    pv2012_kWh: None,
                    pv2022_kWh: Some(42.0),
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: None,
                    water_m3: None,
                }],
                input_ts: 1500,
                input_pv2012: Some(10.0),
                input_gas: Some(20.0),
                input_water: Some(30.0),
                expected: vec![Data202303 {
                    timestamp: 1500,
                    pv2012_kWh: Some(10.0),
                    pv2022_kWh: Some(42.0), // preserved
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: Some(20.0),
                    water_m3: Some(30.0),
                }],
            },
            Case {
                name: "update_exact_match (2 pre-existing elements)",
                existing: vec![
                    Data202303 {
                        timestamp: 1498,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(41.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1500,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(42.0),
                        peak_conso_kWh: Some(43.0),
                        off_conso_kWh: Some(44.0),
                        peak_inj_kWh: Some(45.0),
                        off_inj_kWh: Some(46.0),
                        gas_m3: None,
                        water_m3: None,
                    },
                ],
                input_ts: 1500,
                input_pv2012: Some(10.0),
                input_gas: Some(20.0),
                input_water: Some(30.0),
                expected: vec![
                    Data202303 {
                        timestamp: 1498,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(41.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1500,
                        pv2012_kWh: Some(10.0),
                        pv2022_kWh: Some(42.0),     // preserved
                        peak_conso_kWh: Some(43.0), // preserved
                        off_conso_kWh: Some(44.0),  // preserved
                        peak_inj_kWh: Some(45.0),   // preserved
                        off_inj_kWh: Some(46.0),    // preserved
                        gas_m3: Some(20.0),
                        water_m3: Some(30.0),
                    },
                ],
            },
            Case {
                name: "update_exact_match (3 pre-existing elements)",
                existing: vec![
                    Data202303 {
                        timestamp: 1498,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(41.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1500,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(42.0),
                        peak_conso_kWh: Some(43.0),
                        off_conso_kWh: Some(44.0),
                        peak_inj_kWh: Some(45.0),
                        off_inj_kWh: Some(46.0),
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1501,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(42.1),
                        peak_conso_kWh: Some(43.1),
                        off_conso_kWh: Some(44.1),
                        peak_inj_kWh: Some(45.1),
                        off_inj_kWh: Some(46.1),
                        gas_m3: None,
                        water_m3: None,
                    },
                ],
                input_ts: 1500,
                input_pv2012: Some(10.0),
                input_gas: Some(20.0),
                input_water: Some(30.0),
                expected: vec![
                    Data202303 {
                        timestamp: 1498,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(41.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1500,
                        pv2012_kWh: Some(10.0),
                        pv2022_kWh: Some(42.0),     // preserved
                        peak_conso_kWh: Some(43.0), // preserved
                        off_conso_kWh: Some(44.0),  // preserved
                        peak_inj_kWh: Some(45.0),   // preserved
                        off_inj_kWh: Some(46.0),    // preserved
                        gas_m3: Some(20.0),
                        water_m3: Some(30.0),
                    },
                    Data202303 {
                        timestamp: 1501,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(42.1),
                        peak_conso_kWh: Some(43.1),
                        off_conso_kWh: Some(44.1),
                        peak_inj_kWh: Some(45.1),
                        off_inj_kWh: Some(46.1),
                        gas_m3: None,
                        water_m3: None,
                    },
                ],
            },
            Case {
                name: "insert_no_close_match_more_than_60s_away",
                existing: vec![Data202303 {
                    timestamp: 1000,
                    pv2012_kWh: None,
                    pv2022_kWh: Some(1.23),
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: None,
                    water_m3: None,
                }],
                input_ts: 1200, // 200s away, should insert
                input_pv2012: Some(9.0),
                input_gas: Some(8.0),
                input_water: Some(7.0),
                expected: vec![
                    Data202303 {
                        timestamp: 1000,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(1.23),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1200,
                        pv2012_kWh: Some(9.0),
                        pv2022_kWh: None,
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: Some(8.0),
                        water_m3: Some(7.0),
                    },
                ],
            },
            Case {
                name: "update_when_exactly_60s_apart",
                existing: vec![Data202303 {
                    timestamp: 1000,
                    pv2012_kWh: None,
                    pv2022_kWh: Some(55.0),
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: None,
                    water_m3: None,
                }],
                input_ts: 1060, // exactly 60s away
                input_pv2012: Some(1.0),
                input_gas: Some(2.0),
                input_water: Some(3.0),
                expected: vec![Data202303 {
                    timestamp: 1000,
                    pv2012_kWh: Some(1.0),
                    pv2022_kWh: Some(55.0),
                    peak_conso_kWh: None,
                    off_conso_kWh: None,
                    peak_inj_kWh: None,
                    off_inj_kWh: None,
                    gas_m3: Some(2.0),
                    water_m3: Some(3.0),
                }],
            },
            Case {
                name: "update_earlier_of_two_candidates",
                existing: vec![
                    Data202303 {
                        timestamp: 940,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(10.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                    Data202303 {
                        timestamp: 1060,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(20.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                ],
                input_ts: 1000, // equally 60s away from 940 and 1060
                input_pv2012: Some(9.0),
                input_gas: Some(8.0),
                input_water: Some(7.0),
                expected: vec![
                    Data202303 {
                        timestamp: 940,
                        pv2012_kWh: Some(9.0), // should update earliest
                        pv2022_kWh: Some(10.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: Some(8.0),
                        water_m3: Some(7.0),
                    },
                    Data202303 {
                        timestamp: 1060,
                        pv2012_kWh: None,
                        pv2022_kWh: Some(20.0),
                        peak_conso_kWh: None,
                        off_conso_kWh: None,
                        peak_inj_kWh: None,
                        off_inj_kWh: None,
                        gas_m3: None,
                        water_m3: None,
                    },
                ],
            },
        ];

        for case in cases {
            let state: SharedState = Arc::new(RwLock::new(AppState::default()));
            let mut w = state.write().unwrap();
            for d in &case.existing {
                w.data.push(clone_data202303(d));
            }

            save_manual_inputs(
                &mut w,
                DateTime::from_timestamp_nanos(case.input_ts * 1_000_000_000).into(),
                case.input_pv2012,
                case.input_gas,
                case.input_water,
            );

            w.data.with_view(|vw| {
                let got: Vec<_> = vw.into_iter().map(clone_data202303).collect();
                assert_eq!(got, case.expected, "failed case: {}", case.name);
            });
        }
    }
}
