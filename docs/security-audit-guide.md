# Sonde 前后端 Web 安全审计方案（上线前）

> 适用对象：Sonde 自托管遥测分析平台
> 后端：Rust 1.95 / actix-web 4 / SeaORM 2（SQLite/MySQL/Postgres）/ argon2 / rustls
> 前端：React 19 / TypeScript / Vite 7 / react-router-dom 7（pnpm 管理）
> 部署：Docker / docker-compose，前端经 rust-embed 嵌入后端二进制分发

本文档将安全审计拆分为四类——**静态代码审计（SAST）、动态运行时审计（DAST）、依赖组件审计（SCA）、配置合规审计**——分别说明其目标范围、检测方法、适用阶段与产出结果，最后给出上线前必须完成的检查要点清单。

---

## 0. 四类审计总览与差异对比

| 维度 | 静态代码审计（SAST） | 动态运行时审计（DAST） | 依赖组件审计（SCA） | 配置合规审计 |
|---|---|---|---|---|
| **核心问题** | "代码里写了什么危险的东西？" | "运行起来的系统实际能被怎么攻击？" | "引入的第三方代码本身有没有洞？" | "部署与运行配置是否安全合规？" |
| **审计对象** | 源码、模板、构建脚本 | 运行中的服务与 HTTP 接口 | Cargo.lock、pnpm-lock、基础镜像、工具链 | 环境变量、Dockerfile、反向代理、安全响应头、密钥管理 |
| **是否需要运行** | 否（离线分析） | 是（黑盒/灰盒攻击测试） | 否（清单比对） | 部分需要（验证实际生效的配置） |
| **主要阶段** | 开发期（每 PR）+ 上线前全量 | 测试期（staging）+ 上线前回归 | 开发期 CI + 上线前 + 上线后持续 | 上线前必做 + 每次配置变更 |
| **典型产出** | 带文件/行号的漏洞清单 + 修复建议 | 可复现 PoC + CVSS 评级 | CVE 清单 + 升级/缓解方案 + SBOM | 基线核对表 + 不合规项 + 合规配置样例 |
| **局限性** | 误报多，难发现逻辑/越权漏洞 | 覆盖率依赖爬取深度，难及非 HTTP 面 | 无法发现自研代码漏洞 | 只保证"配置正确"，不保证"代码正确" |

四类审计互为补充：SAST 找代码缺陷、DAST 验证真实可利用性、SCA 堵供应链风险、配置审计保证部署态安全。**任何一类都不能替代其他三类。**

---

## 1. 静态代码审计（SAST）

### 1.1 目标范围

- **后端 Rust 源码**（`src/` 全部 134 个文件，按架构分层重点覆盖）：
  - `api` 层：路由注册、请求提取与校验、传输层大小限制；
  - `services` 层：认证授权逻辑、应用（多租户）隔离、备份导出/恢复编排；
  - `database` 层：SeaORM 查询构造，是否存在拼接 raw SQL；
  - `domain` 层：业务规则与权限判定；
  - 专项：`build.rs`、`sdk/`、`benches/`、`tests/`（测试凭据是否泄漏真实密钥）。
- **前端源码**（`web/src` 的 tsx/ts）：
  - 所有用户输入渲染路径（XSS 关注点）；
  - 认证令牌的存储与传输；
  - 路由守卫与前端权限控制（仅作体验，不作安全边界）。
- **构建脚本**：`Dockerfile`、`docker-compose.yml`、CI workflow、`scripts/`。

### 1.2 检测方法

**自动化工具：**

| 目标 | 工具与命令 |
|---|---|
| Rust 通用缺陷 | `cargo clippy --all-targets --all-features --locked -- -D warnings`（项目已强制） |
| Rust 安全告警 | `cargo audit`（RustSec 数据库） |
| 多策略依赖与许可 | `cargo deny check` |
| Rust/TS 通用漏洞模式 | CodeQL（`codeql database create` 双语言）、Semgrep（自定义规则集） |
| TS/React 检查 | `tsc --noEmit`（strict）、ESLint（`eslint-plugin-security`、`eslint-plugin-react`） |

**人工代码走查重点（自动化工具覆盖不到的逻辑漏洞）：**

