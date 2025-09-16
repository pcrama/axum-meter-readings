use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use std::borrow::Borrow;
use std::error::Error;
use std::num::ParseFloatError;
use std::str::FromStr;

// 0-0:1.0.0(241025191816S)
//
// 1-0:1.8.1(002654.919*kWh)
//
// 1-0:1.8.2(002420.293*kWh)
//
// 1-0:2.8.1(006254.732*kWh)
//
// 1-0:2.8.2(002457.202*kWh)

fn strip_prefix_and_suffix<'a>(line: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    if line.starts_with(prefix) && line.ends_with(suffix) {
        Some(&line[prefix.len()..line.len() - suffix.len()])
    } else {
        None
    }
}

fn parse_kwh(line: &str, prefix: &str) -> Result<Option<f64>, ParseFloatError> {
    match strip_prefix_and_suffix(line, prefix, "*kWh)") {
        Some(kwh) => f64::from_str(kwh).map(Some),
        None => Ok(None),
    }
}

fn parse_date_time(line: &str) -> Result<Option<DateTime<Utc>>, Box<dyn Error>> {
    const DATA_LEN: usize = 13;
    match strip_prefix_and_suffix(line, "0-0:1.0.0(", ")") {
        Some(yymmddhhmmssx) => {
            if yymmddhhmmssx.len() == DATA_LEN
                && yymmddhhmmssx
                    .chars()
                    .nth(DATA_LEN - 1)
                    .map(|summer_or_winter| summer_or_winter == 'S' || summer_or_winter == 'W')
                    .unwrap_or(false)
            {
                let yy = 2000 + i32::from_str(&yymmddhhmmssx[0..2])?;
                let mm = u32::from_str(&yymmddhhmmssx[2..4])?;
                let dd = u32::from_str(&yymmddhhmmssx[4..6])?;
                let hours = u32::from_str(&yymmddhhmmssx[6..8])?;
                let mins = u32::from_str(&yymmddhhmmssx[8..10])?;
                let secs = u32::from_str(&yymmddhhmmssx[10..12])?;
                let offset = FixedOffset::east_opt(
                    (if yymmddhhmmssx.chars().nth(12).unwrap_or('?') == 'S' {
                        2 // Central European Summer Time
                    } else {
                        1 // Central European Time
                    }) * 3600,
                )
                .unwrap();
                if let Some(datetime) = offset
                    .with_ymd_and_hms(yy, mm, dd, hours, mins, secs)
                    .single()
                {
                    Ok(Some(datetime.to_utc()))
                } else {
                    Err("Unable to build datetime object from P1 0-0:1.0.0".into())
                }
            } else {
                Ok(None) // I should (but am not going to) define an error type here
            }
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefix_and_suffix_prefix_mismatch_expect_none() {
        assert_eq!(
            strip_prefix_and_suffix("1-0:2.8.2(002457.202*kWh)", "mismatch", "*kWh)"),
            None
        )
    }

    #[test]
    fn strip_prefix_and_suffix_suffix_mismatch_expect_none() {
        assert_eq!(
            strip_prefix_and_suffix("1-0:2.8.2(002457.202*kWh)", "1-0:2.8.2(", "mismatch)"),
            None
        )
    }

    #[test]
    fn strip_prefix_and_suffix_match_expect_sub_string() {
        assert_eq!(
            strip_prefix_and_suffix("1-0:2.8.2(002457.202*kWh)", "1-0:2.8.2(", "*kWh)"),
            Some("002457.202")
        )
    }

    #[test]
    fn parse_kwh_expect_float() {
        assert_eq!(parse_kwh("prefix(12.34*kWh)", "prefix("), Ok(Some(12.34)))
    }

    #[test]
    fn parse_kwh_mismatch_expect_none() {
        assert_eq!(parse_kwh("prefix(12.34*kWh)", "mismatch"), Ok(None));
        assert_eq!(parse_kwh("prefix(12.34*mismatch)", "prefix("), Ok(None))
    }

    #[test]
    fn parse_kwh_bad_float_format_expect_err() {
        assert!(parse_kwh("prefix(bad-float*kWh)", "prefix(").is_err())
    }

    #[test]
    fn parse_date_time_mismatch_expect_none() {
        assert_eq!(parse_kwh("prefix(241025191816S)", "mismatch"), Ok(None));
        assert_eq!(
            parse_kwh("prefix(241025191816S)mismatch", "prefix("),
            Ok(None)
        )
    }

    #[test]
    fn parse_date_time_good_date_returned_daylight_saving_time() {
        let datetime = parse_date_time("0-0:1.0.0(240815191816S)")
            .expect("Some(date) expected here")
            .expect("date expected here");
        let expected = Utc.with_ymd_and_hms(2024, 8, 15, 17, 18, 16).unwrap();
        assert_eq!(datetime, expected);
    }

    #[test]
    fn parse_date_time_good_date_returned_winter_time() {
        let datetime = parse_date_time("0-0:1.0.0(231222191618W)")
            .expect("Some(date) expected here")
            .expect("date expected here");
        let expected = Utc.with_ymd_and_hms(2023, 12, 22, 18, 16, 18).unwrap();
        assert_eq!(datetime, expected);
    }

    #[test]
    fn parse_date_time_bad_date_error() {
        assert!(parse_date_time("0-0:1.0.0(249925191816S)").is_err());
        assert!(parse_date_time("0-0:1.0.0(240230191816S)").is_err());
        assert_eq!(
            parse_date_time("0-0:1.0.0(2410230191816S)").expect("No error expected here"),
            None
        );
        assert!(parse_date_time("0-0:1.0.0(241023241816S)").is_err());
        assert!(parse_date_time("0-0:1.0.0(241023196016S)").is_err());
        assert!(parse_date_time("0-0:1.0.0(241023195699S)").is_err());
        assert_eq!(
            parse_date_time("0-0:1.0.0(241023195609A)").expect("No error expected here"),
            None
        );
    }

    #[test]
    fn parse_lines_nonsense_returns_ok_none() {
        assert_eq!(
            parse_lines("a\nb\nc".lines()).expect("Ok(None) expected here"),
            None,
        )
    }

    #[test]
    fn parse_lines_happy_path() {
        assert_eq!(
            parse_lines("\n0-0:1.0.0(241025000000S)\n\n1-0:1.8.1(002654.919*kWh)\n\n1-0:1.8.2(002420.293*kWh)\n\n1-0:2.8.1(006254.732*kWh)\n\n1-0:2.8.2(002457.202*kWh)".lines()).expect("Ok(some meas) expected here"),
            Some(CompleteP1Measurement { timestamp: Utc.with_ymd_and_hms(2024, 10, 24, 22, 0, 0).unwrap(), peak_hour_consumption: 2654.919, off_hour_consumption: 2420.293, peak_hour_injection: 6254.732, off_hour_injection: 2457.202 }),
        )
    }

    #[test]
    fn parse_lines_skips_suffix_of_previous_datagram() {
        assert_eq!(
            parse_lines(".1(000054.732*kWh)\n\n1-0:2.8.2(000057.202*kWh)\n\n0-0:1.0.0(241025020000S)\n\n1-0:1.8.1(002654.919*kWh)\n\n1-0:1.8.2(002420.293*kWh)\n\n1-0:2.8.1(006254.732*kWh)\n\n1-0:2.8.2(002457.202*kWh)".lines()).expect("Ok(some meas) expected here"),
            Some(CompleteP1Measurement { timestamp: Utc.with_ymd_and_hms(2024, 10, 25, 0,0,0).unwrap(), peak_hour_consumption: 2654.919, off_hour_consumption: 2420.293, peak_hour_injection: 6254.732, off_hour_injection: 2457.202 }),
        )
    }

    #[test]
    fn parse_lines_returns_first_full_datagram() {
        assert_eq!(
            parse_lines(".1(000054.732*kWh)\n\n1-0:2.8.2(000057.202*kWh)\n\n0-0:1.0.0(241025000000S)\n\n1-0:1.8.1(002654.919*kWh)\n\n1-0:1.8.2(002420.293*kWh)\n\n1-0:2.8.1(006254.732*kWh)\n\n1-0:2.8.2(002457.202*kWh)\n\n0-0:1.0.0(251126000000W)\n\n1-0:1.8.1(992654.919*kWh)\n\n1-0:1.8.2(992420.293*kWh)\n\n1-0:2.8.1(996254.732*kWh)\n\n1-0:2.8.2(992457.202*kWh)".lines()).expect("Ok(some meas) expected here"),
            Some(CompleteP1Measurement { timestamp: Utc.with_ymd_and_hms(2024, 10, 24, 22,0,0).unwrap(), peak_hour_consumption: 2654.919, off_hour_consumption: 2420.293, peak_hour_injection: 6254.732, off_hour_injection: 2457.202 }),
        )
    }
}

#[derive(PartialEq, Debug)]
struct PartialP1Measurement {
    timestamp: Option<DateTime<Utc>>,
    peak_hour_consumption: Option<f64>,
    off_hour_consumption: Option<f64>,
    peak_hour_injection: Option<f64>,
    off_hour_injection: Option<f64>,
}

#[derive(PartialEq, Debug)]
pub struct CompleteP1Measurement {
    pub timestamp: DateTime<Utc>,
    pub peak_hour_consumption: f64,
    pub off_hour_consumption: f64,
    pub peak_hour_injection: f64,
    pub off_hour_injection: f64,
}

fn complete_p1_measurement(
    partial: PartialP1Measurement,
) -> Result<CompleteP1Measurement, PartialP1Measurement> {
    match partial {
        PartialP1Measurement {
            timestamp: Some(timestamp),
            peak_hour_consumption: Some(peak_hour_consumption),
            off_hour_consumption: Some(off_hour_consumption),
            peak_hour_injection: Some(peak_hour_injection),
            off_hour_injection: Some(off_hour_injection),
        } => Ok(CompleteP1Measurement {
            timestamp,
            peak_hour_consumption,
            off_hour_consumption,
            peak_hour_injection,
            off_hour_injection,
        }),
        _ => Err(partial),
    }
}

fn step_partial_p1_measurement(
    partial: PartialP1Measurement,
    line: &str,
) -> Result<PartialP1Measurement, Box<dyn Error>> {
    match partial {
        PartialP1Measurement {
            timestamp: None, ..
        } => match parse_date_time(line)? {
            Some(timestamp) => Ok(PartialP1Measurement {
                timestamp: Some(timestamp),
                peak_hour_consumption: None,
                off_hour_consumption: None,
                peak_hour_injection: None,
                off_hour_injection: None,
            }),
            _ => Ok(partial),
        },
        PartialP1Measurement {
            timestamp: Some(timestamp),
            peak_hour_consumption: None,
            ..
        } => match parse_kwh(line, "1-0:1.8.1(")? {
            Some(kwh) => Ok(PartialP1Measurement {
                timestamp: Some(timestamp),
                peak_hour_consumption: Some(kwh),
                off_hour_consumption: None,
                peak_hour_injection: None,
                off_hour_injection: None,
            }),
            _ => Ok(partial),
        },
        PartialP1Measurement {
            timestamp: Some(timestamp),
            peak_hour_consumption: Some(peak_hour_consumption),
            off_hour_consumption: None,
            ..
        } => match parse_kwh(line, "1-0:1.8.2(")? {
            Some(kwh) => Ok(PartialP1Measurement {
                timestamp: Some(timestamp),
                peak_hour_consumption: Some(peak_hour_consumption),
                off_hour_consumption: Some(kwh),
                peak_hour_injection: None,
                off_hour_injection: None,
            }),
            _ => Ok(partial),
        },
        PartialP1Measurement {
            timestamp: Some(timestamp),
            peak_hour_consumption: Some(peak_hour_consumption),
            off_hour_consumption: Some(off_hour_consumption),
            peak_hour_injection: None,
            ..
        } => match parse_kwh(line, "1-0:2.8.1(")? {
            Some(kwh) => Ok(PartialP1Measurement {
                timestamp: Some(timestamp),
                peak_hour_consumption: Some(peak_hour_consumption),
                off_hour_consumption: Some(off_hour_consumption),
                peak_hour_injection: Some(kwh),
                off_hour_injection: None,
            }),
            _ => Ok(partial),
        },
        PartialP1Measurement {
            timestamp: Some(timestamp),
            peak_hour_consumption: Some(peak_hour_consumption),
            off_hour_consumption: Some(off_hour_consumption),
            peak_hour_injection: Some(peak_hour_injection),
            off_hour_injection: None,
        } => match parse_kwh(line, "1-0:2.8.2(")? {
            Some(kwh) => Ok(PartialP1Measurement {
                timestamp: Some(timestamp),
                peak_hour_consumption: Some(peak_hour_consumption),
                off_hour_consumption: Some(off_hour_consumption),
                peak_hour_injection: Some(peak_hour_injection),
                off_hour_injection: Some(kwh),
            }),
            _ => Ok(partial),
        },
        _ => Ok(partial),
    }
}

pub fn parse_lines<T>(lines: T) -> Result<Option<CompleteP1Measurement>, Box<dyn Error>>
where
    T: IntoIterator,
    T::Item: Borrow<str>,
{
    let mut partial = PartialP1Measurement {
        timestamp: None,
        peak_hour_consumption: None,
        off_hour_consumption: None,
        peak_hour_injection: None,
        off_hour_injection: None,
    };
    for line in lines.into_iter() {
        match complete_p1_measurement(step_partial_p1_measurement(partial, line.borrow())?) {
            Ok(complete) => return Ok(Some(complete)),
            Err(new_partial) => partial = new_partial,
        }
    }
    return Ok(None);
}
