# Sonde 代码安全审计报告（2026-09-14）

范围：`src/`（Rust / actix-web 后端，134 个文件）、`web/src`（React 19 + TypeScript）、`Dockerfile`、`docker-compose.yml`、`.github/workflows/ci.yml`、依赖清单（`Cargo.lock`、`web/pnpm-lock.yaml`）。
方法：静态代码审计（人工走查 + 模式检索）+ 依赖组件审计（工具扫描）+ 配置合规核对。动态运行时审计（DAST）未在本轮执行，见文末"未覆盖项"。

**总体结论：自研代码未发现可直接导致未授权访问或数据泄露的严重/高危缺陷**，认证、会话、CSRF、授权、SQL 注入、SSRF、签名与重放防护等关键面实现质量高于同类项目平均水平。但**依赖组件审计直接命中 1 个需优先修复的漏洞（actix-http 请求走私，SEC-19）**，另有 6 项中危、12 项低危问题，主要集中在**第二因子在线爆破面、前端产物泄露、部署配置一致性**三处。累计 20 项发现。

**处理进度：14 项已修复，3 项已部分处理/缓解（SEC-13 供应链固定、SEC-16 安装向导限速、SEC-20 依赖确认无补丁并给出部署缓解），3 项为功能级改动待排期（SEC-15 备份加密、SEC-17 限速持久化、SEC-18 TOTP 密钥加密）。** 详见第八节（修复记录）与第九节（依赖更新与供应链治理）。

---

## 零、依赖组件漏洞（工具扫描直接命中，优先修复）

### SEC-19　actix-http 3.11.2 存在 HTTP/1.1 CL.TE 请求走私（CVE-2026-73051 / GHSA-xhj4-vrgc-hr34）

- **位置**：`Cargo.lock` → `actix-http v3.11.2`（由 `actix-web 4.12.1` 与 `awc 3.8.1` 引入，`cargo tree -i actix-http` 确认）
- **问题**：`actix-http ≤ 3.12.0` 的 HTTP/1.1 解析器在请求同时包含 `Content-Length` 与 `Transfer-Encoding: chunked` 时未拒绝该歧义请求，而是选择 chunked 解码。
- **影响**：在"前端代理/WAF/负载均衡按 Content-Length 组帧并复用后端连接"的 CL.TE 拓扑下，未认证远程攻击者可实现**请求走私**，使后端连接上的请求边界与代理不一致（评分 CWSS/CVSS4：`AV:N/AC:L/AT:P/PR:N/UI:N/VI:L`，Medium）。Sonde 作为自托管服务几乎必然部署在 Nginx/Caddy/CDN 之后，且 actix-web 默认对后端连接启用 keep-alive（`src/main.rs:75`），构成该漏洞所需的拓扑条件。走私可用于绕过前置代理的鉴权/限速规则，并把恶意请求"拼"到其他用户的请求之前。
- **修复**：`cargo update -p actix-http`（实测可将 3.11.2 升级到 **3.13.5**，满足 ≥ 3.12.1 的修复版本，且无需改动 `Cargo.toml`）。升级后执行一次全量验证。
- **临时缓解**（若无法立即升级）：在反向代理层拒绝同时携带 `Content-Length` 与 `Transfer-Encoding` 的 HTTP/1.1 请求，并关闭代理到后端的连接复用。

### SEC-20　h2 0.3.27 空 DATA 帧无界排队（RUSTSEC-2026-0258）

- **位置**：`Cargo.lock` → `h2 v0.3.27`（经 `actix-http 3.11.2` 与 `awc 3.8.1` 引入）
- **问题**：受影响版本会无限制接受并排队空的 DATA 帧，在流未被及时消费时可导致内存无界增长，长度溢出时 panic（Low severity）。
- **影响**：仅在客户端以 **HTTP/2** 连接时可达。actix-web 在明文监听端口支持 h2c、在 TLS 下支持 ALPN h2，因此若直接暴露 8080 或由代理转发 HTTP/2，则构成 DoS 面；纯 HTTP/1.1 部署不可达。h2 `0.3.x` 分支**没有修复版本**（补丁在 `0.4.16`），需等待上游迁移。
- **修复**：短期由反向代理终止 HTTP/2 并以 HTTP/1.1 回源（顺带缓解 SEC-19 的前置条件）；中期跟踪 actix-http/awc 迁移到 h2 0.4 后升级。

**关于 rkyv 0.7.46（RUSTSEC-2026-0235）**：该版本存在于 `Cargo.lock`，但 `cargo tree -i rkyv` 与 `cargo tree --target all -i rkyv` 均输出 "nothing to print"（未进入任何构建配置的依赖图），**当前二进制不可利用**。建议执行一次 `cargo update` 并在 CI 中固化 SCA 门禁，避免该条目被误判或日后被意外启用。

---

## 一、中危问题

### SEC-01　2FA 校验接口缺少速率限制与失败计数（TOTP 可在线爆破）

