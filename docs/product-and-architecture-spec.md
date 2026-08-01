# Product and Architecture Specification (Archived)

> **Status**: Archived. This is the original task specification that initiated
> the Deve Sub project. The current engineering authority lives in
> `docs/plan/`. This document is retained as product and architecture history;
> where it conflicts with `docs/plan/` or `docs/contracts/`, the plan and
> contracts prevail.
>
> Name corrections have been applied: the project name is **Deve Sub**,
> packages use the `deve-sub-*` prefix, the binary is `deve-sub`, and the V3
> template namespace is `deve-sub.io/v1`.

---

## 技术结论

### 1. 全栈 Rust 可以采用，但建议设置技术验证门槛

截至 2026 年 8 月，Dioxus 最新稳定分支为 0.7.x，当前发布版本为 0.7.9；其 Fullstack 模式已经集成 Axum，支持服务器函数、静态资源、SSR、文件流、SSE 和 WebSocket，0.7 还增加了第一方组件原语。Leptos 当前稳定版为 0.8.20，0.9 仍处于测试阶段，官方服务端集成也优先推荐 Axum。两者都足以开发生产级后台。

对这个项目，我的首选是：

```text
Dioxus 0.7.x Web
+ Axum 0.8
+ Tokio
+ SQLx
+ SQLite
```

但不能把 Dioxus Server Functions 当成整个系统的架构。应当保留明确的应用接口：

```text
Dioxus UI
    ↓ typed API client
/api/v1
    ↓
Application Commands / Queries
    ↓
Domain Modules
    ↓
Ports
    ↓
SQLite、HTTP、GeoIP、探针等 Adapters
```

这样即使将来发现 Rust 前端在复杂数据表格、拖拽或无障碍方面成本过高，只需要替换 UI Adapter，后端、CLI、协议引擎和数据库层都不需要重写。

正式开发前应先做一个两周的 UI 技术验证，必须同时通过：

- 10,000 节点虚拟列表；
- 多选、分页、过滤；
- 500 个项目的拖拽排序；
- 中英文切换；
- 日间、夜间、自定义主题；
- SSE 任务进度；
- 30 天流量图；
- 移动端基本操作；
- Playwright 自动化测试。

没有通过时，前端改用 React，生产服务端仍保持纯 Rust。Node.js 只存在于前端构建和 CI，不进入生产运行环境。

### 2. 数据库选择：SQLite 默认，PostgreSQL 可选，不使用 redb 作为主库

redb 已经是稳定、维护中的 Rust 嵌入式 ACID 键值数据库，也支持 MVCC；但它是键值数据库，不适合这个项目大量存在的关联、分页、搜索、审计、权限、统计和版本查询。使用 redb 会迫使项目自行维护二级索引、关联关系和迁移机制。

推荐：

```text
默认部署：SQLite + WAL
多实例部署：PostgreSQL，可在后续版本提供
```

SQLite 很适合单机应用服务器和中低并发网站。WAL 模式允许读取和写入并行，但同一时刻仍然只有一个写入者，因此必须保持写事务短小。

PostgreSQL 适合多实例、较高写并发和高可用部署，其 MVCC 能减少读写锁冲突，也有完整的复制和故障转移体系。

SQLx 当前支持 SQLite 和 PostgreSQL，包含连接池、Migration 和可选的编译期查询检查，适合作为数据库访问层。

第一版不要同时维护两套 SQL。架构上定义存储 Port，先完整实现 SQLite Adapter，PostgreSQL 作为后续里程碑。

### 3. 三个概念需要明确

你说的 `shadowsocket（apple）` 应拆成：

- **Shadowsocks**：代理协议；
- **Shadowrocket**：Apple 平台客户端。

"UDP 延迟"也不能简单等价于 TCP Ping。任意 UDP 端口不一定返回数据，超时不能说明节点失效。系统应展示：

```text
TCP 连接延迟
TLS / QUIC 握手延迟
真实代理请求延迟
UDP / QUIC 可达状态
```

Hysteria2 和 TUIC 可以测 QUIC 握手 RTT；其他 UDP 能力应由真实代理测试器验证，禁止生成没有实际意义的"UDP Ping"。

此外，订阅聚合器本身无法测量用户实际经过代理节点的流量。"流量限制"必须依赖 Nezha、DStatus、Komari、机场响应头或手工数据。系统可以根据探针结果停止分发订阅，但不能仅通过订阅下载次数推算真实代理流量。

---

# Deve Sub 开发任务书

## 一、项目定义

项目名：

```text
Deve Sub
```

名称、Logo、站点标题必须集中配置，禁止散落在代码中。

本项目是一个自托管的代理订阅基础设施管理系统。功能和业务流程可以参考 miaomiaowu，但必须独立实现：

- 不复制其源代码；
- 不复制其 Logo、插画或品牌资源；
- 不进行像素级照搬；
- 可以参考其页面信息架构、业务流程和使用习惯；
- 删除旧模板系统和覆写脚本；
- 重新实现协议模型、生成器、任务系统和权限体系。

核心业务链路：

```text
订阅源和单节点
        ↓
抓取、解析、标准化
        ↓
统一节点库
        ↓
筛选、去重、排序、编辑、链式代理
        ↓
代理组和规则编排
        ↓
生成多个客户端格式
        ↓
用户授权、随机密钥、流量与到期控制
        ↓
长期订阅 URL
```

系统定位是**模块化单体应用**，不是微服务集合。

---

## 二、技术栈

### 2.1 生产技术栈

```text
Rust stable
Dioxus 0.7.x Web / Fullstack
Axum 0.8
Tokio
Tower / tower-http
SQLx
SQLite
Serde
Reqwest + rustls
Tracing
Clap
Argon2id
XChaCha20-Poly1305
OpenAPI
```