后端：
1. **认证**：argon2 参数强度（内存/迭代次数是否符合 OWASP 2023 建议）；password pepper（`data/*.password-pepper`）的读取与错误提示是否泄露存在性；会话/令牌生成熵源与过期撤销。
2. **越权（IDOR）**：所有按 `app_id`/记录 ID 查询的接口，是否逐一校验当前用户对该应用的所有权——遥测平台最典型的高危点。
3. **注入**：全局搜索 raw SQL / `Statement::from_string` / 字符串拼接查询；SeaORM 默认参数化，但自定义 SQL 片段需逐条确认。
4. **SSRF**：`awc` / `reqwest` 的所有出站调用，URL 是否可被用户输入影响，是否限制协议与内网地址。
5. **备份/恢复**：恢复流程是否严格"先全量校验（framing、manifest、digest、计数）后落库"；是否拒绝非当前版本的备份文件；恢复目标路径是否可被备份内容操纵（zip-slip 类路径穿越）。
6. **错误处理**：API 错误响应不回显内部凭据、SQL、文件路径（AGENTS.md 已有规则，逐 handler 抽查）。
7. **密码学用途**：依赖中的 `sha1` 仅可用于非安全场景（如对象指纹/ETag），任何安全用途（签名、口令派生）必须是 SHA-256/HMAC。
8. **panic 面**：`unwrap/expect/panic` 已被 lint 禁止，另需关注数组越界、整数溢出（`overflow-checks` 在 release 是否开启）。

前端：
1. `dangerouslySetInnerHTML`、`innerHTML`、`document.write` 全局搜索并逐处评估输入可控性；
2. `href={userInput}` / `window.open` 的 `javascript:` scheme 注入；
3. `target="_blank"` 是否统一带 `rel="noopener noreferrer"`；
4. 令牌存储位置（localStorage 有 XSS 可窃取风险，评估改 HttpOnly Cookie 的可行性）；
5. i18next 插值是否开启 `interpolation.escapeValue`（默认开启，勿关闭）；
6. 前端权限判断不得作为唯一防线——后端必须重复校验。

### 1.3 适用阶段

- **开发期**：随 CI 在每个 PR 上运行自动化 SAST（clippy、audit、CodeQL、ESLint）。
- **上线前**：做一次全量人工走查（重点 1.2 人工清单），此时功能已冻结，缺陷定位最准确。

### 1.4 产出结果

- 漏洞清单：编号、文件路径与行号、缺陷类型（OWASP Top 10 / CWE 映射）、危险输入路径说明；
- 风险评级：按 CVSS 4.0 或高/中/低分级；
- 修复建议：具体到代码层级的修改方案；
- 误报标注记录（避免下轮重复排查）。

---

## 2. 动态运行时审计（DAST）

### 2.1 目标范围

以**部署在 staging 环境的真实运行实例**为对象：

- 全部 HTTP 路由：认证、遥测上报、查询、备份导出/恢复、静态资源（嵌入的前端）；
- 认证会话全生命周期：登录、令牌刷新/过期、登出、密码重置；
- 运行时行为：错误响应内容、响应头、速率限制、并发行为。

### 2.2 检测方法

**自动化扫描：**

| 方法 | 说明 |
|---|---|
| OWASP ZAP | 主动 + 被动扫描：先爬取路由（含登录后的会话爬取），再跑主动扫描器 |
| Burp Suite | 手工渗透的核心工具：拦截重放、越权测试、参数篡改 |
| nuclei | 用公开模板 + 自写模板批量验证（弱头、路径穿越、默认凭据等） |
| HTTP 响应头检查 | `curl -sI` 或 securityheaders.com 评级（见第 4 类） |

**重点手工测试用例（结合本项目特性）：**

1. **认证**：爆破登录接口验证速率限制与账号锁定；错误提示是否区分"用户不存在/密码错误"；argon2 响应时间侧信道。
2. **越权（必测）**：用户 A 的令牌访问用户 B 的应用数据 / 遥测记录 / 备份文件——逐一替换 ID 测试所有 `/apps/{id}/...` 类路由。
3. **备份导出/恢复**：上传篡改过 digest 的备份必须被拒且目标库不变；上传裁断文件（framing 错误）必须被拒；旧版本备份必须被拒；恢复过程不得产生半写状态。
4. **注入**：对查询参数、filter 表达式、排序字段尝试 SQL 注入payload；对 JSON 字段尝试二阶注入。
5. **SSRF**：若存在任何用户可配置的出站 URL（webhook、导入源），指向 `http://127.0.0.1`、`http://169.254.169.254`、`file://` 验证防护。
6. **DoS**：超大 JSON payload、超深嵌套、超长字段；流式导出时并发拉取验证背压；慢速攻击（slowloris）验证 actix-web 超时配置。
7. **会话**：令牌固定/重放、登出后令牌是否失效、Cookie 属性（若用 Cookie）。
8. **并发**：利用已有 `tests/api_concurrency.rs` 思路，对认证与备份恢复做并发竞争测试。
9. **前端运行时**：在输入框/展示位注入 XSS payload（尤其图表标签、应用名等会回显到 SVG 的位置——recharts 的 label 若用原生渲染需验证）。

