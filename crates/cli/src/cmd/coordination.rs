use std::sync::Arc;

use anyhow::{Context, Result};
use proto::methods::*;
use proto::types::{CoordinationDecisionRule, CoordinationMode, CoordinationStepType};
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

pub async fn propose(
    client: Arc<Client>,
    target: String,
    mode: String,
    decision_rule: String,
    participants: Vec<String>,
    plan_json: Option<String>,
) -> Result<()> {
    let plan = parse_json_or_null(plan_json, "--plan-json")?;
    let res: CoordinationProposeResult = client
        .call(
            method::COORDINATION_PROPOSE,
            json!({
                "target": target,
                "mode": parse_mode(&mode)?,
                "decisionRule": parse_decision_rule(&decision_rule)?,
                "participants": participants,
                "plan": plan,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("coordination {}\t{:?}", res.session.id, res.session.status);
    }
    Ok(())
}

pub async fn commit(client: Arc<Client>, session_id: String) -> Result<()> {
    let res: CoordinationCommitResult = client
        .call(
            method::COORDINATION_COMMIT,
            json!({ "sessionId": session_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("coordination {}\t{:?}", res.session.id, res.session.status);
    }
    Ok(())
}

pub async fn respond(
    client: Arc<Client>,
    session_id: String,
    accept: bool,
    reason: Option<String>,
) -> Result<()> {
    let res: CoordinationRespondResult = client
        .call(
            method::COORDINATION_RESPOND,
            json!({
                "sessionId": session_id,
                "accept": accept,
                "reason": reason.unwrap_or_default(),
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("coordination {}\t{:?}", res.session.id, res.session.status);
    }
    Ok(())
}

pub async fn step(
    client: Arc<Client>,
    session_id: String,
    base_revision: u64,
    step_type: String,
    output_json: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let res: CoordinationStepResult = client
        .call(
            method::COORDINATION_STEP,
            json!({
                "sessionId": session_id,
                "baseRevision": base_revision,
                "stepType": parse_step_type(&step_type)?,
                "output": parse_json_or_null(output_json, "--output-json")?,
                "messageBody": message,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "coordination {}\trevision={}",
            res.session.id, res.session.revision
        );
    }
    Ok(())
}

pub async fn skip(
    client: Arc<Client>,
    session_id: String,
    base_revision: u64,
    reason: Option<String>,
) -> Result<()> {
    let res: CoordinationSkipResult = client
        .call(
            method::COORDINATION_SKIP,
            json!({
                "sessionId": session_id,
                "baseRevision": base_revision,
                "reason": reason.unwrap_or_default(),
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "coordination {}\trevision={}",
            res.session.id, res.session.revision
        );
    }
    Ok(())
}

pub async fn reassign(
    client: Arc<Client>,
    session_id: String,
    from_actor_id: String,
    to_actor_id: String,
    base_revision: u64,
) -> Result<()> {
    let res: CoordinationReassignResult = client
        .call(
            method::COORDINATION_REASSIGN,
            json!({
                "sessionId": session_id,
                "fromActorId": from_actor_id,
                "toActorId": to_actor_id,
                "baseRevision": base_revision,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "coordination {}\trevision={}",
            res.session.id, res.session.revision
        );
    }
    Ok(())
}

fn parse_mode(raw: &str) -> Result<CoordinationMode> {
    serde_json::from_value(json!(raw.trim())).context("invalid --mode")
}

fn parse_decision_rule(raw: &str) -> Result<CoordinationDecisionRule> {
    serde_json::from_value(json!(raw.trim())).context("invalid --decision-rule")
}

fn parse_step_type(raw: &str) -> Result<CoordinationStepType> {
    serde_json::from_value(json!(raw.trim())).context("invalid --step-type")
}

fn parse_json_or_null(raw: Option<String>, label: &str) -> Result<Value> {
    raw.map(|value| serde_json::from_str(&value).with_context(|| format!("parse {label}")))
        .transpose()
        .map(|value| value.unwrap_or(Value::Null))
}
