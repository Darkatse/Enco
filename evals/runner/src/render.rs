use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use std::collections::BTreeMap;

pub(crate) struct Renderer {
    pub(crate) start: DateTime<Utc>,
    pub(crate) timezone: Tz,
    pub(crate) mocks: BTreeMap<String, String>,
}

impl Renderer {
    pub(crate) fn for_trial(task: &crate::task::Task) -> Result<Self> {
        let start = Utc::now()
            .with_second(0)
            .and_then(|time| time.with_nanosecond(0))
            .context("trial start time")?;
        Ok(Self {
            start,
            timezone: task.definition.environment.timezone,
            mocks: task.mock_urls(),
        })
    }

    pub(crate) fn text(&self, input: &str) -> Result<String> {
        let mut output = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(begin) = rest.find("{{") {
            output.push_str(&rest[..begin]);
            let tail = &rest[begin + 2..];
            let end = tail.find("}}").context("unclosed placeholder")?;
            output.push_str(&self.value(&tail[..end])?);
            rest = &tail[end + 2..];
        }
        output.push_str(rest);
        Ok(output)
    }

    fn value(&self, expression: &str) -> Result<String> {
        let (kind, argument) = expression
            .split_once(':')
            .context("placeholder needs a kind and value")?;
        if kind == "mock" {
            return self
                .mocks
                .get(argument)
                .cloned()
                .with_context(|| format!("unknown mock {argument}"));
        }
        ensure!(
            kind == "time" || kind == "date",
            "unknown placeholder {expression}"
        );
        let (offset, format) = argument.split_once('|').unwrap_or((argument, ""));
        let time = shift(self.start.with_timezone(&self.timezone), offset)?;
        match (kind, format) {
            ("time", "") => Ok(time.to_rfc3339()),
            ("date", "") => Ok(time.format("%Y-%m-%d").to_string()),
            ("date", "en") => Ok(time.format("%B %-d, %Y").to_string()),
            ("date", "zh") => {
                let day = ["一", "二", "三", "四", "五", "六", "日"]
                    [time.weekday().num_days_from_monday() as usize];
                Ok(format!("{} 月 {} 日（周{day}）", time.month(), time.day()))
            }
            _ => bail!("unknown placeholder format {expression}"),
        }
    }
}

fn shift(mut time: DateTime<Tz>, offset: &str) -> Result<DateTime<Tz>> {
    if offset == "0" {
        return Ok(time);
    }
    let sign = match offset.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => bail!("offset must start with + or -: {offset}"),
    };
    let mut rest = &offset[1..];
    ensure!(!rest.is_empty(), "offset has no units: {offset}");
    while !rest.is_empty() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        ensure!(digits > 0 && digits < rest.len(), "invalid offset {offset}");
        let count: i64 = rest[..digits].parse().context("offset magnitude")?;
        let amount = count.checked_mul(sign).context("offset overflow")?;
        let unit = rest[digits..].chars().next().context("offset unit")?;
        rest = &rest[digits + unit.len_utf8()..];
        if unit == 'b' {
            ensure!(
                rest.is_empty() && digits + 2 == offset.len(),
                "business days cannot be combined with other units"
            );
            let mut local = time.naive_local();
            let mut remaining = count;
            while remaining > 0 {
                local = local
                    .checked_add_signed(Duration::days(sign))
                    .context("date overflow")?;
                if !matches!(local.weekday(), Weekday::Sat | Weekday::Sun) {
                    remaining -= 1;
                }
            }
            if count > 0 {
                time = local_time(time.timezone(), local)?;
            }
        } else {
            let seconds = match unit {
                'd' => 86400,
                'h' => 3600,
                'm' => 60,
                's' => 1,
                _ => bail!("unknown offset unit in {offset}"),
            };
            let delta = amount
                .checked_mul(seconds)
                .and_then(Duration::try_seconds)
                .context("offset overflow")?;
            if unit == 'd' && amount != 0 {
                let local = time
                    .naive_local()
                    .checked_add_signed(delta)
                    .context("date overflow")?;
                time = local_time(time.timezone(), local)?;
            } else {
                time = time.checked_add_signed(delta).context("date overflow")?;
            }
        }
    }
    Ok(time)
}

fn local_time(timezone: Tz, local: chrono::NaiveDateTime) -> Result<DateTime<Tz>> {
    timezone
        .from_local_datetime(&local)
        .earliest()
        .or_else(|| chrono_tz::GapInfo::new(&local, &timezone)?.end)
        .context("date outside the timezone range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_and_calendar_days_resolve_dst_transitions() {
        for (start, offset, expected) in [
            ("2026-03-06T14:00:00Z", "+1b", "2026-03-09T09:00:00-04:00"),
            ("2026-03-08T06:30:00Z", "+1h", "2026-03-08T03:30:00-04:00"),
            ("2026-03-07T07:30:00Z", "+1d", "2026-03-08T03:00:00-04:00"),
            ("2026-11-01T06:30:00Z", "0", "2026-11-01T01:30:00-05:00"),
            ("2026-10-31T05:30:00Z", "+1d", "2026-11-01T01:30:00-04:00"),
        ] {
            let render = Renderer {
                start: start.parse().unwrap(),
                timezone: chrono_tz::America::New_York,
                mocks: BTreeMap::new(),
            };
            assert_eq!(
                render.text(&format!("{{{{time:{offset}}}}}")).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn unknown_units_are_task_errors() {
        let render = Renderer {
            start: Utc::now(),
            timezone: chrono_tz::America::New_York,
            mocks: BTreeMap::new(),
        };
        assert!(render.text("{{date:+1天}}").is_err());
    }
}
