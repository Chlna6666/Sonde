# Sonde

[English](README.md) | [MIT 开源协议](LICENSE)

Sonde 是面向多应用团队的自托管遥测分析平台。它将高吞吐的 Actix Web API 与内嵌 React 管理控制台整合为一个可部署的独立服务。

延伸阅读：[架构](docs/architecture.md)、[性能](docs/performance.md)、[性能测试结果](docs/performance-results.md)、[测试](docs/testing.md)。

## 能做什么

- 高效批量采集自定义事件、数值指标与结构化日志。
- 按应用和环境隔离数据，提供限定范围的摄入密钥与审计日志。
- 提供实时看板、遥测检索、告警规则和基于 SSE 的实时更新。
- 支持 SQLite、PostgreSQL 与 MySQL，并提供首次运行初始化向导。
- 安全导入 Cloudflare D1 SQL 导出：上传文件仅会被校验和解析，绝不会直接执行其中的 SQL。
- 内置简体中文和英文界面，以及跟随系统、浅色、深色主题。

## 安全与隐私

Sonde 面向自托管部署：密码采用 Argon2id 哈希；交互会话、临时 2FA 状态、TOTP 防重放、摄入启动阶段限流窗口以及签名请求 Nonce 防重放状态均存储在已配置的数据库中。系统还提供 RBAC、CSRF 校验、设备绑定短期摄入 Token、HMAC 请求签名与自适应设备风险控制。高吞吐遥测请求的 request/byte/item 快速预算以及自适应登录挑战目前仍属于进程内状态，因此水平扩展部署仍应在边缘或共享流量控制层统一约束热路径总流量。

请勿提交运行时生成的 `data/` 目录、数据库文件、`sonde.password-pepper`、生产环境连接串、摄入密钥或本地 `.env` 文件。仓库的 [`.gitignore`](.gitignore) 已默认排除这些内容；本地配置请从脱敏的 [`.env.example`](.env.example) 开始。

## 使用 Docker 快速启动

```bash
git clone https://github.com/Chlna6666/Sonde.git
cd Sonde
docker compose up -d --build
```

访问 <http://127.0.0.1:8080> 并完成初始化向导。Docker 会将运行数据持久化到 `sonde_data` 卷中。

## 本地开发

依赖：Rust 1.95.0+、Node.js 22+、pnpm 10+。

先启动支持热更新的前端：

```powershell
cd web
pnpm install --frozen-lockfile
pnpm dev
```

再在另一个终端通过 Vite 代理启动后端：

```powershell
$env:SONDE_DEV_PROXY = "http://127.0.0.1:5173"
cargo run
```

Debug 构建会跳过内嵌前端的生产打包。若需在 Debug 模式构建前端，可设置 `SONDE_BUILD_WEB=1`；生产构建请使用 `cargo build --release`。

## 配置

| 变量 | 用途 | 默认值 |
| --- | --- | --- |
| `SONDE_BIND` | HTTP 监听地址 | `127.0.0.1:8080`（Docker 中为 `0.0.0.0:8080`） |
| `SONDE_DATA_DIR` | 运行配置与 SQLite 数据目录 | `data` |
| `SONDE_CONFIG_PATH` | 覆盖生成的配置文件位置 | `$SONDE_DATA_DIR/sonde.json` |
| `SONDE_PEPPER_PATH` | 覆盖生成的密码 Pepper 文件位置 | `$SONDE_DATA_DIR/sonde.password-pepper` |
| `SONDE_DATABASE_URL` | 覆盖数据库连接字符串 | 未设置 |
| `SONDE_DEV_PROXY` | Vite 开发服务器地址 | 未设置 |
| `SONDE_BUILD_WEB` | 在 Debug 构建中打包前端资源 | 未设置 |
| `RUST_LOG` | 日志过滤规则 | `sonde=info,actix_web=info` |

## Rust SDK 接入

官方 Rust SDK 位于仓库的 `sdk/rust`，设置了 `publish = false`，不会发布到 crates.io。Cargo 对 Git dependency 会遍历仓库寻找目标 crate，因此可以直接使用仓库根地址：

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

生产构建建议固定 commit：

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", rev = "<SONDE_COMMIT_SHA>" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

应用只需要持久化一个高熵、伪匿名的安装/设备 ID。不要直接使用 MAC 地址、硬件序列号、账户名或其它可直接识别用户/硬件的信息。

```rust
use sonde_sdk::{Event, SondeClient, load_or_create_device_id};

#[tokio::main]
async fn main() -> sonde_sdk::Result<()> {
    let device_id = load_or_create_device_id("data/sonde-device-id")?;
    let sonde = SondeClient::builder(
        "http://127.0.0.1:8080",
        "sonde_your_bootstrap_key",
        device_id,
    )
    .app_version(env!("CARGO_PKG_VERSION"))
    .system_language("zh-CN")
    // 可选：启用崩溃/断电恢复 WAL；默认不启用磁盘 spool。
    .disk_spool("data/sonde-spool")
    .connect()
    .await?;

    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    sonde.shutdown().await?;
    Ok(())
}
```