### 2.3 适用阶段

- **测试期（staging 就绪后）**：第一轮完整 DAST + 渗透测试。
- **上线前**：修复后回归验证（只复测已发现漏洞 + 快速全扫）。
- **上线后**：每次大版本发布前、以及季度性复扫。

### 2.4 产出结果

- 可复现漏洞报告：每个漏洞附 HTTP 请求/响应证据（PoC）、复现步骤；
- CVSS 评分与风险评级（结合暴露面：公网自托管产品默认按更高暴露等级评分）；
- 修复验证记录（复测通过才可关闭）。

---

## 3. 依赖组件审计（SCA）

### 3.1 目标范围

- **后端**：`Cargo.lock` 全部传递依赖（actix-web、tokio、rustls、sea-orm/sqlx、reqwest、awc 等均为高关注度组件）；
- **前端**：`pnpm-lock.yaml` 全部 npm 依赖（react、vite、recharts、radix-ui、i18next 等）；
- **构建链**：Rust 工具链版本、Node/pnpm 版本、`Dockerfile` 基础镜像（含系统库 CVE）、CI 所用 GitHub Actions 版本；
- **间接面**：`web/*.map`（source map 是否随产物发布）。

### 3.2 检测方法

| 方法 | 命令/工具 | 说明 |
|---|---|---|
| Rust 安全通告 | `cargo audit` | 比对 RustSec，检查 Cargo.lock |
| 多维策略检查 | `cargo deny check advisories bans licenses sources` | 通告 + 禁用清单 + 许可证 + 来源锁定 |
| npm 审计 | `pnpm audit --prod` | 生产依赖漏洞 |
| 统一多生态扫描 | OSV-Scanner（同时覆盖 Cargo + pnpm） | 一条命令扫双生态 |
| 镜像扫描 | Trivy image / Grype | Dockerfile 基础镜像与系统包 |
| 密钥扫描 | gitleaks / trufflehog | 历史提交中是否泄漏密钥（含 pepper、API key） |
| SBOM | `cargo auditable build` + syft | 生成物料清单，供上线后响应新 CVE 反查 |

**策略要求：**
- 构建/发布一律 `--locked`（Cargo 与 pnpm 均锁定 lockfile，禁止现场解析版本）；
- CI 中将 SCA 设为阻断步骤：存在"高危且已有修复版本"的 CVE 时禁止合并；
- 基础镜像优先选 distroless / minimal 变体并固定 digest（而非 tag）。

### 3.3 适用阶段

- **开发期**：CI 每次构建自动运行（成本低、收益高，应最早接入）。
- **上线前**：全量扫描 + 生成 SBOM 归档，作为发布物料的一部分。
- **上线后（持续）**：订阅 RustSec / GitHub Dependabot / OSV 通告，新 CVE 披露时凭 SBOM 快速判断是否受影响——这是唯一"上线后仍持续进行"的审计类型。

### 3.4 产出结果

- CVE 漏洞清单：CVE 编号、影响组件与版本、CVSS/KEV 标记、是否存在利用补丁；
- 处置决策：升级 / 版本回退 / 缓解措施（如禁用受影响 feature）/ 接受风险（需书面记录理由）；
- 许可证合规报告（Sonde 为 MIT，需确认无 GPL 等传染性许可混入）；
- SBOM 文件（CycloneDX/SPDX 格式）随版本发布归档。

---

## 4. 配置合规审计

### 4.1 目标范围