- **位置**：`src/api/authentication.rs:176-193`（`verify_2fa`）、`src/services/authentication.rs:189-219`、`src/security.rs:415-438`（`record_failure`）、`src/security.rs:434-438`（`record_success`）、`src/database/auth_state.rs:139-173`
- **问题**：`POST /api/v1/auth/2fa/verify` 没有任何限速（既不限 IP 也不限用户），也不统计验证码失败次数。临时令牌 `auth_2fa_pending` 虽为一次性（首次请求即标记 `consumed_at`），但攻击者在**已知口令**的前提下只需重新登录即可获得新的临时令牌，而登录成功会调用 `record_success` **清空失败计数**，登录接口的指数退避（`record_failure`）只在"登录失败"时生效。
- **影响**：第二因子被在线遍历击穿。校验窗口为 ±2 步（`src/totp.rs:95`，共 5 个 6 位码，单次猜中概率 5×10⁻⁶），期望约 20 万次尝试；按每次尝试需一次 Argon2id 登录（~50–100 ms）估算，单机数小时至一天内可完成。口令泄露后的纵深防御失效。
- **修复建议**：
  1. 为 `/2fa/verify` 增加独立限速（按 `user_id` 与来源 IP，如 5 次/15 分钟）与失败上限；
  2. 将临时令牌改为"允许 N 次尝试"（例如 5 次）后再作废，并在令牌表中记录 `attempts`；
  3. 失败达到阈值时写入审计日志并对该账户临时锁定；
  4. 登录成功不应无条件清零 `attempts`（尤其当同一账户刚发生 2FA 失败）。

### SEC-02　生产构建发布 source map，随二进制一并对外提供

- **位置**：`web/vite.config.ts:16`（`sourcemap: true`）、`web/dist/assets/*.js.map`（实测存在，如 `chunk-ExplorerPage-DOPHSJXZ.js.map`）、`src/web_assets.rs:4-6`（`#[folder = "web/dist"]` 全量嵌入）、`src/web_assets.rs:26-28`（无 `.map` 过滤）、`Dockerfile:13`
- **问题**：Vite 生产构建开启 sourcemap，`web/dist` 中的 `.map` 文件被 `rust-embed` 打包进发布二进制，并由 `web_assets::serve` 原样返回（缓存 1 年）。路径校验只阻止穿越，不阻止 `.map`。
- **影响**：任何人可下载完整前端原始源码（组件结构、内部 API 路径、注释、表单字段语义、错误处理分支），显著降低后续攻击成本；同时增大二进制体积。
- **修复建议**：生产构建改为 `sourcemap: false`；若需保留错误回溯能力，改用 `sourcemap: "hidden"` 并仅上传到内部监控系统，同时在 `web_assets::serve` 中对 `.map` 返回 404。

### SEC-03　告警规则在评估阶段未重新校验，过滤字段白名单可被绕过

- **位置**：`src/services/alerts.rs:305-308`（从库中反序列化 `rule.query`）、`src/services/alerts.rs:718-725`（`Alias::new(&filter.field)` 直接作为列名）、`src/services/alerts.rs:50-52`（仅在创建/更新时校验）、`src/domain/alert.rs:67-95`（白名单）
- **问题**：字段白名单只在写入时（`create`/`update`）执行。评估器从数据库读出 `query_json` 后仅做反序列化，随即把用户的 `field` 字符串当作 SQL 列名使用。任何绕过 API 的写入路径（**全系统备份恢复**、直接改库）都会跳过白名单。
- **影响**：可构造任意列名进入 `WHERE` 子句。sea-query 会对标识符加引号并转义内嵌引号，因此更可能是"过滤任意列/语义破坏/错误信息泄露"而非典型 SQL 注入，但属于明确的纵深防御缺口，且经备份恢复即可触发（恢复者需 owner 权限）。
- **修复建议**：在 `src/services/alerts.rs:305` 反序列化成功后立即调用 `expression.validate()`，校验失败则跳过该规则并记录告警日志。

### SEC-04　全系统备份恢复上传无总量上限（磁盘耗尽）

- **位置**：`src/api/backup.rs:157-204`（流式落盘，仅统计"单条记录"字节）、`src/database/backup_archive/format.rs:27`（`MAX_RECORD_BYTES = 8 MiB`）
- **问题**：恢复接口使用裸 `web::Payload` 读取，因此 `main.rs:64` 的 `PayloadConfig::default().limit(4 MiB)` 不生效；代码只对"相邻换行之间的记录长度"设 8 MiB 上限，**没有总量、总记录数或 Content-Length 上限**，数据被直接写入系统临时目录的 `NamedTempFile`。
- **影响**：具备 `settings.manage` 权限者（或凭据泄露后）可持续上传直至耗尽数据盘与系统盘 `/tmp`，造成服务不可用（自托管场景为实际风险）。
- **修复建议**：累计字节数达上限（如 2 GiB，可配置）即中断并返回 413；优先在 `data_dir` 下的专用子目录创建暂存文件；对总记录数也设上限。

### SEC-05　`secure_cookie` 由前端在安装时决定，服务端未与部署形态校验

- **位置**：`src/api/setup.rs:36-37,97`（客户端传入）、`src/services/setup.rs:58-63`（直接采纳）、对比 `src/bootstrap.rs:63`（恢复路径使用 `!runtime.bind_is_loopback()`）、`src/api/authentication.rs:285-302`
- **问题**：安装流程完全信任前端提交的 `secureCookie` 布尔值。同样的部署形态（`SONDE_BIND=0.0.0.0:8080`）在两条代码路径下得到相反的判定：自动恢复路径强制开启 Secure Cookie，Web 安装路径可被关闭。关闭后 Cookie 名退化为无 `__Host-` 前缀的 `sonde-session`（`src/auth.rs:11-12`），且不带 `Secure`。
- **影响**：在非回环绑定（公网/内网暴露）时若关闭该选项，会话 Cookie 可经明文 HTTP 传输，被中间人截获后直接复用会话（会话令牌本身有效期内无二次校验）。
- **修复建议**：`bind` 非回环时服务端强制 `secure_cookie = true`（忽略客户端值或返回校验错误）；前端默认勾选并给出"未启用 TLS 时不应关闭"的显著警告。

