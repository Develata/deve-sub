# 全局质量与性能检查 — 2026-09-22

基线：`23cf9ee`，工作分支 `fix/global-quality-performance`。覆盖 M4/M5/M7/M8
的源管理、节点选择与链图、分层守卫、CI 编排和真实浏览器旅程。
这是有边界的工程检查，不是整个产品不存在缺陷或全部部署环境通过的证明。

主 agent 负责整合、独立复核和最终执行；backend_perf、ci_matrix、ui_experience
分别负责后端、CI 和 UI。ci_matrix 对其余切片进行独立只读交叉审查，主 agent
审查 CI 实现。所有接受的阻断发现均在提交前闭合。

## 发现与修复

| 程度 | 可复现问题 | 修复与证明 |
|---|---|---|
| P1 | 源编辑把 GET 的脱敏 URL 重新 PUT，单纯改名也会覆盖真实订阅地址。 | 编辑时地址留空；省略/null 保留当前密文，非空字符串明确替换，空字符串拒绝。应用分发一个 `SourceConfigUpdate`，SQLite 原子更新并失效缓存；不先读旧记录再整体回写。HTTP、真实加密存储、12 轮并发与故障回滚回归覆盖。 |
| P1 | 节点链递归 DFS 假设节点池至多 500 个，但没有该上限；20,000 节点在 256 KiB 栈线程上真实 SIGABRT。模板组存在同根递归问题。 | 共用私有显式 DFS 栈，保留稳定根/邻居顺序及完整环路径。20,000 节点、4,000 组的小栈测试通过；512 种三顶点图逐一与旧算法结果对照。 |
| P1（配置相关） | HTTP 客户端自动继承系统代理，代理可能另行解析主机，破坏已校验地址与实际连接的一致性。当前代理环境中 IPv6 fixture 超时，去代理则通过。 | 订阅抓取与面板公共客户端禁用隐式代理。隔离子进程设置不可达代理，验证 `.invalid` 主机仍只经测试 checker 固定地址访问本地 fixture；先分别复现抓取、面板失败，再验证修复。未对生产网络进行攻击测试。 |
| P2 | 固定节点模板仍读取、解密全池，quick group 再扫描全池后求交。 | Fixed 只批量取引用节点；组仅扫描选中候选集，借用条目而不复制凭据；Dynamic 不读取无效 pinned ID。回归保留顺序、分页、重复选择和缺失诊断。 |
| P2 | 浏览器 lifecycle、legacy、functional 在同一 job 串行执行；CI 没有工作流替换策略及明确 job 超时。 | 保留 9 个 Rust 分片，拆成 4 个浏览器 lane；所有必需结果汇总、失败可见、有限超时；仅同一 PR 的旧运行被取消。main/发布运行使用独立并发组。 |
| P2 | 架构守卫只实质约束少数 crate；可遗漏 Application/Web 越层依赖及同名 target-specific 非可选 OpenAPI 依赖。 | 为全部 16 个包分类，覆盖 normal/build/target 依赖、库错误类型、源码硬阈值及新文件；9 个对抗回归。target-specific 漏检由独立 reviewer 发现并闭合。 |
| P2 | 源弹窗缺少模态/标签/焦点语义，矮屏操作不便；手机列表操作列被长 URL 挤出视口。 | 原生 dialog、首尾 Tab、Escape、默认取消、提交期间防重复操作、失败保留草稿；小屏源列表按行堆叠，操作直接可见，桌面保留表格。 |

不增加数据库迁移、服务进程或通用调度框架。公开 REST 更新请求保持已有字符串
URL 的兼容性，新增省略/null 语义；OpenAPI 由真实 CLI 重新生成。

## 算法与测量

固定选择包含 selector 与所有显式组引用，不能仅按有效选中节点数计费。设
R 为这些引用的出现总数、U 为去重后数量、K 为有效选中节点数、G 为组数：
全池 N 条记录的读取/解密变为最多 U 条，引用归一化为 O(R log R)，候选排序
为 O(K log K)；固定每条匹配成本时，quick filter 扫描由 O(GN) 降为 O(GK)。
组自身的成员排序、诊断与输出成本保留，数据库索引查找成本也未消失。
动态选择仍需检查全池，未宣称其复杂度降低。链图遍历仍为 O(V+E)，另加既有
确定性排序开销；辅助堆空间 O(V+E)，调用栈深度不再随图深度增长。

Criterion 固定 16 节点、无缓存生成，同机 dev profile：

| 全池节点数 | 修改前均值 | 修改后均值 |
|---:|---:|---:|
| 100 | 49.54 ms | 26.83 ms |
| 1,000 | 467.74 ms | 17.52 ms |
| 10,000 | 5,277.68 ms | 23.58 ms |

原始样本：`target/criterion/generate_{100,1000,10000}/generate/fixed-16-uncached/`
中的 `scope-before` 与 `new`。共享构建环境存在其他负载；该表证明这一场景的
规模效应与读取范围收益，不是生产 P95、吞吐承诺或全局加速倍率。

