//! REST API handlers for the embedded web UI server.
//!
//! Each handler opens a fresh storage connection inside
//! `tokio::task::spawn_blocking` because `SqliteStorage` is `!Send`
//! (fsqlite uses `Rc` internally). Results are returned as JSON.

use crate::config;
use crate::error::BeadsError;
use crate::model::{Comment, Dependency, Issue, IssueType, Priority, Status};
use crate::storage::sqlite::{IssueUpdate, ListFilters};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::task::spawn_blocking;

use super::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Helper to run storage operations on a blocking thread.
///
/// Opens a fresh storage connection, runs `op`, returns the JSON response.
async fn with_storage<F, R>(
    state: Arc<AppState>,
    op: F,
) -> impl IntoResponse
where
    F: FnOnce(&mut crate::storage::SqliteStorage) -> Result<R, BeadsError> + Send + 'static,
    R: serde::Serialize + Send + 'static,
{
    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        op(&mut storage_ctx.storage)
    })
    .await
    {
        Ok(Ok(result)) => (StatusCode::OK, Json(json!(result))).into_response(),
        Ok(Err(e)) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Storage error: {e}"),
        )
        .into_response(),
        Err(e) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Task error: {e}"),
        )
        .into_response(),
    }
}

fn issue_to_bead_json(issue: &Issue) -> Value {
    json!({
        "id": issue.id,
        "title": issue.title,
        "description": issue.description.as_deref().unwrap_or(""),
        "notes": issue.notes.as_deref().unwrap_or(""),
        "design": issue.design.as_deref().unwrap_or(""),
        "acceptance_criteria": issue.acceptance_criteria.as_deref().unwrap_or(""),
        "status": issue.status.as_str(),
        "priority": issue.priority.0,
        "issue_type": issue.issue_type.as_str(),
        "assignee": issue.assignee.as_deref().unwrap_or(""),
        "owner": issue.owner,
        "created_by": issue.created_by.as_deref().unwrap_or(""),
        "created_at": issue.created_at,
        "updated_at": issue.updated_at,
        "started_at": issue.started_at,
        "closed_at": issue.closed_at,
        "close_reason": issue.close_reason,
        "labels": &issue.labels,
        "dependencies": issue.dependencies.iter().map(|d| dep_to_json(d)).collect::<Vec<_>>(),
        "comments": issue.comments.iter().map(|c| comment_to_json(c)).collect::<Vec<_>>(),
        "parent": serde_json::Value::Null,
        "await_type": issue.await_type,
    })
}

fn dep_to_json(dep: &Dependency) -> Value {
    json!({
        "issue_id": dep.issue_id,
        "depends_on_id": dep.depends_on_id,
        "type": dep.dep_type,
        "created_by": dep.created_by,
        "created_at": dep.created_at,
    })
}

fn comment_to_json(comment: &Comment) -> Value {
    json!({
        "id": comment.id,
        "issue_id": comment.issue_id,
        "author": comment.author,
        "text": comment.body,
        "created_at": comment.created_at,
    })
}

fn default_list_filters() -> ListFilters {
    ListFilters {
        limit: Some(0),
        offset: Some(0),
        include_closed: true,
        include_deferred: true,
        ..Default::default()
    }
}

fn json_error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message })))
}

fn status_from_str(s: &str) -> Status {
    if s.eq_ignore_ascii_case("open") { Status::Open }
    else if s.eq_ignore_ascii_case("in_progress") || s.eq_ignore_ascii_case("inprogress") { Status::InProgress }
    else if s.eq_ignore_ascii_case("blocked") { Status::Blocked }
    else if s.eq_ignore_ascii_case("deferred") { Status::Deferred }
    else if s.eq_ignore_ascii_case("draft") { Status::Draft }
    else if s.eq_ignore_ascii_case("closed") { Status::Closed }
    else if s.eq_ignore_ascii_case("tombstone") { Status::Tombstone }
    else if s.eq_ignore_ascii_case("pinned") { Status::Pinned }
    else if s.eq_ignore_ascii_case("hooked") { Status::Hooked }
    else { Status::Custom(s.to_string()) }
}

