/*
 * Author & Copyright: Ruben Fiszel 2021
 * This file and its contents are licensed under the AGPLv3 License.
 * Please see the included NOTICE for copyright information and
 * LICENSE-AGPL for a copy of the license.
 */

use std::str::FromStr;

use crate::{
    audit::{audit_log, ActionKind},
    db::UserDB,
    error::{self, JsonResult, Result},
    jobs::{self, push, JobPayload},
    users::Authed,
    utils::{get_owner_from_path, Pagination, StripPath},
};
use axum::{
    extract::{Extension, Path, Query},
    routing::{get, post},
    Json, Router,
};

use chrono::{DateTime, Duration, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{FromRow, Postgres, Transaction};

pub fn workspaced_service() -> Router {
    Router::new()
        .route("/list", get(list_schedule))
        .route("/get/*path", get(get_schedule))
        .route("/create", post(create_schedule))
        .route("/update/*path", post(edit_schedule))
        .route("/setenabled/*path", post(set_enabled))
}

pub fn global_service() -> Router {
    Router::new().route("/preview", post(preview_schedule))
}

#[derive(FromRow, Serialize, Deserialize, Debug)]
pub struct Schedule {
    pub workspace_id: String,
    pub path: String,
    pub edited_by: String,
    pub edited_at: DateTime<chrono::Utc>,
    pub schedule: String,
    pub offset_: i32,
    pub enabled: bool,
    pub script_path: String,
    pub is_flow: bool,
    pub args: Option<serde_json::Value>,
    pub extra_perms: serde_json::Value,
}

#[derive(Deserialize)]
pub struct NewSchedule {
    pub path: String,
    pub schedule: String,
    pub offset: i32,
    pub script_path: String,
    pub is_flow: bool,
    pub args: Option<serde_json::Value>,
}

pub async fn push_scheduled_job<'c>(
    mut tx: Transaction<'c, Postgres>,
    schedule: Schedule,
) -> Result<Transaction<'c, Postgres>> {
    let sched = cron::Schedule::from_str(&schedule.schedule)
        .map_err(|e| error::Error::BadRequest(e.to_string()))?;

    let offset = Duration::minutes(schedule.offset_.into());
    let next = sched
        .after(&(chrono::Utc::now() - offset + Duration::seconds(1)))
        .next()
        .expect("a schedule should have a next event")
        + offset;

    let mut args: Option<Map<String, Value>> = None;

    if let Some(args_v) = schedule.args {
        if let Value::Object(args_m) = args_v {
            args = Some(args_m)
        } else {
            return Err(error::Error::ExecutionErr(
                "args of scripts needs to be dict".to_string(),
            ));
        }
    }

    let payload = if schedule.is_flow {
        JobPayload::Flow(schedule.script_path)
    } else {
        JobPayload::ScriptHash {
            hash: jobs::get_latest_hash_for_path(
                &mut tx,
                &schedule.workspace_id,
                &schedule.script_path,
            )
            .await?,
            path: schedule.script_path,
        }
    };
    let (_, tx) = push(
        tx,
        &schedule.workspace_id,
        payload,
        args,
        &schedule_to_user(&schedule.path),
        get_owner_from_path(&schedule.path),
        Some(next),
        Some(schedule.path),
        None,
        false,
    )
    .await?;
    Ok(tx)
}

