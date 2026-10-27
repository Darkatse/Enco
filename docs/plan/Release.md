# 发布构建与分发

> 工程基础设施，不属于路线图的某个阶段。计划的生命周期见[路线图](../Roadmap.md)。

## 目标

不装 Rust 也能用上 Enco。每次推送到 `main` 更新一次 nightly 预发布，打 `v*` 标签时发布正式版；一条命令安装：

```sh
curl -fsSL https://install.enco.sh | sh
```

或者使用 Docker 镜像。发布用的二进制同时照顾体积与插件编译速度。

## 对齐的判断

- **发布构建用单独的 `dist` profile，`release` 不动。** 工程专题已有判断：宿主的 `release` 开启增量编译，换取改动后的重建速度。发布构建要的是体积与运行速度，构建慢一些无妨，两者分开，本地开发不受影响。

- **`dist` 整体按体积优化，只有编译器的热路径按速度优化。** 宿主最耗 CPU 的工作是用 Cranelift 把插件编译成机器码。以下数据在 homelab（x86_64，32 核）上测得，"编译"一列是单线程编译 8 个出厂插件、不使用缓存的耗时：

  | 方案 | 体积 | 构建耗时 | 编译 |
  |---|---|---|---|
  | 现在的 `release` | 72.1 MB | — | 1.175 s |
  | 只去符号 | 53.9 MB | 54 s | 1.180 s |
  | fat LTO、单代码单元、opt "s"、去符号（TauriTavern 的做法） | 31.5 MB | 137 s | 1.255 s |
  | 同上，opt 3 | 45.2 MB | 186 s | 1.060 s |
  | 依赖只开用到的特性，opt "s" | 29.9 MB | 123 s | 1.264 s |
  | 依赖只开用到的特性，整体 opt "s"，编译器热路径 opt 3 | 33.2 MB | 138 s | 1.078 s |

  最后一行就是 `dist`：体积比现在小 54%，编译比现在快约 8%，与整体 opt 3 一样快。热路径是 6 个 crate：`cranelift-codegen`、`cranelift-assembler-x64`、`regalloc2`、`wasmparser`、`wasmtime-internal-cranelift`、`wasmtime-environ`。`panic` 保持 `unwind`，与 TauriTavern 一致：依赖在某个任务里 panic 时，只结束那个任务，不结束整个守护进程。

- **依赖只开用到的特性。** 默认特性里有很多 Enco 用不到的东西：

  | 依赖 | 去掉 | 依据 |
  |---|---|---|
  | wasmtime | wat、gc、threads、profiling、coredump、调试支持、compile-time-builtins 等 | Enco 只用组件模型、异步、缓存与 epoch 中断 |
  | wasmtime-wasi | p1、p3 | Enco 只用 p2 |
  | reqwest | charset | Enco 只流式读取原始字节，从不解码文本 |
  | zstd | 旧格式解码等 | Enco 只用 `encode_all` 与 `decode_all` |

  依赖图因此少了 15 个包，其中包括 Intel VTune 的 C 库、wasm-compose 和 wiggle。唯一用到 wat 的是一个以文本格式写组件的测试，wat 改由 `apps/enco` 的 dev-dependencies 开启，不进发布二进制。剥离之后完整门禁通过；`dist` 构建的二进制在 homelab 上加载了全部出厂插件，守护进程正常就绪。

  剩下的大头都是必需的：嵌入的出厂插件、手册、插件源码与 SDK 约 3.85 MB，标准库约 3.3 MiB，Cranelift 约 4.3 MiB。rustls 的密码学后端 aws-lc 约占 1.3 MiB，换成 ring 能再省约 1 MB，但要在启动时多安装一次 provider，暂不换。

- **平台：Linux 与 macOS 各两种架构。**
  - **Linux**：x64 与 arm64 在 `ubuntu-22.04` 上构建，链接 glibc，要求目标系统的 glibc 在 2.35 以上（Debian 12、Ubuntu 22.04 及以后）。
  - **macOS**：arm64 在 `macos-latest` 上构建，跟随 GitHub 的更新。Rust 默认把最低系统版本定在 macOS 11，与运行器的系统无关；Linux 要固定 `ubuntu-22.04`，是因为它决定了 glibc 的下限。x64 在 Intel 运行器 `macos-15-intel` 上构建，这是目前唯一的 Intel 标签，预计在 2027 年秋随 macOS 15 镜像退役，届时再改为交叉编译。
  - **Windows**：Enco 尚不支持，不发布。

