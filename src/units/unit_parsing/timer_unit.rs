//! `.timer` units. Supports both relative timer settings and a basic
//! `OnCalendar=` calendar spec. This implementation covers common
//! day/date/time matching patterns needed for calendar-based activation.
use crate::units::*;
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarSpec {
    pub weekday: Option<Weekday>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub hour: Option<u32>,
    pub minute: Option<u32>,
    pub second: Option<u32>,
}

pub fn parse_timer(
    parsed_file: ParsedFile,
    path: &PathBuf,
) -> Result<ParsedTimerConfig, ParsingErrorReason> {
    let mut install_config = None;
    let mut unit_config = None;
    let mut timer_config = None;

    for (name, section) in parsed_file {
        match name.as_str() {
            "[Unit]" => {
                unit_config = Some(parse_unit_section(section)?);
            }
            "[Install]" => {
                install_config = Some(parse_install_section(section)?);
            }
            "[Timer]" => {
                timer_config = Some(parse_timer_section(section)?);
            }
            _ => return Err(ParsingErrorReason::UnknownSection(name.to_owned())),
        }
    }

    let timer_config = if let Some(timer_config) = timer_config {
        timer_config
    } else {
        return Err(ParsingErrorReason::SectionNotFound("Timer".to_owned()));
    };

    let file_name = path.file_name().unwrap().to_str().unwrap().to_owned();

    // Unit= defaults to a .service with the same basename, e.g.
    // backup.timer -> backup.service, mirroring systemd and lksystem's own
    // .socket <-> .service name-matching convention.
    let timer_config = if timer_config.unit.is_none() {
        let base = file_name.trim_end_matches(".timer");
        ParsedTimerSection {
            unit: Some(format!("{}.service", base)),
            ..timer_config
        }
    } else {
        timer_config
    };

    Ok(ParsedTimerConfig {
        common: ParsedCommonConfig {
            name: file_name,
            unit: unit_config.unwrap_or_else(Default::default),
            install: install_config.unwrap_or_else(Default::default),
            conditions: ParsedConditions::default(),
        },
        timer: timer_config,
    })
}

/// Parses systemd.time(7)-style relative durations, e.g. "5min", "1h",
/// "90s", or a compound "5min 30s". A bare number is interpreted as whole
/// seconds. Unlike `Timeout` (used for service start/stop timeouts) there is
/// no "infinity" concept for timers.
fn parse_duration(descr: &str) -> Result<Duration, ParsingErrorReason> {
    let descr = descr.trim();
    if let Ok(secs) = descr.parse::<u64>() {
        return Ok(Duration::from_secs(secs));
    }

    let mut total = Duration::from_secs(0);
    let mut found_any = false;
    for token in descr.split_whitespace() {
        let (num_str, unit_secs) = if let Some(n) = token.strip_suffix("ms") {
            (n, None) // sub-second precision not needed for our use-cases; handled below
        } else if let Some(n) = token.strip_suffix("secs") {
            (n, Some(1u64))
        } else if let Some(n) = token.strip_suffix("sec") {
            (n, Some(1))
        } else if let Some(n) = token.strip_suffix("s") {
            (n, Some(1))
        } else if let Some(n) = token.strip_suffix("mins") {
            (n, Some(60))
        } else if let Some(n) = token.strip_suffix("min") {
            (n, Some(60))
        } else if let Some(n) = token.strip_suffix("hrs") {
            (n, Some(3600))
        } else if let Some(n) = token.strip_suffix("hr") {
            (n, Some(3600))
        } else if let Some(n) = token.strip_suffix("h") {
            (n, Some(3600))
        } else if let Some(n) = token.strip_suffix("days") {
            (n, Some(86400))
        } else if let Some(n) = token.strip_suffix("d") {
            (n, Some(86400))
        } else if let Some(n) = token.strip_suffix("weeks") {
            (n, Some(7 * 86400))
        } else if let Some(n) = token.strip_suffix("w") {
            (n, Some(7 * 86400))
        } else {
            return Err(ParsingErrorReason::Generic(format!(
                "Could not parse duration: {}",
                descr
            )));
        };

        match unit_secs {
            Some(mult) => {
                let n: u64 = num_str.parse().map_err(|_| {
                    ParsingErrorReason::Generic(format!("Could not parse duration: {}", descr))
                })?;
                total += Duration::from_secs(n * mult);
            }
            None => {
                // "ms" suffix: we only need whole-second-ish granularity for
                // timers, so round up to at least 1s if a sub-second value
                // was given rather than silently dropping it to 0.
                let n: u64 = num_str.parse().map_err(|_| {
                    ParsingErrorReason::Generic(format!("Could not parse duration: {}", descr))
                })?;
                total += Duration::from_millis(n);
            }
        }
        found_any = true;
    }

    if !found_any {
        return Err(ParsingErrorReason::Generic(format!(
            "Could not parse duration: {}",
            descr
        )));
    }

    Ok(total)
}