所有版本必须固定在 `Cargo.lock`，禁止生产镜像使用未固定的 Git 依赖或 `latest` 语义。

### 2.2 自动化测试例外

浏览器端 E2E 使用 Playwright。Node.js 只存在于 CI 和开发环境，不进入最终镜像。

其他测试：

```text
cargo test
sqlx::test
proptest
cargo-fuzz
testcontainers
Playwright
k6 或 oha
```

### 2.3 前端定位

前端必须是薄前端，只负责：

- 表单状态；
- 交互；
- 数据展示；
- 本地未提交编辑；
- 国际化；
- 主题渲染；
- 调用应用接口。

前端不得负责：

- 节点解析；
- 协议转换；
- 最终订阅生成；
- 客户端兼容判断；
- 安全字段修正；
- 订阅源合并；
- 核心 YAML 生成；
- 权限判断。

最终生成和验证必须在 Rust 服务端执行。

---

## 三、总体架构

采用六边形架构和轻量 CQRS：

```text
┌──────────────── Delivery ────────────────┐
│ Dioxus Web │ REST API │ CLI │ Public Sub │
└──────────────────┬───────────────────────┘
                   │
┌──────────────── Application ─────────────┐
│ Commands │ Queries │ Jobs │ Event Handler │
└──────────────────┬───────────────────────┘
                   │
┌────────────────── Domain ────────────────┐
│ Source │ Node │ Template │ Subscription   │
│ Identity │ Probe │ Traffic │ Compatibility│
└──────────────────┬───────────────────────┘
                   │ Ports
┌──────────────── Adapters ────────────────┐
│ SQLite │ HTTP │ GeoIP │ Probe │ Files     │
│ Release Updater │ Notification │ Test Core│
└──────────────────────────────────────────┘
```

### 3.1 模块规则

每个业务模块至少包含：

```text
domain.rs
commands.rs
queries.rs
service.rs
ports.rs
events.rs
errors.rs
dto.rs
```

规则：

1. Domain 不依赖 Axum、Dioxus、SQLx。
2. Application 不直接执行 SQL。
3. API Handler 不包含业务规则。
4. 前端不能直接依赖数据库模型。
5. 模块之间通过公开 Service、Command、Query 或 Domain Event 通信。
6. 不允许循环依赖。
7. 不使用全局可变状态。
8. 不为每个数据库表创建一个无业务意义的 Service。
9. 不建设通用"万能 Repository"。
10. 不采用完整事件溯源；只使用普通状态表、审计日志和持久化 Outbox。

### 3.2 UI 按钮与接口映射

一个 UI 操作必须映射到明确的应用用例。例如：

```text
"立即更新订阅源"
    ↓
POST /api/v1/sources/{id}/refresh
    ↓
RefreshSourceCommand
    ↓
SourceRefreshService
    ↓
SourceFetcher Port
    ↓
NodeReconciler
    ↓
SourceRefreshed Event
    ↓
SubscriptionCacheInvalidator
```

禁止按钮直接触发一系列散落的数据库修改。

---

## 四、Workspace 结构

建议控制在 12～16 个 crate，避免一项功能一个 crate 导致编译时间失控。

```text
apps/
├── server/                 # HTTP、Web UI、公开订阅
├── cli/                    # CLI 与 headless 管理
└── web/                    # Dioxus Web

crates/
├── kernel/                 # ID、时间、分页、通用错误
├── contract/               # API DTO、事件 DTO、客户端能力
├── domain/                 # 领域模型
├── application/            # Commands、Queries、业务用例
├── protocol/               # 输入解析和统一节点模型
├── emitter/                # 目标格式输出
├── compatibility/          # 客户端能力 Profile
├── storage-sqlite/
├── storage-postgres/       # 后续版本
├── adapters/
├── scheduler/
├── security/
├── observability/
└── testkit/

frontend-assets/
migrations/
fixtures/
docs/
scripts/
deploy/
```

---

## 五、统一节点模型

### 5.1 输入格式与代理协议分离

输入格式：

```text
分享 URI 列表
Base64 通用订阅
Mihomo / Clash YAML
sing-box JSON
Xray JSON
V2Ray JSON
Shadowrocket 分享列表或配置
```

协议：

```text
VLESS
VMess
Trojan
Shadowsocks
Hysteria2
TUIC v5
NaiveProxy
SOCKS5
HTTP
Hysteria v1
AnyTLS
Snell
WireGuard
ShadowTLS
SSH
```

P0 强制完整支持：

```text
VLESS Reality
Hysteria2
TUIC v5
NaiveProxy
Shadowsocks
VMess
Trojan
```

### 5.2 Canonical Node Model

```rust
pub struct Node {
    pub id: NodeId,
    pub display_name: String,
    pub protocol: Protocol,
    pub endpoint: Endpoint,
    pub authentication: Authentication,
    pub transport: Option<Transport>,
    pub tls: Option<TlsConfig>,
    pub udp: UdpCapability,
    pub multiplex: Option<MultiplexConfig>,
    pub obfuscation: Option<Obfuscation>,
    pub congestion: Option<CongestionConfig>,
    pub chain: Option<ChainTarget>,
    pub source: NodeSource,
    pub tags: Vec<TagId>,
    pub region: RegionAssignment,
    pub extras: BTreeMap<String, serde_json::Value>,
}
```

地址模型：

```rust
pub enum Host {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Domain(DomainName),
}
```

输出 IPv6 URI 时必须自动加方括号：

```text
vless://uuid@[2001:db8::1]:443?...
```

数据库中不得将 IPv6 地址以随意字符串处理后再拼接。

### 5.3 安全字段

TLS 模型必须区分：

```text
未提供
明确为 false
明确为 true
```

不能全部压缩为一个布尔默认值。