### SEC-06　`SONDE_DEV_PROXY` 未做构建/环境隔离，可把服务变成转发代理

- **位置**：`src/web_assets.rs:18-23`（环境变量存在即启用）、`src/web_assets.rs:39-72`（`proxy_dev_request`）
- **问题**：只要环境变量存在且非空，`default_service` 就会把**所有**未匹配路由的请求转发到该地址，并原样转发的请求头中包含 `cookie`、`authorization`（仅剔除 `host`/`content-length`），且不限制目标、不要求目标为环回地址、不做鉴权。
- **影响**：生产环境误配置（或镜像中残留）会让实例成为开放代理，并可能把用户会话 Cookie 转发到外部主机；配合路由前缀还可能与真实 API 混淆。
- **修复建议**：仅在 `#[cfg(debug_assertions)]` 或显式 `SONDE_ENV=development` 下启用；目标地址限制为环回地址；启动时若检测到该变量则打印醒目告警。

---

## 二、低危问题

| 编号 | 位置 | 问题与影响 | 修复建议 |
|---|---|---|---|
| SEC-07 | `src/api/request_auth.rs:8-12` | 服务端无条件同时接受 `sonde-session` 与 `__Host-sonde-session` 两种 Cookie 名。启用 `__Host-` 模式时影响有限（优先读取带前缀者），但为同父域"cookie tossing/会话固定"留出窗口 | 仅在开发模式接受无前缀 Cookie 名，或与 `secure_cookie` 联动判断 |
| SEC-08 | `src/api/authentication.rs:117-174` | `POST /auth/login` 未校验 `Origin`/`Referer`（对比 `src/api/setup.rs:104-115` 已有 `require_same_origin`），存在登录 CSRF（将受害者置于攻击者账户，其后续录入数据落入攻击者账号） | 登录/2FA 接口同样校验同源 |
| SEC-09 | `src/ingest_signature.rs:36` | `(now_ms - request.timestamp_ms).abs()` 未用饱和/checked 运算；`timestamp` 来自请求头且可为 `i64::MIN`。release（默认关闭溢出检查）不会 panic，但 debug/test 构建会 panic 返回 500 | 改用 `saturating_sub` 后取绝对值 |
| SEC-10 | `src/api/explorer.rs:116`、`src/api/devices.rs:66`、`src/database/explorer.rs:293` | `page` 无上限（仅 `max(1)`），`page_size` 已 clamp；超大 `page` 触发深 OFFSET 全表扫描（查询放大 DoS 面），且 `(page-1)*page_size` 存在乘法溢出（release 回绕） | 对 `page` 设上限（如 10 000），偏移量用 `checked_mul` |
| SEC-11 | `src/api/stream.rs:18-33`、`src/api/ingest.rs:187-197` | SSE 直播流仅要求全局 `telemetry.read`，向订阅者广播**所有**应用的上报活动（含 `applicationId`、类型、条数），无法按应用隔离 | 按用户可见应用过滤后再推送 |
| SEC-12 | `src/api/error_response.rs:71-93` | Query/Path 反序列化失败时把 serde 错误原文拼入 `validation_error` 返回客户端（内部字段名/类型提示泄露） | 统一为通用文案，细节仅记日志 |
| SEC-13 | `Dockerfile:1,9,17`、`.github/workflows/ci.yml:27,62,68` | 基础镜像与 GitHub Actions 使用可变 tag（`node:22-bookworm-slim`、`rust:1.95-bookworm`、`debian:bookworm-slim`、`actions/*@v4`），供应链不可复现 | 固定镜像 digest 与 Action commit SHA，配置自动更新 |
| SEC-14 | `docker-compose.yml:8-15` | 默认 `"8080:8080"` 直连发布、无 TLS、无 `read_only`/`cap_drop`/`no-new-privileges`，`RUST_LOG` 固定 info；生产直接暴露即明文 HTTP | 绑定 `127.0.0.1` 由 TLS 反代终止；容器能力最小化 |
| SEC-15 | `src/database/backup_archive/format.rs:218,410,674` | 备份包含用户 `password_hash` 与 API key 哈希（`contains_secrets: true`），当前实现不提供加密或口令保护 | 按机密文件管理（加密存储、严格 ACL、传输加密），文档明示 |
| SEC-16 | `src/api/setup.rs:64-75`、`src/services/setup.rs:24-35` | 未安装状态下匿名可调用 `/api/v1/setup/test`，使服务器连接任意 postgres/mysql URL（受同源校验与协议前缀限制） | 仅允许本地管理员，或加一次性安装令牌 |
| SEC-17 | `src/security.rs:354-475` | 登录限速/挑战状态为**进程内内存** `HashMap`：重启即清零、多副本不共享（数据库已有 `auth_shared_state` 迁移表可承载持久化） | 需要横向扩展时迁移到数据库/Redis |
| SEC-18 | `src/database/auth.rs`（`enable_totp`） | TOTP 密钥明文存库；备份有意排除（`totp_secrets_included: false`，设计合理），但库文件本身泄露即等同于第二因子失效 | 考虑用 pepper 派生密钥加密存储 |