- **密钥与凭据**：`data/*.password-pepper` 的文件权限（应 0600，仅服务账号可读）、数据库凭据、CI/CD secrets、docker-compose 中的环境变量是否明文入仓。
- **传输安全**：对外是否强制 TLS（rustls 终止或反向代理终止）、HSTS、HTTP→HTTPS 重定向。
- **安全响应头（由后端或反代输出）**：`Content-Security-Policy`、`X-Content-Type-Options: nosniff`、`X-Frame-Options`/`frame-ancestors`、`Referrer-Policy`、`Cache-Control`（API 响应 `no-store`，防止敏感数据经缓存泄露）。
- **会话与 Cookie**（若使用 Cookie）：`HttpOnly`、`Secure`、`SameSite=Strict/Lax`、域与路径范围。
- **CORS**：跨域白名单是否收敛为明确来源，禁用 `*` 配合凭据。
- **Docker/部署**：容器以非 root 运行；`docker-compose.yml` 端口只暴露必要端口（数据库不应对外暴露）；data 卷权限；多阶段构建，最终镜像不含构建工具与源码。
- **数据库**：SQLite 文件与 WAL/SHM 的目录权限；MySQL/Postgres 使用最小权限账号（禁 DDL、禁超级用户）。
- **日志**：tracing 配置确认不输出请求体中的敏感字段、口令、令牌。
- **合规基线**：以 OWASP ASVS L2 为基线逐条核对（自托管多用户系统建议 L2）；部署主机可参照 CIS Docker Benchmark。

### 4.2 检测方法

1. **清单核对**：按 ASVS L2 / CIS 逐项打勾（人工，半天内可完成）；
2. **自动化配置扫描**：Trivy config（Dockerfile/compose 规则）、docker-bench-security（宿主机运行时）；
3. **实际生效验证**（区别于"配置文件写了什么"）：
   - `curl -sI https://staging/api/...` 逐项核对响应头；
   - 错误页面与 404 响应不泄露框架版本号；
   - 用 SSL Labs 或 `testssl.sh` 评级 TLS 配置；
   - 用错误令牌访问 API 验证返回 401 而非 500；
4. **密钥卫生**：gitleaks 全历史扫描 + 仓库中搜索 pepper/secret/password 字样。

### 4.3 适用阶段

- **上线前（必做，且最晚定稿）**：部署形态（域名、TLS、反代、容器配置）只有到上线前才最终定型，因此本类审计天然是"上线前关口"。
- **每次配置变更时**：任何 docker-compose / 环境变量 / 反代改动都触发增量复查。

### 4.4 产出结果

- 基线核对表（ASVS/CIS 条目 → 满足/不满足/不适用）；
- 不合规项清单与整改责任人、期限；
- 合规配置样例（可直接使用的反代/CSP/响应头配置片段）；
- 上线放行签字依据。

---

## 5. 前后端专项覆盖要点汇总

### 5.1 前端（React/Vite）专项
| 风险 | 要点 |
|---|---|
| XSS | React 默认转义安全；重点审查 `dangerouslySetInnerHTML`、SVG/图表标签回显、i18n 插值 |
| 令牌存储 | 优先 HttpOnly Cookie；若 localStorage，必须有强 CSP 兜底 |
| Source Map | 生产构建不发布 `.map`（当前 `web/` 内存在 .map 文件，上线前确认不进产物） |
| 依赖供应链 | lockfile 必须入库；`pnpm install` 禁用 scripts 或白名单 |
| 前端权限 | 路由守卫仅控制显隐，所有数据接口后端重复鉴权 |
| 敏感信息 | bundle 中不得硬编码 API key、内网地址 |

### 5.2 后端（Rust/actix-web）专项
| 风险 | 要点 |
|---|---|
| 认证 | argon2 参数达标；pepper 文件权限与轮换预案；登录限速 |
| 越权 | 所有 app/记录级访问校验属主（IDOR 为遥测平台首要风险） |
| 注入 | SeaORM 参数化；raw SQL 逐条审计 |
| SSRF | awc/reqwest 出站 URL 白名单化，禁内网与非常规协议 |
| 备份恢复 | 先验证后落库（framing/manifest/digest/计数）；拒绝旧版本；防路径穿越 |
| DoS | 请求体大小限制、字段长度限制、流式接口背压、超时配置 |
| 错误与日志 | 不泄露凭据/SQL/路径；tracing 不落敏感字段 |
| 密码学 | sha1 仅限非安全用途；随机数统一 `rand`（OS 熵） |

---

