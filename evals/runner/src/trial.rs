use crate::{
    config::{Config, Deploy, Model},
    enco::{Binary, Enco},
    grade::Grading,
    home::Home,
    judge,
    mock::Mock,
    proxy::Proxy,
    render::Renderer,
    result::{self, Execution, Facts, Trial},
    server::{Servers, reply},
    snapshot::{self, Snapshot},
    task::Task,
};
use anyhow::Result;
use http_body_util::BodyExt;
use hyper::{
    StatusCode,
    header::{CONTENT_TYPE, HeaderValue},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::Instant;

pub(crate) struct TrialDeps {
    pub(crate) repository: PathBuf,
    pub(crate) directory: PathBuf,
    pub(crate) config: Config,
    /// The check command selects the fixed-reply model by leaving this absent.
    pub(crate) model: Option<Model>,
    pub(crate) judge: judge::Client,
}

pub(crate) struct Variant {
    pub(crate) name: String,
    pub(crate) binary: Binary,
    pub(crate) deploy: Vec<Deploy>,
    pub(crate) plugin: toml::Table,
    pub(crate) variables: BTreeMap<String, OsString>,
}

pub(crate) async fn run(
    deps: &TrialDeps,
    task: &Task,
    variant: &Variant,
    index: usize,
) -> Result<Trial> {
    let began = Instant::now();
    let renderer = Renderer::for_trial(task)?;
    let start = renderer.start;
    let relative = PathBuf::from("trials")
        .join(&task.id)
        .join(&variant.name)
        .join(index.to_string());
    let directory = deps.directory.join(&relative);
    let snapshot_dir = directory.join("snapshot");
    tokio::fs::create_dir_all(&snapshot_dir).await?;
    let mut home = Home::new(&deps.config.trial);
    let mut services = Services::default();
    let prepared = prepare(
        deps,
        task,
        &renderer,
        &mut home,
        &mut services,
        &directory,
        variant,
    )
    .await;
    let executed = match prepared {
        Ok(log) => {
            execute(
                variant,
                task,
                &home,
                log,
                &snapshot_dir,
                began + Duration::from_secs(task.definition.budget.timeout_secs),
                &[&deps.repository, &deps.directory],
            )
            .await
        }
        Err(error) => Ok((
            Execution::InstrumentError {
                reason: format!("prepare: {error:#}"),
            },
            Snapshot::default(),
        )),
    };
    // All paths close the owned resources before returning a trial or a deployment error.
    let cleanup = services.finish(&snapshot_dir).await;
    let archive = home.archive(&directory.join("home")).await;
    let (mut execution, snapshot) = executed?;
    let violations = match cleanup {
        Ok(count) => count,
        Err(error) => {
            execution.mark_instrument_error(error);
            0
        }
    };
    if let Err(error) = archive {
        execution.mark_instrument_error(error);
    }
    let wall_secs = began.elapsed().as_secs_f64();
    let facts = Facts {
        start,
        home: home.path.clone(),
        execution,
        export_errors: snapshot.export_errors,
    };
    result::write_json(&directory.join("trial.json"), &facts).await?;
    let grading = Grading {
        task,
        snapshot: &snapshot_dir,
        trial: &directory,
        output: &directory,
        renderer,
        judge: Some(&deps.judge),
    };
    let criteria = if matches!(facts.execution, Execution::InstrumentError { .. }) {
        BTreeMap::new()
    } else {
        grading.all(None).await
    };
    result::write_json(&directory.join("grades.json"), &criteria).await?;
    let (result, reason) = result::outcome(&facts.execution, &criteria);
    Ok(Trial {
        task: task.id.clone(),
        variant: variant.name.clone(),
        index,
        dir: relative,
        start,
        result,
        reason,
        criteria,
        rounds: snapshot.metrics.rounds,
        wall_secs,
        usage: snapshot.metrics.usage,
        generations: snapshot.metrics.generations,
        network_violations: violations,
    })
}

/// Start the mocks, the proxy and the model, then build the home; returns the daemon's log file.
async fn prepare(
    deps: &TrialDeps,
    task: &Task,
    renderer: &Renderer,
    home: &mut Home,
    services: &mut Services,
    directory: &Path,
    variant: &Variant,
) -> Result<std::fs::File> {
    let (model, address) = services.start(task, renderer, deps.model.as_ref()).await?;
    home.prepare(
        task,
        &model,
        renderer,
        &format!("http://{address}"),
        &services.hosts,
        variant,
    )
    .await?;
    Ok(tokio::fs::File::create(directory.join("daemon.log"))
        .await?
        .into_std()
        .await)
}

/// Only a failed variant deployment returns an error; it aborts the run.
async fn execute(
    variant: &Variant,
    task: &Task,
    home: &Home,
    log: std::fs::File,
    directory: &Path,
    deadline: Instant,
    forbidden: &[&Path],
) -> Result<(Execution, Snapshot)> {
    let mut enco = match Enco::start(&variant.binary, home, log) {
        Ok(enco) => enco,
        Err(error) => return Ok((Execution::failed(error), Snapshot::default())),
    };
    let driven = match enco.ready(deadline).await {
        Ok(()) => {
            for deploy in &variant.deploy {
                enco.deploy(deploy).await?;
            }
            inputs(&enco, task, deadline).await
        }
        Err(error) => Err(error),
    };
    let mut execution = match driven {
        Ok(()) => Execution::Completed,
        Err(error) => Execution::failed(error),
    };
    let exported = snapshot::export(&enco, directory, forbidden).await;
    let stopped = enco.stop().await;
    let snapshot = match exported {
        Ok(snapshot) => snapshot,
        Err(error) => {
            execution.mark_instrument_error(error);
            Snapshot::default()
        }
    };
    if matches!(execution, Execution::Completed) {
        if let Some(reason) = snapshot.failures.first() {
            execution = Execution::failed(reason);
        } else if let Some((name, reason)) = snapshot.export_errors.first_key_value() {
            execution = Execution::failed(format!("export {name}: {reason}"));
        }
    }
    if let Err(error) = stopped {
        execution.mark_instrument_error(error);
    }
    Ok((execution, snapshot))
}

async fn inputs(enco: &Enco, task: &Task, deadline: Instant) -> Result<()> {
    let sent = async {
        for input in &task.definition.input {
            enco.send(&input.session, &input.text, deadline).await?;
        }
        Ok(())
    }
    .await;
    let idle = enco.idle(deadline).await;
    sent.and(idle)
}

#[derive(Default)]
struct Services {
    servers: Servers,
    mocks: BTreeMap<String, Mock>,
    proxy: Option<Proxy>,
    hosts: BTreeSet<String>,
}

impl Services {
    async fn start(
        &mut self,
        task: &Task,
        renderer: &Renderer,
        model: Option<&Model>,
    ) -> Result<(Model, std::net::SocketAddr)> {
        let mut routes = BTreeMap::new();
        for (name, definition) in &task.mocks {
            let mock = Mock::start(&mut self.servers, &task.root, definition, renderer).await?;
            routes.insert(definition.host.clone(), mock.address);
            self.mocks.insert(name.clone(), mock);
        }
        let model = match model {
            Some(model) => model.clone(),
            None => {
                let address = null_model(&mut self.servers).await?;
                let host = "model.enco.test";
                routes.insert(host.into(), address);
                null_configuration(host)?
            }
        };
        if !routes.contains_key(crate::reader::HOST) {
            let address = crate::reader::start(&mut self.servers, routes.clone()).await?;
            routes.insert(crate::reader::HOST.into(), address);
        }
        self.hosts = routes.keys().cloned().collect();
        let proxy = Proxy::start(&mut self.servers, routes).await?;
        let address = proxy.address;
        self.proxy = Some(proxy);
        Ok((model, address))
    }

    async fn finish(self, snapshot: &Path) -> Result<usize> {
        self.servers.stop().await?;
        for (name, mock) in self.mocks {
            mock.export(&snapshot.join("mock").join(name)).await?;
        }
        match self.proxy {
            Some(proxy) => proxy.export(&snapshot.join("network.jsonl")).await,
            None => Ok(0),
        }
    }
}

async fn null_model(servers: &mut Servers) -> Result<std::net::SocketAddr> {
    servers.start(|request| async move {
        let path = request.uri().path().to_owned();
        let bytes = request.into_body().collect().await?.to_bytes();
        let body: Value = serde_json::from_slice(&bytes)?;
        let value = match path.as_str() {
            "/chat/completions" => json!({ "choices": [{ "message": { "role": "assistant", "content": "好的。" }, "finish_reason": "stop" }], "usage": { "prompt_tokens": 0, "completion_tokens": 0 } }),
            "/embeddings" => {
                let count = body["input"].as_array().map_or(1, Vec::len);
                let data: Vec<_> = (0..count).map(|index| json!({ "index": index, "embedding": vec![0.0; crate::limits::NULL_EMBEDDING_DIMENSIONS] })).collect();
                json!({ "data": data })
            }
            _ => return Ok(reply(StatusCode::NOT_FOUND, "not found")),
        };
        let mut response = reply(StatusCode::OK, serde_json::to_vec(&value)?);
        response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(response)
    }).await
}

fn null_configuration(host: &str) -> Result<Model> {
    let source = format!(
        r#"[endpoint.chat]
plugin = "openai-compatible"
base_url = "http://{host}"
model = "chat"
window_tokens = 128000
max_output_tokens = 8192
[profile.default]
reply = ["chat"]
compaction = ["chat"]
[memory]
embedding = "embed"
[endpoint.embed]
plugin = "openai-compatible"
base_url = "http://{host}"
model = "embedding"
dimensions = {}
"#,
        crate::limits::NULL_EMBEDDING_DIMENSIONS
    );
    Model::parse(source)
}