---

## 三、已确认的安全实现（本轮验证为正确，无需整改）

**认证与会话**
- 口令：Argon2id（`Argon2id`/`V0x13`/`Params::default()`，`src/auth.rs:64-72`）+ 每安装 32 字节 pepper；口令长度 15–128 与弱口令黑名单（`src/auth.rs:30-45`）。
- 抗用户名枚举：不存在账户时使用 `dummy_password_hash` 走同样的 Argon2 校验路径（`src/services/authentication.rs:101-122`）；失败响应统一为 `invalid_credentials`（`src/api/authentication.rs:145-153`）。
- 凭据比对：CSRF 令牌与 TOTP 码均使用恒定时间比较（`src/services/authentication.rs:457-465`、`src/totp.rs:133-142`）。
- 会话：32 字节随机令牌，库中仅存 SHA-256 哈希；8 小时绝对 + 30 分钟空闲超时，超时即删除（`src/database/auth_state.rs:10-89`）；登出、禁用 2FA 均撤销会话（`:91-109`）。
- 2FA 重放：单调递增的 `last_step` 条件更新 + 唯一键兜底（`src/database/auth_state.rs:175-222`）；TOTP 实现通过 RFC 6238 官方测试向量（`src/totp.rs:178-201`）。
- Cookie：HttpOnly + SameSite=Strict + `__Host-` 前缀（安全模式）+ `Cache-Control: no-store`（`src/api/authentication.rs:285-302`）。
- 登录限速：账户维度与来源维度（IP|UA）双键指数退避（上限 300 s）+ 一次性挑战（`src/security.rs:381-474`）；来源 IP 仅在 peer 属于 `SONDE_TRUSTED_PROXIES` 时才采纳 `Forwarded`/`X-Forwarded-For`（`src/api/request_auth.rs:30-59`）。

**授权**
- 全量路由核对：所有写操作均使用 `authenticate_mutation`（会话 + `x-csrf-token` 恒定时间比对），读操作使用 `authenticate`——**未发现漏挂 CSRF 的变更端点**；仅 `/api/v1/setup/*` 与 `/api/v1/ingest/*` 为例外（分别为安装前与令牌认证，设计合理）。
- 应用级资源在服务层统一校验：`ensure_app_access`（`src/services/applications.rs:344-365`，owner / 全局管理员 / 作用域权限三分支）与 `user.require(perm, Some(app_id))`（explorer、errors、alerts、devices、statistics、backup 全覆盖）；`PermissionGrant.allows` 的作用域匹配正确——应用级授权不会匹配到其它应用（`src/domain/permission.rs:72-85`）。
- 备份导出/恢复要求非作用域 owner（`src/services/backup.rs:243-249`）；应用导入要求全局 `apps.manage`。
- API key 原值仅创建/轮换时返回一次（`shownOnce: true`），库中只存 SHA-256（`src/services/applications.rs:94-115`）。

**注入**
- 全库检索未发现拼接 SQL 执行：`Statement::from_string`/`execute_unprepared` 零命中；所有 `Expr::cust*` 的插值片段均为内部常量或经过 `DbBackend` 分支的固定 SQL（`src/services/alerts.rs:539-566`、`src/database/device_session.rs:216-261`、`src/database/dimension_rollup.rs:145`）。
- 动态标识符均可溯源到白名单/枚举：告警过滤字段（`src/domain/alert.rs:81-95`）、explorer 搜索列（`src/database/explorer.rs:249-279`）；LIKE 通配符在 `contains_like_pattern` 中被剥离（`src/database/query.rs:131-141`）。
- D1 迁移导入**不执行 SQL**：仅解析 `INSERT INTO events` 为结构化行并做列白名单映射，其余语句一律拒绝（`src/services/migrations/parser/sql.rs:7-42`）。
- 批量写入使用参数绑定并遵守 900 个绑定参数预算（`src/database/query.rs:9-116`）。

**SSRF 与出站**
- `src/domain/outbound.rs`：仅 http/https、禁 userinfo、域名黑名单 + 私网/环回/链路本地/NAT64/Teredo/IPv4-mapped 全覆盖判定；告警发送前**再解析 DNS 并复核所有解析结果**、禁用重定向、10 s 超时（`src/services/alerts.rs:941-964`、`:869-882`）；渠道配置在写入与发送两处校验，回显中密钥字段脱敏（`:966-981`）。公开链接强制 https，`javascript:` 被拒（`src/domain/outbound.rs:160-165`）。

