use std::sync::Arc;

use anyhow::{bail, Result};
use proto::methods::*;
use proto::types::{TaskAssignmentStatus, TaskAssignmentType, TaskStatus};
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(
    client: Arc<Client>,
    actor_id: String,
    source_event_id: String,
    title: Option<String>,
    description: String,
    owner: Option<String>,
    status: Option<String>,
) -> Result<()> {
    let status = parse_task_status_opt(status)?;
    let res: TaskCreateResult = client
        .call(
            method::TASK_CREATE,
            json!({
                "sourceEventId": source_event_id,
                "title": title,
                "description": description,
                "requesterActorId": actor_id,
                "ownerActorId": owner,
                "status": status,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "task #{} {} ({:?})",
            res.task.number, res.task.id, res.task.status
        );
    }
    Ok(())
}

pub async fn list(
    client: Arc<Client>,
    channel_id: Option<String>,
    source_event_id: Option<String>,
    owner: Option<String>,
    statuses: Vec<String>,
) -> Result<()> {
    let statuses = statuses
        .into_iter()
        .map(parse_task_status)
        .collect::<Result<Vec<_>>>()?;
    let res: TaskListResult = client
        .call(
            method::TASK_LIST,
            json!({
                "channelId": channel_id,
                "sourceEventId": source_event_id,
                "ownerActorId": owner,
                "statuses": statuses,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.tasks.is_empty() {
        println!("(no tasks)");
    } else {
        for task in res.tasks {
            println!(
                "#{:<4} {:<16} {:<14?} {:<20} {}",
                task.number,
                task.id,
                task.status,
                task.owner_actor_id.as_deref().unwrap_or("-"),
                task.title
            );
        }
    }
    Ok(())
}

pub async fn show(client: Arc<Client>, task_id: String) -> Result<()> {
    let res: TaskGetResult = client
        .call(method::TASK_GET, json!({ "taskId": task_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        let task = res.task;
        println!("task #{} {}", task.number, task.id);
        println!("title       {}", task.title);
        println!("status      {:?}", task.status);
        println!("channel     {}", task.channel_id);
        println!("source      {}", task.source_event_id);
        println!("thread      {}", task.canonical_thread_id);
        println!("requester   {}", task.requester_actor_id);
        println!(
            "owner       {}",
            task.owner_actor_id.as_deref().unwrap_or("-")
        );
        if !task.result_summary.trim().is_empty() {
            println!("result      {}", task.result_summary);
        }
        if !task.artifact_ids.is_empty() {
            println!("artifacts   {}", task.artifact_ids.join(", "));
        }
        if !res.assignments.is_empty() {
            println!("assignments");
            for assignment in res.assignments {
                println!(
                    "  {} {:?} {:?} -> {}",
                    assignment.id,
                    assignment.assignment_type,
                    assignment.status,
                    assignment.to_actor_id
                );
                if !assignment.result_summary.trim().is_empty() {
                    println!("    result {}", assignment.result_summary);
                }
            }
        }
    }
    Ok(())
}

pub async fn update(
    client: Arc<Client>,
    task_id: String,
    status: Option<String>,
    owner: Option<String>,
    result: Option<String>,
    artifact_ids: Vec<String>,
) -> Result<()> {
    let status = parse_task_status_opt(status)?;
    let res: TaskUpdateResult = client
        .call(
            method::TASK_UPDATE,
            json!({
                "taskId": task_id,
                "status": status,
                "ownerActorId": owner,
                "resultSummary": result,
                "appendArtifactIds": artifact_ids,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "task #{} {} ({:?})",
            res.task.number, res.task.id, res.task.status
        );
    }
    Ok(())
}

pub async fn assign(
    client: Arc<Client>,
    actor_id: String,
    task_id: String,
    to: String,
    assignment_type: String,
    instruction: String,
) -> Result<()> {
    let assignment_type = parse_assignment_type(&assignment_type)?;
    let res: TaskAssignmentCreateResult = client
        .call(
            method::TASK_ASSIGNMENT_CREATE,
            json!({
                "taskId": task_id,
                "fromActorId": actor_id,
                "toActorId": to,
                "type": assignment_type,
                "instruction": instruction,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "assignment {} -> {} (event {})",
            res.assignment.id, res.assignment.to_actor_id, res.event.id
        );
    }
    Ok(())
}

pub async fn assignment_update(
    client: Arc<Client>,
    assignment_id: String,
    status: Option<String>,
    result_event: Option<String>,
    result: Option<String>,
) -> Result<()> {
    let status = parse_assignment_status_opt(status)?;
    let res: TaskAssignmentUpdateResult = client
        .call(
            method::TASK_ASSIGNMENT_UPDATE,
            json!({
                "assignmentId": assignment_id,
                "status": status,
                "resultEventId": result_event,
                "resultSummary": result,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "assignment {} ({:?})",
            res.assignment.id, res.assignment.status
        );
    }
    Ok(())
}

fn parse_task_status_opt(value: Option<String>) -> Result<Option<TaskStatus>> {
    value.map(|v| parse_task_status(&v)).transpose()
}

fn parse_assignment_status_opt(value: Option<String>) -> Result<Option<TaskAssignmentStatus>> {
    value.map(|v| parse_assignment_status(&v)).transpose()
}

fn parse_task_status(value: impl AsRef<str>) -> Result<TaskStatus> {
    match normalize(value.as_ref()).as_str() {
        "todo" => Ok(TaskStatus::Todo),
        "claimed" => Ok(TaskStatus::Claimed),
        "inprogress" => Ok(TaskStatus::InProgress),
        "waitingreview" => Ok(TaskStatus::WaitingReview),
        "done" => Ok(TaskStatus::Done),
        "failed" => Ok(TaskStatus::Failed),
        "canceled" | "cancelled" => Ok(TaskStatus::Canceled),
        other => bail!("unknown task status `{other}`"),
    }
}

fn parse_assignment_type(value: &str) -> Result<TaskAssignmentType> {
    match normalize(value).as_str() {
        "generate" => Ok(TaskAssignmentType::Generate),
        "review" => Ok(TaskAssignmentType::Review),
        "investigate" => Ok(TaskAssignmentType::Investigate),
        "fix" => Ok(TaskAssignmentType::Fix),
        "verify" => Ok(TaskAssignmentType::Verify),
        "other" => Ok(TaskAssignmentType::Other),
        other => bail!("unknown assignment type `{other}`"),
    }
}

fn parse_assignment_status(value: &str) -> Result<TaskAssignmentStatus> {
    match normalize(value).as_str() {
        "pending" => Ok(TaskAssignmentStatus::Pending),
        "running" => Ok(TaskAssignmentStatus::Running),
        "completed" => Ok(TaskAssignmentStatus::Completed),
        "failed" => Ok(TaskAssignmentStatus::Failed),
        "canceled" | "cancelled" => Ok(TaskAssignmentStatus::Canceled),
        other => bail!("unknown assignment status `{other}`"),
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', '-'], "")
}