```rust
pub struct TlsConfig {
    pub enabled: bool,
    pub server_name: Option<String>,
    pub skip_cert_verify: Option<bool>,
    pub alpn: Vec<String>,
    pub client_fingerprint: Option<String>,
    pub certificate_pins: Vec<CertificatePin>,
    pub reality: Option<RealityConfig>,
}
```

映射：

```text
allowInsecure=0 → Some(false)
allowInsecure=1 → Some(true)
参数不存在       → None
```

### 5.4 节点原始值与人工覆写

远程订阅更新不得覆盖管理员手工修改。

采用：

```text
上游节点原始模型
        +
人工 Override
        =
最终有效节点
```

人工覆写字段包括：

- 名称；
- 地区；
- 标签；
- 启用状态；
- SNI；
- 证书校验；
- 指纹；
- 链式代理；
- 排序；
- 备注。

上游节点被删除时：

- 标记为 `missing_from_source`；
- 已被订阅使用的节点不能立即物理删除；
- 管理员可以恢复、替换或清理；
- 保留来源快照供比较。

---

## 六、协议要求

### 6.1 VLESS Reality

支持：

```text
uuid
server
port
encryption
flow
network
security
sni
fp
pbk
sid
spx
allowInsecure
packetEncoding
udp
xudp
```

约束：

- `short-id` 始终是字符串；
- 不允许 YAML 将纯数字 short ID 转成整数；
- `pbk` 必须进行 Base64URL 字符集校验；
- `security=reality` 才明确表示 Reality；
- `xtls-rprx-vision` 必须原样建模；
- 不支持 Vision 的输出 Profile 必须排除并报告；
- 不能为了兼容而自动设置 `skip-cert-verify=true`。

### 6.2 Hysteria2

支持：

```text
hysteria2://
hy2://
password / auth
sni
alpn
skip-cert-verify
pinSHA256
obfs
obfs-password
up
down
ports
port hopping
hop interval
fast-open
lazy
```

### 6.3 TUIC v5

支持：

```text
uuid
password
token
sni
alpn
skip-cert-verify
congestion-controller
udp-relay-mode
zero-rtt-handshake
heartbeat
disable-sni
```

内部统一以 `Duration` 保存，输出时根据目标格式转换，禁止混淆秒和毫秒。

### 6.4 NaiveProxy

支持：

```text
username
password
server
port
sni
alpn
quic
http2
http3
skip-cert-verify
certificate pin
```

不得将 Naive 转成普通 HTTP 节点。

目标客户端不支持时：

```text
默认排除
生成兼容性报告
允许设置为严格失败
禁止静默损坏
```

### 6.5 节点导出

节点导出支持：

- 当前筛选结果；
- 选中节点；
- 全部节点；
- 按来源；
- 按标签；
- 按协议。

文本格式必须是一行一个标准分享 URI，UTF-8 和 LF 换行。

测试示例必须使用保留测试地址，不得把用户真实节点提交到仓库：

```text
vless://00000000-0000-4000-8000-000000000001@[2001:db8::1]:443?security=reality&type=tcp&allowInsecure=0&sni=example.com&fp=chrome&flow=xtls-rprx-vision&sid=01020304&pbk=TEST_PUBLIC_KEY&encryption=none#IPv6-Test
```

---

## 七、客户端输出 Profile

P0 输出目标：

```text
Mihomo
FlClash
sing-box
Xray
v2rayN
v2rayNG
Shadowrocket
```

FlClash 即使通常消费 Mihomo 配置，也必须是独立 Profile，以便维护版本差异和 UI 特性限制。

每个 Profile 保存：

```text
Profile 名称
目标客户端
最低测试版本
支持的协议
支持的传输
支持的 TLS 字段
支持的代理链
支持的代理组类型
输出格式
不兼容策略
测试 Fixture 版本
```

生成结果返回：

```json
{
  "profile": "flclash",
  "included": 84,
  "excluded": 3,
  "warnings": 2,
  "excluded_nodes": [
    {
      "node_id": "...",
      "name": "naive-test",
      "reason_code": "UNSUPPORTED_PROTOCOL"
    }
  ]
}
```

显式 Profile 优先于 User-Agent 自动识别：

```text
/sub/{token}/mihomo
/sub/{token}/flclash
/sub/{token}/sing-box
/sub/{token}/xray
/sub/{token}/v2rayn
/sub/{token}/v2rayng
/sub/{token}/shadowrocket
```

---

## 八、订阅源管理

字段：

```text
名称
类型
URL
HTTP 方法
请求头
Cookie
User-Agent
抓取超时
最大响应体
自动更新
更新间隔
启用状态
代理抓取设置
包含正则
排除正则
名称前缀
名称后缀
标签
是否继承流量信息
失败时保留旧结果
备注
```

抓取流程：

```text
创建 SourceRefresh Job
→ 校验 URL 和 SSRF
→ 条件请求 ETag / Last-Modified
→ 下载到限制缓冲区
→ 检测格式
→ 解析到 Canonical Model
→ 验证
→ 去重和 Diff
→ 写入新 Snapshot
→ 事务提交
→ 原子切换活动 Snapshot
→ 失效相关订阅缓存
```

要求：

- 更新失败保留上一次成功版本；
- 解析为零节点不能自动覆盖旧版本；
- 支持手动刷新单项和全部；
- 支持暂停自动更新；
- 支持查看新增、删除、变化和重复数量；
- 敏感请求头在 UI 和日志中掩码；
- 订阅源 URL 中的密码不能写入日志；
- 可查看脱敏后的原始响应；
- 同一订阅源不能并发刷新；
- 提供取消和重试；
- 失败使用指数退避；
- 远程响应支持 gzip、br 和 zstd。

---

## 九、节点管理

必须实现：