**入站签名与资源治理**
- ingest：HMAC-SHA256 覆盖 上下文 + 时间戳 + nonce + 方法 + 路径 + 请求体 SHA-256（`src/ingest_signature.rs:56-77`），±60 s 漂移窗口，nonce 落库去重（`src/services/telemetry.rs:310-348`），令牌与设备 + UA 指纹绑定，TTL ≤ 5 分钟（`src/security.rs:240-265`）。
- 限速分层：IP 请求/字节/条目、设备请求/字节/条目、令牌签发额度，并按设备风险分档（`src/security.rs:14-20,115-192`、`src/services/telemetry.rs:218-253,690-733`）。
- 背压：有界队列 256 + 批量窗口 + 写入并发闸门（`src/services/ingest_writer.rs:12-16,142-172`）；分析查询信号量 8、写入 64（`src/state.rs:13-14`）。
- 请求面：全局 JSON 2 MB / payload 4 MB，ingest 1 MB、token/facts 16 KB（`src/main.rs:61-64`、`src/api/ingest.rs:19-21`）；超时 15 s、keep-alive 75 s（`src/main.rs:73-76`）。
- 备份：先全量校验 framing/manifest/digest/计数、拒绝非当前格式版本，校验通过后才落库；直方图语义（桶数 = 边界数 + 1、桶和 = 计数、有限值）逐项校验（`src/services/backup.rs:114-241`）。
- 错误处理：内部错误统一脱敏为 `internal operation failed`，仅返回 `error_id` 供日志关联（`src/error.rs:96-114`、`src/api/error_response.rs:41-62`）。

**前端**
- 无 `dangerouslySetInnerHTML` / `innerHTML` / `eval` / `new Function`；`localStorage` 仅存 locale 与主题，**CSRF 令牌仅存内存**（`web/src/lib/api.ts:15-32`）；外链 `rel="noreferrer"`；公开应用链接在服务端经 `validate_public_link` 强制 https（`src/services/applications.rs:367-374`）。
- 安全响应头：严格 CSP（无 `unsafe-inline`）、非回环绑定时输出 HSTS、`nosniff`、`X-Frame-Options: DENY`、`no-referrer`、`permissions-policy`；无 CORS 中间件，默认同源（`src/main.rs:47-58`）。

---

## 四、依赖组件审计（SCA）

执行结果：

| 生态 | 方式 | 覆盖量 | 结果 |
|---|---|---|---|
| 后端 Rust | OSV `querybatch` 逐包比对 `Cargo.lock`（`cargo audit` 因本机无法访问 github.com 无法拉取 RustSec 库，改用 OSV 数据库，等效覆盖 RustSec + GHSA） | 452 个锁定包 | **3 条命中**：actix-http 3.11.2（SEC-19）、h2 0.3.27（SEC-20）、rkyv 0.7.46（未进入依赖图，不可利用） |
| 前端 npm | `pnpm audit`（生产依赖与全量各一次） | 368 个锁定包 | **No known vulnerabilities found** |
| 前端 npm | OSV `querybatch` 独立复核 | 368 个锁定包 | 无命中（与 `pnpm audit` 结论一致） |

后端请在本机/CI 恢复 GitHub 访问后补跑官方工具，并建议直接接入 CI 门禁：

```bash
cargo install cargo-audit cargo-deny --locked
cargo audit                       # RustSec 通告比对
cargo deny check advisories bans licenses sources
cargo update -p actix-http        # 修复 SEC-19（实测升至 3.13.5）
```

补充说明：`sha1` 仅用于 TOTP 的 HMAC-SHA1（`src/totp.rs:3-5`，RFC 6238 规定算法），**不是**用于口令或签名摘要，不构成缺陷；口令使用 Argon2id，会话令牌与 API key 使用 SHA-256。

**流程缺口**：`.github/workflows/ci.yml` 只有 fmt/check/clippy/test/typecheck/build，缺少 SCA 与 SAST 门禁。建议补充 `cargo audit`、`cargo deny`、`pnpm audit`、CodeQL 与 gitleaks（密钥泄漏扫描），并把"高危且已有修复版本"设为阻断条件——SEC-19 这类问题本应由 CI 在提交阶段拦截。

---

## 五、配置合规审计（含上线前整改清单）

| 检查项 | 现状 | 结论 |
|---|---|---|
| 容器非 root 运行 | `Dockerfile:18,22` 创建 uid 10001 并 `USER sonde` | ✅ 已满足 |
| 构建工具不入最终镜像 | 多阶段构建，仅复制二进制 | ✅ 已满足 |
| 数据目录权限 | `/app/data` 归属 sonde；pepper 文件创建时 0600 / Windows ACL 收紧（`src/config.rs:84-176`） | ✅ 已满足（注意：已存在的 pepper 文件不会被重新收紧权限） |
| 密钥不入仓 | `.gitignore` 覆盖 `/data/`、`*.password-pepper`、`*.sqlite`、`.env`；`data/sonde.sqlite` 未被 git 跟踪（实测） | ✅ 已满足 |
| TLS/HSTS | 非回环绑定输出 HSTS，但**自身不提供 TLS**，compose 默认明文 8080 | ⚠️ 需整改：置于 TLS 反代后并强制 `secure_cookie`（见 SEC-05、SEC-14） |
| 生产不发布 source map | `sourcemap: true` 且随二进制提供 | ❌ 需整改（SEC-02） |
| 关闭开发代理 | `SONDE_DEV_PROXY` 无环境隔离 | ❌ 需整改（SEC-06） |
| 镜像/Actions 固定版本 | 使用可变 tag | ⚠️ 建议整改（SEC-13） |
| 容器能力收紧 | 无 `read_only`/`cap_drop`/`no-new-privileges` | ⚠️ 建议整改（SEC-14） |
| 备份机密保护 | 含口令哈希与 key 哈希，未加密 | ⚠️ 需按机密文件管理（SEC-15） |
| 日志不含敏感数据 | 内部错误细节只进 tracing，响应脱敏；`Logger` 默认记录请求行（含查询串） | ⚠️ 建议确认查询串中不含令牌类参数 |
| 会话 Cookie 属性 | HttpOnly + SameSite=Strict + `__Host-`（安全模式） | ✅ 已满足 |
| 安全响应头 | CSP/HSTS/nosniff/DENY/no-referrer/permissions-policy 齐全 | ✅ 已满足 |
| 依赖扫描门禁 | 前端已干净；CI 未接入 | ⚠️ 需整改（第四节） |