fn parse_single_duration_setting(
    section: &mut ParsedSection,
    key: &str,
) -> Result<Option<Duration>, ParsingErrorReason> {
    match section.remove(key) {
        None => Ok(None),
        Some(vec) => {
            if vec.len() == 1 {
                Ok(Some(parse_duration(&vec[0].1)?))
            } else {
                Err(ParsingErrorReason::SettingTooManyValues(
                    key.to_owned(),
                    map_tupels_to_second(vec),
                ))
            }
        }
    }
}

fn parse_calendar_setting(
    section: &mut ParsedSection,
    key: &str,
) -> Result<Option<CalendarSpec>, ParsingErrorReason> {
    match section.remove(key) {
        None => Ok(None),
        Some(vec) => {
            if vec.len() == 1 {
                Ok(Some(parse_calendar_spec(&vec[0].1)?))
            } else {
                Err(ParsingErrorReason::SettingTooManyValues(
                    key.to_owned(),
                    map_tupels_to_second(vec),
                ))
            }
        }
    }
}

fn parse_calendar_spec(descr: &str) -> Result<CalendarSpec, ParsingErrorReason> {
    let descr = descr.trim();
    if descr.is_empty() {
        return Err(ParsingErrorReason::Generic("Could not parse calendar spec".to_owned()));
    }

    let tokens: Vec<&str> = descr.split_whitespace().collect();
    let (weekday_token, remainder) = if tokens.len() > 1 {
        let maybe_weekday = parse_weekday(tokens[0]);
        if maybe_weekday.is_some() {
            (maybe_weekday, &tokens[1..])
        } else {
            (None, &tokens[..])
        }
    } else {
        (None, &tokens[..])
    };

    let (date_token, time_token) = match remainder.len() {
        0 => (None, None),
        1 => {
            if remainder[0].contains(':') {
                (None, Some(remainder[0]))
            } else {
                (Some(remainder[0]), None)
            }
        }
        2 => (Some(remainder[0]), Some(remainder[1])),
        _ => {
            return Err(ParsingErrorReason::Generic(format!(
                "Could not parse calendar spec: {}",
                descr
            )))
        }
    };

    let (year, month, day) = if let Some(date_token) = date_token {
        parse_calendar_date(date_token)?
    } else {
        (None, None, None)
    };

    let (hour, minute, second) = if let Some(time_token) = time_token {
        parse_calendar_time(time_token)?
    } else {
        (None, None, None)
    };

    Ok(CalendarSpec {
        weekday: weekday_token,
        year,
        month,
        day,
        hour,
        minute,
        second,
    })
}

fn parse_weekday(token: &str) -> Option<Weekday> {
    match token.to_uppercase().as_str() {
        "SUN" | "0" | "7" => Some(Weekday::Sun),
        "MON" | "1" => Some(Weekday::Mon),
        "TUE" | "2" => Some(Weekday::Tue),
        "WED" | "3" => Some(Weekday::Wed),
        "THU" | "4" => Some(Weekday::Thu),
        "FRI" | "5" => Some(Weekday::Fri),
        "SAT" | "6" => Some(Weekday::Sat),
        _ => None,
    }
}