## CI 与边界

`tools/ci` 是不依赖业务 crate 的仓库工具，替换 Python inventory/gate。
GitHub Actions 仍负责执行；保留 Python 的系统/产物工具和 TypeScript 的真实
浏览器驱动。9 个 Rust 分片完整覆盖 16 包；浏览器分为 legacy、API、desktop、
mobile，后者每个 runner 2 workers，矩阵最多 4 runners、`fail-fast: false`。
收集验证证明 15 API + 31 desktop + 31 mobile 三组无交集，合起来正好是完整
77 项 functional 测试。没有改为按改动选择性跳过测试。

Rust 工具的 17 个测试覆盖删包、重复分片、命令被过滤/吞错、遗漏 job、取消、
job 超时配置、PR multiarch 特许跳过及真实 CLI 报告写入；Python 11 个产物/发布策略
测试保留。产物身份与内容校验仍在消费前执行，诊断产物按 lane 分开。

远端 Actions、实际取消操作和远端耗时尚未执行。未 push、发布、部署或调整
仓库设置。不能把本地配置变化报告为已生效的远端提速。

## 运行验证记录

最终全量 Rust 测试明确退出 0（1,100 通过、8 个忽略：7 个需外部验证器，
另 1 个是已单独执行的真实进程 soak），
fmt/check/严格 Clippy、doc tests、依赖审计与文档门禁均通过。详见
[验收门禁](../acceptance/gates.md)。原始本地日志使用
`/tmp/deve-sub-quality-*`，定向 UI 证据在 `/tmp/deve-sub-ui-review-results`。
这些临时路径不是长期归档；此文保留结论、条件与可复跑入口。

首轮同时运行调试后端、Rust 构建/基准、多个浏览器和 soak：functional 76/77，
legacy 15/16。失败分别为模板导航 click 10 秒超时、10k 列表等待 15 秒超时；
后者服务日志显示请求实际 18.84 秒后返回 200。最终改用 CI 同类型 release
后端分组复核，保持原超时和零自动重试：functional 77/77（81.81 秒）、
legacy 16/16（31.17 秒）通过，0 flaky、0 skipped。浏览器使用已安装的
Chromium build 1234（151.0.7922.34），临时配置仅切换到完整 Chromium 通道
并分开输出目录；CI 的浏览器选择未改动。不隐藏首轮失败，也不据此宣称
所有机器上均能达到相同耗时。

90 秒真实 debug 进程 soak 已通过：502 个循环、2,450 个请求、0 非预期失败、
0 ERROR、0 task panic，退出后 tracked jobs 为 0。FD 尾段稳定在 24，RSS
尾段约 26–32 MiB。覆盖导入、生成/下载、SSRF 刷新失败、探测与退出；
不代替长期泄漏证明或真实外部源成功刷新。

## 安装环境核对补充

本轮新增 `tools/ci` 后遗漏了 Docker source stage 的目录复制，工具清单复核时
发现并补上 `COPY tools/ tools/`。离线临时目录按真实 COPY 输入重现：旧版
`cargo metadata --locked --no-deps` 退出 101，缺少 `tools/ci/Cargo.toml`；
补齐后退出 0，识别全部 16 个 workspace 包。两次检查均有 60 秒超时。
这先证明了源构建输入回归；完整源码镜像的后续结果见下节。

## 工具补齐后的继续验收

用户安装项目固定版本验证器与默认 Headless Shell 后，重新执行实际客户端和
原始浏览器配置：

- 在 `/home/deve/.local/share/deve-sub/validators` 使用 Mihomo 1.19.0、
  sing-box 1.13.14、Xray 26.3.27。显式运行三个 emitter 集成测试文件的
  `--ignored --test-threads=1`，3 + 2 + 2 共 7 项通过，无跳过。这里证明的是
  受 profile 兼容性过滤的 fixture 配置加载、坏配置拒绝及 Mihomo 原生路由
  模板检查，不是所有协议/字段或真实代理网络连通性。
- 原 `functional.config.ts` 使用 2 workers，77/77 通过（94.253 秒）；
  原 `playwright.config.ts` 16/16 通过（38.8 秒），`lifecycle.config.ts`
  3/3 通过（0.774 秒）。无 channel 替换、无超时放宽、无自动重试。
  实际启动的是 Headless Shell build 1234；8 个浏览器进程均正常退出，
  本轮测试服务与临时 E2E 目录无残留。390px 源列表截图再次目检通过。
- 复核发现 multiarch CI 只启动 amd64，ARM64 只有构建。这不满足 M8 要求。
  CI 配置现要求加载并运行两种架构，共用 `scripts/tests/test_docker_health.py`；
  校验真实镜像架构、原生内置 healthcheck、60 秒内 healthy、live/ready/Web
  HTTP 和 UID 1000。临时数据使用 tmpfs，失败和正常中断都清理本次容器；
  两架构均尝试，任何一个失败即阻断。Rust 清单守卫防止删除加载步骤、跳过
  检查或吞错；工具测试从 17 项增至 19 项。
