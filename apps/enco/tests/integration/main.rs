//! Integration tests share one binary, so an edit relinks the host once rather than per file.
mod channel_mapper;
mod channel_socket;
#[path = "../../src/client.rs"]
mod client;
mod composer;
mod configuration;
mod daemon;
mod decision;
mod development;
mod discord_mapper;
mod gemini;
mod live;
mod memory;
mod onebot;
mod process;
#[path = "../../src/protocol.rs"]
mod protocol;
mod quotes;
mod reminder;
mod send;
mod support;
mod telegram_images;
mod telegram_outgoing;
mod web;