- **发布方式参照 TT-Sync，发布在 Darkatse/Enco。** 每个平台一个裸二进制，另附 `SHA256SUMS.txt`。推送到 `main` 时覆盖 `nightly` 预发布，并把 `nightly` 标签移到这次提交；推送 `v*` 标签时发布正式版，标签必须与工作区版本一致，否则失败。

- **发布工作流不跑门禁。** 门禁由提交者在本地跑完再提交，这是现在的做法。发布工作流只负责构建与分发；在 CI 上跑门禁是另一件事，需要时单独讨论。

- **安装脚本只有一份，放在 Enco 仓库，不替主人安装 Rust。**
  - **脚本位置**：`scripts/install.sh` 与发布物在同一个仓库，随代码一起修改。`install.enco.sh` 是 Cloudflare 上的一条重定向，指向这份脚本在 `main` 上的原文，不另存一份。
  - **安装流程**：脚本识别系统与架构，下载对应的二进制与 `SHA256SUMS.txt`，校验之后放进安装目录。默认装最新的正式版，还没有正式版时装 nightly；`--nightly` 与 `--version` 可以指定。
  - **不替主人装 Rust**：Enco 自己构建插件时需要固定版本的 Rust 工具链。工程专题已有判断：工具链由主人安装，Enco 不替主人下载。安装脚本照此只提示，不安装 Rust。

- **Docker 镜像用发布的二进制，不在镜像里重新编译。** TT-Sync 在镜像里编译，基础镜像用 `scratch`。Enco 不能照搬，原因有两点：
  - **环境**：Enco 要在一个真实的用户环境里执行命令，`scratch` 连 `sh` 都没有。
  - **构建成本**：在 QEMU 中按 fat LTO 编译 arm64 版本会非常慢。

  所以镜像以 `debian:bookworm-slim` 为基础（glibc 2.36），放入同一次工作流构建出的 Linux 二进制，镜像里的就是发布的那一份。

  镜像里带上固定版本的 Rust 工具链与 `wasm32-wasip2`。这样容器里的 Enco 也能自己构建插件，自我构建是 Enco 的核心能力；代价是镜像约 1 GB，Docker 主要用于服务器，体积不是问题。

  镜像发布到 GHCR（`ghcr.io/darkatse/enco`）；配置了 `DOCKERHUB_TOKEN` 时，以用户 `darkatse` 同步到 Docker Hub（`darkatse/enco`）。标签与 TT-Sync 相同：`main` 上为 `edge`，正式版为语义化版本，另有按提交的 `sha-` 标签。

## 范围

- **`Cargo.toml`**：新增 `[profile.dist]` 及 6 个热路径 crate 的 opt 3；wasmtime、wasmtime-wasi、reqwest、zstd 只开用到的特性。
- **`apps/enco/Cargo.toml`**：dev-dependencies 为测试开启 wasmtime 的 `wat`。
- **`.github/workflows/release.yml`**：四个平台的构建、nightly 与正式版的发布、Docker 镜像。
- **`scripts/install.sh`**：Linux 与 macOS 的一键安装。
- **`Dockerfile`、`.dockerignore`**：由发布的 Linux 二进制组成镜像。
- **`docker-compose.yaml`**：把宿主机的 `./enco-home` 挂为容器用户的家目录，`config.toml` 可以直接在宿主机上编辑；以 `restart: unless-stopped` 运行 `serve`。
- **Cloudflare**：`install.enco.sh` 重定向到 `https://raw.githubusercontent.com/Darkatse/Enco/main/scripts/install.sh`。
- **文档**：README 与 README.zh-CN 写明安装方式，与主人正在修改的版本合并；`docs/Usage.md` 写明 Docker 的用法。

## 设计

### `dist` profile

```toml
# Release artifacts: fat LTO over one codegen unit, optimized for size, symbols stripped. The
# Wasm compiler's hot paths stay at full speed; only CI and release builds use this profile.
[profile.dist]
inherits = "release"
incremental = false
codegen-units = 1
lto = "fat"
opt-level = "s"
strip = "symbols"

[profile.dist.package.cranelift-codegen]
opt-level = 3
# 其余五个热路径 crate 同上
```

这些 crate 的名字随 wasmtime 升级可能变化；名字对不上时 Cargo 只给警告，所以升级 wasmtime 时要核对一次。

### 发布工作流

`release.yml` 在推送到 `main`、推送 `v*` 标签与手动触发时运行：

1. **构建**，四个平台并行：
   - 准备环境：检出代码，`rustup toolchain install` 按 `rust-toolchain.toml` 装好固定版本与 `wasm32-wasip2`；按 README 的循环为每个工作区 `cargo fetch`，xtask 离线运行。
   - 编译：`cargo xtask build-factory`，再 `cargo build --profile dist -p enco --locked`。
   - 上传：二进制按 `enco-<os>-<arch>` 命名。

   运行器分别是 `ubuntu-22.04`、`ubuntu-22.04-arm`、`macos-latest` 与 `macos-15-intel`。