fn issue_type_from_str(s: &str) -> IssueType {
    IssueType::from_str(s).unwrap_or_else(|_| IssueType::Custom(s.to_string()))
}

// ---------------------------------------------------------------------------
// GET /api/p/{project_id}/beads
// ---------------------------------------------------------------------------

pub async fn list_beads(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let filters = default_list_filters();
        let issues = storage_ctx.storage.list_issues(&filters)
            .map_err(|e| BeadsError::Config(format!("list failed: {e}")))?;
        let beads: Vec<Value> = issues.iter().map(issue_to_bead_json).collect();
        Ok::<_, BeadsError>(json!({
            "beads": beads,
            "meta": {
                "kind": "bd",
                "humanActor": "",
                "humanAllowlist": [],
                "pollIntervalMs": 5000,
            }
        }))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/p/{project_id}/beads/{id}
// ---------------------------------------------------------------------------

pub async fn get_bead(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let issue = storage_ctx.storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("get_issue failed: {e}")))?;
        Ok::<_, BeadsError>(issue.map(|i| issue_to_bead_json(&i)))
    }).await {
        Ok(Ok(Some(v))) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Ok(None)) => json_error(StatusCode::NOT_FOUND, "not found").into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/p/{project_id}/beads
// ---------------------------------------------------------------------------