---

## 六、修复优先级建议

1. **上线前必须完成**：
   - **SEC-19（`cargo update -p actix-http`，一条命令，成本最低收益最高）**、SEC-20（由反代终止 HTTP/2 并 HTTP/1.1 回源）
   - SEC-01（2FA 限速）、SEC-02（关闭 sourcemap）、SEC-05（强制 Secure Cookie）、SEC-06（开发代理隔离）、SEC-04（恢复上传总量上限）、SEC-03（评估期复校验）
2. **上线前建议完成**：SEC-08、SEC-09、SEC-10、SEC-11、SEC-14 与 CI 的 SCA/SAST 门禁。
3. **上线后可跟踪**：SEC-07、SEC-12、SEC-13、SEC-15～SEC-18。

## 七、未覆盖项（需另行安排）

- **动态运行时审计（DAST）**：本轮未对运行实例做主动扫描与渗透，未验证越权、上传畸形备份、超大/畸形 payload 的实际表现。建议在 staging 上执行 OWASP ZAP 主动扫描 + 手工用例：跨应用 IDOR（逐个替换 `applicationId` 访问 explorer/devices/stats/keys）、2FA 限速实测、恢复篡改 digest/截断文件/旧版本文件、`page` 深度翻页耗时、并发限速边界、**CL.TE 走私实测（升级前后各一次，用于验证 SEC-19 修复）**。
- **后端 Rust 官方工具复核**：本机无法访问 github.com，RustSec 库未能拉取，已用 OSV 数据库等效替代；请在能访问 GitHub 的机器/CI 上补跑 `cargo audit` 与 `cargo deny`。
- **基础设施层**：宿主系统与容器运行时的 CIS 基线、TLS 配置评级（testssl.sh/SSL Labs）未纳入本轮。

---

## 八、修复记录（本轮已实施）

| 编号 | 修复内容 | 涉及文件 |
|---|---|---|
| SEC-19 | `cargo update -p actix-http`：**3.11.2 → 3.13.5**（≥ 修复版 3.12.1），无需改动 `Cargo.toml` | `Cargo.lock` |
| SEC-01 | `AuthSecurity` 新增 `two_factor_gate` / `record_two_factor_failure`，并把 `record_failure` 重构为共用 `bump_attempts`；`verify_2fa_login` 增加 `source` 参数，按**来源**与**账户**双键限速，账户键用 `2fa-account:{user_id}` 从而跨登录累计，成功后清零。新增单元测试覆盖退避曲线 | `src/security.rs`、`src/services/authentication.rs`、`src/api/authentication.rs` |
| SEC-02 | 生产构建 `sourcemap: false`；静态资源处理器对 `.map` 一律返回 404（防止旧 `web/dist` 残留）；重新构建 `web/dist` | `web/vite.config.ts`、`src/web_assets.rs` |
| SEC-03 | 告警规则评估前重新执行 `expression.validate()`，不合法则跳过并告警 | `src/services/alerts.rs` |
| SEC-04 | 恢复上传新增 `MAX_RESTORE_BYTES = 2 GiB`：`Content-Length` 提前拒绝 + 流式累计超限即中断 | `src/api/backup.rs` |
| SEC-05 | 新增 `RuntimeConfig::requires_secure_cookies()`（非回环绑定即强制 `Secure` Cookie），`SONDE_ALLOW_INSECURE_COOKIES=1` 是唯一显式豁免；安装流程与数据库恢复流程改用同一判定，消除两处不一致；安装时与启动时均输出告警 | `src/config.rs`、`src/services/setup.rs`、`src/bootstrap.rs`、`src/lib.rs` |
| SEC-06 | `SONDE_DEV_PROXY` 仅接受环回目标（http/https + `127.0.0.1`/`::1`/`localhost`），否则忽略并告警 | `src/web_assets.rs` |
| SEC-07 | `AuthRequest::session_token(secure_cookie)` 只读取与配置一致的那一个 Cookie 名 | `src/services/authentication.rs`、`src/api/request_auth.rs` |
| SEC-08 | 新增 `reject_cross_site_origin`（Origin 存在即与 `Host` 比对，刻意不用可被转发头影响的 `ConnectionInfo`），应用于 `/auth/login` 与 `/auth/2fa/verify` | `src/api/request_auth.rs`、`src/api/authentication.rs` |
| SEC-09 | 时间戳漂移改为 `checked_sub` + `unsigned_abs`，极端时间戳一律判为过期而非溢出 | `src/ingest_signature.rs` |
| SEC-10 | 新增 `security::MAX_PAGE = 10_000` 与 `bounded_page` / `bounded_page_size`，应用于 explorer、devices、errors、audit 四个分页端点；`explorer.rs`、`applications.rs` 偏移量补齐饱和乘法 | `src/security.rs`、`src/api/{explorer,devices,errors,access}.rs`、`src/database/{explorer,applications}.rs` |
| SEC-11 | 广播载荷改为 `LiveUpdate { application_id, payload }`，SSE 按订阅者的 `telemetry.read` 作用域逐条过滤 | `src/state.rs`、`src/api/ingest.rs`、`src/api/stream.rs` |
| SEC-12 | Query/Path 解析错误与请求体读取错误改为通用文案，细节降级到 `tracing::debug` | `src/api/error_response.rs` |
| SEC-14 | compose 增加 `no-new-privileges`、`cap_drop: ALL`、`/tmp` tmpfs，并写入 TLS / Secure Cookie 部署说明 | `docker-compose.yml` |

