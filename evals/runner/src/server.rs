use anyhow::{Context, Result};
use http_body_util::Full;
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{future::Future, net::SocketAddr};
use tokio::{net::TcpListener, sync::oneshot, task::JoinSet};

pub(crate) type Reply = Response<Full<Bytes>>;

/// A trial owns all listeners and stops their connections before exporting state.
#[derive(Default)]
pub(crate) struct Servers {
    stops: Vec<oneshot::Sender<()>>,
    tasks: JoinSet<Result<()>>,
}

impl Servers {
    pub(crate) async fn start<F, R>(&mut self, handler: F) -> Result<SocketAddr>
    where
        F: Fn(Request<Incoming>) -> R + Clone + Send + Sync + 'static,
        R: Future<Output = Result<Reply>> + Send + 'static,
    {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let (stop, mut stopped) = oneshot::channel();
        self.stops.push(stop);
        self.tasks.spawn(async move {
            let mut connections = JoinSet::new();
            let result = loop {
                tokio::select! {
                    _ = &mut stopped => break Ok(()),
                    accepted = listener.accept() => {
                        let (stream, _) = match accepted {
                            Ok(pair) => pair,
                            Err(error) => break Err(error.into()),
                        };
                        let handler = handler.clone();
                        connections.spawn(async move {
                            // Request and disconnect errors belong to this client connection.
                            let _ = http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service_fn(handler))
                                .await;
                        });
                    }
                    Some(joined) = connections.join_next() => {
                        if let Err(error) = joined { break Err(error.into()); }
                    }
                }
            };
            drop(listener);
            connections.shutdown().await;
            result
        });
        Ok(address)
    }

    pub(crate) async fn stop(mut self) -> Result<()> {
        self.stops.clear();
        let mut result = Ok(());
        while let Some(joined) = self.tasks.join_next().await {
            result = result.and(
                joined
                    .context("HTTP listener task")
                    .and_then(|result| result),
            );
        }
        result
    }
}

pub(crate) fn reply(status: hyper::StatusCode, body: impl Into<Bytes>) -> Reply {
    let mut response = Response::new(Full::new(body.into()));
    *response.status_mut() = status;
    response
}