async fn create_schedule(
    authed: Authed,
    Extension(user_db): Extension<UserDB>,
    Path(w_id): Path<String>,
    Json(ns): Json<NewSchedule>,
) -> Result<String> {
    cron::Schedule::from_str(&ns.schedule).map_err(|e| error::Error::BadRequest(e.to_string()))?;
    let mut tx = user_db.begin(&authed).await?;

    let schedule = sqlx::query_as!(Schedule,
        "INSERT INTO schedule (workspace_id, path, schedule, offset_, edited_by, script_path, is_flow, args) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *",
        w_id,
        ns.path,
        ns.schedule,
        ns.offset,
        &authed.username,
        ns.script_path,
        ns.is_flow,
        ns.args
    )
    .fetch_one(&mut tx)
    .await?;

    audit_log(
        &mut tx,
        &authed.username,
        "schedule.create",
        ActionKind::Create,
        &w_id,
        Some(&ns.path.to_string()),
        Some(
            [
                Some(("schedule", ns.schedule.as_str())),
                Some(("script_path", ns.script_path.as_str())),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
    )
    .await?;

    let tx = push_scheduled_job(tx, schedule).await?;
    tx.commit().await?;
    Ok(ns.path.to_string())
}

#[derive(Deserialize)]
pub struct EditSchedule {
    pub schedule: String,
    pub script_path: String,
    pub is_flow: bool,
    pub args: Option<serde_json::Value>,
}

async fn clear_schedule<'c>(db: &mut Transaction<'c, Postgres>, path: &str) -> Result<()> {
    sqlx::query!("DELETE FROM queue WHERE schedule_path = $1", path)
        .execute(db)
        .await?;
    Ok(())
}

async fn edit_schedule(
    authed: Authed,
    Extension(user_db): Extension<UserDB>,
    Path((w_id, path)): Path<(String, StripPath)>,
    Json(es): Json<EditSchedule>,
) -> Result<String> {
    let path = path.to_path();

    cron::Schedule::from_str(&es.schedule).map_err(|e| error::Error::BadRequest(e.to_string()))?;

    let mut tx = user_db.begin(&authed).await?;

    clear_schedule(&mut tx, path).await?;
    let schedule = sqlx::query_as!(Schedule,
        "UPDATE schedule SET schedule = $1, script_path = $2, is_flow = $3, args = $4 WHERE path = $5 AND workspace_id = $6 RETURNING *",
        es.schedule,
        es.script_path,
        es.is_flow,
        es.args,
        path,
        w_id,
    )
    .fetch_one(&mut tx)
    .await?;

    if schedule.enabled {
        tx = push_scheduled_job(tx, schedule).await?;
    }

    audit_log(
        &mut tx,
        &authed.username,
        "schedule.edit",
        ActionKind::Update,
        &w_id,
        Some(&path.to_string()),
        Some(
            [
                Some(("schedule", es.schedule.as_str())),
                Some(("script_path", es.script_path.as_str())),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
    )
    .await?;

    tx.commit().await?;
    Ok(path.to_string())
}

async fn list_schedule(
    authed: Authed,
    Extension(user_db): Extension<UserDB>,
    Path(w_id): Path<String>,
    Query(pagination): Query<Pagination>,
) -> JsonResult<Vec<Schedule>> {
    let (per_page, offset) = crate::utils::paginate(pagination);
    let mut tx = user_db.begin(&authed).await?;

    let rows = sqlx::query_as!(
        Schedule,
        "SELECT * FROM schedule WHERE workspace_id = $1 ORDER BY edited_at desc LIMIT $2 OFFSET $3",
        w_id,
        per_page as i64,
        offset as i64
    )
    .fetch_all(&mut tx)
    .await?;
    tx.commit().await?;
    Ok(Json(rows))
}

pub async fn get_schedule_opt<'c>(
    db: &mut Transaction<'c, Postgres>,
    w_id: &str,
    path: &str,
) -> Result<Option<Schedule>> {
    let schedule_opt = sqlx::query_as!(
        Schedule,
        "SELECT * FROM schedule WHERE path = $1 AND workspace_id = $2",
        path,
        w_id
    )
    .fetch_optional(db)
    .await?;
    Ok(schedule_opt)
}
async fn get_schedule(
    authed: Authed,
    Extension(user_db): Extension<UserDB>,
    Path((w_id, path)): Path<(String, StripPath)>,
) -> JsonResult<Schedule> {
    let path = path.to_path();
    let mut tx = user_db.begin(&authed).await?;

    let schedule_o = get_schedule_opt(&mut tx, &w_id, path).await?;
    let schedule = crate::utils::not_found_if_none(schedule_o, "Schedule", path)?;
    tx.commit().await?;
    Ok(Json(schedule))
}

#[derive(Deserialize)]
pub struct PreviewPayload {
    pub schedule: String,
    pub offset: Option<i32>,
}

pub async fn preview_schedule(
    Json(PreviewPayload { schedule, offset }): Json<PreviewPayload>,
) -> JsonResult<Vec<DateTime<chrono::Utc>>> {
    let schedule =
        cron::Schedule::from_str(&schedule).map_err(|e| error::Error::BadRequest(e.to_string()))?;
    let upcoming: Vec<DateTime<chrono::Utc>> = schedule
        .upcoming(get_offset(offset))
        .take(10)
        .map(|x| x.into())
        .collect();
    Ok(Json(upcoming))
}

fn get_offset(offset: Option<i32>) -> FixedOffset {
    FixedOffset::west(offset.unwrap_or(0) * 60)
}

#[derive(Deserialize)]
pub struct SetEnabled {
    pub enabled: bool,
}

pub async fn set_enabled(
    authed: Authed,
    Extension(user_db): Extension<UserDB>,
    Path((w_id, path)): Path<(String, StripPath)>,
    Json(SetEnabled { enabled }): Json<SetEnabled>,
) -> Result<String> {
    let path = path.to_path();
    let mut tx = user_db.begin(&authed).await?;

    let schedule_o = sqlx::query_as!(
        Schedule,
        "UPDATE schedule SET enabled = $1 WHERE path = $2 AND workspace_id = $3 RETURNING *",
        enabled,
        path,
        w_id
    )
    .fetch_optional(&mut tx)
    .await?;

    let schedule = crate::utils::not_found_if_none(schedule_o, "Schedule", path)?;

    clear_schedule(&mut tx, path).await?;

    if enabled {
        tx = push_scheduled_job(tx, schedule).await?;
    }
    audit_log(
        &mut tx,
        &authed.username,
        "schedule.setenabled",
        ActionKind::Update,
        &w_id,
        Some(path),
        Some([("enabled", enabled.to_string().as_ref())].into()),
    )
    .await?;
    tx.commit().await?;
    Ok(format!(
        "succesfully updated schedule at path {} to status {}",
        path, enabled
    ))
}

fn schedule_to_user(path: &str) -> String {
    format!("schedule-{}", path.replace('/', "-"))
}