### 行为变更（需要知悉）

- **SEC-05 会改变非回环 + 纯 HTTP 部署的登录行为**：`SONDE_BIND` 不是 127.0.0.1/::1/localhost 时，会话 Cookie 被强制标记 `Secure`（并使用 `__Host-` 前缀）。浏览器只在 HTTPS（或 `http://localhost`）下保存此类 Cookie，因此：
  - 通过 `http://<局域网IP>:8080` 直接访问将**无法保持登录**——这是有意的安全默认；
  - 如需继续用纯 HTTP 直连，显式设置 `SONDE_ALLOW_INSECURE_COOKIES=1`（启动日志会持续告警）；
  - 推荐做法：保持 `SONDE_BIND=0.0.0.0:8080` 并把 Sonde 放在 TLS 反向代理之后。
- 会话 Cookie 名与配置绑定后，从"不安全模式"切到"安全模式"（或反向）会让既有会话失效，用户需重新登录一次。
- 前端产物已变更，**上线前必须重新构建 release 二进制**（`web/dist` 经 `rust-embed` 在编译期嵌入）。

### 本轮未修复（原因）

| 编号 | 状态与原因 |
|---|---|
| SEC-13 | **已部分处理**：CI 中全部 Action 已固定到 commit SHA，新增 Dependabot 覆盖 cargo/npm/docker/github-actions（见第九节）。基础镜像保留 tag 而非 digest 属有意决策——Dependabot 只能推进 tag，digest 固定会同时冻结安全更新；改为"每周自动提 PR + 每次发布重建镜像" |
| SEC-15 | 备份加密是功能级改动（需密钥管理设计），**建议单独立项** |
| SEC-16 | **已实施限速缓解**：`/api/v1/setup/test` 与 `/complete` 增加按来源 IP 的固定窗口限速（30 次/分钟，见第九节）。"仅允许本地调用"仍会改变 LAN 首次安装用法，**需产品决策** |
| SEC-17 | 登录限速持久化涉及多副本架构选型（数据库/Redis），**建议随水平扩展方案一起做** |
| SEC-18 | TOTP 密钥加密存储需要基于 pepper 派生密钥的方案与数据迁移，**建议单独立项** |
| SEC-20 | 已用 `cargo update` 验证：h2 仍为 **0.3.27**，且 0.3 分支无补丁版本（修复只在 h2 0.4.16），**必须等 actix-http/awc 迁移到 h2 0.4**；当前缓解措施为反代终止 HTTP/2 |

### 验证

| 检查 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| `cargo check --all-targets --all-features --locked` | 通过 |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 通过（零告警） |
| `cargo test --lib --locked` | **107 项单元测试全部通过**（含新增：2FA 退避曲线、分页边界、极端时间戳、跨站 Origin 拒绝、反代改写 Host 兼容） |
| `cargo test --all-targets --all-features --locked` | 首次完整执行**全绿**：全部集成测试（含 `authorization_boundaries`、`path_traversal_prevention`、`backup_archive`、`api_concurrency`）+ `hot_path` 基准均通过 |
| 定向集成回归（最终代码） | `authorization_boundaries`(6) / `path_traversal_prevention`(1) / `application_devices_route`(1) / `api_concurrency`(5) / `backup_archive`(3) **全部通过** |
| 前端 `tsc --noEmit` + `vitest run` | 通过（4 个测试文件 / 9 项测试） |
| 前端 `vite build` | 通过；产物 `web/dist` **不含任何 `.map` 文件**（已逐一核对） |

> 环境说明：本机沙箱在 Windows 上偶发拦截 `target/**` 的文件写入（`os error 5 拒绝访问`），表现为"编译依赖文件写入失败"而非测试失败；重试并定向执行后全部通过。另需注意 `vite build` 默认的 `emptyOutDir` 删除 `web/dist` 会被沙箱阻塞导致构建挂起，需先把旧产物目录改名再构建（本次即以此方式完成）。

### 上线前仍需人工确认

1. 按新的 `Secure Cookie` 策略决定部署形态：TLS 反代（推荐）或 `SONDE_ALLOW_INSECURE_COOKIES=1`。
2. 重新构建 release 二进制并做一次 staging 冒烟：登录、2FA（含连续输错触发 429）、备份导出/恢复、SSE 实时流。
3. 执行第七节的 DAST 用例，其中 **CL.TE 请求走私需在升级前后各测一次**以确认 SEC-19 已消除。
4. 后端补跑官方 `cargo audit` / `cargo deny`（需可访问 GitHub 的网络环境）。

---

## 九、第二轮：依赖更新与供应链治理

### 9.1 依赖更新（已在仓库执行并验证）

**后端 `cargo update`**（80 个包，MSRV 感知：只选 Rust 1.95 兼容版本）：