```text
分页
虚拟列表
关键字搜索
协议筛选
来源筛选
标签筛选
地区筛选
状态筛选
延迟筛选
排序
批量选择
批量启停
批量改名
批量标签
批量分配地区
批量证书校验设置
批量 SNI 设置
批量客户端指纹设置
批量删除
导入
导出
克隆
查看原始值
查看标准化值
查看输出预览
查看解析警告
```

去重策略：

```text
原始 URI
规范化配置哈希
协议 + 地址 + 端口
协议 + 地址 + 端口 + 凭据
名称
```

默认使用"规范化配置哈希"，名称只作为辅助，不得作为唯一默认条件。

### 9.1 地区检测

优先级：

```text
人工指定
→ 节点标签或名称中的 ISO / Emoji
→ 本地 GeoIP 数据库
→ 未知
```

要求：

- 支持 IPv4 和 IPv6；
- 域名解析 A 和 AAAA；
- 双栈地址允许记录多个候选地区；
- 不默认请求第三方 GeoIP API；
- GeoIP 数据库可离线更新；
- 手工地区不会被自动识别覆盖；
- 地区和国旗分离存储；
- 无法确定时显示"未知"，禁止猜测。

### 9.2 节点检测

显示：

```text
TCP Connect RTT
TLS / QUIC Handshake RTT
真实代理 HTTP RTT
UDP / QUIC 可达性
最近检测时间
错误类型
连续失败次数
```

真实代理检测使用可插拔 Runner：

```text
sing-box runner
mihomo runner
```

Runner 可以是受控子进程或单独 Sidecar。

### 9.3 链式代理

统一建模为有向图：

```text
节点 → 节点
节点 → 代理组
代理组 → 节点
代理组 → 代理组
```

要求：

- 保存前检测循环；
- 删除上游前显示依赖；
- 节点失效时显示受影响订阅；
- 输出时由 Profile 判断是否支持；
- 不支持时生成明确错误；
- 测速时可选择测原节点或完整代理链。

---

## 十、V3 模板系统

只实现一套新的 V3 模板，不保留 V1、V2，不实现覆写管理或 JavaScript 覆写脚本。

模板采用版本化、声明式结构：

```yaml
apiVersion: deve-sub.io/v1
kind: SubscriptionTemplate

metadata:
  name: default-mihomo
  description: 默认 Mihomo 模板
  version: 1

spec:
  targetProfiles:
    - mihomo
    - flclash

  variables: {}

  nodeSelector: {}

  proxyGroups: []

  rules: []

  dns: {}

  tun: {}

  output: {}
```

功能：

- 创建；
- 编辑；
- 克隆；
- 删除；
- 导入；
- 导出；
- 版本历史；
- 恢复版本；
- Schema 验证；
- 变量提示；
- 实时预览；
- 输出验证；
- 查看依赖；
- 模板包导入导出。

安全要求：

- 不执行任意 JavaScript、Lua、Shell；
- 不允许模板访问文件系统；
- 不允许模板直接发起网络请求；
- 远程规则抓取由受控 Fetcher 执行；
- 必须限制 YAML Alias、嵌套深度和文件大小。

---

## 十一、订阅生成器

流程：

```text
选择节点来源
→ 选择动态条件或固定快照
→ 筛选节点
→ 配置排序和去重
→ 配置链式代理
→ 编辑代理组
→ 选择 V3 模板
→ 选择输出 Profile
→ 服务端预览
→ 兼容性检查
→ 保存和发布
```

### 11.1 节点选择模式

动态模式：

```text
每次生成时重新应用筛选条件
```

固定快照：

```text
保存具体节点 ID 和版本
```

UI 必须显著显示当前模式。

### 11.2 可视化代理组

支持：

```text
select
url-test
fallback
load-balance
relay
direct
reject
```

实际可选类型由目标 Profile 决定。

交互：

- 创建分组；
- 拖拽排序；
- 拖拽节点；
- 拖拽子组；
- 按地区快速分组；
- 按协议快速分组；
- 按标签快速分组；
- 添加全部剩余节点；
- 搜索节点；
- 隐藏已使用节点；
- 检测循环；
- 检测缺失成员；
- 缺失节点替换；
- 组依赖可视化。

### 11.3 生成缓存

生成缓存键：

```text
订阅版本
+ 节点 Revision
+ 模板版本
+ Profile 版本
+ 规则版本
```

新配置只有在完整生成和验证成功后才能原子发布。失败时继续返回上一个成功版本。

---

## 十二、订阅管理与链接

订阅字段：

```text
名称
Slug
描述
所有者
Profile
模板
节点选择模式
筛选条件
固定节点
代理组
规则
流量限制
到期时间
公开 Token
短码
启用状态
最近生成状态
最近成功版本
创建时间
更新时间
```

链接功能：

- 永久 URL；
- 临时 URL；
- 自定义短码；
- QR Code；
- Token 重置；
- Token 轮换宽限期；
- 请求次数；
- 最近请求时间；
- Profile 链接复制；
- 文件下载；
- 版本回滚；
- 生成差异查看。

随机 Token：

```text
CSPRNG 生成至少 32 字节
Base64URL 无 Padding
数据库只保存 HMAC-SHA256 摘要
日志中脱敏
禁止使用用户 ID、时间戳或 UUID 直接作为 Token
```

密码使用 Argon2id，但高频订阅 Token 不使用高成本密码哈希。

响应：

```text
ETag
Last-Modified
Content-Type
Content-Disposition
Cache-Control: private, no-cache
subscription-userinfo
```

---

## 十三、仪表盘与 UI 设计

原"流量信息"改名为"仪表盘"。

### 13.1 首页结构

参考附图的核心布局：

```text
顶部横向导航
        ↓
四个核心指标卡片
        ↓
30 天流量趋势
        ↓
少量关键异常和任务信息
```

