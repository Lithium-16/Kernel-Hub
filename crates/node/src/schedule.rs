//! Scheduled actions from `[[schedule]]` in node.toml: "back up at 04:00", "restart Sundays".
//!
//! Times are the node PC's local time. Runs missed while the PC was off or asleep are skipped,
//! not caught up. Each run goes through the same permission checks and activity log as a
//! button press, as an automation.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, TimeDelta, TimeZone, Weekday};
use kernel_protocol::{ActionInvoke, Actor, ActorKind, EventLevel};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::watch;

use crate::node::Node;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfig {
    pub name: String,
    /// "daily 04:00", "sun 03:30", "mon,wed,fri 18:00", "weekdays 07:00", "every 30m", "every 6h".
    pub at: String,
    pub module: String,
    pub action: String,
    #[serde(default)]
    pub params: Map<String, Value>,
    /// Lets actions that normally need your approval run on this schedule (you wrote it
    /// yourself). Actions marked button-only never run on a schedule.
    #[serde(default)]
    pub approved: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq)]
pub enum When {
    /// At `time` on these weekdays (all seven for "daily").
    Weekly {
        days: Vec<Weekday>,
        time: NaiveTime,
    },
    Every(Duration),
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tuesday" => Weekday::Tue,
        "wed" | "wednesday" => Weekday::Wed,
        "thu" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

pub fn parse(at: &str) -> Result<When, String> {
    let at = at.trim().to_lowercase();
    let bad = || {
        format!(
            "'{at}' should look like \"daily 04:00\", \"sun 03:30\", \"mon,fri 18:00\" or \"every 30m\""
        )
    };
    if let Some(rest) = at.strip_prefix("every ") {
        let rest = rest.trim();
        let (n, unit) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?);
        let n: u64 = n.parse().map_err(|_| bad())?;
        let secs = match unit.trim() {
            "m" | "min" | "mins" | "minutes" => n * 60,
            "h" | "hour" | "hours" => n * 3600,
            _ => return Err(bad()),
        };
        if secs < 60 {
            return Err("schedules run at most once a minute".into());
        }
        return Ok(When::Every(Duration::from_secs(secs)));
    }
    // The time is the last word; the days are everything before it ("mon, fri 18:30").
    let (head, rest) = at.rsplit_once(char::is_whitespace).ok_or_else(bad)?;
    let (head, rest) = (head.trim(), rest.trim());
    let time = NaiveTime::parse_from_str(rest, "%H:%M").map_err(|_| bad())?;
    let days = match head {
        "daily" | "everyday" => vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ],
        "weekdays" => vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ],
        "weekends" => vec![Weekday::Sat, Weekday::Sun],
        list => list
            .split(',')
            .map(|d| weekday(d.trim()).ok_or_else(bad))
            .collect::<Result<_, _>>()?,
    };
    Ok(When::Weekly { days, time })
}

impl When {
    /// The first run strictly after `after`. `start` anchors intervals (when the node started).
    pub fn next<Tz: TimeZone>(&self, after: &DateTime<Tz>, start: &DateTime<Tz>) -> DateTime<Tz> {
        match self {
            When::Every(every) => {
                let step = TimeDelta::from_std(*every).expect("interval fits");
                let elapsed = after.clone() - start.clone();
                let n = elapsed.num_seconds().div_euclid(step.num_seconds()) + 1;
                start.clone() + step * n as i32
            }
            When::Weekly { days, time } => {
                let tz = after.timezone();
                let today: NaiveDate = after.date_naive();
                for offset in 0..=8 {
                    let day = today + TimeDelta::days(offset);
                    if !days.contains(&day.weekday()) {
                        continue;
                    }
                    // A time that doesn't exist (clocks going forward) moves to the next day.
                    if let Some(t) = tz.from_local_datetime(&day.and_time(*time)).earliest()
                        && t > *after
                    {
                        return t;
                    }
                }
                after.clone() + TimeDelta::days(7)
            }
        }
    }
}

/// What the Node module shows for each schedule.
#[derive(Debug, Clone, Default)]
pub struct RunState {
    pub next: Option<DateTime<Local>>,
    pub last: Option<DateTime<Local>>,
    pub last_result: Option<String>,
}

pub struct Scheduler {
    pub entries: Vec<(ScheduleConfig, Result<When, String>)>,
    pub state: Mutex<Vec<RunState>>,
}

impl Scheduler {
    pub fn new(configs: &[ScheduleConfig]) -> Self {
        let entries: Vec<_> = configs.iter().map(|c| (c.clone(), parse(&c.at))).collect();
        let state = Mutex::new(vec![RunState::default(); entries.len()]);
        Self { entries, state }
    }

    /// For the Node module's status: one row per schedule.
    pub fn status(&self) -> Value {
        let state = self.state.lock().expect("schedule state");
        let fmt = |t: &Option<DateTime<Local>>| {
            t.map(|t| t.to_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string())
        };
        Value::Array(
            self.entries
                .iter()
                .zip(state.iter())
                .map(|((c, when), s)| {
                    json!({
                        "name": c.name,
                        "at": c.at,
                        "action": format!("{}/{}", c.module, c.action),
                        "next_run": if c.enabled && when.is_ok() { fmt(&s.next) } else { None },
                        "last_run": fmt(&s.last),
                        "last_result": match (&when, c.enabled) {
                            (Err(e), _) => Some(e.clone()),
                            (_, false) => Some("off".into()),
                            _ => s.last_result.clone(),
                        },
                    })
                })
                .collect(),
        )
    }
}