`load_or_create_device_id()` 首次启动时使用不覆盖已有文件的方式创建设备 ID，之后始终复用同一值。若已有身份文件损坏，SDK 会明确报错而不是自动生成新 ID，避免把同一次安装错误统计成新设备。该文件应放在应用自己的持久化数据目录中。

`connect()` 会先完成设备 Token 交换和一次可信 heartbeat，随后默认每 60 秒自动 heartbeat。SDK 内部负责 Token 缓存/刷新、精确原始 JSON HMAC 签名、nonce、签名时间以及 Events / Metrics / Logs / Errors 四类独立后台队列。

默认不启用磁盘 spool 时，`event().await` / `metric().await` / `log().await` / `error().await` 的成功表示已经进入有界内存队列；队列满时异步 API 会施加背压，`try_*` 则立即返回 `Error::QueueFull`。默认每种遥测最多排队 4096 条、每批最多 256 条、1 秒自动 flush，并对连接失败、超时、模糊响应、HTTP 429 与 5xx 做指数退避重试。

需要崩溃恢复时显式启用：

```rust
let sonde = SondeClient::builder(server, bootstrap_key, device_id)
    .disk_spool("data/sonde-spool")
    .connect()
    .await?;
```

启用后，每条遥测先序列化并 append 到对应类型的 WAL，`sync_data()` 成功后才进入 worker 队列。因此 async enqueue 返回成功代表记录已经本地持久化，而不是服务端已经确认。Events / Metrics / Logs / Errors 分别使用独立 spool；默认每段 4 MiB、每种遥测最多 64 MiB。磁盘达到上限且没有已确认 segment 可以回收时返回 `Error::SpoolFull`，不会删除未确认记录。

服务端 terminal 回执会推进 append-only ACK journal；ACK 自身持久化成功后才允许删除完全确认的旧 segment。若服务器已经接收但客户端在 ACK 落盘前崩溃，重启后可能重复发送，因此 durable 模式是 **at-least-once**。需要业务去重的 Event 应设置稳定 `idempotency_key`。

持续网络错误在 durable 模式下耗尽当前 retry budget 后不会 dropped，而会保持 deferred 并留在 WAL；后续 worker 继续尝试，正常退出仍失败的记录会在下次启动恢复。普通永久 4xx 与服务端逐项 rejected 属于 terminal，不无限重放。

WAL 活动 segment 支持断电造成的末尾半条 frame：重启扫描时会截断到最后一条校验通过的完整记录；完整记录内部 checksum 失败则明确报错。每类 spool 同时持有跨进程 exclusive lock，并通过 SHA-256 metadata 绑定 endpoint、device ID 与 bootstrap key，避免两个进程并发写或把旧 WAL 发到另一个 Sonde 身份。bootstrap key 不以明文写入 metadata。

Durable 模式下 `try_event()` / `try_metric()` / `try_log()` / `try_error()` 故意不可用，因为“不等待”和“成功前完成 WAL fsync”无法同时保证；应使用对应 async enqueue API。

可通过 `delivery_stats()` 观察：

```rust
let stats = sonde.delivery_stats();
println!(
    "events persisted={} recovered={} delivered={} rejected={} dropped={} deferred={} retries={}",
    stats.events.persisted,
    stats.events.recovered,
    stats.events.delivered,
    stats.events.rejected,
    stats.events.dropped,
    stats.events.deferred,
    stats.events.retries,
);
```

需要确认此前入队数据已经处理时调用 `sonde.flush().await?`；应用正常退出时调用 `sonde.shutdown().await?`。

业务 telemetry 类型**没有** `anonymousId`、`timestamp` 或 `sessionId` 字段。设备身份来自短期 Token；首次/最后出现时间、Session、在线时长、DAU/WAU/MAU 与累计统计全部由 Sonde 服务端根据可信请求推导。

更完整的内存队列、重试和 `SpoolOptions` 调整说明见 [`sdk/rust/README.md`](sdk/rust/README.md)。底层签名协议仅用于实现其它语言 SDK 或协议调试，普通应用不应自行重复实现 HMAC 链路。

## 性能

`sonde` 进程使用 Microsoft **mimalloc v3** 作为全局分配器（`mimalloc` 0.1.52+ 默认实现；不要打开 crate 的 `v2` feature）。摄入 HMAC 校验、设备作用域哈希、SSE 实时推送和写入合并也会避免热路径上的短命堆分配。详见 [docs/performance.md](docs/performance.md) 与实测记录 [docs/performance-results.md](docs/performance-results.md)。

```powershell
cargo bench --bench hot_path --locked
```

## 质量检查

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo test --test api_concurrency --locked
cargo bench --bench hot_path --locked -- --quick

cargo fmt --manifest-path sdk/rust/Cargo.toml --check
cargo check --manifest-path sdk/rust/Cargo.toml
cargo clippy --manifest-path sdk/rust/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path sdk/rust/Cargo.toml

cd web
pnpm typecheck
pnpm test
pnpm build
```

## 开源协议

Sonde 采用 [MIT License](LICENSE) 开源。