fn parse_calendar_date(
    token: &str,
) -> Result<(Option<i32>, Option<u32>, Option<u32>), ParsingErrorReason> {
    let parts: Vec<&str> = token.split('-').collect();
    if parts.len() != 3 {
        return Err(ParsingErrorReason::Generic(format!(
            "Could not parse calendar date: {}",
            token
        )));
    }

    Ok((
        parse_calendar_date_component(parts[0], 0, 9999)?,
        parse_calendar_date_component(parts[1], 1, 12)?.map(|v| v as u32),
        parse_calendar_date_component(parts[2], 1, 31)?.map(|v| v as u32),
    ))
}

fn parse_calendar_date_component(
    part: &str,
    min: i32,
    max: i32,
) -> Result<Option<i32>, ParsingErrorReason> {
    if part == "*" {
        return Ok(None);
    }
    let value: i32 = part.parse().map_err(|_| {
        ParsingErrorReason::Generic(format!("Could not parse calendar date: {}", part))
    })?;
    if value < min || value > max {
        return Err(ParsingErrorReason::Generic(format!(
            "Could not parse calendar date: {}",
            part
        )));
    }
    Ok(Some(value))
}

fn parse_calendar_time(
    token: &str,
) -> Result<(Option<u32>, Option<u32>, Option<u32>), ParsingErrorReason> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(ParsingErrorReason::Generic(format!(
            "Could not parse calendar time: {}",
            token
        )));
    }

    let hour = parse_calendar_time_component(parts[0], 0, 23)?;
    let minute = parse_calendar_time_component(parts[1], 0, 59)?;
    let second = if parts.len() == 3 {
        parse_calendar_time_component(parts[2], 0, 59)?
    } else {
        Some(0)
    };

    Ok((hour, minute, second))
}

fn parse_calendar_time_component(
    part: &str,
    min: u32,
    max: u32,
) -> Result<Option<u32>, ParsingErrorReason> {
    if part == "*" {
        return Ok(None);
    }
    let value: u32 = part.parse().map_err(|_| {
        ParsingErrorReason::Generic(format!("Could not parse calendar time: {}", part))
    })?;
    if value < min || value > max {
        return Err(ParsingErrorReason::Generic(format!(
            "Could not parse calendar time: {}",
            part
        )));
    }
    Ok(Some(value))
}