/// Run one schedule until shutdown.
pub async fn run(node: Arc<Node>, index: usize, mut stop: watch::Receiver<bool>) {
    let (cfg, when) = &node.scheduler.entries[index];
    let Ok(when) = when.clone() else { return };
    if !cfg.enabled {
        return;
    }
    if !node.has_action(&cfg.module, &cfg.action) {
        tracing::error!(schedule = %cfg.name, "schedule names an action that doesn't exist");
        node.scheduler.state.lock().expect("schedule state")[index].last_result =
            Some(format!("no action {}/{}", cfg.module, cfg.action));
        return;
    }
    let start = Local::now();
    loop {
        let next = when.next(&Local::now(), &start);
        node.scheduler.state.lock().expect("schedule state")[index].next = Some(next);
        // Wake at least once a minute and check the wall clock, so sleep, hibernation and
        // clock changes don't push a run late.
        loop {
            let left = (next - Local::now()).to_std().unwrap_or_default();
            if left.is_zero() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(left.min(Duration::from_secs(60))) => {}
                _ = stop.changed() => return,
            }
        }
        // Too late (the PC was asleep through it): skip rather than run at a surprising time.
        if Local::now() - next > TimeDelta::minutes(5) {
            continue;
        }
        let req = ActionInvoke {
            module: cfg.module.clone(),
            action: cfg.action.clone(),
            params: cfg.params.clone(),
            actor: Actor {
                kind: ActorKind::Automation,
                reference: Some(format!("schedule: {}", cfg.name)),
            },
            approval_id: None,
        };
        tracing::info!(schedule = %cfg.name, module = %cfg.module, action = %cfg.action, "running schedule");
        let result = node.invoke_scheduled(req, cfg.approved).await;
        let outcome = match &result.error {
            None => "ok".to_string(),
            Some(e) => {
                node.events.emit(
                    crate::builtin::ID,
                    "schedule.failed",
                    EventLevel::Warn,
                    format!("Schedule \"{}\" failed: {}", cfg.name, e.message),
                    json!({"schedule": cfg.name, "code": e.code.as_str()})
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                );
                format!("{}: {}", e.code.as_str(), e.message)
            }
        };
        let mut state = node.scheduler.state.lock().expect("schedule state");
        state[index].last = Some(Local::now());
        state[index].last_result = Some(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn parses_schedules() {
        assert!(
            matches!(parse("daily 04:00"), Ok(When::Weekly { ref days, .. }) if days.len() == 7)
        );
        assert_eq!(
            parse("Mon, Fri 18:30").unwrap(),
            When::Weekly {
                days: vec![Weekday::Mon, Weekday::Fri],
                time: NaiveTime::from_hms_opt(18, 30, 0).unwrap()
            }
        );
        assert_eq!(
            parse("every 30m").unwrap(),
            When::Every(Duration::from_secs(1800))
        );
        assert_eq!(
            parse("every 6 hours").unwrap(),
            When::Every(Duration::from_secs(6 * 3600))
        );
        for bad in [
            "",
            "daily",
            "daily 25:00",
            "someday 04:00",
            "every 0m",
            "every m",
            "every 5s",
        ] {
            assert!(parse(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn finds_the_next_run() {
        // 2026-09-30 is a Wednesday.
        let now = t("2026-09-30T10:00:00+02:00");
        let daily = parse("daily 04:00").unwrap();
        assert_eq!(daily.next(&now, &now), t("2026-10-01T04:00:00+02:00"));
        let later_today = parse("daily 18:00").unwrap();
        assert_eq!(later_today.next(&now, &now), t("2026-09-30T18:00:00+02:00"));
        let sunday = parse("sun 03:30").unwrap();
        assert_eq!(sunday.next(&now, &now), t("2026-10-04T03:30:00+02:00"));
        let wednesday_now = parse("wed 10:00").unwrap();
        assert_eq!(
            wednesday_now.next(&now, &now),
            t("2026-10-07T10:00:00+02:00")
        );
        let every = parse("every 30m").unwrap();
        let start = t("2026-09-30T09:10:00+02:00");
        assert_eq!(every.next(&now, &start), t("2026-09-30T10:10:00+02:00"));
        assert_eq!(every.next(&start, &start), t("2026-09-30T09:40:00+02:00"));
    }

    #[test]
    fn status_rows() {
        let cfg: ScheduleConfig = toml::from_str(
            "name = 'Backup'\nat = 'daily 04:00'\nmodule = 'minecraft'\naction = 'world.backup'",
        )
        .unwrap();
        let bad = ScheduleConfig {
            at: "whenever".into(),
            ..cfg.clone()
        };
        let s = Scheduler::new(&[cfg, bad]);
        let rows = s.status();
        assert_eq!(rows[0]["action"], "minecraft/world.backup");
        assert!(rows[0]["last_result"].is_null());
        assert!(
            rows[1]["last_result"]
                .as_str()
                .unwrap()
                .contains("should look like")
        );
    }
}