2. **发布**：汇总四个二进制，生成 `SHA256SUMS.txt`。`main` 上覆盖 `nightly` 预发布并移动标签；`v*` 标签先核对版本，再发布正式版。
3. **镜像**：取两个 Linux 二进制，用 Buildx 构建 `linux/amd64` 与 `linux/arm64` 镜像，推送到 GHCR，有令牌时同步到 Docker Hub。

### 安装脚本

在 TT-Sync 的 `install.sh` 上修改：

- **名字**：仓库与二进制名改为 Darkatse/Enco 与 `enco`。
- **平台**：只支持 Linux 与 macOS 的 x64 和 arm64。
- **装完之后**提示两件事：运行 `enco init`；如果要让 Enco 自己构建插件，需要安装 rustup。所需的工具链版本由 Enco 在第一次构建插件时报告（`Rust 1.99.0: not installed`），脚本不写死版本。

### Docker 镜像

- **工具链**：把仓库的 `rust-toolchain.toml` 复制进镜像，用 rustup 的 minimal profile 按它安装固定版本与 `wasm32-wasip2`。版本只有这一个来源，工具链升级时镜像随之更新。
- **用户与目录**：以非 root 用户 `enco` 运行，家目录 `/home/enco` 是卷，里面是 Enco 的 home（`~/.enco`）与 Cargo 的下载缓存；`HOME` 显式设定，挂载宿主机目录时可以换成目录所有者的 uid 运行。默认命令是 `enco serve`。
- **首次使用**：先用同一个卷运行一次 `init`，编辑配置后再启动。
- **操作**：用 `docker exec` 执行 `enco chat`、`enco status` 等命令。

Enco 的渠道只向外连接，所以不需要开放端口。

## 验收

- **homelab**：`cargo build --profile dist` 的体积与编译耗时和上表一致；完整门禁通过；守护进程加载全部出厂插件。
- **nightly**：推送到 `main` 后，`nightly` 预发布带有四个二进制与 `SHA256SUMS.txt`。四个二进制 `--version` 打印这次提交；`file` 核对 macOS 两个二进制的架构。
- **安装**：在 homelab（Linux x64）与本机（macOS arm64）上运行 `curl -fsSL https://install.enco.sh | sh`。在还没有正式版的情况下应装上 nightly，`enco --version` 正确，并能完成 `enco init`。
- **镜像**：`docker run --rm ghcr.io/darkatse/enco:edge --version` 在两种架构上都能运行；挂卷完成 `init` 后，`serve` 加载全部出厂插件；在容器中修改一个插件并 `enco apply`，构建与登记成功。Docker Hub 上的 `darkatse/enco:edge` 与 GHCR 的是同一个镜像。
- **正式版**：首个 `v*` 标签由主人决定何时打；打标签之后，正式版、安装脚本的默认路径与镜像的语义化标签逐一核对。

## 待定

- **首个正式版本号与时机**：不急，由主人决定。在此之前，安装脚本与镜像都使用 nightly 与 `edge`。

## 核对记录

推送之前在 homelab 与本机上完成：

- **`dist` 构建**：
  - 改动后的 `cargo build --profile dist` 产出 33.2 MB，与上表一致。
  - 在 `ubuntu:22.04` 容器中按 CI 的步骤构建用了 177 秒。二进制依赖的 glibc 符号最高为 2.34，比计划写的 2.35 下限更宽。
- **门禁**：
  - homelab 上完整门禁通过。
  - 本机跑了两次，都只有计时敏感的测试超时：第一次是 `channel::attachments_are_authorized_before_fetch_and_retry_after_the_committed_prefix` 与 `trials_past_their_deadline_cancel_the_run_and_fail`，第二次只有后者。两个测试单独运行都通过；后者在并行门禁下超时，是 M32 已记录的问题。
- **镜像**：用 22.04 构建的二进制组成镜像，1.17 GB。`--version` 与 `init` 正常，`serve` 加载全部出厂插件；容器中的 cargo 为 1.99.0，`enco plugin scaffold hello` 之后 `enco apply` 构建并登记成功。
- **`install.enco.sh`**：返回 302，指向 `raw.githubusercontent.com/Darkatse/Enco/main/scripts/install.sh`。推送之前这个地址是 404。
- **待推送后核对**：
  - nightly 的四个二进制与 `SHA256SUMS.txt`；
  - 两台机器上的一键安装；
  - GHCR 与 Docker Hub 上的 `edge`。