核心卡片：

- 总流量配额；
- 已用流量；
- 剩余流量；
- 使用率。

不在首页堆叠大量系统详情。节点、订阅源和任务统计放在次级区域或展开面板。

### 13.2 内置主题

至少提供：

```text
Minimal Warm
Fantasy Violet
```

Minimal Warm：

- 米白背景；
- 暖橙强调色；
- 细边框；
- 轻微硬阴影；
- 简洁卡片；
- 大面积留白。

Fantasy Violet：

- 淡紫棋盘格；
- 紫色强调；
- 星形装饰；
- 装饰边框；
- 仍然保持内容可读。

两种主题都必须提供暗色版本。

### 13.3 主题引擎

使用设计 Token：

```text
--color-background
--color-surface
--color-text
--color-muted
--color-primary
--color-border
--shadow-card
--radius-card
--nav-height
--content-width
--background-pattern
```

管理员可配置：

- Logo；
- 站点名称；
- 主色；
- 背景色；
- 卡片颜色；
- 边框；
- 圆角；
- 阴影；
- 背景图案；
- 装饰粒子开关；
- 字体栈。

禁止管理员注入任意 JavaScript。自定义 CSS 默认不开放；如后续开放，必须进行严格隔离和 CSP 控制。

支持：

- 中文；
- 英文；
- 跟随浏览器语言；
- 用户级语言设置；
- 日间；
- 夜间；
- 跟随系统；
- `prefers-reduced-motion`；
- 键盘导航；
- 可见焦点；
- 基本 WCAG 对比度。

---

## 十四、用户管理

角色：

```text
admin
user
```

管理员功能：

- 创建用户；
- 禁用用户；
- 删除用户；
- 修改备注；
- 重置密码；
- 设置到期时间；
- 设置流量额度；
- 绑定订阅；
- 解绑订阅；
- 重置订阅 Token；
- 查看会话；
- 强制注销；
- 查看审计日志。

普通用户：

- 查看自己的订阅；
- 复制订阅链接；
- 查看流量；
- 修改密码；
- 设置语言和主题；
- 配置 2FA；
- 管理会话；
- 重置自己的订阅 Token。

认证：

- HttpOnly Cookie；
- Secure；
- SameSite；
- CSRF 防护；
- Argon2id；
- TOTP；
- 恢复码；
- Session 撤销；
- 管理员强制注销。

---

## 十五、探针与流量

Adapter：

```text
Nezha
DStatus
Komari
通用 HTTP Probe
机场 subscription-userinfo
手工数据
```

数据模型必须区分：

```text
探针上传
探针下载
订阅源上传
订阅源下载
手工修正
最终聚合值
```

额度计算必须可追溯，仪表盘能够显示数据来源。

当用户超过额度或到期时，可配置：

```text
仅警告
停止订阅下载
返回空订阅
返回明确错误
```

默认建议停止下载并返回明确的 HTTP 状态和错误，不返回格式正确但内容为空的假配置。

---

## 十六、SQLite 配置

默认：

```text
journal_mode=WAL
foreign_keys=ON
busy_timeout=5000
synchronous=NORMAL
temp_store=MEMORY
```

要求：

- 写事务保持短小；
- 大批量节点导入使用分批事务；
- 配置周期性 WAL Checkpoint；
- 监控 WAL 大小；
- 不把 SQLite 放在 NFS 或网络卷；
- Docker 数据目录必须是本地持久卷；
- 使用在线备份 API 或 `VACUUM INTO`；
- 不允许运行中直接复制数据库主文件作为备份；
- 支持数据库完整性检查；
- 支持 Migration 回滚前备份。

PostgreSQL Adapter 留到后续里程碑，不阻塞 P0。

---

## 十七、Headless 和 CLI

同一代码库提供：

```text
deve-sub serve
deve-sub serve --headless
deve-sub init
deve-sub doctor
deve-sub config validate
deve-sub migrate
deve-sub source list
deve-sub source add
deve-sub source refresh
deve-sub node import
deve-sub node export
deve-sub subscription generate
deve-sub user create
deve-sub backup
deve-sub restore
deve-sub update check
deve-sub update apply
```

CLI 要求：

- 支持 stdin/stdout；
- 支持 `--json`；
- 支持非交互模式；
- 明确退出码；
- 密码支持从文件或 stdin 读取；
- 不强制把密码放在命令参数或环境变量；
- Headless 模式不提供 Web UI，但继续提供 API 和公开订阅；
- 所有 CLI 操作调用 Application Service，不复制业务逻辑；
- 支持 Shell Completion。

---

## 十八、更新机制

### Linux 裸机

支持：

```text
deve-sub update check
deve-sub update apply --version x.y.z
```

要求：

- 获取签名 Release Manifest；
- SHA-256 校验；
- Ed25519 签名验证；
- 下载到临时文件；
- 备份当前二进制；
- 原子替换；
- 运行健康检查；
- 失败自动回滚；
- 记录更新审计日志。

Web 更新页面只能发起同一更新用例，不能自己拼接 Shell 命令。

### Docker

容器内不自动替换自身镜像。

Web 页面只提供：

- 检查新版本；
- 显示 Changelog；
- 显示镜像标签；
- 提供 Compose 更新命令；
- 可选调用受控的外部更新服务，但默认关闭。

---

## 十九、部署

### 19.1 Docker Compose，推荐

提供：

```text
compose.sqlite.yml
compose.postgres.yml
compose.1panel.yml
.env.example
```

SQLite 默认只需要一个应用容器。

可选服务：

```text
probe-runner
postgres
```

### 19.2 Docker

提供多阶段构建：

- Rust Builder；
- Dioxus Web Builder；
- 最小运行镜像；
- 非 root 用户；
- amd64；
- arm64；
- Healthcheck；
- SBOM；
- 镜像签名。