## 6. 上线前安全审计检查要点（Checklist）

### 静态代码（SAST）
- [ ] `cargo clippy -D warnings` / `cargo fmt --check` 全绿（CI 已有）
- [ ] `cargo audit`、`cargo deny` 无未处置高危项
- [ ] CodeQL/Semgrep 全量扫描完成，高危项清零
- [ ] 人工走查完成：认证、越权、注入、SSRF、备份恢复、错误处理六大专项
- [ ] 前端 `dangerouslySetInnerHTML`/`innerHTML` 全部审查并留注释
- [ ] 仓库全历史无密钥泄漏（gitleaks）
- [ ] 测试代码无真实凭据/生产数据

### 动态运行时（DAST）
- [ ] staging 环境完成 ZAP 主动+被动扫描，高危/中危清零
- [ ] 越权专项：跨用户访问应用/记录/备份全部被拒（逐一替换 ID 验证）
- [ ] 备份恢复：篡改 digest、截断文件、旧版本全部被拒且库状态不变
- [ ] 登录接口限速与锁定策略验证通过
- [ ] 超大/畸形 payload 不导致 5xx 或内存失控
- [ ] 无效令牌统一 401，无堆栈/内部信息回显
- [ ] XSS 注入测试（含图表标签、应用名等回显位）通过

### 依赖组件（SCA）
- [ ] `cargo audit` + `pnpm audit` + OSV-Scanner 双生态扫描无高危未处置项
- [ ] 基础镜像 Trivy 扫描通过，镜像以 digest 固定
- [ ] 所有发布构建使用 `--locked`（Cargo 与 pnpm）
- [ ] SBOM（CycloneDX/SPDX）生成并随发布归档
- [ ] Dependabot/OSV 通告订阅已开启（上线后持续项）
- [ ] 许可证检查通过，无传染性许可混入

### 配置合规
- [ ] 容器以非 root 用户运行；最小端口暴露；数据库不对外
- [ ] `data/`（含 pepper、SQLite+WAL）权限最小化，未随镜像分发
- [ ] 强制 HTTPS + HSTS；TLS 评级 A 以上（testssl/SSL Labs）
- [ ] 安全响应头齐全：CSP、nosniff、frame-ancestors、Referrer-Policy、API `Cache-Control: no-store`
- [ ] CORS 白名单收敛；无凭据 + 通配符组合
- [ ] Cookie（若有）：HttpOnly + Secure + SameSite
- [ ] 生产 source map 未发布
- [ ] tracing/日志确认不含口令、令牌、敏感字段
- [ ] OWASP ASVS L2 核对完成，不满足项均有书面接受理由
- [ ] 应急预案就绪：密钥轮换步骤、备份恢复演练通过、漏洞响应流程与联系人明确

### 放行标准（建议）
- **严重（Critical）/ 高危（High）**：清零方可上线；
- **中危（Medium）**：有修复排期（≤2 周）+ 临时缓解措施；
- **低危（Low）**：登记跟踪，不阻断上线；
- 所有"接受风险"决策需书面记录并经负责人签字。

---

## 7. 审计执行时间线

| 阶段 | 动作 |
|---|---|
| 开发期（持续） | CI 内置：clippy、cargo audit/deny、pnpm audit、CodeQL、ESLint、gitleaks |
| 测试期（staging 就绪） | 第一轮完整 DAST（ZAP + 手工渗透：越权/备份/认证专项） |
| 上线前（冻结后 1–2 周） | 全量人工 SAST 走查 → DAST 回归 → SCA 全量 + SBOM 归档 → 配置合规核对 → 放行评审 |
| 上线后（持续） | SCA 通告订阅与响应；季度 DAST 复扫；配置变更增量审计 |

---

## 8. 风险评级参考

| 等级 | 定义（示例） | 处置时限 |
|---|---|---|
| 严重 | 未授权访问任意用户数据（越权全量绕过）、备份恢复可注入任意数据、RCE | 立即修复，阻断上线 |
| 高 | 认证绕过、SQL 注入、SSRF 可打内网、pepper 泄露 | 上线前必须修复 |
| 中 | 缺失安全响应头、速率限制缺失、中危 CVE 无补丁但有缓解 | ≤2 周 + 缓解措施 |
| 低 | 信息泄露（版本号）、低危 CVE、日志 verbosity | 登记跟踪 |
