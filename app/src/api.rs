//! API handlers and routing for the application server.
//!
//! This module defines the HTTP endpoints using the `warp` framework, handling
//! requests related to system status and Todo item management.

use crate::prelude::*;
use warp::{Filter, Rejection, Reply, http::StatusCode};
use todo::todo_prelude::TodoItem;
use warp::query::query; // NEW: Import query filter

/// State shared across API handlers.
#[derive(Clone)]
pub struct ApiState {
    // Placeholder for shared state if needed later
}

// --- Status Endpoints ---

/// Handler for the /status endpoint.
///
/// Returns the current status of all initialized systems and the overall Go/NoGo status.
pub async fn get_status(
    systems_status: SystemsStatus,
    go_nogo_status: SystemsGoNogo,
) -> Result<impl Reply, Rejection> {
    // Combine status into a single response object for easy consumption by the TUI
    #[derive(Serialize)]
    struct StatusResponse {
        systems: SystemsStatus,
        overall: SystemsGoNogo,
    }

    let response = StatusResponse {
        systems: systems_status,
        overall: go_nogo_status,
    };

    Ok(warp::reply::json(&response))
}

// --- Todo CRUD Endpoints ---

/// GET /api/v1/todo/summary - Read todo summary statistics
pub async fn read_todo_summary_handler() -> Result<impl Reply, Rejection> {
    match todo::get_summary().await {
        Ok(summary) => Ok(warp::reply::json(&summary)),
        Err(e) => {
            error!("Failed to read todo summary: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo - Create a new todo item
pub async fn create_todo_handler(item: TodoItem) -> Result<impl Reply, Rejection> {
    match todo::create_item(item).await {
        Ok(new_item) => Ok(warp::reply::with_status(warp::reply::json(&new_item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to create todo item: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// GET /api/v1/todo - Read all todo items (non-archived)
pub async fn read_todos_handler() -> Result<impl Reply, Rejection> {
    match todo::read_items().await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to read todo items: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo/resync - Force an immediate cache resync against the
/// live backend, rather than waiting out the background monitor's interval
/// (e.g. right after editing a todo directly via the raw `nb` CLI).
pub async fn resync_todos_handler() -> Result<impl Reply, Rejection> {
    match todo::sync_cache().await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to resync todo cache: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// PUT /api/v1/todo/:id - Update an existing todo item
pub async fn update_todo_handler(id: i64, item: TodoItem) -> Result<impl Reply, Rejection> {
    if item.id != Some(id) {
        return Err(warp::reject::custom(ApiError::MismatchedId));
    }
    
    match todo::update_item(item).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to update todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// PATCH /api/v1/todo/:id/done - Toggle completed state without touching other fields
#[derive(Deserialize)]
pub struct SetDoneBody { pub done: bool }

pub async fn set_todo_done_handler(id: i64, body: SetDoneBody) -> Result<impl Reply, Rejection> {
    match todo::complete_item(id, body.done).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to set done for todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo/:id/print - Manually print a todo item ticket
pub async fn print_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::print_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to print todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo/:id/archive - Archive a todo item
pub async fn archive_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::archive_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to archive todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// DELETE /api/v1/todo/:id - Delete a todo item
pub async fn delete_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::delete_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// GET /api/v1/todo/:id - Fetch a single todo item as JSON
pub async fn get_single_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::get_item(id).await {
        Ok(item) => Ok(warp::reply::json(&item)),
        Err(e) => {
            error!("Failed to get todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// GET /todo/:id - Task detail page with a "Mark Complete" button
pub async fn get_todo_page_handler(id: i64) -> Result<impl Reply, Rejection> {
    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Task #{id}</title>
<style>
:root{{--bg:#111318;--surface:#1f2227;--surface2:#22252b;--surface3:#262930;--border:#41474d;--accent:#a8c7fa;--accent-on:#0a305f;--accent-dim:#284777;--accent-cont:#d6e3ff;--secondary-cont:#3e4759;--secondary-on:#dae2f9;--green:#6dd58c;--green-cont:#005227;--green-on:#89f3a7;--red-cont:#93000a;--red-on:#ffdad6;--text:#e2e2e6;--text-dim:#c1c7ce;--radius:12px;--shadow:0 2px 6px rgba(0,0,0,0.45);}}
*{{box-sizing:border-box;margin:0;padding:0;}}
body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',system-ui,sans-serif;background:var(--bg);color:var(--text);min-height:100vh;padding:16px;max-width:600px;margin:0 auto;}}
.card{{background:var(--surface);border-radius:var(--radius);border:1px solid var(--border);padding:20px;margin-bottom:12px;box-shadow:var(--shadow);}}
.task-id{{color:var(--text-dim);font-size:12px;margin-bottom:8px;}}
.task-title{{font-size:22px;font-weight:500;margin-bottom:12px;line-height:1.3;letter-spacing:.01em;}}
.meta-row{{display:flex;flex-wrap:wrap;gap:8px;margin-bottom:4px;}}
.badge{{display:inline-block;padding:4px 10px;border-radius:50px;font-size:12px;font-weight:500;background:var(--surface2);border:1px solid var(--border);}}
.badge.project{{background:var(--accent-dim);color:var(--accent-cont);border-color:transparent;}}
.badge.due{{background:var(--secondary-cont);color:var(--secondary-on);border-color:transparent;}}
.badge.label{{background:var(--green-cont);color:var(--green-on);border-color:transparent;}}
.badge.p1{{background:var(--green-cont);color:var(--green-on);border-color:transparent;}}
.badge.p2,.badge.p3{{background:var(--secondary-cont);color:#ffb76b;border-color:transparent;}}
.badge.p4,.badge.p5{{background:var(--red-cont);color:var(--red-on);border-color:transparent;}}
.section-label{{font-size:11px;font-weight:600;color:var(--text-dim);text-transform:uppercase;letter-spacing:.8px;margin-bottom:8px;}}
.description{{line-height:1.6;white-space:pre-wrap;}}
.subtask{{display:flex;align-items:center;gap:10px;padding:8px 0;border-bottom:1px solid rgba(255,255,255,.06);}}
.subtask:last-child{{border-bottom:none;}}
.check{{width:18px;height:18px;border-radius:4px;border:2px solid var(--border);flex-shrink:0;display:flex;align-items:center;justify-content:center;}}
.check.done{{background:var(--accent);border-color:var(--accent);}}
.check.done::after{{content:'✓';color:var(--accent-on);font-size:11px;}}
.sub-title.done{{text-decoration:line-through;color:var(--text-dim);}}
.btn{{width:100%;padding:16px;background:var(--accent);color:var(--accent-on);border:none;border-radius:50px;font-size:16px;font-weight:600;cursor:pointer;margin-top:4px;transition:filter .2s,transform .1s;letter-spacing:.01em;}}
.btn:hover{{filter:brightness(1.1);}}
.btn:active{{transform:scale(.98);filter:brightness(.95);}}
.btn:disabled{{background:var(--surface2);color:var(--text-dim);cursor:not-allowed;filter:none;}}
.btn.done{{background:var(--green-cont);color:var(--green-on);}}
.msg{{text-align:center;padding:12px;border-radius:8px;margin-top:8px;font-weight:600;display:none;}}
.msg.ok{{background:var(--green-cont);color:var(--green-on);display:block;}}
.msg.err{{background:var(--red-cont);color:var(--red-on);display:block;}}
#loading{{text-align:center;padding:60px;color:var(--text-dim);}}
#err{{text-align:center;padding:60px;color:var(--red-on);display:none;}}
#app{{display:none;}}
.badge.overdue{{background:var(--red-cont);color:var(--red-on);border-color:transparent;}}
.task-info-row{{display:flex;justify-content:space-between;padding:6px 0;border-bottom:1px solid rgba(255,255,255,.06);font-size:13px;margin-top:8px;}}
.task-info-row:last-child{{border-bottom:none;}}
.task-info-label{{color:var(--text-dim);}}
.task-info-value{{font-weight:500;max-width:60%;text-align:right;}}
.history-item{{display:flex;justify-content:space-between;align-items:baseline;padding:8px 0;border-bottom:1px solid rgba(255,255,255,.06);font-size:13px;}}
.history-item:last-child{{border-bottom:none;}}
.history-label{{color:var(--text-dim);}}
.history-val{{text-align:right;}}
.history-abs{{font-weight:500;}}
.history-rel{{color:var(--text-dim);font-size:11px;}}
.reminder-list{{display:flex;flex-direction:column;gap:6px;}}
.reminder-item{{display:flex;align-items:center;gap:8px;font-size:13px;padding:6px 8px;background:var(--surface2);border:1px solid var(--border);border-radius:8px;}}
.completed-banner{{background:var(--green-cont);border-radius:8px;padding:12px 16px;margin-bottom:12px;color:var(--green-on);font-weight:600;font-size:13px;}}
</style>
</head>
<body>
<div id="loading">Loading task&hellip;</div>
<div id="err">Task not found</div>
<div id="app">
  <div class="card">
    <div class="task-id" id="tid"></div>
    <div class="task-title" id="title"></div>
    <div id="task-info"></div>
    <div class="meta-row" id="meta"></div>
  </div>
  <div class="card" id="desc-card" style="display:none">
    <div class="section-label">Description</div>
    <div class="description" id="desc"></div>
  </div>
  <div class="card" id="subs-card" style="display:none">
    <div class="section-label" id="subs-label"></div>
    <div id="subs"></div>
  </div>
  <div class="card" id="rem-card" style="display:none">
    <div class="section-label">Reminders</div>
    <div class="reminder-list" id="rems"></div>
  </div>
  <div class="card" id="hist-card" style="display:none">
    <div class="section-label">History</div>
    <div id="hist"></div>
  </div>
  <div class="completed-banner" id="comp-banner" style="display:none"></div>
  <button class="btn" id="btn" onclick="complete()">Mark Complete</button>
  <div class="msg" id="msg"></div>
  <button class="btn" id="close-btn" onclick="window.location.href='/'" style="margin-top:8px;background:transparent;color:var(--accent);border:1px solid var(--border);">Go to Dashboard</button>
</div>
<script>
const ID={id};
const PRI=['UNSET','LOW','MEDIUM','HIGH','URGENT','DO NOW'];
function fmtDate(d){{if(!d)return null;return new Date(d).toLocaleDateString('en-GB',{{weekday:'short',day:'numeric',month:'short',year:'numeric'}});}}
function fmtRel(d){{if(!d)return'';const days=Math.floor((Date.now()-new Date(d))/86400000);if(days<1)return'today';if(days===1)return'yesterday';if(days<7)return days+' days ago';if(days<30)return Math.floor(days/7)+' wks ago';return Math.floor(days/30)+' mo ago';}}
async function load(){{
  try{{
    const r=await fetch('/api/v1/todo/'+ID);
    if(!r.ok)throw 0;
    render(await r.json());
  }}catch{{
    document.getElementById('loading').style.display='none';
    document.getElementById('err').style.display='block';
  }}
}}
function render(t){{
  document.getElementById('tid').textContent='TODO #'+(t.id||ID);
  document.getElementById('title').textContent=t.title||('Task #'+ID);
  const ti=document.getElementById('task-info');
  if(t.project_title)ti.innerHTML+=`<div class="task-info-row"><span class="task-info-label">&#128193; Project</span><span class="task-info-value">${{t.project_title}}</span></div>`;
  if(t.created_at)ti.innerHTML+=`<div class="task-info-row"><span class="task-info-label">&#128197; Created</span><span class="task-info-value">${{fmtDate(t.created_at)}}</span></div>`;
  const m=document.getElementById('meta');
  if(t.due_date){{const ov=!t.completed&&new Date(t.due_date)<new Date();m.innerHTML+=`<span class="badge due${{ov?' overdue':''}}">${{ov?'&#9888;':'&#128197;'}} ${{fmtDate(t.due_date)}}</span>`;}}

  if(t.priority>0)m.innerHTML+=`<span class="badge p${{t.priority}}">&#9873; ${{PRI[Math.min(t.priority,5)]}}</span>`;
  (t.labels||[]).forEach(l=>m.innerHTML+=`<span class="badge label">${{l}}</span>`);
  if(t.description){{document.getElementById('desc').textContent=t.description;document.getElementById('desc-card').style.display='block';}}
  const subs=t.subtasks||[];
  if(subs.length){{
    const done=subs.filter(s=>s.done).length;
    document.getElementById('subs-label').textContent=`Subtasks [${{done}}/${{subs.length}}]`;
    const c=document.getElementById('subs');
    subs.forEach(s=>c.innerHTML+=`<div class="subtask"><div class="check ${{s.done?'done':''}}"></div><span class="sub-title ${{s.done?'done':''}}">${{s.title}}</span></div>`);
    document.getElementById('subs-card').style.display='block';
  }}
  document.title=t.title||('Task #'+ID);
  const hist=document.getElementById('hist');
  if(t.updated_at)hist.innerHTML+=`<div class="history-item"><span class="history-label">&#9998; Updated</span><div class="history-val"><div class="history-abs">${{fmtDate(t.updated_at)}}</div><div class="history-rel">${{fmtRel(t.updated_at)}}</div></div></div>`;
  if(t.printed_at)hist.innerHTML+=`<div class="history-item"><span class="history-label">&#128438; Printed</span><div class="history-val"><div class="history-abs">${{fmtDate(t.printed_at)}}</div><div class="history-rel">${{fmtRel(t.printed_at)}}</div></div></div>`;
  document.getElementById('hist-card').style.display='block';
  const rems=t.reminders||[];
  if(rems.length){{
    const rc=document.getElementById('rems');
    rems.sort((a,b)=>new Date(a)-new Date(b)).forEach(r=>{{
      const d=new Date(r);
      const past=d<new Date();
      rc.innerHTML+=`<div class="reminder-item">${{past?'&#9201;':'&#9200;'}} ${{d.toLocaleDateString('en-GB',{{weekday:'short',day:'numeric',month:'short',year:'numeric'}})}} ${{d.toLocaleTimeString('en-GB',{{hour:'2-digit',minute:'2-digit'}})}}</div>`;
    }});
    document.getElementById('rem-card').style.display='block';
  }}
  if(t.completed&&t.completed_at){{
    const cb=document.getElementById('comp-banner');
    cb.textContent='✓ Completed on '+fmtDate(t.completed_at);
    cb.style.display='block';
  }}
  const btn=document.getElementById('btn');
  if(t.completed){{btn.textContent='Already Completed';btn.classList.add('done');btn.disabled=true;}}
  document.getElementById('loading').style.display='none';
  document.getElementById('app').style.display='block';
}}
async function complete(){{
  const btn=document.getElementById('btn'),msg=document.getElementById('msg');
  btn.disabled=true;btn.textContent='Completing\u2026';msg.className='msg';
  try{{
    const r=await fetch('/api/v1/todo/'+ID+'/done',{{method:'PATCH',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{done:true}})}});
    if(!r.ok)throw 0;
    btn.textContent='\u2713 Completed!';btn.classList.add('done');
    msg.textContent='Task marked as complete!';msg.className='msg ok';
  }}catch{{
    btn.disabled=false;btn.textContent='Mark Complete';
    msg.textContent='Failed to complete task. Please try again.';msg.className='msg err';
  }}
}}
load();
</script>
</body>
</html>"#, id = id);
    Ok(warp::reply::with_header(
        warp::reply::html(html),
        "Cache-Control",
        "no-cache, no-store, must-revalidate",
    ))
}

// --- List Endpoints ---

/// GET /api/v1/lists/groups
pub async fn list_groups_handler() -> Result<impl Reply, Rejection> {
    match lists::list_groups().await {
        Ok(groups) => Ok(warp::reply::json(&groups)),
        Err(e) => {
            error!("Failed to list groups: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/groups
#[derive(Deserialize)]
pub struct AddGroupBody { pub name: String }

pub async fn add_group_handler(body: AddGroupBody) -> Result<impl Reply, Rejection> {
    match lists::add_group(&body.name).await {
        Ok(group) => Ok(warp::reply::with_status(warp::reply::json(&group), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add group: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/groups/:id
pub async fn delete_group_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_group(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete group {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// GET /api/v1/lists/groups/:group_id/categories
pub async fn list_categories_handler(group_id: i64) -> Result<impl Reply, Rejection> {
    match lists::list_categories(group_id).await {
        Ok(cats) => Ok(warp::reply::json(&cats)),
        Err(e) => {
            error!("Failed to list categories for group {}: {}", group_id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/groups/:group_id/categories
#[derive(Deserialize)]
pub struct AddCategoryBody { pub name: String }

pub async fn add_category_handler(group_id: i64, body: AddCategoryBody) -> Result<impl Reply, Rejection> {
    match lists::add_category(group_id, &body.name).await {
        Ok(cat) => Ok(warp::reply::with_status(warp::reply::json(&cat), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add category: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/categories/:id
pub async fn delete_category_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_category(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// GET /api/v1/lists/categories/:id/items
pub async fn list_items_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::list_items(id).await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to list items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/items
#[derive(Deserialize)]
pub struct AddItemBody { pub name: String, pub quantity: Option<String> }

pub async fn add_item_handler(cat_id: i64, body: AddItemBody) -> Result<impl Reply, Rejection> {
    match lists::add_item(cat_id, &body.name, body.quantity.as_deref()).await {
        Ok(item) => Ok(warp::reply::with_status(warp::reply::json(&item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add item: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// PATCH /api/v1/lists/items/:id/check
#[derive(Deserialize)]
pub struct CheckItemBody { pub checked: bool }

pub async fn check_item_handler(id: i64, body: CheckItemBody) -> Result<impl Reply, Rejection> {
    match lists::check_item(id, body.checked).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to check item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/items/:id
pub async fn delete_item_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// PATCH /api/v1/lists/categories/:id/name
#[derive(Deserialize)]
pub struct RenameCategoryBody { pub name: String }

pub async fn rename_category_handler(id: i64, body: RenameCategoryBody) -> Result<impl Reply, Rejection> {
    match lists::rename_category(id, body.name).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to rename category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// PATCH /api/v1/lists/categories/:id/checkboxes
#[derive(Deserialize)]
pub struct SetCheckboxesBody { pub has_checkboxes: bool }

pub async fn set_checkboxes_handler(id: i64, body: SetCheckboxesBody) -> Result<impl Reply, Rejection> {
    match lists::set_checkboxes(id, body.has_checkboxes).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to set checkboxes for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// PATCH /api/v1/lists/categories/:id/quick-add
#[derive(Deserialize)]
pub struct SetQuickAddBody { pub has_quick_add: bool }

pub async fn set_quick_add_handler(id: i64, body: SetQuickAddBody) -> Result<impl Reply, Rejection> {
    match lists::set_quick_add(id, body.has_quick_add).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to set quick-add for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/reorder
#[derive(Deserialize)]
pub struct ReorderItemsBody { pub ids: Vec<i64> }

pub async fn reorder_items_handler(id: i64, body: ReorderItemsBody) -> Result<impl Reply, Rejection> {
    match lists::reorder_items(id, body.ids).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to reorder items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/clear
pub async fn clear_checked_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::clear_checked(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to clear checked items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/print
pub async fn print_list_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::print_list(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to print list for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

// --- Common Items Endpoints ---

/// GET /api/v1/lists/categories/:id/common
pub async fn list_common_items_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::list_common_items(id).await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to list common items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/common
#[derive(Deserialize)]
pub struct AddCommonItemBody { pub name: String, pub quantity: Option<String> }

pub async fn add_common_item_handler(id: i64, body: AddCommonItemBody) -> Result<impl Reply, Rejection> {
    match lists::add_common_item(id, &body.name, body.quantity.as_deref()).await {
        Ok(item) => Ok(warp::reply::with_status(warp::reply::json(&item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add common item: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/common/:id
pub async fn delete_common_item_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_common_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete common item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/common/:id/add
pub async fn add_item_from_common_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::add_item_from_common(id).await {
        Ok(item) => Ok(warp::reply::with_status(warp::reply::json(&item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add item from common {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

// --- Notes Endpoints ---

#[derive(Deserialize)]
pub struct NoteQuery {
    pub notebook: Option<String>,
    pub tag: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteIdQuery {
    pub notebook: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteSearchQuery { pub q: Option<String> }

#[derive(Deserialize)]
pub struct DailyLogQuery {
    pub days: Option<i64>,
}

/// GET /api/v1/notes?notebook=&tag=
pub async fn list_notes_handler(q: NoteQuery) -> Result<impl Reply, Rejection> {
    match notes::list(q.notebook, q.tag).await {
        Ok(notes) => Ok(warp::reply::json(&notes)),
        Err(e) => {
            error!("Failed to list notes: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// POST /api/v1/notes/resync - Force an immediate cache resync against the
/// live `nb` notebooks, rather than waiting out the background monitor's
/// interval (e.g. right after editing a note directly via the raw `nb` CLI).
pub async fn resync_notes_handler() -> Result<impl Reply, Rejection> {
    match notes::sync_cache().await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to resync notes cache: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// POST /api/v1/notes
pub async fn create_note_handler(req: notes::CreateNoteRequest) -> Result<impl Reply, Rejection> {
    match notes::create(req).await {
        Ok(note) => Ok(warp::reply::with_status(warp::reply::json(&note), StatusCode::CREATED)),
        Err(notes::notes_error::NotesLibError::InvalidInput(msg)) => {
            Err(warp::reject::custom(ApiError::NotesInvalidInput(msg)))
        }
        Err(e) => {
            error!("Failed to create note: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// POST /api/v1/notes/daily — appends a titled, tagged entry to today's
/// daily log via nb's `daily` plugin, always in the "log" notebook.
pub async fn create_log_handler(req: notes::CreateLogRequest) -> Result<impl Reply, Rejection> {
    match notes::create_log(req).await {
        Ok(()) => Ok(warp::reply::with_status("Log entry saved".to_string(), StatusCode::CREATED)),
        Err(notes::notes_error::NotesLibError::InvalidInput(msg)) => {
            Err(warp::reject::custom(ApiError::NotesInvalidInput(msg)))
        }
        Err(e) => {
            error!("Failed to create log entry: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// GET /api/v1/notes/daily?days= — entries from the last N days (default 7)
/// of the daily log, most recent first.
pub async fn list_log_handler(q: DailyLogQuery) -> Result<impl Reply, Rejection> {
    let days = q.days.unwrap_or(7);
    match notes::recent_logs(days).await {
        Ok(entries) => Ok(warp::reply::json(&entries)),
        Err(e) => {
            error!("Failed to list log entries: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// GET /api/v1/notes/search?q=
pub async fn search_notes_handler(q: NoteSearchQuery) -> Result<impl Reply, Rejection> {
    let query = q.q.unwrap_or_default();
    match notes::search(&query).await {
        Ok(notes) => Ok(warp::reply::json(&notes)),
        Err(e) => {
            error!("Failed to search notes: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// GET /api/v1/notes/folders
pub async fn list_note_folders_handler() -> Result<impl Reply, Rejection> {
    match notes::folders().await {
        Ok(folders) => Ok(warp::reply::json(&folders)),
        Err(e) => {
            error!("Failed to list note folders: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// GET /api/v1/notes/tags
pub async fn list_note_tags_handler() -> Result<impl Reply, Rejection> {
    match notes::tags().await {
        Ok(tags) => Ok(warp::reply::json(&tags)),
        Err(e) => {
            error!("Failed to list note tags: {}", e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// GET /api/v1/notes/:id?notebook=
pub async fn get_note_handler(id: u64, q: NoteIdQuery) -> Result<impl Reply, Rejection> {
    let notebook = q.notebook.as_deref().unwrap_or("home");
    match notes::get(id, notebook).await {
        Ok(note) => Ok(warp::reply::json(&note)),
        Err(notes::notes_error::NotesLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get note {}:{}: {}", notebook, id, e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// PUT /api/v1/notes/:id?notebook=
pub async fn update_note_handler(id: u64, q: NoteIdQuery, req: notes::UpdateNoteRequest) -> Result<impl Reply, Rejection> {
    let notebook = q.notebook.as_deref().unwrap_or("home");
    match notes::update(id, notebook, req).await {
        Ok(note) => Ok(warp::reply::json(&note)),
        Err(notes::notes_error::NotesLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(notes::notes_error::NotesLibError::InvalidInput(msg)) => {
            Err(warp::reject::custom(ApiError::NotesInvalidInput(msg)))
        }
        Err(e) => {
            error!("Failed to update note {}:{}: {}", notebook, id, e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// DELETE /api/v1/notes/:id?notebook=
pub async fn delete_note_handler(id: u64, q: NoteIdQuery) -> Result<impl Reply, Rejection> {
    let notebook = q.notebook.as_deref().unwrap_or("home");
    match notes::delete(id, notebook).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(notes::notes_error::NotesLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to delete note {}:{}: {}", notebook, id, e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// POST /api/v1/notes/:id/print?notebook=
pub async fn print_note_handler(id: u64, q: NoteIdQuery) -> Result<impl Reply, Rejection> {
    let notebook = q.notebook.as_deref().unwrap_or("home");
    match notes::print(id, notebook).await {
        Ok(()) => Ok(warp::reply::json(&true)),
        Err(notes::notes_error::NotesLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to print note {}:{}: {}", notebook, id, e);
            Err(warp::reject::custom(ApiError::NotesOperationFailed))
        }
    }
}

/// GET /notes/:id?notebook= - rendered markdown viewer page
pub async fn get_note_page_handler(id: u64, q: NoteIdQuery) -> Result<impl Reply, Rejection> {
    let notebook = q.notebook.clone().unwrap_or_else(|| "home".to_string());
    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Note</title>
<style>
:root{{--bg:#1a1a2e;--surface:#16213e;--surface2:#0f3460;--accent:#e94560;--text:#eaeaea;--text-dim:#9a9ab0;--radius:12px;}}
*{{box-sizing:border-box;margin:0;padding:0;}}
body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:var(--bg);color:var(--text);min-height:100vh;padding:16px;max-width:800px;margin:0 auto;}}
.meta-bar{{background:var(--surface);border-radius:var(--radius);padding:14px 18px;margin-bottom:12px;display:flex;flex-wrap:wrap;gap:10px;align-items:center;}}
.title{{font-size:24px;font-weight:700;margin-bottom:4px;}}
.badge{{display:inline-block;padding:3px 10px;border-radius:20px;font-size:12px;font-weight:600;}}
.badge.notebook{{background:#2a2a1a;color:#ffd54f;}}
.badge.tag{{background:var(--surface2);color:var(--text);}}
.dates{{font-size:11px;color:var(--text-dim);margin-left:auto;}}
.content-card{{background:var(--surface);border-radius:var(--radius);padding:20px 24px;}}
.markdown-body h1,.markdown-body h2,.markdown-body h3{{color:var(--text);margin:1em 0 .5em;line-height:1.3;}}
.markdown-body h1{{font-size:1.8em;border-bottom:1px solid rgba(255,255,255,.1);padding-bottom:.3em;}}
.markdown-body h2{{font-size:1.4em;}}
.markdown-body h3{{font-size:1.2em;}}
.markdown-body p{{margin:.75em 0;line-height:1.7;}}
.markdown-body ul,.markdown-body ol{{padding-left:1.5em;margin:.75em 0;}}
.markdown-body li{{margin:.3em 0;line-height:1.6;}}
.markdown-body code{{background:rgba(255,255,255,.1);padding:2px 6px;border-radius:4px;font-family:monospace;font-size:.9em;}}
.markdown-body pre{{background:#0d1117;border-radius:8px;padding:16px;overflow-x:auto;margin:1em 0;}}
.markdown-body pre code{{background:none;padding:0;}}
.markdown-body blockquote{{border-left:3px solid var(--accent);padding-left:1em;color:var(--text-dim);margin:1em 0;}}
.markdown-body a{{color:#64b5f6;text-decoration:none;}}
.markdown-body a:hover{{text-decoration:underline;}}
.markdown-body hr{{border:none;border-top:1px solid rgba(255,255,255,.1);margin:1.5em 0;}}
.markdown-body table{{width:100%;border-collapse:collapse;margin:1em 0;}}
.markdown-body th,.markdown-body td{{padding:8px 12px;border:1px solid rgba(255,255,255,.1);text-align:left;}}
.markdown-body th{{background:var(--surface2);}}
.actions{{display:flex;gap:8px;margin-top:12px;}}
.btn{{padding:10px 18px;border:none;border-radius:8px;font-size:14px;font-weight:600;cursor:pointer;transition:opacity .2s;}}
.btn-back{{background:var(--surface2);color:var(--text);}}
.btn:hover{{opacity:.85;}}
#loading{{text-align:center;padding:60px;color:var(--text-dim);}}
#app{{display:none;}}
</style>
</head>
<body>
<div id="loading">Loading note&hellip;</div>
<div id="app">
  <div class="meta-bar">
    <div>
      <div class="title" id="title"></div>
      <div style="margin-top:6px;display:flex;flex-wrap:wrap;gap:6px;" id="badges"></div>
    </div>
    <div class="dates" id="dates"></div>
  </div>
  <div class="content-card markdown-body" id="content"></div>
  <div class="actions">
    <button class="btn btn-back" onclick="history.back()">Back</button>
  </div>
</div>
<script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"></script>
<script>
const NB_ID={id};
const NOTEBOOK="{notebook}";
function fmtDate(d){{return new Date(d).toLocaleDateString('en-GB',{{day:'numeric',month:'short',year:'numeric'}});}}
async function load(){{
  try{{
    const r=await fetch('/api/v1/notes/'+NB_ID+'?notebook='+NOTEBOOK);
    if(!r.ok)throw 0;
    const n=await r.json();
    document.title='Note: '+(n.title||'Untitled');
    document.getElementById('title').textContent=n.title||'Untitled';
    const b=document.getElementById('badges');
    b.innerHTML=`<span class="badge notebook">&#128193; ${{n.notebook}}</span>`;
    (n.tags||[]).forEach(t=>b.innerHTML+=`<span class="badge tag">#${{t}}</span>`);
    document.getElementById('dates').innerHTML=`Updated ${{fmtDate(n.updated_at)}}<br>Created ${{fmtDate(n.created_at)}}`;
    document.getElementById('content').innerHTML=marked.parse(n.content||'');
    document.getElementById('loading').style.display='none';
    document.getElementById('app').style.display='block';
  }}catch{{document.getElementById('loading').textContent='Note not found';}}
}}
load();
</script>
</body>
</html>"#, id = id, notebook = notebook);
    Ok(warp::reply::html(html))
}

// --- Log Endpoints ---

/// Query parameters for fetching logs.
#[derive(Deserialize)]
pub struct LogQuery {
    limit: Option<u32>,
}

/// GET /api/v1/logs - Read latest log entries
pub async fn read_logs_handler(query: LogQuery) -> Result<impl Reply, Rejection> {
    let limit = query.limit.unwrap_or(20); // Default to 20 logs
    
    match db::log_read_latest(limit).await {
        Ok(logs) => Ok(warp::reply::json(&logs)),
        Err(e) => {
            error!("Failed to read logs: {}", e);
            Err(warp::reject::custom(ApiError::LogOperationFailed))
        }
    }
}

// --- Project Endpoints ---

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
}

/// GET /api/v1/project - list all projects (including archived)
pub async fn list_projects_handler() -> Result<impl Reply, Rejection> {
    match project::list_projects().await {
        Ok(projects) => Ok(warp::reply::json(&projects)),
        Err(e) => {
            error!("Failed to list projects: {}", e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// POST /api/v1/project - create a new project
pub async fn create_project_handler(body: CreateProjectRequest) -> Result<impl Reply, Rejection> {
    match project::create_project(&body.name).await {
        Ok(p) => Ok(warp::reply::with_status(warp::reply::json(&p), StatusCode::CREATED)),
        Err(ProjectLibError::InvalidInput(msg)) => Err(warp::reject::custom(ApiError::ProjectInvalidInput(msg))),
        Err(ProjectLibError::DuplicateName(name)) => Err(warp::reject::custom(ApiError::ProjectInvalidInput(
            format!("a project named '{}' already exists", name),
        ))),
        Err(e) => {
            error!("Failed to create project: {}", e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// GET /api/v1/project/:id - project metadata only
pub async fn get_project_handler(id: i64) -> Result<impl Reply, Rejection> {
    match project::get_project(id).await {
        Ok(p) => Ok(warp::reply::json(&p)),
        Err(ProjectLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// GET /api/v1/project/:id/detail - aggregated todos+notes+logs+lists
/// (metadata-only once the project is archived)
pub async fn project_detail_handler(id: i64) -> Result<impl Reply, Rejection> {
    match project::project_detail(id).await {
        Ok(detail) => Ok(warp::reply::json(&detail)),
        Err(ProjectLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project detail {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// POST /api/v1/project/:id/archive - archives a project (never deletes)
pub async fn archive_project_handler(id: i64) -> Result<impl Reply, Rejection> {
    match project::archive_project(id).await {
        Ok(p) => Ok(warp::reply::json(&p)),
        Err(ProjectLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to archive project {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// POST /api/v1/project/:id/restore - un-archives a project (reverse of archive)
pub async fn restore_project_handler(id: i64) -> Result<impl Reply, Rejection> {
    match project::restore_project(id).await {
        Ok(p) => Ok(warp::reply::json(&p)),
        Err(ProjectLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to restore project {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// DELETE /api/v1/project/:id - permanently deletes an archived project
pub async fn delete_project_handler(id: i64) -> Result<impl Reply, Rejection> {
    match project::delete_project(id).await {
        Ok(()) => Ok(warp::reply::with_status(warp::reply(), StatusCode::NO_CONTENT)),
        Err(ProjectLibError::NotFound(_)) => Err(warp::reject::not_found()),
        Err(ProjectLibError::InvalidInput(msg)) => Err(warp::reject::custom(ApiError::ProjectInvalidInput(msg))),
        Err(e) => {
            error!("Failed to delete project {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// GET /api/v1/project/:id/todos - todos scoped to the project
pub async fn project_todos_handler(id: i64) -> Result<impl Reply, Rejection> {
    let p = match project::get_project(id).await {
        Ok(p) => p,
        Err(ProjectLibError::NotFound(_)) => return Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project {}: {}", id, e);
            return Err(warp::reject::custom(ApiError::ProjectOperationFailed));
        }
    };
    match project::project_todos(&p).await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to list project {} todos: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// POST /api/v1/project/:id/todos - creates a todo scoped to the project
pub async fn create_project_todo_handler(id: i64, mut item: TodoItem) -> Result<impl Reply, Rejection> {
    let p = match project::get_project(id).await {
        Ok(p) => p,
        Err(ProjectLibError::NotFound(_)) => return Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project {}: {}", id, e);
            return Err(warp::reject::custom(ApiError::ProjectOperationFailed));
        }
    };
    item.project_title = Some(p.slug);
    match todo::create_item(item).await {
        Ok(new_item) => Ok(warp::reply::with_status(warp::reply::json(&new_item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to create project todo: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// GET /api/v1/project/:id/notes - notes scoped to the project
pub async fn project_notes_handler(id: i64) -> Result<impl Reply, Rejection> {
    let p = match project::get_project(id).await {
        Ok(p) => p,
        Err(ProjectLibError::NotFound(_)) => return Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project {}: {}", id, e);
            return Err(warp::reject::custom(ApiError::ProjectOperationFailed));
        }
    };
    match project::project_notes(&p).await {
        Ok(notes) => Ok(warp::reply::json(&notes)),
        Err(e) => {
            error!("Failed to list project {} notes: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// GET /api/v1/project/:id/logs?days= - log entries scoped to the project
/// (default 30 days)
pub async fn project_logs_handler(id: i64, q: DailyLogQuery) -> Result<impl Reply, Rejection> {
    let days = q.days.unwrap_or(30);
    let p = match project::get_project(id).await {
        Ok(p) => p,
        Err(ProjectLibError::NotFound(_)) => return Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project {}: {}", id, e);
            return Err(warp::reject::custom(ApiError::ProjectOperationFailed));
        }
    };
    match project::project_logs(&p, days).await {
        Ok(logs) => Ok(warp::reply::json(&logs)),
        Err(e) => {
            error!("Failed to list project {} logs: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

/// GET /api/v1/project/:id/lists - list categories scoped to the project
pub async fn project_lists_handler(id: i64) -> Result<impl Reply, Rejection> {
    let p = match project::get_project(id).await {
        Ok(p) => p,
        Err(ProjectLibError::NotFound(_)) => return Err(warp::reject::not_found()),
        Err(e) => {
            error!("Failed to get project {}: {}", id, e);
            return Err(warp::reject::custom(ApiError::ProjectOperationFailed));
        }
    };
    match project::project_lists(&p).await {
        Ok(lists) => Ok(warp::reply::json(&lists)),
        Err(e) => {
            error!("Failed to list project {} lists: {}", id, e);
            Err(warp::reject::custom(ApiError::ProjectOperationFailed))
        }
    }
}

// --- Error Handling ---

/// Custom API errors used for rejection handling.
#[derive(Debug)]
enum ApiError {
    TodoOperationFailed,
    MismatchedId,
    LogOperationFailed,
    ListsOperationFailed,
    NotesOperationFailed,
    NotesInvalidInput(String),
    ProjectOperationFailed,
    ProjectInvalidInput(String),
}

impl warp::reject::Reject for ApiError {}

/// Handles custom rejections and converts them into appropriate HTTP responses.
async fn handle_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    if let Some(ApiError::TodoOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Todo operation failed".to_string(), StatusCode::INTERNAL_SERVER_ERROR))
    } else if let Some(ApiError::MismatchedId) = err.find() {
        Ok(warp::reply::with_status("ID in path does not match ID in body".to_string(), StatusCode::BAD_REQUEST))
    } else if let Some(ApiError::LogOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Log operation failed".to_string(), StatusCode::INTERNAL_SERVER_ERROR))
    } else if let Some(ApiError::ListsOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Lists operation failed".to_string(), StatusCode::INTERNAL_SERVER_ERROR))
    } else if let Some(ApiError::NotesOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Notes operation failed".to_string(), StatusCode::INTERNAL_SERVER_ERROR))
    } else if let Some(ApiError::NotesInvalidInput(msg)) = err.find() {
        Ok(warp::reply::with_status(msg.clone(), StatusCode::BAD_REQUEST))
    } else if let Some(ApiError::ProjectOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Project operation failed".to_string(), StatusCode::INTERNAL_SERVER_ERROR))
    } else if let Some(ApiError::ProjectInvalidInput(msg)) = err.find() {
        Ok(warp::reply::with_status(msg.clone(), StatusCode::BAD_REQUEST))
    } else {
        Err(err)
    }
}

// --- Route Definition ---

/// Defines routes related to Todo item management.
fn todo_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let todo_base = warp::path("todo");

    // GET /api/v1/todo/summary
    let summary = todo_base
        .and(warp::path("summary"))
        .and(warp::get())
        .and_then(read_todo_summary_handler);

    // POST /api/v1/todo/resync
    let resync = todo_base
        .and(warp::path("resync"))
        .and(warp::post())
        .and_then(resync_todos_handler);

    // POST /api/v1/todo
    let create = todo_base
        .and(warp::post())
        .and(warp::body::json())
        .and_then(create_todo_handler);

    // GET /api/v1/todo
    let read_all = todo_base
        .and(warp::get())
        .and_then(read_todos_handler);

    // PUT /api/v1/todo/:id
    let update = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::put())
        .and(warp::body::json())
        .and_then(|id, item| update_todo_handler(id, item));
        
    // PATCH /api/v1/todo/:id/done
    let set_done = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path("done"))
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| set_todo_done_handler(id, body));

    // POST /api/v1/todo/:id/print
    let print = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path("print"))
        .and(warp::post())
        .and_then(print_todo_handler);
        
    // POST /api/v1/todo/:id/archive
    let archive = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path("archive"))
        .and(warp::post())
        .and_then(archive_todo_handler);

    // DELETE /api/v1/todo/:id
    let delete = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::delete())
        .and_then(delete_todo_handler);

    // GET /api/v1/todo/:id — single task JSON
    let get_one = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::get())
        .and_then(get_single_todo_handler);

    summary.or(resync).or(read_all).or(get_one).or(create).or(update).or(set_done).or(print).or(archive).or(delete)
}

/// Defines routes related to system status.
fn status_routes(
    systems_status: SystemsStatus,
    go_nogo_status: SystemsGoNogo,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let systems_status_filter = warp::any().map(move || systems_status);
    let go_nogo_status_filter = warp::any().map(move || go_nogo_status);

    warp::path("status")
        .and(warp::get())
        .and(systems_status_filter)
        .and(go_nogo_status_filter)
        .and_then(get_status)
}

/// Defines routes for the generic lists module.
///
/// URL structure:
///   /api/v1/lists/groups                          — list groups CRUD
///   /api/v1/lists/groups/:gid/categories          — categories within a group
///   /api/v1/lists/categories/:id/items            — items within a category
///   /api/v1/lists/items/:id/check                 — toggle item checked
fn list_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let lists      = warp::path("lists");
    let groups_seg = warp::path("groups");
    let categories = warp::path("categories");
    let items      = warp::path("items");

    // GET /api/v1/lists/groups
    let list_groups = lists
        .and(groups_seg)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_groups_handler);

    // POST /api/v1/lists/groups
    let add_group = lists
        .and(groups_seg)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(add_group_handler);

    // DELETE /api/v1/lists/groups/:id
    let delete_group = lists
        .and(groups_seg)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_group_handler);

    // GET /api/v1/lists/groups/:group_id/categories
    let list_cats = lists
        .and(groups_seg)
        .and(warp::path::param::<i64>())
        .and(categories)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_categories_handler);

    // POST /api/v1/lists/groups/:group_id/categories
    let add_cat = lists
        .and(groups_seg)
        .and(warp::path::param::<i64>())
        .and(categories)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|gid, body| add_category_handler(gid, body));

    // DELETE /api/v1/lists/categories/:id
    let delete_cat = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_category_handler);

    // GET /api/v1/lists/categories/:id/items
    let list_items = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(items)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_items_handler);

    // POST /api/v1/lists/categories/:id/items
    let add_item = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(items)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|id, body| add_item_handler(id, body));

    // PATCH /api/v1/lists/items/:id/check
    let check_item = lists
        .and(items)
        .and(warp::path::param::<i64>())
        .and(warp::path("check"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| check_item_handler(id, body));

    // DELETE /api/v1/lists/items/:id
    let delete_item = lists
        .and(items)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_item_handler);

    // PATCH /api/v1/lists/categories/:id/name
    let rename_cat = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("name"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| rename_category_handler(id, body));

    // PATCH /api/v1/lists/categories/:id/checkboxes
    let set_checkboxes = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("checkboxes"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| set_checkboxes_handler(id, body));

    // PATCH /api/v1/lists/categories/:id/quick-add
    let set_quick_add = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("quick-add"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| set_quick_add_handler(id, body));

    // POST /api/v1/lists/categories/:id/reorder
    let reorder = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("reorder"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|id, body| reorder_items_handler(id, body));

    // POST /api/v1/lists/categories/:id/clear
    let clear = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("clear"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(clear_checked_handler);

    // POST /api/v1/lists/categories/:id/print
    let print = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("print"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(print_list_handler);

    let common_seg = warp::path("common");

    // GET /api/v1/lists/categories/:id/common
    let list_common = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(common_seg)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_common_items_handler);

    // POST /api/v1/lists/categories/:id/common
    let add_common = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(common_seg)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|id, body| add_common_item_handler(id, body));

    // DELETE /api/v1/lists/common/:id
    let delete_common = lists
        .and(common_seg)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_common_item_handler);

    // POST /api/v1/lists/common/:id/add
    let add_from_common = lists
        .and(common_seg)
        .and(warp::path::param::<i64>())
        .and(warp::path("add"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(add_item_from_common_handler);

    list_groups
        .or(add_group)
        .or(delete_group)
        .or(list_cats)
        .or(add_cat)
        .or(delete_cat)
        .or(list_items)
        .or(add_item)
        .or(check_item)
        .or(delete_item)
        .or(rename_cat)
        .or(set_checkboxes)
        .or(set_quick_add)
        .or(reorder)
        .or(clear)
        .or(print)
        .or(list_common)
        .or(add_common)
        .or(delete_common)
        .or(add_from_common)
}

/// Defines routes for the notes subsystem.
fn notes_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let notes_seg = warp::path("notes");

    // GET /api/v1/notes?notebook=&tag=
    let list = notes_seg
        .and(warp::path::end())
        .and(warp::get())
        .and(query::<NoteQuery>())
        .and_then(list_notes_handler);

    // POST /api/v1/notes
    let create = notes_seg
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(create_note_handler);

    // POST /api/v1/notes/daily
    let create_log = notes_seg
        .and(warp::path("daily"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(create_log_handler);

    // GET /api/v1/notes/daily?days=
    let list_log = notes_seg
        .and(warp::path("daily"))
        .and(warp::path::end())
        .and(warp::get())
        .and(query::<DailyLogQuery>())
        .and_then(list_log_handler);

    // GET /api/v1/notes/search?q=
    let search = notes_seg
        .and(warp::path("search"))
        .and(warp::path::end())
        .and(warp::get())
        .and(query::<NoteSearchQuery>())
        .and_then(search_notes_handler);

    // GET /api/v1/notes/folders
    let folders = notes_seg
        .and(warp::path("folders"))
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_note_folders_handler);

    // GET /api/v1/notes/tags
    let tags = notes_seg
        .and(warp::path("tags"))
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_note_tags_handler);

    // POST /api/v1/notes/resync
    let resync = notes_seg
        .and(warp::path("resync"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(resync_notes_handler);

    // GET /api/v1/notes/:id?notebook=
    let get_one = notes_seg
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(warp::get())
        .and(query::<NoteIdQuery>())
        .and_then(get_note_handler);

    // PUT /api/v1/notes/:id?notebook=
    let update = notes_seg
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(warp::put())
        .and(query::<NoteIdQuery>())
        .and(warp::body::json())
        .and_then(|id, q, req| update_note_handler(id, q, req));

    // DELETE /api/v1/notes/:id?notebook=
    let delete = notes_seg
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and(query::<NoteIdQuery>())
        .and_then(|id, q| delete_note_handler(id, q));

    // POST /api/v1/notes/:id/print?notebook=
    let print = notes_seg
        .and(warp::path::param::<u64>())
        .and(warp::path("print"))
        .and(warp::path::end())
        .and(warp::post())
        .and(query::<NoteIdQuery>())
        .and_then(|id, q| print_note_handler(id, q));

    search.or(folders).or(tags).or(resync).or(list).or(create).or(create_log).or(list_log).or(print).or(get_one).or(update).or(delete)
}

/// Defines routes for the project subsystem.
///
/// URL structure:
///   /api/v1/project                — list/create
///   /api/v1/project/:id            — metadata only
///   /api/v1/project/:id/detail     — aggregated todos+notes+logs+lists
///   /api/v1/project/:id/archive    — archive (never deletes)
///   /api/v1/project/:id/restore    — un-archive (reverse of archive)
///   /api/v1/project/:id            — DELETE permanently deletes (archived projects only)
///   /api/v1/project/:id/todos      — scoped todos, GET + POST
///   /api/v1/project/:id/notes      — scoped notes
///   /api/v1/project/:id/logs       — scoped log entries, ?days=
///   /api/v1/project/:id/lists      — scoped list categories
fn project_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let project_seg = warp::path("project");

    // GET /api/v1/project
    let list = project_seg
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_projects_handler);

    // POST /api/v1/project
    let create = project_seg
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(create_project_handler);

    // GET /api/v1/project/:id/detail
    let detail = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("detail"))
        .and(warp::path::end())
        .and(warp::get())
        .and_then(project_detail_handler);

    // POST /api/v1/project/:id/archive
    let archive = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("archive"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(archive_project_handler);

    // POST /api/v1/project/:id/restore
    let restore = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("restore"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(restore_project_handler);

    // DELETE /api/v1/project/:id
    let delete_one = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_project_handler);

    // GET /api/v1/project/:id/todos
    let todos = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("todos"))
        .and(warp::path::end())
        .and(warp::get())
        .and_then(project_todos_handler);

    // POST /api/v1/project/:id/todos
    let create_todo = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("todos"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|id, item| create_project_todo_handler(id, item));

    // GET /api/v1/project/:id/notes
    let notes = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("notes"))
        .and(warp::path::end())
        .and(warp::get())
        .and_then(project_notes_handler);

    // GET /api/v1/project/:id/logs?days=
    let logs = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("logs"))
        .and(warp::path::end())
        .and(warp::get())
        .and(query::<DailyLogQuery>())
        .and_then(|id, q| project_logs_handler(id, q));

    // GET /api/v1/project/:id/lists
    let lists = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path("lists"))
        .and(warp::path::end())
        .and(warp::get())
        .and_then(project_lists_handler);

    // GET /api/v1/project/:id
    let get_one = project_seg
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::get())
        .and_then(get_project_handler);

    list.or(create)
        .or(detail)
        .or(archive)
        .or(restore)
        .or(delete_one)
        .or(todos)
        .or(create_todo)
        .or(notes)
        .or(logs)
        .or(lists)
        .or(get_one)
}

/// Defines routes related to logging.
fn log_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::path("logs")
        .and(warp::get())
        .and(query::<LogQuery>())
        .and_then(read_logs_handler)
}


/// Defines the API routes.
pub fn routes(
    systems_status: SystemsStatus,
    go_nogo_status: SystemsGoNogo,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let api_v1 = warp::path("api").and(warp::path("v1"));

    // Task detail page — lives outside /api/v1/ so QR-code URLs are short.
    let task_page = warp::path("todo")
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::get())
        .and_then(get_todo_page_handler);

    // Note viewer page — outside /api/v1/ for clean URLs.
    let note_page = warp::path("notes")
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(warp::get())
        .and(query::<NoteIdQuery>())
        .and_then(get_note_page_handler);

    task_page.or(note_page).or(
        api_v1.and(
            status_routes(systems_status, go_nogo_status)
            .or(todo_routes())
            .or(log_routes())
            .or(list_routes())
            .or(notes_routes())
            .or(project_routes())
        )
    )
    .recover(handle_rejection)
}

/// Starts the HTTP server.
pub async fn start_server(systems_status: SystemsStatus, go_nogo_status: SystemsGoNogo) {
    let routes = routes(systems_status, go_nogo_status);
    let addr = ([0, 0, 0, 0], 8080);
    info!("Starting API server on http://0.0.0.0:8080");
    warp::serve(routes).run(addr).await;
}