### 19.3 Linux 一键安装

安装脚本必须：

- 检测架构；
- 下载签名二进制；
- 校验；
- 创建专用系统用户；
- 创建 `/etc/deve-sub`；
- 创建 `/var/lib/deve-sub`；
- 创建 systemd Unit；
- 配置日志；
- 支持安装、更新、卸载、保留数据卸载；
- 不让服务以 root 身份运行。

健康检查：

```text
/health/live
/health/ready
```

---

## 二十、安全

必须实现：

- SSRF 防护；
- DNS 重绑定防护；
- 私网地址白名单；
- 重定向逐跳检查；
- 上传大小限制；
- JSON/YAML 深度限制；
- YAML Alias 限制；
- 路径穿越防护；
- SQL 参数化；
- 登录限流；
- 订阅限流；
- 短码探测防护；
- 可信反向代理列表；
- IP Header 仅对可信代理生效；
- CSRF；
- CSP；
- Cookie 安全属性；
- 请求 ID；
- 审计日志；
- 敏感字段加密存储；
- Token 日志脱敏。

订阅 URL、Cookie、自定义 Header 等敏感字段使用 XChaCha20-Poly1305 加密，主密钥由文件或 Secret Mount 提供。

---

# 二十一、自动化验收矩阵

Agent 必须同时维护：

```text
docs/acceptance-matrix.md
tests/acceptance/matrix.yaml
```

YAML 每条包含：

```yaml
id:
title:
priority:
layer:
dimensions:
preconditions:
steps:
assertions:
fixtures:
evidence:
```

测试结果输出：

- JUnit XML；
- Playwright Trace；
- 失败截图；
- 后端日志；
- 生成配置；
- 兼容性报告。

## 21.1 参数化维度

所有适用测试自动组合：

```text
地址：
IPv4
IPv6 literal
仅 A 域名
仅 AAAA 域名
双栈域名

输入：
URI
Base64
Mihomo YAML
sing-box JSON
Xray JSON
V2Ray JSON
Shadowrocket

协议：
VLESS Reality
Hysteria2
TUIC v5
Naive
VMess
Trojan
Shadowsocks

输出：
Mihomo
FlClash
sing-box
Xray
v2rayN
v2rayNG
Shadowrocket
```

## 21.2 最低强制验收项

### 基础与 UI

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| UI-001 | 首次启动 | 进入管理员初始化，不出现默认弱密码 | E2E |
| UI-002 | 中英文切换 | 当前页面和导航即时切换，刷新后保留 | E2E |
| UI-003 | 日间/夜间 | 所有页面无不可读文字 | E2E |
| UI-004 | Minimal Warm | 布局和设计 Token 正确 | Visual |
| UI-005 | Fantasy Violet | 棋盘背景、装饰和卡片仍保持可读 | Visual |
| UI-006 | 自定义主题 | 修改颜色、Logo 后持久化 | E2E |
| UI-007 | 减少动画 | 系统设置后关闭非必要动画 | E2E |
| UI-008 | 10,000 节点 | 滚动、筛选和选择不明显卡顿 | Performance |
| UI-009 | 移动端 | 能查看订阅、刷新源和复制链接 | E2E |
| UI-010 | 键盘导航 | 导航、表格和对话框可使用键盘 | E2E |

### 认证与用户

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| AUTH-001 | 初始化管理员 | 成功创建且只能执行一次 | Integration |
| AUTH-002 | 正确登录 | 创建安全 Session | E2E |
| AUTH-003 | 错误密码 | 不泄露用户名是否存在 | Integration |
| AUTH-004 | 登录限流 | 超阈值后暂时限制 | Integration |
| AUTH-005 | 2FA | 开启、登录、关闭完整通过 | E2E |
| AUTH-006 | 恢复码 | 单次使用后立即失效 | Integration |
| AUTH-007 | 禁用用户 | 现有 Session 被撤销 | Integration |
| AUTH-008 | 普通用户越权 | 无法访问管理员资源 | Security |
| AUTH-009 | Token 重置 | 旧订阅 Token 失效 | Integration |
| AUTH-010 | 强制注销 | 指定会话立即失效 | E2E |

### 订阅源

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| SRC-001 | 新建 HTTP 源 | 保存并脱敏显示凭据 | E2E |
| SRC-002 | 手动刷新 | 创建任务并显示进度 | E2E |
| SRC-003 | 自动刷新 | 按间隔执行且不重复并发 | Integration |
| SRC-004 | ETag | 未变化返回 304 时不重复解析 | Integration |
| SRC-005 | 抓取失败 | 保留上次成功节点 | Integration |
| SRC-006 | 解析为零节点 | 不覆盖旧 Snapshot | Integration |
| SRC-007 | 响应过大 | 中止并记录明确错误 | Security |
| SRC-008 | 请求超时 | 任务超时且可重试 | Integration |
| SRC-009 | 取消刷新 | 取消后不发布半成品 | Integration |
| SRC-010 | 条件过滤 | 包含和排除规则正确 | Unit/E2E |
| SRC-011 | IPv6 URL | 能抓取 IPv6 literal 源 | Integration |
| SRC-012 | 压缩响应 | gzip、br、zstd 正确处理 | Integration |
| SRC-013 | 多个刷新 | 并发受控且互不污染 | Integration |
| SRC-014 | Diff | 新增、删除、修改数量正确 | Integration |