- 独立 review 复现 socket 空闲超时不能限制持续慢传：旧探测 7.007 秒仍成功；
  整次请求增加 POSIX 总期限后，同输入 5.003 秒受控失败，正常响应 0.011 秒
  通过，计时器与原信号处理器恢复。旧镜像仅用于验证新工具自身，不能提升
  当前源码的部署验收状态。
- 当前生产源码与 `e33114e` 一致的 amd64 镜像完整构建成功；构建包含
  Dioxus CLI、后端及实际 WASM 前端，未使用旧应用镜像代替。首次构建因
  Debian 软件源 HTTP 502 在 253.660 秒失败；原命令重试在 808.250 秒成功。
  这是本机冷构建记录，不是 GitHub Actions 耗时；未关闭 TLS 校验或更换源。
  镜像 ID 为
  `sha256:c27921b3f46e4bb71b5b9191f824b4e6d2e9069429720ef3c93b912f8afd3c8e`，
  本地标签为 `deve-sub:followup-e33114e-amd64-b4f420a032`。
- 新镜像默认入口在 5.751 秒变为 Docker healthy；live/ready/Web 均为 200，
  UID 1000，没有匿名卷。环境变量/Web 初始化与登录、重建保留原管理员、
  缺失/过短初始凭据在 HTTP 启动前拒绝均通过。日志写入 45,000 条后保留
  26,567,322 字节，起始记录为 19,030、最新结尾保留。各脚本清理自身资源。
- 真实浏览器访问新镜像提供的 Web：初始化、登录、源创建、地址留空改名、
  刷新确认及删除通过；观察到改名 PUT 省略 URL，WASM 200 且类型正确，
  JavaScript 异常为 0。1280×800 与 390×700 截图目检通过；本次容器、
  浏览器进程和临时 profile 均已清理。这补充验证了 Docker 内实际产物，
  不仅是宿主机 release CLI 与本地 dist 的组合。
- ARM64 完整构建在 21.625 秒失败：runtime stage 的 ARM64 `/bin/sh`
  执行 `apt-get` 前即 `exec format error`，尚未产生当前 ARM64 镜像。
  本机 Docker Desktop builder 宣告 `linux/arm64`，但实际模拟执行不可用；
  未注册特权 binfmt、修改 daemon 或把构建失败改成跳过通过。CI 已配置
  setup-qemu；其远端实际结果仍未运行。

因此 DEPLOY-003 更新为 pass，DEPLOY-004 为 blocked。M8 要求两种架构
healthcheck 均通过，故 DEPLOY-005 整体仍为 blocked，只有 amd64 子维度通过。
当前矩阵为 157 项：151 pass、2 blocked、4 not-run，497 个通过项证明引用。
Rust CI 工具 19 项、慢传回归 2 项、架构守卫 9 项、Python CI 工具 11 项
与 soak harness 回归 1 项均通过；fmt、全 workspace 严格 Clippy、doc tests、
actionlint、静态 CI 清单与文档门禁通过。本节没有修改产品 Rust 源码；前轮
1,100 项全量测试仍对应同一生产实现，本轮重新验证发生修改的 CI 工具。

补充日志使用 `/tmp/deve-sub-followup-*` 和
`/tmp/deve-sub-docker-web-smoke-20260922-*`。前轮 8 个 ignored 仍是普通 Cargo
命令的显式跳过，其中 7 个外部验证器测试本轮单独执行通过，soak 前轮已单独
执行；不修改 `#[ignore]` 来制造无外部工具时的通过结果。

## 保留的验证与设计缺口

- 仍未运行两个签名更新项和两个 10k 节点性能预算项；ARM64 运行被本地模拟
  环境阻塞。Rust/浏览器通过不会自动提升这些验收状态。
- 本轮不证明 ARM64 运行、所有外部客户端、断电恢复或长期资源上界。
- WASM 编译仍有既有前端 warning；native 严格 Clippy 不等于 WASM 零警告。
- Web 的本地产品名称偏好和服务端集中品牌配置仍有设计漂移：设置页保存到
  localStorage，侧栏仍含固定名称（`apps/web/src/main_wasm.rs`）；现有 UI-006
  验证持久化而未证明全站品牌一致。该问题作为非阻断后续项记录，本轮不通过
  弱化集中配置要求把它标为已完成。
- URL 保留由浏览器请求语义、真实 HTTP 与加密存储分层证明；没有放宽 SSRF
  来让生产服务访问测试回环源，也未使用真实订阅凭据。

参考依据：[Reqwest 0.12.28 no_proxy](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html#method.no_proxy)、
[GitHub workflow concurrency](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#concurrency)。