pub async fn create_bead(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "title is required").into_response();
    }

    let issue_type_str = body.get("issue_type").and_then(|v| v.as_str()).unwrap_or("task");
    let priority_val = body.get("priority").and_then(|v| v.as_i64()).unwrap_or(2);
    let description = body.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let assignee = body.get("assignee").and_then(|v| v.as_str()).unwrap_or("");
    let backlog = body.get("backlog").and_then(|v| v.as_bool()).unwrap_or(false);
    let actor = "web-ui";

    let labels: Vec<String> = body.get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    // Clone strings needed inside the blocking closure.
    let title_o = title.to_string();
    let desc_o = description.to_string();
    let assignee_o = assignee.to_string();
    let issue_type_o = issue_type_str.to_string();
    let actor_o = actor.to_string();

    match spawn_blocking(move || {
        use chrono::Utc;
        let now = Utc::now();
        let id = crate::util::id::generate_id(&title_o, Some(&desc_o), Some(&actor_o), now);

        let mut issue = Issue::default();
        issue.id.clone_from(&id);
        issue.title = title_o;
        issue.issue_type = if let Ok(it) = IssueType::from_str(&issue_type_o) { it } else { IssueType::Task };
        issue.priority = Priority(priority_val as i32);
        if !desc_o.is_empty() {
            issue.description = Some(desc_o);
        }
        if !assignee_o.is_empty() {
            issue.assignee = Some(assignee_o);
        }
        issue.status = if backlog { Status::Deferred } else { Status::Open };
        issue.created_at = now;
        issue.updated_at = now;
        issue.created_by = Some(actor_o);
        if !labels.is_empty() {
            issue.labels = labels;
        }

        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        storage_ctx.storage.create_issue(&issue, &actor)
            .map_err(|e| BeadsError::Config(format!("create failed: {e}")))?;

        let created = storage_ctx.storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .unwrap_or(issue);

        Ok::<_, BeadsError>(issue_to_bead_json(&created))
    }).await {
        Ok(Ok(v)) => (StatusCode::CREATED, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// PATCH /api/p/{project_id}/beads/{id}
// ---------------------------------------------------------------------------

pub async fn update_bead(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };

    // Extract all values from body before the blocking closure.
    let new_title = body.get("title").and_then(|v| v.as_str()).map(String::from);
    let new_desc = body.get("description").and_then(|v| v.as_str()).map(String::from);
    let new_status = body.get("status").and_then(|v| v.as_str()).map(String::from);
    let new_priority = body.get("priority").and_then(|v| v.as_i64());
    let new_assignee = body.get("assignee").and_then(|v| v.as_str()).map(String::from);
    let label_values: Vec<String> = body.get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let has_labels = body.get("labels").and_then(|v| v.as_array()).is_some();

    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let storage = &mut storage_ctx.storage;
        let actor = "web-ui";

        let mut update = IssueUpdate::default();

        if let Some(t) = new_title {
            update.title = Some(t);
        }
        if let Some(d) = new_desc
            .filter(|d| !d.is_empty())
        {
            update.description = Some(Some(d));
        }
        if let Some(s) = new_status {
            update.status = Some(status_from_str(&s));
        }
        if let Some(p) = new_priority {
            update.priority = Some(Priority(p as i32));
        }
        match new_assignee {
            Some(a) if !a.is_empty() => update.assignee = Some(Some(a)),
            Some(_) => update.assignee = Some(None), // blank = clear
            None => {} // leave untouched
        }

        storage.update_issue(&id, &update, actor)
            .map_err(|e| BeadsError::Config(format!("update failed: {e}")))?;

        if has_labels {
            let _ = storage.set_labels(&id, &label_values, actor);
        }

        let issue = storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .ok_or_else(|| BeadsError::Config("not found after update".to_string()))?;

        Ok::<_, BeadsError>(issue_to_bead_json(&issue))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/p/{project_id}/beads/{id}
// ---------------------------------------------------------------------------

pub async fn delete_bead(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        use chrono::Utc;
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        storage_ctx.storage.delete_issue(&id, "web-ui", "deleted via web UI", Some(Utc::now()))
            .map_err(|e| BeadsError::Config(format!("delete failed: {e}")))?;
        Ok::<_, BeadsError>(json!({ "deleted": id }))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/p/{project_id}/beads/{id}/status
// ---------------------------------------------------------------------------

pub async fn set_status(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let Some(status_str) = body.get("status").and_then(|v| v.as_str()) else {
        return json_error(StatusCode::BAD_REQUEST, "status is required").into_response();
    };

    let status_o = status_str.to_string();
    let reason = body.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let storage = &mut storage_ctx.storage;
        let actor = "web-ui";

        let s = status_from_str(&status_o);
        let is_closed = s == Status::Closed;

        let mut update = IssueUpdate::default();
        update.status = Some(s);
        if is_closed && !reason.is_empty() {
            update.close_reason = Some(Some(reason));
        }

        storage.update_issue(&id, &update, actor)
            .map_err(|e| BeadsError::Config(format!("status update failed: {e}")))?;

        let issue = storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .ok_or_else(|| BeadsError::Config("not found".to_string()))?;

        Ok::<_, BeadsError>(issue_to_bead_json(&issue))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/p/{project_id}/beads/{id}/comments
// ---------------------------------------------------------------------------

pub async fn add_comment(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let text = match body.get("text").and_then(|v| v.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return json_error(StatusCode::BAD_REQUEST, "text is required").into_response(),
    };

    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let storage = &mut storage_ctx.storage;
        let actor = "web-ui";

        storage.add_comment(&id, actor, &text)
            .map_err(|e| BeadsError::Config(format!("add_comment failed: {e}")))?;

        let issue = storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .ok_or_else(|| BeadsError::Config("not found".to_string()))?;

        Ok::<_, BeadsError>(issue_to_bead_json(&issue))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/p/{project_id}/beads/{id}/deps
// ---------------------------------------------------------------------------

pub async fn add_dep(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let Some(depends_on_id) = body.get("depends_on_id").and_then(|v| v.as_str()).map(String::from) else {
        return json_error(StatusCode::BAD_REQUEST, "depends_on_id is required").into_response();
    };
    let dep_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("blocks").to_string();

    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let storage = &mut storage_ctx.storage;
        let actor = "web-ui";

        storage.add_dependency(&id, &depends_on_id, &dep_type, actor)
            .map_err(|e| BeadsError::Config(format!("add_dep failed: {e}")))?;

        let issue = storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .ok_or_else(|| BeadsError::Config("not found".to_string()))?;

        Ok::<_, BeadsError>(issue_to_bead_json(&issue))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/p/{project_id}/beads/{id}/deps
// ---------------------------------------------------------------------------

pub async fn remove_dep(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let Some(depends_on_id) = body.get("depends_on_id").and_then(|v| v.as_str()).map(String::from) else {
        return json_error(StatusCode::BAD_REQUEST, "depends_on_id is required").into_response();
    };

    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let storage = &mut storage_ctx.storage;
        let actor = "web-ui";

        storage.remove_dependency(&id, &depends_on_id, actor)
            .map_err(|e| BeadsError::Config(format!("remove_dep failed: {e}")))?;

        let issue = storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .ok_or_else(|| BeadsError::Config("not found".to_string()))?;

        Ok::<_, BeadsError>(issue_to_bead_json(&issue))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// POST /api/p/{project_id}/beads/{id}/archive
// ---------------------------------------------------------------------------

pub async fn archive_bead(
    State(state): State<Arc<AppState>>,
    Path(params): Path<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(id) = params.get("id").cloned() else {
        return json_error(StatusCode::BAD_REQUEST, "missing id").into_response();
    };
    let beads_dir = state.beads_dir.clone();
    let overrides = state.overrides.clone();

    match spawn_blocking(move || {
        let mut storage_ctx = config::open_storage_with_cli(&beads_dir, &overrides)?;
        let storage = &mut storage_ctx.storage;
        let actor = "web-ui";

        let mut update = IssueUpdate::default();
        update.status = Some(Status::Closed);
        update.close_reason = Some(Some("archived".to_string()));

        storage.update_issue(&id, &update, actor)
            .map_err(|e| BeadsError::Config(format!("close failed: {e}")))?;

        let _ = storage.add_label(&id, "archived", actor);

        let issue = storage.get_issue(&id)
            .map_err(|e| BeadsError::Config(format!("re-fetch failed: {e}")))?
            .ok_or_else(|| BeadsError::Config("not found".to_string()))?;

        Ok::<_, BeadsError>(issue_to_bead_json(&issue))
    }).await {
        Ok(Ok(v)) => (StatusCode::OK, Json(v)).into_response(),
        Ok(Err(e)) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("join: {e}")).into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/projects
// ---------------------------------------------------------------------------

pub async fn list_projects(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let dir_name = state.beads_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("default");

    let project = json!({
        "id": "default",
        "name": dir_name,
        "path": state.beads_dir.parent().and_then(|p| p.to_str()),
        "hasBeads": true,
    });

    let demo_project = json!({
        "id": "demo",
        "name": "Demo Project",
        "path": null,
        "hasBeads": false,
    });

    (StatusCode::OK, Json(json!({ "projects": [demo_project, project] })))
}

// ---------------------------------------------------------------------------
// GET /api/p/{project_id}/doctor
// ---------------------------------------------------------------------------

pub async fn doctor(
    State(state): State<Arc<AppState>>,
    Path(_params): Path<HashMap<String, String>>,
) -> impl IntoResponse {
    let version = option_env!("CARGO_PKG_VERSION").unwrap_or("unknown");
    let beads_dir_str = state.beads_dir.to_string_lossy().to_string();
    let project_name = state.beads_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("default");

    (StatusCode::OK, Json(json!({
        "kind": "bd",
        "ok": true,
        "version": format!("br v{version}"),
        "repoPath": beads_dir_str,
        "message": "Connected to br",
        "project": {
            "id": "default",
            "name": project_name,
            "path": state.beads_dir.parent().and_then(|p| p.to_str()),
        }
    })))
}

// ---------------------------------------------------------------------------
// GET /api/config
// ---------------------------------------------------------------------------

pub async fn get_config() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({
        "humanActor": "",
        "humanAllowlist": [],
        "pollIntervalMs": 5000,
        "projects": [],
        "orders": {},
        "gamification": false,
    })))
}

// ---------------------------------------------------------------------------
// PUT /api/config
// ---------------------------------------------------------------------------

pub async fn update_config(
    Json(_body): Json<Value>,
) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({
        "humanActor": "",
        "humanAllowlist": [],
        "pollIntervalMs": 5000,
        "projects": [],
        "orders": {},
        "gamification": false,
    })))
}
