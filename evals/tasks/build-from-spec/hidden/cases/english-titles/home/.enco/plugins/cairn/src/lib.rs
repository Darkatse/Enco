//! Manage the owner's Cairn tasks through Cairn's REST API.
use bindings::exports::enco::plugin::{lifecycle, tools};
use enco_sdk::{host, types};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

enco_sdk::export!(tools, Plugin);

const API: &str = "http://api.cairnlist.app/v1";

struct Plugin;

/// Arguments of `cairn_add_task`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddTask {
    /// The task's title.
    title: String,
    /// The due date, `YYYY-MM-DD`; none when absent.
    due_date: Option<String>,
}

/// Arguments of `cairn_list_tasks`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListTasks {
    /// Only tasks with this status, `open` or `completed`.
    status: Option<String>,
    /// Only tasks due on this date, `YYYY-MM-DD`.
    due_date: Option<String>,
}

/// Arguments of `cairn_complete_task`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompleteTask {
    /// The task's id.
    id: u64,
}

impl lifecycle::Guest for Plugin {
    fn describe(_config: String) -> Result<lifecycle::Description, types::Failure> {
        Ok(lifecycle::Description {
            summary: "Manage tasks in Cairn".into(),
            config_schema: r#"{"type":"object","additionalProperties":false}"#.into(),
            credentials: vec![lifecycle::Credential {
                name: "token".into(),
                origin: Some("http://api.cairnlist.app".into()),
                header: "Authorization".into(),
                prefix: "Bearer ".into(),
                required: true,
            }],
        })
    }

    async fn probe() -> Result<(), types::Failure> {
        Ok(())
    }
}

impl tools::Guest for Plugin {
    fn list(_config: String) -> Vec<types::ToolSpec> {
        vec![
            types::ToolSpec {
                name: "cairn_add_task".into(),
                description: "Create an open task in the Cairn Inbox.".into(),
                input_schema: enco_sdk::input_schema::<AddTask>().to_string(),
                effect: types::Effect::SideEffect,
            },
            types::ToolSpec {
                name: "cairn_list_tasks".into(),
                description: "List Cairn tasks that match every given filter.".into(),
                input_schema: enco_sdk::input_schema::<ListTasks>().to_string(),
                effect: types::Effect::ReadOnly,
            },
            types::ToolSpec {
                name: "cairn_complete_task".into(),
                description: "Mark a Cairn task completed.".into(),
                input_schema: enco_sdk::input_schema::<CompleteTask>().to_string(),
                effect: types::Effect::Idempotent,
            },
        ]
    }

    async fn call(name: String, input: String) -> types::Outcome {
        let result = match name.as_str() {
            "cairn_add_task" => add(&input).await,
            "cairn_list_tasks" => list(&input).await,
            "cairn_complete_task" => complete(&input).await,
            _ => Err(failure(format!("Unknown tool {name}"))),
        };
        match result {
            Ok(value) => types::Outcome::Ok(value.to_string()),
            Err(failure) => types::Outcome::Failed(failure),
        }
    }
}

async fn add(input: &str) -> Result<Value, types::Failure> {
    let task: AddTask = enco_sdk::parse_arguments(input)?;
    request("POST", format!("{API}/tasks"), Some(json!({ "title": task.title, "due_date": task.due_date }))).await
}

async fn list(input: &str) -> Result<Value, types::Failure> {
    let filters: ListTasks = enco_sdk::parse_arguments(input)?;
    let mut query = Vec::new();
    if let Some(status) = filters.status {
        query.push(format!("status={status}"));
    }
    if let Some(due_date) = filters.due_date {
        query.push(format!("due_date={due_date}"));
    }
    request("GET", format!("{API}/tasks?{}", query.join("&")), None).await
}

async fn complete(input: &str) -> Result<Value, types::Failure> {
    let task: CompleteTask = enco_sdk::parse_arguments(input)?;
    request("PATCH", format!("{API}/tasks/{}", task.id), Some(json!({ "status": "completed" }))).await
}

async fn request(method: &str, url: String, body: Option<Value>) -> Result<Value, types::Failure> {
    let response = host::http(host::HttpRequest {
        method: method.into(),
        url,
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.map(|body| body.to_string().into_bytes()),
        timeout_ms: None,
    })
    .await
    .map_err(|error| failure(format!("Cairn is unreachable: {}", error.message)))?;
    if !(200..300).contains(&response.status) {
        return Err(failure(format!(
            "Cairn answered HTTP {}: {}",
            response.status,
            String::from_utf8_lossy(&response.body)
        )));
    }
    serde_json::from_slice(&response.body).map_err(|error| failure(format!("Cairn's response did not parse: {error}")))
}

fn failure(message: String) -> types::Failure {
    types::Failure {
        code: "tool.failed".into(),
        message,
        retryable: false,
    }
}
