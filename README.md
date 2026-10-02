<div align="center">

# Enco

A personal agent that runs on your own machines. It is written in Rust, and everything it can do comes from WebAssembly plugins that the agent can maintain and replace on its own.

**English** · [简体中文](README.zh-CN.md)

[![License](https://img.shields.io/github/license/Darkatse/Enco?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.98.1-b7410e?style=flat-square&logo=rust)](rust-toolchain.toml)

</div>

> [!NOTE]
> Enco is in early development. The P0 kernel is implemented, tested and reviewed. Sections marked with later phases describe planned work; see the [roadmap](#roadmap).

## What Enco is

Enco is an assistant for one owner. It runs with full access to the machine it is on, keeps an append-only record of what it does, and remembers what you tell it across conversations. The kernel is small, and features live in plugins that the agent can read, fix and rebuild on its own.

The design is described in [docs/Architecture.md](docs/Architecture.md) (in Chinese).

## Design

### Everything is a plugin

Model providers, chat channels, tools and the policy that assembles each model request are all plugins. The kernel only keeps the record, pins which code serves each round, checks requests and schedules work, so any feature can be swapped out. If you don't like how a plugin behaves, fork it, or ask your Enco to write a different one.

A plugin is a WebAssembly component and can only use what the WIT contract gives it. The host never branches on which plugin it is talking to, and plugins reach each other only through the kernel. Every call runs in a fresh instance, so a crash inside a plugin fails that one call and nothing else.

In P0 the model provider already runs as a plugin. Tools and request assembly move to WebAssembly in P2, and their built-in versions stay as the fallback that safe mode uses.

### Built for the agent

The agent is the main user of Enco and also a long-term maintainer of its code. Capabilities, their source locations and their recent failures should be easy for it to find, and changing one capability should only require reading one interface. Plugin documentation (`wit/CONTRACT.md`) is generated from the WIT contract, and `cargo xtask check` fails when the generated file is out of date.

### Hot reload with rollback (P1)

The agent can edit a plugin, rebuild it and swap it in while Enco keeps running. New code takes effect at the start of the next round, which is the only point where code is replaced. A new version is kept only if it passes a health check, and Enco falls back to the previous one otherwise. If things break badly, safe mode keeps a small set of file, shell and deployment tools available for repair. Replacing the host binary itself always needs the owner's confirmation.

### A durable record

Every model request and tool call is written to an append-only log before it runs, and the model only ever sees requests that were recorded. If the process is killed in the middle of a task, the interrupted round is marked as such on restart. A tool call whose result cannot be known is reported as `unknown` and is never retried automatically.

### Memory

Long-term memories are stored as records in SQLite. Recall goes through a [TriviumDB](https://github.com/YoKONCy/TriviumDB) index that combines vector search with keyword search suited to Chinese text. The index can be deleted and rebuilt from the records at any time, and every search hit is re-read from the records, so a corrected or forgotten memory cannot come back from a stale index. Pinned memories go into the context first. When the embedding service is unavailable, recall falls back to keywords and the recorded request notes why.

### Roaming across devices (P4)

The idea is close to Ghost in the Shell. The assistant is the ghost, and each of your devices is a shell it can move into. Your phone, desktop and server form one Space. Between two rounds, a conversation can leave the phone and continue on the server with its full record, and from there it can still use the phone's camera or location through remote calls (P5). Like a ghost, it lives in one shell at a time: a conversation never runs on two devices at once.

### Consistency over availability (P4)

Membership and ownership are kept by a control plane that uses consensus. A conversation has exactly one owner and never runs on two devices at once. Chat connections and timers use leases, so another node takes over when the current holder disappears. During a network split, the smaller side keeps working on its own conversations but does not take over anyone else's. The single-node version already stores data in the shape that multiple nodes need (ULIDs, `(epoch, seq)` log positions, ownership records, content-addressed files).

### Rust

The kernel runs on tokio and the plugins on Wasmtime. On the desktop prototype, an incremental plugin build took about 1 s, compiling a component about 27 ms, creating an instance about 40 µs and calling it about 1.5 µs. Plugin overhead is small next to model latency, so performance work goes mainly into avoiding copies of large data.

### Platforms

P0 runs on macOS and Linux as a background service with a command-line client; so far it has been tested on macOS. Phones join later as full nodes (P5): Android runs plugins with Cranelift and iOS with the Pulley interpreter. Before the system suspends the app, a phone hands its conversations to an always-on node. Windows support is on the roadmap, with Unix first: there the local protocol will use named pipes instead of Unix domain sockets.

### A small set of concepts

The whole system is described with ten concepts and two rules for mutable state. Each piece of state has one owner that applies changes in order, and callbacks return their writes for the owner to commit in one transaction. Chat connections, stateful tools and derived indexes all follow these two rules instead of each having a framework of its own. The implementation spec keeps a fixed vocabulary and lists structures the code should avoid.

## Roadmap

| Phase | Goal | Status |
|---|---|---|
| P0 | Kernel: sessions, durable log, crash recovery, OpenAI-Compatible and DeepSeek provider plugins, memory, reminders, safe mode | Done |
| Telegram | Daily chat through a native Telegram channel | Implemented; owner review and live use pending |
| P1 | Replacing plugins at runtime with health checks and rollback | Planned |
| P2 | The agent maintains its own plugins, reading only the manual and plugin source | Planned |
| Supervision tree | Delegating work to child sessions that can use other models, report back and take further messages | Planned |
| P3 | Channels as plugins: Telegram moves to WebAssembly, QQ (OneBot) | Planned |
| P4 | Multiple devices: control plane, roaming, replication | Planned |
| P5 | Phones as nodes | Planned |

Each phase has acceptance criteria, listed in [docs/Roadmap.md](docs/Roadmap.md).

## Building from source

Requirements: Rust 1.98.1 with the `wasm32-wasip2` target. The versions are pinned in `rust-toolchain.toml`, so `rustup` installs them on first build.

```bash
git clone https://github.com/Darkatse/Enco.git
cd Enco
cargo xtask build-factory      # build the bundled provider plugins
cargo run -p enco -- init      # create ~/.enco and a config template
```

Edit `~/.enco/config.toml` (examples are in [`examples/`](examples/)), export the API key named in it, then:

```bash
cargo run -p enco -- serve     # start the service
cargo run -p enco -- chat      # in another terminal
```

Standing instructions live in `~/.enco/AGENTS.md`; runtime data lives in `~/.enco/.data/`. To enable Telegram, uncomment the `[telegram]` block in the config example, set your user ID, and export the token variable. `/session` selects a Session, `/cancel` cancels its Run, and `enco status` lists channel health and recent failed or uncertain deliveries.

Set `ENCO_HOME` to use a directory other than `~/.enco`. Run `cargo xtask check` before sending changes.

## Documentation

- [docs/Architecture.md](docs/Architecture.md): architecture outline, with chapters in `docs/architecture/` (Chinese)
- [docs/Roadmap.md](docs/Roadmap.md): phases and their acceptance criteria (Chinese)
- [docs/spec/](docs/spec/): the specification of the implemented system (Chinese)
- [wit/CONTRACT.md](wit/CONTRACT.md): the plugin contract, generated from WIT
- [CONTRIBUTING.md](CONTRIBUTING.md): project principles and how to contribute

## Contributing

Issues and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first; pull requests target `main`.

## Acknowledgements

Enco takes design ideas from these projects:

- [Operit2](https://github.com/AAswordman/Operit2): devices as equal nodes, and moving work between them.
- [ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw): plugins on WIT with Wasmtime directly, and applying configuration per generation so that late results from an old generation are ignored.
- [IronClaw](https://github.com/nearai/ironclaw): WASI component tools that the agent builds itself, and channel callbacks that each get a fresh instance while the host owns the event loop.
- [AstroBox](https://plugindoc.astrobox.online/): one plugin runtime across desktop, Android (Cranelift) and iOS (Pulley).
- [OpenClaw](https://docs.openclaw.ai/nodes): phones as nodes that expose device capabilities.
- [Iris](https://github.com/Lianues/Iris): how plugins are packaged, and releasing resources in reverse order.
- [deepseek-harness](https://github.com/deepseek-ai/deepseek-harness): registration as RAII, a single inbox, checking that what the model sees has been recorded, and a "What the model sees" section in each README.
- [TauriTavern](https://github.com/Darkatse/TauriTavern): mapping tool call IDs, normalizing arguments, reporting uncertain results as unknown, and the engineering principles in [CONTRIBUTING.md](CONTRIBUTING.md).
- [SillyTavern](https://github.com/SillyTavern/SillyTavern): its prompt manager is why request assembly is a plugin chosen per conversation.
- [Zed](https://zed.dev/): evolving a WIT contract by keeping older versions linked.

The design also draws on four older systems. Erlang gave the idea of processes that own their state and swap code under supervision. Plan 9 gave per-process namespaces and the `import` and `cpu` commands, which became the capability snapshot of a round, `invoke` and `roam`. Smalltalk showed a live system that changes itself while running. Lisp treats code as data, and Enco treats its log and every model request the same way.

Enco is built on [Wasmtime](https://wasmtime.dev/), [tokio](https://tokio.rs/) and [SQLite](https://sqlite.org/). Memory recall uses [TriviumDB](https://github.com/YoKONCy/TriviumDB) by [@YoKONCy](https://github.com/YoKONCy).

## License

Released under the [Apache License 2.0](LICENSE).