### 解析与导出

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| PARSE-001 | VLESS Reality URI | 所有 Reality 字段无损 | Golden |
| PARSE-002 | HY2 URI | obfs、端口跳跃和证书字段无损 | Golden |
| PARSE-003 | TUIC v5 URI | 心跳和拥塞字段无损 | Golden |
| PARSE-004 | Naive URI | 不降级为 HTTP | Golden |
| PARSE-005 | Mihomo YAML | 可解析支持协议和 IPv6 | Golden |
| PARSE-006 | sing-box JSON | 可解析 Outbound | Golden |
| PARSE-007 | Xray JSON | 可解析 Outbound | Golden |
| PARSE-008 | V2Ray JSON | 可解析 Outbound | Golden |
| PARSE-009 | Shadowrocket | 分享列表正确解析 | Golden |
| PARSE-010 | Base64 Padding | 有无 Padding 都正确 | Unit |
| PARSE-011 | URL 编码 | 中文名称和特殊符号无损 | Unit |
| PARSE-012 | IPv6 URI | 导出地址自动加方括号 | Golden |
| PARSE-013 | short-id 纯数字 | 导出仍是字符串 | Regression |
| PARSE-014 | allowInsecure=0 | 输出证书校验为 false，不反转 | Regression |
| PARSE-015 | 未提供 allowInsecure | 保持 None，不擅自补值 | Regression |
| PARSE-016 | 一行一个 URI | 输出格式和换行正确 | E2E |
| PARSE-017 | Round-trip | 输入→模型→同格式输出语义一致 | Property |
| PARSE-018 | 非法输入 Fuzz | 不崩溃、不无限分配 | Fuzz |

### 节点管理

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| NODE-001 | 粘贴批量节点 | 成功、失败和重复分别统计 | E2E |
| NODE-002 | 文件导入 | 大文件流式处理 | Integration |
| NODE-003 | 自动去重 | 不误删不同凭据节点 | Regression |
| NODE-004 | 批量启停 | 状态全部正确 | E2E |
| NODE-005 | 批量标签 | 标签查询即时更新 | E2E |
| NODE-006 | 人工地区 | 不被后续自动识别覆盖 | Integration |
| NODE-007 | 自动地区 IPv4 | 本地 GeoIP 正确 | Integration |
| NODE-008 | 自动地区 IPv6 | IPv6 GeoIP 正确 | Integration |
| NODE-009 | 双栈域名 | 候选 IP 均被记录 | Integration |
| NODE-010 | 人工覆写 | 上游刷新后仍生效 | Regression |
| NODE-011 | 上游删除 | 节点标记缺失，不直接消失 | Integration |
| NODE-012 | TCP 延迟 | 记录连接 RTT 和错误分类 | Integration |
| NODE-013 | QUIC 延迟 | HY2/TUIC 记录握手 RTT | Integration |
| NODE-014 | UDP 无响应 | 不伪造延迟或直接判死 | Regression |
| NODE-015 | 真实代理测速 | Runner 结果正确回传 | Integration |
| NODE-016 | 批量取消测速 | 未完成任务被取消 | E2E |
| NODE-017 | 链式代理 | 完整链可保存和测试 | Integration |
| NODE-018 | 代理环 | 保存前拒绝并展示路径 | Regression |

### V3 模板与生成器

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| GEN-001 | 新建 V3 模板 | Schema 验证通过 | E2E |
| GEN-002 | 非法模板 | 返回字段级错误 | Integration |
| GEN-003 | 模板版本 | 编辑创建新版本 | Integration |
| GEN-004 | 模板回滚 | 恢复后生成内容一致 | E2E |
| GEN-005 | 动态选择 | 新节点自动进入结果 | Integration |
| GEN-006 | 固定快照 | 新节点不会自动加入 | Integration |
| GEN-007 | 按地区分组 | 分组成员正确 | E2E |
| GEN-008 | 按协议分组 | 分组成员正确 | E2E |
| GEN-009 | 拖拽排序 | 保存并重新打开顺序一致 | E2E |
| GEN-010 | 多中转组 | UI 不溢出，生成正确 | Regression |
| GEN-011 | 删除节点 | 相关组引用被提示处理 | Regression |
| GEN-012 | 循环组依赖 | 服务端拒绝 | Integration |
| GEN-013 | 目标不兼容 | 节点被排除并生成报告 | Integration |
| GEN-014 | 严格模式 | 存在不兼容节点时生成失败 | Integration |
| GEN-015 | 原子发布 | 生成失败继续返回旧版本 | Regression |
| GEN-016 | 预览一致性 | 预览内容等于发布版本 | E2E |

### 订阅输出

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| OUT-001 | Mihomo | 官方 Core 可加载 | Compatibility |
| OUT-002 | FlClash | 指定测试版本可导入 | Compatibility |
| OUT-003 | sing-box | `check` 验证通过 | Compatibility |
| OUT-004 | Xray | 配置验证通过 | Compatibility |
| OUT-005 | v2rayN | URI 列表可导入 | Compatibility |
| OUT-006 | v2rayNG | URI 列表可导入 | Compatibility |
| OUT-007 | Shadowrocket | 测试设备或格式 Fixture 通过 | Compatibility |
| OUT-008 | ETag | 未变化请求返回 304 | Integration |
| OUT-009 | Token 错误 | 返回 404 或受控错误，不泄露存在性 | Security |
| OUT-010 | 用户过期 | 按策略拒绝下载 | Integration |
| OUT-011 | 流量超额 | 按策略拒绝下载 | Integration |
| OUT-012 | Token 轮换 | 宽限期和失效时间正确 | Integration |
| OUT-013 | 短码冲突 | 原子拒绝重复值 | Integration |
| OUT-014 | 并发生成 | 所有客户端只看到完整版本 | Concurrency |

