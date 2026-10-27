<div align="center">

# Enco

Enco is a personal agent assistant that runs on your own machines. Built on a Rust kernel and WebAssembly plugins, able to modify, verify and roll back its own capabilities while running.
> Light, free, self-building. Always on.

**English** · [简体中文](README.zh-CN.md)

[![License](https://img.shields.io/github/license/Darkatse/Enco?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99.0-b7410e?style=flat-square&logo=rust)](rust-toolchain.toml)

</div>

> [!NOTE]
> Enco is still in early development. The kernel (P0), plugin hot-swapping and automatic rollback (P1), self-maintained plugins (P2), and conversation backtracking, Telegram and Discord channel plugins (P3 M30 to M32) are implemented. See the [roadmap](#roadmap) for later phases. Features, interfaces and configuration formats may change before 1.0.

## What Enco is

Enco is designed as a personal assistant for a single owner. It runs with full permissions on its host machine, writes everything it does to an append-only log, and keeps its own memory store.

An agent ought to be free and adaptable, yet the real world is full of constraints. Enco sets out to answer one question: when the code, process or machine underneath a running agent changes, how does it adapt and continue safely?

The entire system follows one baseline rule: no change or migration submitted through the official path may break the agent's or the owner's ability to repair it. The full design lives in [docs/design/](docs/design/README.md) (in Chinese).

## Design

### Every capability is a plugin, guarded by the kernel

Every capability lives in a WebAssembly plugin. Enco can rewrite or replace any of them while running.

Model providers (OpenAI-Compatible, Gemini, DeepSeek), relevance decisions (TypeSafe), web search and fetch tools, the policy that assembles each model request (`composer`), and chat channels (Telegram, Discord) are all standalone WebAssembly components. The kernel only records facts, determines which code serves each round, validates requests and schedules execution. If you dislike how a plugin works, fork it, or ask your Enco to write a new one.

A plugin can only call what the [WIT contract](sdk/wit/CONTRACT.md) exposes, while the kernel enforces type and rollback safety, eliminating plugin dependency hell at the root.

### Live self-modification and automatic rollback

The agent is both the user of Enco and its long-term maintainer. Enco compiles the SDK and host documentation into a local manual automatically, so the agent can fix or write plugins by reading the manual and plugin source alone, without reading Enco's host source.

Changes to plugins, configuration and API endpoints are validated and registered together. Any crash or failure is rolled back to the last healthy version by a formal mechanism. When all plugins fail and cannot recover, Enco switches to a minimal built-in safe mode so it stays online, and stays with you.

### The record is its continuity

Every action leaves a record. That record is Enco's continuity: it recovers from crashes and settles on any machine. Designed for cyberspace from day one.

Every model request and tool call is written to an append-only log before it runs, so the model only ever sees recorded requests. A pure-function `composer` plugin assembles each request deterministically and commits it to disk, allowing work to resume across crashes or restarts. The SQLite log is the agent's anchor: a statically linked Rust binary plus a single-file SQLite log lets Enco run anywhere, restart anytime and migrate at will.

### Long-term memory

Long-term memories live in a dedicated SQLite database as the single source of truth. Recall goes through a [TriviumDB](https://github.com/YoKONCy/TriviumDB) index that combines vector search with keyword search tuned for Chinese text.

Memory is the soul of a personal assistant. Enco has shipped with a private memory store from day one: simple in structure, and effective in practice.

### External inputs enter only through mappers

External data is strictly typed into Enco's internal format, keeping every input consistent and reproducible.

Third-party tool servers (such as MCP), model services, web pages, documents, command output and chat protocols sit outside Enco's control, and enter the system only through mappers. A mapper is a zero-dependency WebAssembly component whose sole job is translating raw input into Enco's own types: it requires no trust, and the same input always yields the same result. Strong typing is the foundation of reliability.

### One ghost, roaming across machines (P4 and P5)

The idea is close to Ghost in the Shell: the assistant is the ghost, and each of your devices is a shell it can inhabit. Your phone, computer and server form one Space. Mid-conversation, the agent can leave your phone and continue on your server with its full record, while still calling back to the phone's camera or location remotely (P5).

Membership and ownership are maintained by a consensus control plane (CP). Chat connections and timers use leases so another node takes over when the holder disappears, while during a network split the minority side continues running its own sessions. Even the single-node version already stores data in the multi-node shape (ULIDs, `(epoch, seq)` log positions, ownership bindings and content-addressed files).

When a node hits a fault (network outage, hardware failure or power loss), chat connections and timers automatically fail over to another node once their lease TTL expires; active sessions use sticky ownership so two nodes never execute the same session at once, and can be safely taken over on another node with the owner's confirmation. Once recovered, the node syncs back to the latest state automatically.

### Small concept set and runtime overhead

The whole system is described with ten concepts and two rules for mutable state: each piece of state has one owner that applies changes in order, and callbacks return their writes for the owner to commit in one transaction. Chat connections, stateful tools and derived indexes all follow these two rules instead of each having a framework of its own. The design keeps a fixed vocabulary, where each word names one thing.

The kernel runs on tokio and the plugins on Wasmtime. On the desktop prototype, an incremental plugin build took about 1 s, compiling a component about 27 ms, creating an instance about 40 µs and calling it about 1.5 µs. Plugin overhead is small next to model latency, so performance work goes mainly into avoiding copies of large data.

Enco currently runs on macOS and Linux as a background service with a command-line client, and has been tested on macOS. Phones join later as full nodes (P5): Android runs plugins with Cranelift and iOS with the Pulley interpreter, handing active conversations back to an always-on node before the OS suspends the app. Windows support is on the roadmap, with Unix first: there the local protocol will use named pipes instead of Unix domain sockets.

## Roadmap

| Phase | Goal | Status |
|---|---|---|
| P0 | Kernel: sessions, durable log, crash recovery, OpenAI-Compatible and DeepSeek provider plugins, memory, reminders, safe mode | Done |
| P1 | Replacing plugins and configuration at runtime with health checks and rollback | Done |
| P2 | The agent maintains its own plugins from the generated manual and plugin source, verified by black-box evals | Done |
| P3 | Channels as plugins: conversation backtracking (M30), Telegram (M31) and Discord (M32) mappers, QQ OneBot (M33) | In progress (M30 to M32 done) |
| Supervision tree and scripts | Delegating work to child sessions on other models; sandboxed scripts orchestrating tool calls | Planned |
| P4 | Multiple devices: consensus control plane, session roaming, replication | Planned |
| P5 | Phones as nodes | Planned |

Each phase has acceptance criteria, listed in [docs/Roadmap.md](docs/Roadmap.md).

## AI-assisted setup

Hand the [Enco setup and usage skill](skills/enco-installation/SKILL.md) to your coding agent (such as Claude Code or Codex) to walk through building from source, initializing `~/.enco`, configuring models and memory, connecting Telegram and Discord, and verifying `enco status`. Tools that support skills can load the `skills/enco-installation` directory directly, or you can ask your assistant to read the file.

## Installation and development

### One-line install

On Linux and macOS (x64 and arm64):

```bash
curl -fsSL https://install.enco.sh | sh
```

By default the script installs the latest release, or the latest nightly build. To let Enco write and change its own plugins, also install [rustup](https://rustup.rs).

### Docker

The image `ghcr.io/darkatse/enco:edge` includes the pinned Rust toolchain, so Enco can build plugins inside the container. You can use [`docker-compose.yaml`](docker-compose.yaml):

```bash
mkdir -p enco-home
docker compose run --rm enco init
# edit enco-home/.enco/config.toml and put the API keys it names in .env
docker compose up -d
docker compose exec enco enco chat
```

### Building from source

Requirements: Rust 1.99.0 with the `wasm32-wasip2` target. The versions are pinned in `rust-toolchain.toml`, so `rustup` installs them on first build.

```bash
git clone https://github.com/Darkatse/Enco.git
cd Enco
cargo fetch                   # prefetch once; xtask commands run offline
for manifest in sdk/Cargo.toml evals/runner/Cargo.toml plugins/*/Cargo.toml; do
    cargo fetch --manifest-path "$manifest"
done
cargo xtask build-factory      # build the bundled plugins
cargo run -p enco -- init      # create ~/.enco and a config template
```

Edit `~/.enco/config.toml` (examples are in [`examples/`](examples/)), export the API key named in it, then start the service:

```bash
cargo run -p enco -- serve     # start the service
cargo run -p enco -- chat      # in another terminal
cargo run -p enco -- send --image photo.png "Describe this image"
```

Standing instructions live in `~/.enco/AGENTS.md` and runtime data lives in `~/.enco/.data/`; set `ENCO_HOME` to use a directory other than `~/.enco`. For day-to-day commands, image and file messages, conversation backtracking, plugin management, and Telegram or Discord setup, see the [usage and channel guide](docs/Usage.md) and [plugins/README.md](plugins/README.md) (in Chinese).

Before sending repository changes, run the offline gate:

```bash
cargo xtask check
```

## Documentation

- [docs/Usage.md](docs/Usage.md): CLI commands, multimodal messages, plugin management, and Telegram / Discord setup (Chinese)
- [skills/enco-installation/SKILL.md](skills/enco-installation/SKILL.md): installation and configuration skill for coding agents (Chinese)
- [docs/design/](docs/design/README.md): the design outline and the judgments behind each area (Chinese)
- [docs/Roadmap.md](docs/Roadmap.md): phases and their acceptance criteria (Chinese)
- [LESSONS.md](LESSONS.md): approaches that failed in practice; read before changing code (Chinese)
- [sdk/wit/CONTRACT.md](sdk/wit/CONTRACT.md): the plugin contract, generated from WIT
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

The design also looks through five lenses. Erlang gave the idea of processes that own their state and swap code under supervision. Plan 9 gave per-process namespaces and the `import` and `cpu` commands, which became the capability snapshot of a round, `invoke` and `roam`. Smalltalk showed a live system that changes itself while running. Lisp treats code as data, and Enco treats its log and every model request the same way. Boundary types translate foreign data once, at the edge, into a language's own types: F# and Idris type providers, Elm decoders, Typed Racket's typed imports with blame, and [TypeScript's content mappers](https://github.com/microsoft/typescript-go/pull/4712). Enco's mappers follow them, and add what their history taught: mappers cannot perform I/O, and external contracts are pinned instead of queried live.

Enco is built on [Wasmtime](https://wasmtime.dev/), [tokio](https://tokio.rs/) and [SQLite](https://sqlite.org/). Memory recall uses [TriviumDB](https://github.com/YoKONCy/TriviumDB) by [@YoKONCy](https://github.com/YoKONCy).

## License

Released under the [Apache License 2.0](LICENSE).