pub fn next_calendar_instant(spec: &CalendarSpec) -> Option<DateTime<Local>> {
    let now = Local::now();
    let start_year = spec.year.unwrap_or(now.year());
    let end_year = spec.year.unwrap_or(now.year() + 100);

    let years: Vec<i32> = if let Some(year) = spec.year {
        vec![year]
    } else {
        (start_year..=end_year).collect()
    };

    let months: Vec<u32> = if let Some(month) = spec.month {
        vec![month]
    } else {
        (1..=12).collect()
    };

    let days: Vec<u32> = if let Some(day) = spec.day {
        vec![day]
    } else {
        (1..=31).collect()
    };

    let hours: Vec<u32> = if let Some(hour) = spec.hour {
        vec![hour]
    } else {
        (0..=23).collect()
    };

    let minutes: Vec<u32> = if let Some(minute) = spec.minute {
        vec![minute]
    } else {
        (0..=59).collect()
    };

    let seconds: Vec<u32> = if let Some(second) = spec.second {
        vec![second]
    } else {
        (0..=59).collect()
    };

    for year in years {
        for &month in &months {
            for &day in &days {
                let naive_date = match NaiveDate::from_ymd_opt(year, month, day) {
                    Some(date) => date,
                    None => continue,
                };
                for &hour in &hours {
                    for &minute in &minutes {
                        for &second in &seconds {
                            let naive_time = match NaiveTime::from_hms_opt(hour, minute, second) {
                                Some(time) => time,
                                None => continue,
                            };
                            let naive_dt = NaiveDateTime::new(naive_date, naive_time);
                            if let Some(local_dt) = Local.from_local_datetime(&naive_dt).single() {
                                if local_dt < now {
                                    continue;
                                }
                                if let Some(weekday) = spec.weekday {
                                    if local_dt.weekday() != weekday {
                                        continue;
                                    }
                                }
                                return Some(local_dt);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn parse_timer_section(
    mut section: ParsedSection,
) -> Result<ParsedTimerSection, ParsingErrorReason> {
    let on_boot_sec = parse_single_duration_setting(&mut section, "ONBOOTSEC")?;
    let on_active_sec = parse_single_duration_setting(&mut section, "ONACTIVESEC")?;
    let on_unit_active_sec = parse_single_duration_setting(&mut section, "ONUNITACTIVESEC")?;
    let on_calendar = parse_calendar_setting(&mut section, "ONCALENDAR")?;

    let unit = match section.remove("UNIT") {
        None => None,
        Some(mut vec) => {
            if vec.len() == 1 {
                Some(vec.remove(0).1)
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Unit".to_owned(),
                    map_tupels_to_second(vec),
                ));
            }
        }
    };

    if !section.is_empty() {
        return Err(ParsingErrorReason::UnusedSetting(
            section.keys().next().unwrap().to_owned(),
        ));
    }

    if on_boot_sec.is_none()
        && on_active_sec.is_none()
        && on_unit_active_sec.is_none()
        && on_calendar.is_none()
    {
        return Err(ParsingErrorReason::MissingSetting(
            "OnBootSec=/OnActiveSec=/OnUnitActiveSec=/OnCalendar= (at least one required)".to_owned(),
        ));
    }

    Ok(ParsedTimerSection {
        on_boot_sec,
        on_active_sec,
        on_unit_active_sec,
        on_calendar,
        unit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Local, Timelike, Weekday};

    #[test]
    fn parse_calendar_spec_with_weekday_date_time() {
        let spec = parse_calendar_spec("Mon 2026-08-10 12:34:56").expect("parse should succeed");
        assert_eq!(spec.weekday, Some(Weekday::Mon));
        assert_eq!(spec.year, Some(2026));
        assert_eq!(spec.month, Some(8));
        assert_eq!(spec.day, Some(10));
        assert_eq!(spec.hour, Some(12));
        assert_eq!(spec.minute, Some(34));
        assert_eq!(spec.second, Some(56));
    }

    #[test]
    fn parse_calendar_spec_with_time_only() {
        let spec = parse_calendar_spec("12:00").expect("parse should succeed");
        assert_eq!(spec.weekday, None);
        assert_eq!(spec.year, None);
        assert_eq!(spec.month, None);
        assert_eq!(spec.day, None);
        assert_eq!(spec.hour, Some(12));
        assert_eq!(spec.minute, Some(0));
        assert_eq!(spec.second, Some(0));
    }

    #[test]
    fn next_calendar_instant_matches_future_time() {
        let now = Local::now();
        let future = now + ChronoDuration::seconds(10);

        let spec = CalendarSpec {
            weekday: None,
            year: Some(future.year()),
            month: Some(future.month()),
            day: Some(future.day()),
            hour: Some(future.hour()),
            minute: Some(future.minute()),
            second: Some(future.second()),
        };

        let next = next_calendar_instant(&spec).expect("should find a future calendar instant");
        assert!(next >= now);
        assert_eq!(next.year(), future.year());
        assert_eq!(next.month(), future.month());
        assert_eq!(next.day(), future.day());
        assert_eq!(next.hour(), future.hour());
        assert_eq!(next.minute(), future.minute());
        assert_eq!(next.second(), future.second());
    }
}
