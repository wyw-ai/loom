use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use proto::methods::*;
use proto::types::{
    TaskArtifactLinkStatus, TaskAssignmentStatus, TaskAssignmentType, TaskChangeAckDisposition,
    TaskFactStatus, TaskFactType, TaskProjectionHealth, TaskRefConfidence, TaskRefStatus,
    TaskSnapshotCompleteness, TaskStatus, WorkspaceLeaseMode,
};
use serde_json::{json, Value};

use crate::client::Client;
use crate::{cmd::run, render};

pub async fn create(
    client: Arc<Client>,
    actor_id: String,
    source_message_id: String,
    title: Option<String>,
    description: String,
    owner: Option<String>,
    status: Option<String>,
    parent_source_message: Option<String>,
    parent_task: Option<String>,
    practice_contract_epoch: Option<String>,
) -> Result<()> {
    let status = parse_task_status_opt(status)?;
    let res: TaskCreateResult = client
        .call(
            method::TASK_CREATE,
            json!({
                "sourceMessageId": source_message_id,
                "title": title,
                "description": description,
                "requesterActorId": actor_id,
                "ownerActorId": owner,
                "status": status,
                "parentSourceMessageId": parent_source_message,
                "parentTaskId": parent_task,
                "practiceContractEpoch": practice_contract_epoch,
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
    source_message_id: Option<String>,
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
                "sourceMessageId": source_message_id,
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
        println!("source      {}", task.source_message_id);
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
    let artifact_ids = expand_cli_values(artifact_ids);
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

pub async fn claim(
    client: Arc<Client>,
    task_id: Option<String>,
    source_message: Option<String>,
    actor: Option<String>,
) -> Result<()> {
    if task_id.is_some() == source_message.is_some() {
        bail!("pass exactly one of <task_id> or --source-message");
    }
    let source_message_for_guard = source_message.clone();
    let explicit_actor = actor.is_some();
    let result: Result<TaskUpdateResult> = client
        .call(
            method::TASK_CLAIM,
            json!({
                "taskId": task_id,
                "sourceMessageId": source_message,
                "actorId": actor,
            }),
        )
        .await;
    let res = match result {
        Ok(res) => res,
        Err(error) => {
            let current_actor = std::env::var("LOOM_ACTOR").ok();
            let current_trigger_claim = is_current_trigger_source_claim(
                source_message_for_guard.as_deref(),
                explicit_actor,
                std::env::var("LOOM_RUN_ID").ok().as_deref(),
                std::env::var("LOOM_TRIGGER_MESSAGE_ID").ok().as_deref(),
            );
            let source_task_blocks_claim = if current_trigger_claim {
                match (
                    source_message_for_guard.as_deref(),
                    current_actor.as_deref(),
                ) {
                    (Some(source_message), Some(current_actor)) => {
                        failed_source_claim_has_converged_task(
                            client.as_ref(),
                            source_message,
                            current_actor,
                        )
                        .await
                    }
                    _ => false,
                }
            } else {
                false
            };
            if source_task_blocks_claim {
                let run_id = std::env::var("LOOM_RUN_ID")
                    .expect("a matching active run was checked before sealing the claim");
                let payload = json!({
                    "noReply": true,
                    "replyMode": "none",
                    "reason": "task_claim_conflict_for_current_trigger",
                    "triggerSourceId": source_message_for_guard,
                });
                match run::mark_local_no_reply(&run_id, &payload) {
                    Ok(true) => {
                        bail!(
                            "{error}. This initiating run lost the atomic task claim and has been \
                             marked no-reply automatically. Do not repurpose it using newer \
                             conversation state or bypass the guard; only a separately routed \
                             delivery may produce the requested response."
                        );
                    }
                    Ok(false) => {
                        bail!(
                            "{error}. This initiating run lost the atomic task claim. End it with \
                             `loom --json run ignore --reason task_claim_conflict`; do not \
                             repurpose it using newer conversation state."
                        );
                    }
                    Err(mark_error) => {
                        bail!(
                            "{error}. This initiating run lost the atomic task claim, and its \
                             local no-reply guard could not be recorded: {mark_error}. Do not \
                             publish from this run; end it with `loom --json run ignore`."
                        );
                    }
                }
            }
            return Err(error);
        }
    };
    print_task_update(res);
    Ok(())
}

fn is_current_trigger_source_claim(
    source_message: Option<&str>,
    explicit_actor: bool,
    run_id: Option<&str>,
    trigger_message_id: Option<&str>,
) -> bool {
    let source_message = source_message
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let run_id = run_id.map(str::trim).filter(|value| !value.is_empty());
    let trigger_message_id = trigger_message_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    !explicit_actor
        && run_id.is_some()
        && source_message.is_some()
        && source_message == trigger_message_id
}

async fn failed_source_claim_has_converged_task(
    client: &Client,
    source_message: &str,
    current_actor: &str,
) -> bool {
    let result: Result<TaskListResult> = client
        .call(
            method::TASK_LIST,
            json!({
                "sourceMessageId": source_message,
                "statuses": [],
            }),
        )
        .await;
    result.is_ok_and(|result| {
        result.tasks.into_iter().any(|task| {
            task.source_message_id == source_message
                && task_blocks_current_trigger_claim(
                    task.status,
                    task.owner_actor_id.as_deref(),
                    current_actor,
                )
        })
    })
}

fn task_blocks_current_trigger_claim(
    status: TaskStatus,
    owner_actor_id: Option<&str>,
    current_actor: &str,
) -> bool {
    matches!(
        status,
        TaskStatus::Done | TaskStatus::Failed | TaskStatus::Canceled
    ) || owner_actor_id.is_some_and(|owner| owner != current_actor)
}

pub async fn complete(
    client: Arc<Client>,
    task_id: String,
    result: Option<String>,
    artifact_ids: Vec<String>,
) -> Result<()> {
    let artifact_ids = expand_cli_values(artifact_ids);
    let res: TaskUpdateResult = client
        .call(
            method::TASK_COMPLETE,
            json!({
                "taskId": task_id,
                "resultSummary": result,
                "artifactIds": artifact_ids,
            }),
        )
        .await?;
    print_task_update(res);
    Ok(())
}

pub async fn reopen(client: Arc<Client>, task_id: String, owner: Option<String>) -> Result<()> {
    let res: TaskUpdateResult = client
        .call(
            method::TASK_REOPEN,
            json!({ "taskId": task_id, "ownerActorId": owner }),
        )
        .await?;
    print_task_update(res);
    Ok(())
}

pub async fn cancel(client: Arc<Client>, task_id: String, result: Option<String>) -> Result<()> {
    let res: TaskUpdateResult = client
        .call(
            method::TASK_CANCEL,
            json!({ "taskId": task_id, "resultSummary": result }),
        )
        .await?;
    print_task_update(res);
    Ok(())
}

fn print_task_update(res: TaskUpdateResult) {
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "task #{} {} ({:?})",
            res.task.number, res.task.id, res.task.status
        );
    }
}

pub async fn assign(
    client: Arc<Client>,
    actor_id: String,
    task_id: String,
    to: String,
    assignment_type: String,
    instruction: String,
    contract_json: Option<String>,
    contract_file: Option<PathBuf>,
    idempotency_key: Option<String>,
) -> Result<()> {
    let assignment_type = parse_assignment_type(&assignment_type)?;
    let contract = json_from_inline_or_file(contract_json, contract_file)?;
    let res: TaskAssignmentCreateResult = client
        .call(
            method::TASK_ASSIGNMENT_CREATE,
            json!({
                "taskId": task_id,
                "fromActorId": actor_id,
                "toActorId": to,
                "type": assignment_type,
                "instruction": instruction,
                "contract": contract,
                "idempotencyKey": idempotency_key,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "assignment {} -> {} (message {})",
            res.assignment.id, res.assignment.to_actor_id, res.message.id
        );
    }
    Ok(())
}

pub async fn assignment_update(
    client: Arc<Client>,
    assignment_id: String,
    status: Option<String>,
    result_message: Option<String>,
    result: Option<String>,
    result_envelope_json: Option<String>,
    result_artifact_ids: Vec<String>,
    result_fact_ids: Vec<String>,
    evidence_refs: Vec<String>,
) -> Result<()> {
    let status = parse_assignment_status_opt(status)?;
    let result_artifact_ids = expand_cli_values(result_artifact_ids);
    let result_fact_ids = expand_cli_values(result_fact_ids);
    let evidence_refs = expand_cli_values(evidence_refs);
    let mut result_envelope = json_value_opt(result_envelope_json)?;
    if result_envelope.is_none()
        && status == Some(TaskAssignmentStatus::Completed)
        && (!result_artifact_ids.is_empty()
            || !result_fact_ids.is_empty()
            || !evidence_refs.is_empty())
    {
        result_envelope = Some(json!({
            "assignmentId": assignment_id,
            "status": "completed",
            "summary": result.clone().unwrap_or_default(),
            "resultArtifacts": result_artifact_ids.clone(),
            "resultFacts": result_fact_ids.clone(),
            "evidenceRefs": evidence_refs.clone(),
        }));
    }
    let res: TaskAssignmentUpdateResult = client
        .call(
            method::TASK_ASSIGNMENT_UPDATE,
            json!({
                "assignmentId": assignment_id,
                "status": status,
                "resultMessageId": result_message,
                "resultSummary": result,
                "resultEnvelope": result_envelope,
                "resultArtifactIds": result_artifact_ids,
                "resultFactIds": result_fact_ids,
                "evidenceRefs": evidence_refs,
            }),
        )
        .await?;
    mark_terminal_assignment_handoff(&res);
    if render::is_json() {
        render::print_json(&res);
    } else if is_terminal_assignment_status(res.assignment.status) {
        println!(
            "assignment {} ({:?}); task returned to {} automatically",
            res.assignment.id, res.assignment.status, res.assignment.from_actor_id
        );
    } else {
        println!(
            "assignment {} ({:?})",
            res.assignment.id, res.assignment.status
        );
    }
    Ok(())
}

fn mark_terminal_assignment_handoff(result: &TaskAssignmentUpdateResult) {
    if !is_terminal_assignment_status(result.assignment.status) {
        return;
    }
    let active_assignment_id = std::env::var("LOOM_ASSIGNMENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if active_assignment_id.as_deref() != Some(result.assignment.id.as_str()) {
        return;
    }
    let Some(run_id) = std::env::var("LOOM_RUN_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let payload = json!({
        "noReply": true,
        "replyMode": "none",
        "reason": "assignment_terminal_auto_return",
        "assignmentId": result.assignment.id,
        "assignmentStatus": result.assignment.status,
        "taskId": result.task.id,
        "returnedTo": result.assignment.from_actor_id,
    });
    match run::mark_local_no_reply(&run_id, &payload) {
        Ok(true) => {}
        Ok(false) => eprintln!(
            "loom: assignment {} is terminal and returned to {} automatically; \
             do not send another handoff message",
            result.assignment.id, result.assignment.from_actor_id
        ),
        Err(err) => eprintln!(
            "loom: warning: assignment {} returned automatically, but the local no-reply \
             guard could not be recorded: {err}",
            result.assignment.id
        ),
    }
}

fn is_terminal_assignment_status(status: TaskAssignmentStatus) -> bool {
    matches!(
        status,
        TaskAssignmentStatus::Completed
            | TaskAssignmentStatus::Failed
            | TaskAssignmentStatus::Canceled
    )
}

fn expand_cli_values(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        for part in value.split(',') {
            let trimmed = part.trim();
            if !trimmed.is_empty() && !out.iter().any(|existing| existing == trimmed) {
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

pub async fn ref_attach(
    client: Arc<Client>,
    task_id: String,
    kind: String,
    subtype: String,
    value: String,
    normalized: String,
    confidence: String,
    status: String,
    source_message: Option<String>,
    fields_json: Option<String>,
) -> Result<()> {
    let fields = json_value_opt(fields_json)?.unwrap_or_else(|| json!({}));
    let confidence = parse_ref_confidence(&confidence)?;
    let status = parse_ref_status(&status)?;
    let res: TaskRefAttachResult = client
        .call(
            method::TASK_REF_ATTACH,
            json!({
                "taskId": task_id,
                "kind": kind,
                "subtype": subtype,
                "value": value,
                "normalized": normalized,
                "confidence": confidence,
                "status": status,
                "sourceMessageId": source_message,
                "fields": fields,
            }),
        )
        .await?;
    print_or_line(&res, || format!("ref {} attached", res.task_ref.id));
    Ok(())
}

pub async fn ref_find(
    client: Arc<Client>,
    kind: String,
    subtype: String,
    normalized: String,
    channel: Option<String>,
    confidence: Option<String>,
    status: Option<String>,
) -> Result<()> {
    let confidence = confidence.map(|v| parse_ref_confidence(&v)).transpose()?;
    let status = status.map(|v| parse_ref_status(&v)).transpose()?;
    let res: TaskRefFindResult = client
        .call(
            method::TASK_REF_FIND,
            json!({
                "channelId": channel,
                "kind": kind,
                "subtype": subtype,
                "normalized": normalized,
                "confidence": confidence,
                "status": status,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for task_ref in res.refs {
            println!(
                "{} {} {} {} -> {} ({:?}/{:?})",
                task_ref.id,
                task_ref.kind,
                task_ref.subtype,
                task_ref.normalized,
                task_ref.task_id,
                task_ref.confidence,
                task_ref.status
            );
        }
    }
    Ok(())
}

pub async fn ref_list(client: Arc<Client>, task_id: String) -> Result<()> {
    let res: TaskRefListResult = client
        .call(method::TASK_REF_LIST, json!({ "taskId": task_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for task_ref in res.refs {
            println!(
                "{} {} {} {} ({:?}/{:?})",
                task_ref.id,
                task_ref.kind,
                task_ref.subtype,
                task_ref.normalized,
                task_ref.confidence,
                task_ref.status
            );
        }
    }
    Ok(())
}

pub async fn artifact_attach(
    client: Arc<Client>,
    task_id: String,
    artifact_id: String,
    schema: String,
    role: String,
    status: String,
    lineage_json: Option<String>,
    binding_json: Option<String>,
) -> Result<()> {
    let status = parse_artifact_link_status(&status)?;
    let lineage = json_value_opt(lineage_json)?.unwrap_or_else(|| json!({}));
    let binding = json_value_opt(binding_json)?.unwrap_or_else(|| json!({}));
    let res: TaskArtifactAttachResult = client
        .call(
            method::TASK_ARTIFACT_ATTACH,
            json!({
                "taskId": task_id,
                "artifactId": artifact_id,
                "schema": schema,
                "role": role,
                "status": status,
                "lineage": lineage,
                "binding": binding,
            }),
        )
        .await?;
    print_or_line(&res, || format!("artifact link {} attached", res.link.id));
    Ok(())
}

pub async fn artifact_activate(
    client: Arc<Client>,
    link_id: String,
    supersede_link_ids: Vec<String>,
) -> Result<()> {
    let res: TaskArtifactActivateResult = client
        .call(
            method::TASK_ARTIFACT_ACTIVATE,
            json!({ "linkId": link_id, "supersedeLinkIds": supersede_link_ids }),
        )
        .await?;
    print_or_line(&res, || format!("artifact link {} active", res.link.id));
    Ok(())
}

pub async fn artifact_list(
    client: Arc<Client>,
    task_id: String,
    status: Option<String>,
) -> Result<()> {
    let status = status.map(|v| parse_artifact_link_status(&v)).transpose()?;
    let res: TaskArtifactListResult = client
        .call(
            method::TASK_ARTIFACT_LIST,
            json!({ "taskId": task_id, "status": status }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for link in res.links {
            println!(
                "{} {} {} {} ({:?})",
                link.id, link.artifact_id, link.schema, link.role, link.status
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn fact_append(
    client: Arc<Client>,
    task_id: String,
    target_key: String,
    kind: String,
    fact_type: String,
    signature: Option<String>,
    status: String,
    replaces: Vec<String>,
    authority: String,
    authority_binding_json: Option<String>,
    source_cursor: Option<String>,
    source_snapshot_id: Option<String>,
    external_updated_at: Option<String>,
    observed_fields: Vec<String>,
    unobserved_fields: Vec<String>,
    unavailable_reason: Option<String>,
    snapshot_completeness: Option<String>,
    producer_id: Option<String>,
    summary: String,
    raw_refs: Vec<String>,
    artifact_id: Option<String>,
    payload_schema: String,
    subject_json: Option<String>,
    payload_json: Option<String>,
) -> Result<()> {
    let fact_type = parse_fact_type(&fact_type)?;
    let status = parse_fact_status(&status)?;
    let authority_binding = json_value_opt(authority_binding_json)?.unwrap_or_else(|| json!({}));
    let subject = json_value_opt(subject_json)?.unwrap_or_else(|| json!({}));
    let payload = json_value_opt(payload_json)?.unwrap_or_else(|| json!({}));
    let snapshot_completeness = snapshot_completeness
        .map(|v| parse_snapshot_completeness(&v))
        .transpose()?;
    let res: TaskFactAppendResult = client
        .call(
            method::TASK_FACT_APPEND,
            json!({
                "taskId": task_id,
                "targetKey": target_key,
                "kind": kind,
                "factType": fact_type,
                "signature": signature,
                "status": status,
                "replaces": replaces,
                "authority": authority,
                "authorityBinding": authority_binding,
                "sourceCursor": source_cursor,
                "sourceSnapshotId": source_snapshot_id,
                "externalUpdatedAt": external_updated_at,
                "observedFields": observed_fields,
                "unobservedFields": unobserved_fields,
                "unavailableReason": unavailable_reason,
                "snapshotCompleteness": snapshot_completeness,
                "producerId": producer_id,
                "summary": summary,
                "rawRefs": raw_refs,
                "artifactId": artifact_id,
                "payloadSchema": payload_schema,
                "subject": subject,
                "payload": payload,
            }),
        )
        .await?;
    print_or_line(&res, || {
        if res.created {
            format!("fact {} appended", res.fact.id)
        } else {
            format!("fact {} already existed", res.fact.id)
        }
    });
    Ok(())
}

pub async fn fact_list(
    client: Arc<Client>,
    task_id: String,
    kind: Option<String>,
    status: Option<String>,
    target_key: Option<String>,
) -> Result<()> {
    let status = status.map(|v| parse_fact_status(&v)).transpose()?;
    let res: TaskFactListResult = client
        .call(
            method::TASK_FACT_LIST,
            json!({
                "taskId": task_id,
                "kind": kind,
                "status": status,
                "targetKey": target_key,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for fact in res.facts {
            println!(
                "{} {} {} ({:?}) {}",
                fact.id, fact.target_key, fact.kind, fact.status, fact.summary
            );
        }
    }
    Ok(())
}

pub async fn projection_put(
    client: Arc<Client>,
    task_id: String,
    projection_type: String,
    health: String,
    producer_id: Option<String>,
    watermark_json: Option<String>,
    payload_schema: String,
    payload_json: Option<String>,
) -> Result<()> {
    let health = parse_projection_health(&health)?;
    let watermark = json_value_opt(watermark_json)?.unwrap_or_else(|| json!({}));
    let payload = json_value_opt(payload_json)?.unwrap_or_else(|| json!({}));
    let res: TaskProjectionPutResult = client
        .call(
            method::TASK_PROJECTION_PUT,
            json!({
                "taskId": task_id,
                "projectionType": projection_type,
                "producerActorId": producer_id,
                "health": health,
                "watermark": watermark,
                "payloadSchema": payload_schema,
                "payload": payload,
            }),
        )
        .await?;
    print_or_line(&res, || {
        format!(
            "projection {} ({:?})",
            res.projection.id, res.projection.health
        )
    });
    Ok(())
}

pub async fn projection_get(
    client: Arc<Client>,
    task_id: String,
    projection_type: String,
) -> Result<()> {
    let res: TaskProjectionGetResult = client
        .call(
            method::TASK_PROJECTION_GET,
            json!({ "taskId": task_id, "projectionType": projection_type }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if let Some(projection) = res.projection {
        println!("projection {} ({:?})", projection.id, projection.health);
    } else {
        println!("projection missing ({:?})", res.health);
    }
    Ok(())
}

pub async fn projection_list(client: Arc<Client>, task_id: String) -> Result<()> {
    let res: TaskProjectionListResult = client
        .call(method::TASK_PROJECTION_LIST, json!({ "taskId": task_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for projection in res.projections {
            println!(
                "{} {} ({:?})",
                projection.id, projection.projection_type, projection.health
            );
        }
    }
    Ok(())
}

pub async fn assignment_context(client: Arc<Client>, assignment_id: String) -> Result<()> {
    let res: TaskAssignmentContextResult = client
        .call(
            method::TASK_ASSIGNMENT_CONTEXT,
            json!({ "assignmentId": assignment_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "assignment {} task {} guards {}",
            res.assignment.id, res.task.id, res.guards
        );
    }
    Ok(())
}

pub async fn assignment_preflight(
    client: Arc<Client>,
    assignment_id: String,
    target_key: String,
    head: String,
    effect: String,
) -> Result<()> {
    let res: TaskAssignmentPreflightResult = client
        .call(
            method::TASK_ASSIGNMENT_PREFLIGHT,
            json!({
                "assignmentId": assignment_id,
                "targetKey": target_key,
                "head": head,
                "effect": effect,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "preflight {} allowed={} {}",
            res.preflight.assignment_id, res.preflight.allowed, res.preflight.reason
        );
    }
    Ok(())
}

pub async fn change_list(
    client: Arc<Client>,
    task_id: Option<String>,
    include_handled: bool,
    after_cursor: Option<u64>,
    limit: Option<usize>,
) -> Result<()> {
    let res: TaskChangeListResult = client
        .call(
            method::TASK_CHANGE_LIST,
            json!({
                "taskId": task_id,
                "includeHandled": include_handled,
                "afterCursor": after_cursor,
                "limit": limit,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for delivery in res.deliveries {
            println!(
                "{} #{} {} {:?}",
                delivery.change.id,
                delivery.change.cursor,
                delivery.change.summary,
                delivery.status
            );
        }
    }
    Ok(())
}

pub async fn change_ack(
    client: Arc<Client>,
    change_id: String,
    disposition: String,
    result_ref_ids: Vec<String>,
    reason: String,
) -> Result<()> {
    let disposition = parse_change_disposition(&disposition)?;
    let res: TaskChangeAckResult = client
        .call(
            method::TASK_CHANGE_ACK,
            json!({
                "changeId": change_id,
                "disposition": disposition,
                "resultRefIds": result_ref_ids,
                "reason": reason,
            }),
        )
        .await?;
    print_or_line(&res, || format!("acked {}", res.delivery.change.id));
    Ok(())
}

pub async fn lease_acquire(
    client: Arc<Client>,
    assignment_id: String,
    resource_key: String,
    mode: String,
    ttl_seconds: Option<i64>,
) -> Result<()> {
    let mode = parse_lease_mode(&mode)?;
    let res: WorkspaceLeaseAcquireResult = client
        .call(
            method::TASK_WORKSPACE_LEASE_ACQUIRE,
            json!({
                "assignmentId": assignment_id,
                "resourceKey": resource_key,
                "mode": mode,
                "ttlSeconds": ttl_seconds,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if let Some(lease) = res.lease {
        println!("lease {} active", lease.id);
    } else {
        println!("lease conflict");
        for conflict in res.conflicts {
            println!(
                "  {} {} holder={}",
                conflict.id, conflict.resource_key, conflict.holder_assignment_id
            );
        }
    }
    Ok(())
}

pub async fn lease_release(client: Arc<Client>, lease_id: String) -> Result<()> {
    let res: WorkspaceLeaseReleaseResult = client
        .call(
            method::TASK_WORKSPACE_LEASE_RELEASE,
            json!({ "leaseId": lease_id }),
        )
        .await?;
    print_or_line(&res, || format!("lease {} released", res.lease.id));
    Ok(())
}

pub async fn lease_list(
    client: Arc<Client>,
    resource_key: Option<String>,
    assignment_id: Option<String>,
    active_only: bool,
) -> Result<()> {
    let res: WorkspaceLeaseListResult = client
        .call(
            method::TASK_WORKSPACE_LEASE_LIST,
            json!({
                "resourceKey": resource_key,
                "assignmentId": assignment_id,
                "activeOnly": active_only,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        for lease in res.leases {
            println!(
                "{} {} {:?} {:?} holder={}",
                lease.id, lease.resource_key, lease.mode, lease.status, lease.holder_assignment_id
            );
        }
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

fn parse_ref_confidence(value: &str) -> Result<TaskRefConfidence> {
    match normalize(value).as_str() {
        "confirmed" => Ok(TaskRefConfidence::Confirmed),
        "inferred" => Ok(TaskRefConfidence::Inferred),
        other => bail!("unknown task ref confidence `{other}`"),
    }
}

fn parse_ref_status(value: &str) -> Result<TaskRefStatus> {
    match normalize(value).as_str() {
        "active" => Ok(TaskRefStatus::Active),
        "superseded" => Ok(TaskRefStatus::Superseded),
        "retired" => Ok(TaskRefStatus::Retired),
        other => bail!("unknown task ref status `{other}`"),
    }
}

fn parse_artifact_link_status(value: &str) -> Result<TaskArtifactLinkStatus> {
    match normalize(value).as_str() {
        "active" => Ok(TaskArtifactLinkStatus::Active),
        "proposal" => Ok(TaskArtifactLinkStatus::Proposal),
        "superseded" => Ok(TaskArtifactLinkStatus::Superseded),
        "rejected" => Ok(TaskArtifactLinkStatus::Rejected),
        other => bail!("unknown artifact link status `{other}`"),
    }
}

fn parse_fact_type(value: &str) -> Result<TaskFactType> {
    match normalize(value).as_str() {
        "observation" => Ok(TaskFactType::Observation),
        "status" => Ok(TaskFactType::Status),
        "decision" => Ok(TaskFactType::Decision),
        "action" => Ok(TaskFactType::Action),
        "userdefined" => Ok(TaskFactType::UserDefined),
        other => bail!("unknown fact type `{other}`"),
    }
}

fn parse_fact_status(value: &str) -> Result<TaskFactStatus> {
    match normalize(value).as_str() {
        "active" => Ok(TaskFactStatus::Active),
        "superseded" => Ok(TaskFactStatus::Superseded),
        "retracted" => Ok(TaskFactStatus::Retracted),
        "conflict" => Ok(TaskFactStatus::Conflict),
        other => bail!("unknown fact status `{other}`"),
    }
}

fn parse_snapshot_completeness(value: &str) -> Result<TaskSnapshotCompleteness> {
    match normalize(value).as_str() {
        "complete" => Ok(TaskSnapshotCompleteness::Complete),
        "partial" => Ok(TaskSnapshotCompleteness::Partial),
        "unknown" => Ok(TaskSnapshotCompleteness::Unknown),
        other => bail!("unknown snapshot completeness `{other}`"),
    }
}

fn parse_projection_health(value: &str) -> Result<TaskProjectionHealth> {
    match normalize(value).as_str() {
        "fresh" => Ok(TaskProjectionHealth::Fresh),
        "stale" => Ok(TaskProjectionHealth::Stale),
        "missing" => Ok(TaskProjectionHealth::Missing),
        "invalid" => Ok(TaskProjectionHealth::Invalid),
        "repairrequired" => Ok(TaskProjectionHealth::RepairRequired),
        other => bail!("unknown projection health `{other}`"),
    }
}

fn parse_change_disposition(value: &str) -> Result<TaskChangeAckDisposition> {
    match normalize(value).as_str() {
        "assignmentcreated" => Ok(TaskChangeAckDisposition::AssignmentCreated),
        "assignmentreused" => Ok(TaskChangeAckDisposition::AssignmentReused),
        "actionrequested" => Ok(TaskChangeAckDisposition::ActionRequested),
        "factwritten" => Ok(TaskChangeAckDisposition::FactWritten),
        "artifactwritten" => Ok(TaskChangeAckDisposition::ArtifactWritten),
        "projectionrepaired" => Ok(TaskChangeAckDisposition::ProjectionRepaired),
        "blocked" => Ok(TaskChangeAckDisposition::Blocked),
        "nooprecorded" => Ok(TaskChangeAckDisposition::NoopRecorded),
        "escalated" => Ok(TaskChangeAckDisposition::Escalated),
        other => bail!("unknown task change disposition `{other}`"),
    }
}

fn parse_lease_mode(value: &str) -> Result<WorkspaceLeaseMode> {
    match normalize(value).as_str() {
        "read" => Ok(WorkspaceLeaseMode::Read),
        "write" => Ok(WorkspaceLeaseMode::Write),
        other => bail!("unknown lease mode `{other}`"),
    }
}

fn json_value_opt(raw: Option<String>) -> Result<Option<Value>> {
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}

fn json_from_inline_or_file(
    inline: Option<String>,
    file: Option<PathBuf>,
) -> Result<Option<Value>> {
    match (inline, file) {
        (Some(_), Some(_)) => bail!("use either --contract-json or --contract-file, not both"),
        (Some(raw), None) => Ok(Some(serde_json::from_str(&raw)?)),
        (None, Some(path)) => Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?)),
        (None, None) => Ok(None),
    }
}

fn print_or_line<T: serde::Serialize>(value: &T, line: impl FnOnce() -> String) {
    if render::is_json() {
        render::print_json(value);
    } else {
        println!("{}", line());
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', '-'], "")
}

#[cfg(test)]
mod tests {
    use super::{
        expand_cli_values, is_current_trigger_source_claim, is_terminal_assignment_status,
        task_blocks_current_trigger_claim,
    };
    use proto::types::{TaskAssignmentStatus, TaskStatus};

    #[test]
    fn expand_cli_values_splits_commas_trims_and_dedupes() {
        assert_eq!(
            expand_cli_values(vec![
                "art_1, art_2".into(),
                "art_2".into(),
                "  ".into(),
                "art_3,,art_1".into(),
            ]),
            vec!["art_1", "art_2", "art_3"]
        );
    }

    #[test]
    fn terminal_assignment_statuses_return_control() {
        assert!(is_terminal_assignment_status(
            TaskAssignmentStatus::Completed
        ));
        assert!(is_terminal_assignment_status(TaskAssignmentStatus::Failed));
        assert!(is_terminal_assignment_status(
            TaskAssignmentStatus::Canceled
        ));
        assert!(!is_terminal_assignment_status(
            TaskAssignmentStatus::Running
        ));
    }

    #[test]
    fn only_current_trigger_source_claim_conflicts_seal_agent_run() {
        assert!(is_current_trigger_source_claim(
            Some("msg_root"),
            false,
            Some("run_1"),
            Some("msg_root")
        ));
        assert!(!is_current_trigger_source_claim(
            Some("msg_other"),
            false,
            Some("run_1"),
            Some("msg_root")
        ));
        assert!(!is_current_trigger_source_claim(
            Some("msg_root"),
            true,
            Some("run_1"),
            Some("msg_root")
        ));
    }

    #[test]
    fn source_task_blocks_only_other_owner_or_terminal_state() {
        assert!(task_blocks_current_trigger_claim(
            TaskStatus::Claimed,
            Some("actor_other"),
            "actor_current"
        ));
        assert!(!task_blocks_current_trigger_claim(
            TaskStatus::Claimed,
            Some("actor_current"),
            "actor_current"
        ));
        assert!(!task_blocks_current_trigger_claim(
            TaskStatus::Todo,
            None,
            "actor_current"
        ));
        assert!(task_blocks_current_trigger_claim(
            TaskStatus::Canceled,
            Some("actor_current"),
            "actor_current"
        ));
    }
}
