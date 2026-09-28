use crate::protocol::{Command, Request, ServerMessage};
use anyhow::{Context, Result, bail};
use std::path::Path;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
};
pub(crate) struct Client {
    pub reader: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    next_request_id: u64,
}

impl Client {
    pub async fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket)
            .await
            .context("enco serve is not running")?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(reader).lines(),
            writer,
            next_request_id: 1,
        })
    }

    pub async fn send(&mut self, command: Command) -> Result<u64> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let mut data = serde_json::to_vec(&Request { id, command })?;
        data.push(b'\n');
        self.writer.write_all(&data).await?;
        Ok(id)
    }

    pub async fn request(&mut self, command: Command) -> Result<serde_json::Value> {
        let request_id = self.send(command).await?;
        while let Some(line) = self.reader.next_line().await? {
            match serde_json::from_str::<ServerMessage>(&line)? {
                ServerMessage::Ok { id, data } if id == request_id => return Ok(data),
                ServerMessage::Error { id, code, message } if id == request_id || id == 0 => {
                    bail!("{code}: {message}")
                }
                _ => {}
            }
        }
        bail!("daemon closed the connection")
    }
}
