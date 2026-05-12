use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use proto::methods::*;
use proto::types::ReminderStatus;
use serde_json::json;

use crate::client::Client;
use crate::render;

use super::target::{resolve_target, TargetMode};

pub async fn schedule(
    client: Arc<Client>,
    actor_id: String,
    title: String,
    target: Option<String>,
    msg_id: Option<String>,
    delay_seconds: Option<i64>,
    fire_at: Option<String>,
    repeat: Option<String>,
) -> Result<()> {
    let scope = match target {
        Some(target) => Some(
            resolve_target(&client, &actor_id, &target, TargetMode::Write)
                .await?
                .scope,
        ),
        None => None,
    };
    let fire_at = fire_at.map(parse_time).transpose()?;
    let res: ReminderScheduleResult = client
        .call(
            method::REMINDER_SCHEDULE,
            json!({
                "actorId": actor_id,
                "title": title,
                "scope": scope,
                "msgId": msg_id,
                "delaySeconds": delay_seconds,
                "fireAt": fire_at,
                "repeat": repeat,
            }),
        )
        .await?;
    print_reminder_result(&res.reminder);
    Ok(())
}

pub async fn list(
    client: Arc<Client>,
    actor_id: String,
    status: Vec<String>,
    all: bool,
) -> Result<()> {
    let statuses: Vec<ReminderStatus> = status
        .into_iter()
        .map(|s| parse_status(&s))
        .collect::<Result<Vec<_>>>()?;
    let res: ReminderListResult = client
        .call(
            method::REMINDER_LIST,
            json!({
                "actorId": actor_id,
                "statuses": statuses,
                "all": all,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.reminders.is_empty() {
        println!("(no reminders)");
    } else {
        for r in &res.reminders {
            println!("{}\t{:?}\t{}\t{}", r.id, r.status, r.fire_at, r.title);
        }
    }
    Ok(())
}

pub async fn cancel(client: Arc<Client>, actor_id: String, id: String) -> Result<()> {
    let res: ReminderCancelResult = client
        .call(
            method::REMINDER_CANCEL,
            json!({ "actorId": actor_id, "id": id }),
        )
        .await?;
    print_reminder_result(&res.reminder);
    Ok(())
}

pub async fn snooze(client: Arc<Client>, actor_id: String, id: String, by: String) -> Result<()> {
    let seconds = parse_duration_seconds(&by)?;
    let res: ReminderSnoozeResult = client
        .call(
            method::REMINDER_SNOOZE,
            json!({ "actorId": actor_id, "id": id, "bySeconds": seconds }),
        )
        .await?;
    print_reminder_result(&res.reminder);
    Ok(())
}

pub async fn update(
    client: Arc<Client>,
    actor_id: String,
    id: String,
    title: Option<String>,
    in_after: Option<String>,
    fire_at: Option<String>,
    repeat: Option<String>,
) -> Result<()> {
    let delay_seconds = in_after.map(|s| parse_duration_seconds(&s)).transpose()?;
    let fire_at = fire_at.map(parse_time).transpose()?;
    let res: ReminderUpdateResult = client
        .call(
            method::REMINDER_UPDATE,
            json!({
                "actorId": actor_id,
                "id": id,
                "title": title,
                "delaySeconds": delay_seconds,
                "fireAt": fire_at,
                "repeat": repeat,
            }),
        )
        .await?;
    print_reminder_result(&res.reminder);
    Ok(())
}

fn print_reminder_result(reminder: &proto::types::Reminder) {
    if render::is_json() {
        render::print_json(reminder);
    } else {
        println!(
            "{}\t{:?}\t{}\t{}",
            reminder.id, reminder.status, reminder.fire_at, reminder.title
        );
    }
}

fn parse_time(raw: String) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(&raw)
        .map_err(|e| anyhow!("invalid RFC3339 time `{}`: {}", raw, e))?
        .with_timezone(&Utc))
}

fn parse_status(raw: &str) -> Result<ReminderStatus> {
    match raw.to_ascii_lowercase().as_str() {
        "scheduled" => Ok(ReminderStatus::Scheduled),
        "fired" => Ok(ReminderStatus::Fired),
        "cancelled" | "canceled" => Ok(ReminderStatus::Cancelled),
        other => bail!("unsupported reminder status `{}`", other),
    }
}

fn parse_duration_seconds(raw: &str) -> Result<i64> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("duration is empty");
    }
    let (num, unit) = raw.split_at(raw.len().saturating_sub(1));
    let value: i64 = num.parse()?;
    let seconds = match unit {
        "s" => value,
        "m" => value * 60,
        "h" => value * 60 * 60,
        "d" => value * 60 * 60 * 24,
        _ => raw.parse()?,
    };
    if seconds <= 0 {
        bail!("duration must be positive");
    }
    Ok(seconds)
}