### 探针、系统和部署

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| PROBE-001 | Nezha | 流量同步正确 | Contract |
| PROBE-002 | DStatus | 流量同步正确 | Contract |
| PROBE-003 | Komari | 流量同步正确 | Contract |
| PROBE-004 | 数据源失败 | 保留旧统计并标记过期 | Integration |
| PROBE-005 | 多源聚合 | 计算过程可追溯 | Integration |
| CLI-001 | Headless 启动 | 无 Web UI，API 与订阅正常 | Integration |
| CLI-002 | stdin 导入 | 节点正确导入 | CLI |
| CLI-003 | stdout 导出 | 一行一个 URI | CLI |
| CLI-004 | JSON 输出 | 可用于自动化脚本 | CLI |
| CLI-005 | doctor | 检测数据库、目录、网络和版本 | CLI |
| DEPLOY-001 | SQLite Compose | 一条命令启动并健康 | Smoke |
| DEPLOY-002 | Linux 安装 | systemd 正常运行 | VM Test |
| DEPLOY-003 | amd64 镜像 | 启动通过 | CI |
| DEPLOY-004 | arm64 镜像 | 启动通过 | CI |
| UPDATE-001 | 签名更新 | 正常更新并健康 | VM Test |
| UPDATE-002 | 更新失败 | 自动回滚 | VM Test |

### 安全与性能

| ID | 场景 | 预期 | 层级 |
|---|---|---|---|
| SEC-001 | localhost SSRF | 默认阻止 | Security |
| SEC-002 | 私网 SSRF | 无白名单时阻止 | Security |
| SEC-003 | DNS 重绑定 | 解析与连接目标均校验 | Security |
| SEC-004 | 重定向到内网 | 每一跳重新阻止 | Security |
| SEC-005 | YAML Bomb | 限制资源并拒绝 | Security |
| SEC-006 | 路径穿越 | 上传名称无法越界 | Security |
| SEC-007 | 真实 IP 伪造 | 非可信代理 Header 被忽略 | Security |
| SEC-008 | SPA 路由 | `/nodes` 等不计入短码爆破 | Regression |
| SEC-009 | Token 日志 | 日志中不可见完整 Token | Security |
| SEC-010 | CSRF | 跨站写请求失败 | Security |
| PERF-001 | 10k 节点解析 | 满足内存和耗时基线 | Benchmark |
| PERF-002 | 10k 节点列表 | P95 交互满足预算 | Browser Perf |
| PERF-003 | 缓存订阅 | P95 小于 500 ms | Load |
| PERF-004 | 无缓存生成 | P95 小于 3 秒，不含抓取 | Load |
| PERF-005 | 并发订阅下载 | 不返回部分内容 | Load |
| PERF-006 | 长时间运行 | WAL、内存和任务数不持续增长 | Soak |

---

## 二十二、开发里程碑

### Milestone 0：技术验证

- Dioxus UI Spike；
- SQLite 并发 Spike；
- 10k 节点虚拟列表；
- 拖拽；
- SSE；
- 中英文；
- 两套主题；
- Docker 构建。

未通过 UI 验证时，只替换 UI Adapter。

### Milestone 1：基础设施

- Workspace；
- Domain 边界；
- Axum；
- Dioxus；
- SQLx；
- Migration；
- 配置；
- 日志；
- OpenAPI；
- CLI；
- Docker；
- Healthcheck。

### Milestone 2：认证和用户

- 初始化；
- 登录；
- RBAC；
- Session；
- 2FA；
- 用户管理；
- Token；
- 审计。

### Milestone 3：协议引擎

- Canonical Model；
- VLESS Reality；
- HY2；
- TUIC；
- Naive；
- SS、VMess、Trojan；
- 所有输入格式；
- 所有目标 Emitter；
- Golden 和 Fuzz。

### Milestone 4：订阅源和节点库

- Source CRUD；
- Snapshot；
- 自动刷新；
- Diff；
- 节点库；
- Override；
- 去重；
- 地区；
- 导入导出。

### Milestone 5：生成器和 V3 模板

- 节点选择；
- 代理组；
- 链式代理；
- V3 模板；
- 兼容性报告；
- 原子生成；
- 缓存。

### Milestone 6：订阅分发

- Profile URL；
- 短码；
- 临时链接；
- QR；
- ETag；
- 用户授权；
- 流量和到期策略。

### Milestone 7：探针和检测

- Nezha；
- DStatus；
- Komari；
- TCP；
- QUIC；
- Runner；
- 仪表盘。

### Milestone 8：部署和加固

- 一键安装；
- 自更新；
- 备份恢复；
- SSRF；
- 限流；
- 性能；
- 多架构；
- 完整验收矩阵。

---

## 二十三、Agent 执行约束

1. 第一阶段只提交架构决策记录、ER 图、统一节点模型和技术 Spike。
2. 未完成 Canonical Node Model 前不得开始写多个输出转换器。
3. 未通过 Round-trip 测试不得宣称支持协议。
4. 不允许把核心生成逻辑放入 Dioxus 组件。
5. 不允许 UI 直接访问 SQLx。
6. 不允许 API Handler 跨多个 Repository 手工拼业务事务。
7. 不允许静默丢弃不兼容节点。
8. 不允许自动改变证书校验安全语义。
9. 不允许真实节点凭据进入仓库 Fixture。
10. 不允许模板执行任意脚本。
11. 不允许使用 `latest` 镜像作为正式发布依赖。
12. 每个 Milestone 必须提供可运行的垂直切片。
13. 每次数据库修改必须附 Migration 和恢复测试。
14. 每项 P0 功能必须对应自动化验收编号。
15. 每次 Release 必须生成 SBOM、校验和和签名。
16. 不允许通过增加微服务解决普通模块边界问题。
17. 优先构建模块化单体和清晰 Port，而不是抽象出无业务价值的框架。
18. 兼容性结论必须以客户端验证或官方格式为依据。
19. 失败时必须保留上一个成功订阅版本。
20. 所有异步后台任务必须可以观察、取消和安全关闭。
