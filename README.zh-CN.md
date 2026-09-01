# Sonde

[English](README.md) | [MIT 开源协议](LICENSE)

Sonde 是面向多应用团队的自托管遥测分析平台。它将高吞吐的 Actix Web API 与内嵌 React 管理控制台整合为一个可部署的独立服务。

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

依赖：Rust 1.95+、Node.js 22+、pnpm 10+。

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
```

生产构建建议固定 commit：

```toml
[dependencies]
sonde-sdk = { git = "https://github.com/Chlna6666/Sonde", rev = "<SONDE_COMMIT_SHA>" }
```

应用只需要持久化一个高熵、伪匿名的安装/设备 ID。不要直接使用 MAC 地址、硬件序列号、账户名或其它可直接识别用户/硬件的信息。

```rust
use sonde_sdk::{Event, SondeClient};

#[tokio::main]
async fn main() -> sonde_sdk::Result<()> {
    let sonde = SondeClient::builder(
        "http://127.0.0.1:8080",
        "sonde_your_bootstrap_key",
        load_or_create_installation_id(),
    )
    .app_version(env!("CARGO_PKG_VERSION"))
    .system_language("zh-CN")
    .connect()
    .await?;

    sonde
        .event(Event::new("app_startup").attribute("channel", "stable"))
        .await?;

    Ok(())
}

fn load_or_create_installation_id() -> String {
    // 首次启动生成并持久化；之后复用同一个值。
    sonde_sdk::generate_device_id()
}
```

`connect()` 会先完成设备 Token 交换和一次可信 heartbeat，随后默认每 60 秒自动 heartbeat。SDK 内部负责：

- 长期 Bootstrap Key 仅调用 `/api/v1/ingest/token`；
- 缓存并提前刷新短生命周期 `sndt_` 设备 Token；
- 使用 `sonde-hmac-sha256-v2` 对实际发送的 JSON 原始字节签名；
- 每个请求生成新的 nonce 和请求签名时间；
- 自动上报 heartbeat；
- 批量上报 events / metrics / logs / errors；
- 在客户端侧限制 1000 条/批和 1 MiB payload 上限。

业务 telemetry 类型**没有** `anonymousId`、`timestamp` 或 `sessionId` 字段。设备身份来自短期 Token；首次/最后出现时间、Session、在线时长、DAU/WAU/MAU 与累计统计全部由 Sonde 服务端根据可信请求推导。

底层签名协议仅用于实现其它语言 SDK 或协议调试，参见服务端 `src/ingest_signature.rs` 与 Rust SDK `sdk/rust/src/signing.rs`，普通应用不应自行重复实现 HMAC 链路。

## 质量检查

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets

cargo fmt --manifest-path sdk/rust/Cargo.toml --check
cargo clippy --manifest-path sdk/rust/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path sdk/rust/Cargo.toml

cd web
pnpm typecheck
pnpm test
pnpm build
```

## 开源协议

Sonde 采用 [MIT License](LICENSE) 开源。
