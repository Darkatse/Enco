<div align="center">

# Enco

A personal agent that runs on your own machines. Built on a Rust kernel and a WebAssembly plugin boundary, able to modify, verify and roll back its own capabilities while running.

**English** · [简体中文](README.zh-CN.md)

[![License](https://img.shields.io/github/license/Darkatse/Enco?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99.0-b7410e?style=flat-square&logo=rust)](rust-toolchain.toml)

</div>

> [!NOTE]
> Enco is in early development. The kernel (P0), hot reload with rollback (P1), self-maintained plugins (P2), and conversation backtracking, Telegram and Discord channel plugins (P3 M30 to M32) are implemented, tested and reviewed. Sections marked with later phases describe planned work; see the [roadmap](#roadmap).

## What Enco is

Enco is a personal assistant for one owner. It runs with full access to the machine it is on, writes everything it does to an append-only log, and remembers what you tell it across conversations.

Personal agents usually break down in two places: adding a capability means waiting on a developer to change the codebase, and switching machines, restarting a process or breaking a module drops the work in progress. Enco treats both as the same problem: when the code, process or machine underneath an active task changes, how does the work continue safely?

One rule is non-negotiable: no change or migration submitted through the official path may break the agent's or the owner's ability to repair it. The full design and the reasoning behind each decision live in [docs/design/](docs/design/README.md) (in Chinese).

## Design

### Small kernel, everything else in plugins

Model providers (OpenAI-Compatible, Gemini, DeepSeek), relevance decisions (TypeSafe), web search and fetch tools, the policy that assembles each model request (`composer`), and chat channels (Telegram, Discord) are all standalone WebAssembly components. The kernel only records facts, pins which code serves each round, checks requests and schedules work. If you do not like how a plugin behaves, fork it, or ask your Enco to write a new one.

A plugin can only use what the [WIT contract](sdk/wit/CONTRACT.md) gives it. The host never branches on which plugin it is talking to, and plugins reach each other only through the kernel. Every logical call runs in a fresh Wasm instance, so a crash inside a plugin fails that one call and nothing else. Plugins also never see secret values: configurations name environment variables, and the host attaches credentials at the transport layer only to their bound origin and redacts them before returning responses.

### Self-modifying at runtime, with a way back

The agent is the main user of Enco and also a long-term maintainer of its code. Capabilities, their source locations and their recent failures are directly inspectable, and changing one capability only requires reading one interface. The host renders the WIT contract (`sdk/wit/CONTRACT.md`), SDK interfaces and the running plugins' code structure into a read-only manual under `~/.enco/manual/`, and `cargo xtask check` fails when generated docs fall out of date. The agent can fix or write plugins by reading the manual and plugin source alone, without reading host source.

Plugin code, plugin settings and host configuration are registered together as generations and take effect at the start of the next round, which is the only point where code is replaced. Configuration wiring is probed with a minimal live call during registration, stopping broken wiring before it goes live. New plugin code enters on trial and is kept only after passing health checks; a crash or contract violation rolls it back to the previous generation automatically. If a round still fails, safe mode swaps in embedded factory plugins with empty options and keeps a built-in lifeline toolset (files, shell, registration and status inspection) available for repair. Replacing the host binary itself always requires the owner's confirmation.

### Recorded before it happens

Every model request and tool call is written to an append-only log before it runs, and the model only ever sees requests that were recorded. A pure Wasm `composer` plugin builds each request into a content-addressed plan that is committed to disk first; retries, `enco inspect`, crash recovery and resuming on another machine all resolve that recorded plan instead of reassembling it.

If the process is killed in the middle of a task, the interrupted round is marked as such on restart. Tool calls settle as `ok`, `failed` or `unknown`: a side-effecting call whose result cannot be confirmed after the request went out is recorded as `unknown` and is never retried automatically, leaving the decision to the model and the owner who can read the log.

### Memory: the index nominates, the record decides

Long-term memories are stored as records in SQLite, which is the single source of truth. Recall goes through a [TriviumDB](https://github.com/YoKONCy/TriviumDB) index that combines vector search with keyword search suited to Chinese text. The index is a derived view that can be deleted and rebuilt from SQLite at any time, and every search hit is re-read from SQLite before entering the context, so a corrected or forgotten memory never comes back from a stale index. Pinned memories go into the context first. When the embedding service is unavailable, recall falls back to keywords and the recorded request notes why.

An optional TypeSafe decision plugin filters automatically recalled memories for relevance. Enable `[endpoint.relevance]` and select it with `[memory] relevance` in the [config example](examples/deepseek-gemini.toml). Service failures keep the recalled memories and record the reason; pinned memories and explicit `memory_search` results are not filtered.

### External inputs enter through pure mappers

Third-party tool servers such as MCP, model services, web pages, documents, command output and chat protocols sit outside Enco's control. They enter the system only through mappers. A mapper is a WebAssembly component with no imports, so all it can do is translate raw content into Enco's own types: it needs no trust, and the same input always gives the same result.

Raw arrivals are recorded before reaching the mapper, so a new mapper version can be replayed against historical traffic and compared item by item with the old one. An external service's contract, such as an MCP tool catalog, is normalized and pinned as a file in the owner's intent and registered alongside plugin source; when the upstream service changes, the difference is reported as drift instead of silently changing what the model sees. Telegram and Discord already run on this model, and MCP and streaming model providers follow next: plugins become pure encoders and decoders, the host owns HTTP and sockets and sends requests only to configured endpoints, and raw responses are kept for replay.

### One ghost across your machines (P4 and P5)

The idea is close to Ghost in the Shell. The assistant is the ghost, and each of your devices is a shell it can move into. Your phone, desktop and server form one Space. Between two rounds, a conversation can leave the phone and continue on the server with its full record, and from there it can still use the phone's camera or location through remote calls (P5). Like a ghost, it lives in one shell at a time: a conversation never runs on two devices at once.

Membership and ownership are kept by a consensus control plane (CP). Chat connections and timers use leases, so another node takes over when the current holder disappears. During a network split, the minority side keeps working on its own conversations without taking over anyone else's. The single-node version already stores data in the shape that multiple nodes need (ULIDs, `(epoch, seq)` log positions, ownership bindings and content-addressed files).

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

### One-line install (planned)

Prebuilt binaries and a one-line install script will be provided once the first release is cut; until then, build from source below.

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