| 变化 | 说明 |
|---|---|
| actix-web 4.12.1 → **4.15.0**，awc 3.8.1 → 3.8.2，actix-rt 2.11 → 2.15，actix-server 2.6.0 → 2.9.5 | HTTP 栈整体前进 |
| sea-orm / sea-orm-migration 2.0.2 → **2.0.3**，sea-query 1.0.2 | ORM 补丁版 |
| rustls 0.23.43 → 0.23.44，tokio-rustls 0.26.4 → 0.26.5，rustls-webpki 0.103.15 | TLS 栈补丁版 |
| uuid 1.25.0 → 1.26.1，chrono 0.4.45，zstd-sys 2.0.16 → 2.1.0，encoding_rs 0.8.35 → 0.8.41 | 常规补丁 |
| **移除 rkyv 0.7.46 + bytecheck + ptr_meta + rend + bitvec + ahash 0.7 等** | 陈旧锁定条目被清理，SCA 命中项从 3 条降到 1 条 |

**前端 `pnpm update`**（限定 semver 范围内，避免跨大版本）：

| 变化 | 说明 |
|---|---|
| react / react-dom 19.2.8 → **19.3.0**，react-router-dom 7.18.2 → 7.18.3 | 运行时补丁/次版本 |
| motion 13.1.1 → 13.2.0，tailwind-merge 3.6.0 → 3.7.0 | 常规更新 |
| @types/react / @types/react-dom → 19.3.0，@testing-library/react 16.3.2 → 16.3.3 | 类型与测试工具 |
| **移除 2 个未使用的遗留包**：`@fontsource-variable/jetbrains-mono`、`@fontsource-variable/manrope`（代码中无任何引用，属早期留存的死依赖） | 减少依赖面 |

**未跨大版本**（需要单独迁移，属有意保留）：i18next 25→26、react-i18next 16→17、vite 7→8、vitest 4→5、typescript 5.9→7、jsdom 27→30、@vitejs/plugin-react 5→6、lucide-react 0.468→1.x、@testing-library/jest-dom 6→7。这些都需要代码/配置迁移（Vite 8 + Tailwind 4 插件、Vitest 5 配置、TS 7 类型严格度、图标组件命名）与一轮完整回归，建议单独排期而不是混在安全修复里。

### 9.2 供应链固定

- `.github/workflows/ci.yml`：全部 Action 由可变 tag 改为 **commit SHA 固定**（并保留 `# vX` 注释以便 Dependabot 追踪）：`actions/checkout`、`dtolnay/rust-toolchain`、`Swatinem/rust-cache`、`pnpm/action-setup`、`actions/setup-node`。
- 新增 `.github/dependabot.yml`：cargo / npm / docker / github-actions 四个生态**每周一**检查；minor+patch 合并为单个 PR，major 单独出 PR 以便按 MSRV 与工具链版本逐个人工评审。
- 基础镜像（`node:22-bookworm-slim`、`rust:1.95-bookworm`、`debian:bookworm-slim`）**有意保留 tag**：Dependabot 的 docker 生态只能推进 tag，固定 digest 会让安全更新同时被冻结；配套要求是每次 tag 变动后重建发布镜像。

### 9.3 CI 安全门禁（新增 `security` job）

原来 CI 只有格式/检查/Clippy/测试/前端构建，**没有任何依赖审计**——这正是 SEC-19 能进入 `Cargo.lock` 的原因。现新增阻断式门禁：

- `cargo audit`：任一影响 `Cargo.lock` 的 RustSec 通告即失败；
- `pnpm audit --audit-level=high`：前端高危及以上即失败。

即"高危且已有修复版本"从"上线前人工检查"变成"合并前自动阻断"。若某个漏洞的修复版本超出 MSRV 窗口，必须在 `Cargo.toml`/文档中留下显式例外记录，而不能静默忽略。

### 9.4 第二轮验证结果

| 检查 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| `cargo check --all-targets --all-features --locked` | 通过（新依赖树） |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | 通过（零告警） |
| `cargo test --lib --locked` | **108 项通过**（新增 `pre_install_throttle_limits_each_source`） |
| 集成测试（`api_concurrency`/`authorization_boundaries`/`path_traversal_prevention`/`application_devices_route`/`sqlite_bootstrap`/`backup_archive`/`ingest_nonce_replay`） | 共 **18 项全部通过** |
| 前端 `typecheck` + `vitest run` | 通过（9 项） |
| 前端 `vite build` | 通过（15.6s），产物无 `.map` |
| OSV 复扫（后端 438 包 / 前端 368 包） | 后端仅剩 **h2 0.3.27**（无补丁版本）；**actix-http 命中项已消除，rkyv 已随更新移除**；前端 0 命中 |

### 9.5 下一步建议

1. **h2（SEC-20）**：跟踪 actix-web/awc 迁移到 h2 0.4；在此之前由反向代理终止 HTTP/2、以 HTTP/1.1 回源。
2. **前端大版本迁移**：单独开一轮 Vite 8 / Vitest 5 / TypeScript 7 / i18next 26 的升级，配 Dependabot 的 major PR 一起做。
3. **备份加密（SEC-15）与 TOTP 密钥加密（SEC-18）**：建议合并为一次"静态机密保护"设计（pepper 派生密钥 + 格式版本升级 + 迁移）。
4. **限速状态持久化（SEC-17）**：随多副本部署方案一起落地（可复用数据库中已有的 `auth_shared_state` 表）。

