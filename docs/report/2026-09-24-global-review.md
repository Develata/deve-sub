# 2026-09-24 全局审查与修复

本轮基于 main 的 `aea4dcb`（工作区版本 v0.1.2）检查后端数据一致性、备份、
Web 操作、安装和发布验收。主代理负责综合审查与落地；data、UI、CI 三条
review lane 独立检查并交叉复核，主代理确认复现、契约和最终修复。
改动绑定 M4/M6/M11 与现有 SRC、OUT-016、BACKUP-001、DEPLOY-002 验收。

以下是已复现的问题；没有以全局审查代替完整形式证明，也没有重写协议转换、
引入新服务或增加数据库迁移。当前工作仅在本地分支，不代表远端 CI 或发布生效。

## 发现与修复

| 严重度 | 问题与复现 | 最终行为与回归 |
|---|---|---|
| P1 | 额度 `9223372036854775808` 创建返回 201，写入 SQLite 后变为负数，列表 GET 返回 500 | 应用层返回 400；仓储三个写入口 checked conversion；最大合法值精确存取，拒绝无部分写入。模板版本 pin 同步约束 |
| P1 | 显式传入不存在的备份密钥仍成功，归档缺少要求的 fingerprint | CLI 和环境变量显式密钥先验证；缺失、畸形或目录均失败，已有归档保持原样 |
| P1 | 订阅非法额度输入被 `.ok()` 转为空值，可能意外取消限制 | 保留原始文本，提交时校验正整数和 i64 上限；只有空白取消限制，失败保留草稿 |
| P2 | 源过滤等配置改变后保留旧 ETag，下次 304 跳过重新解析；旧租约任务可能晚到发布或禁用新配置 | 编辑与 Running 租约互斥，冲突返回 409；成功编辑原子清除 ETag；旧任务无权发布、触碰快照或禁用源 |
| P2 | 快照提交后 Publishing 阶段写失败，使已发布任务显示 Failed | 快照/节点/Completed 同事务；304 touch 同样绑定当前租约并原子完成；故障注入证明全部回滚 |
| P2 | 用户和订阅表单保存中可取消并打开新草稿，旧响应关闭新表单 | 保存期间锁定本表单和切换入口；失败保留输入，解除等待后可真实重试 |
| P2 | 安装健康探测固定 IPv4 loopback，非默认监听失败；回滚探测错误使用新端口 | wildcard 映射本地地址，显式 IPv4/IPv6 保留，绕过代理；回滚从旧 unit 获取原地址；无法识别的活动 unit 在停止前拒绝 |
| P2 | 安装 fixture 固定旧版本且 CI 未执行；预发布未显式分类，带 `+metadata` 版本会在 OCI 阶段失败 | fixture 使用实际 binary 版本；legacy lane 必跑安装回归；预发布标记显式输出，metadata 发布前拒绝，发布各阶段设有限超时 |

源更新继续保留 last-good 内容；配置写入和刷新共享 SQLite 事务边界，
没有把业务判断塞入 HTTP handler。订阅输入校验拆入私有 validation 模块，
源配置写入集中在 config_update；文件熔断规则通过，没有提高既有豁免上限。

## 验证证据

- 全 workspace `cargo test --locked --workspace --all-targets --all-features`：
  1,113 passed、0 failed、8 ignored，另 21 个 Criterion test-mode 场景成功；
  这些场景只证明可执行，不是性能提升测量。
- `cargo fmt --all --check`、全 target/feature check、严格 Clippy、doc tests、
  架构检查与 `cargo deny --locked check` 通过。架构检查首次发现两个文件超出
  上限，拆分后通过；模块拆分移除了一个仅供旧 rustdoc 使用的 import，最终
  check/Clippy、doc tests、release 构建和受影响的 50 项集成测试重新通过。
- 原始 Chromium 配置、最终 release 后端和生产 WASM：functional 83/83
  （80.785 秒，2 workers）、legacy 16/16（30.1 秒）、lifecycle 3/3
  （0.585 秒）；0 retries、0 flaky、0 skipped，没有放宽超时。
  桌面和移动端额度错误/失败草稿截图已目检。
- 外部验证器 7/7：Mihomo 1.19.0、sing-box 1.13.14、Xray 26.3.27；
  以原三个 emitter 集成文件显式 `--ignored --test-threads=1` 运行。
  这是 fixture 配置接受/拒绝验证，不等于真实代理网络连通性。
- Rust CI 工具 20 项、Python CI 策略 14 项、架构/超时/soak harness
  12 项通过；静态 inventory 仍是 9 Rust shards、16 packages、4 browser lanes。
- docs gate 通过：157 cases、508 个引用有效；生成的 OpenAPI 与当前代码一致。
- 安装回归 13/13（79.446 秒），覆盖 IPv4/IPv6 显式监听及 wildcard、
  旧端口回滚、损坏或缺失 Web 产物和恢复失败保留备份。
- 最终 release 进程 soak 90.027 秒：2,698 循环、12,691 请求，
  0 非预期失败、0 ERROR、0 task panic；退出后 tracked jobs 为 0。
  RSS 从 40.18 MiB 到 46.86 MiB，FD 从 16 到 24；末段 FD 保持 24，
  RSS 仍有小幅增长，不能据此声称长期无泄漏。SQLite 历史记录随操作增加，
  WAL 峰值约 4.07 MiB。包含预期的 401/429，源刷新覆盖真实 SSRF 拒绝路径；
  成功发布与租约竞争由前述集成测试证明。

可复跑命令和验收限制同步在 [验收门禁](../acceptance/gates.md) 记录。
原始本地日志统一使用 `/tmp/deve-sub-review-*`，临时日志不作为长期归档。
测试绑定在 `tests/acceptance/matrix.yaml`，OpenAPI 由 CLI 生成。

## 验证边界

- 安装回归使用 bubblewrap 隔离文件系统和进程；systemd/账户操作模拟，
  新服务使用真实 CLI、Web 与 HTTP。不能当作本轮真实 systemd VM 验收。
- 本机 actionlint 对已有 `concurrency.queue: max` 报 schema 错误；
  精确排除这一既有字段错误后，其余工作流静态检查通过。未扩大排除范围。
- 本轮没有运行远端 Actions、重新构建 Docker 双架构、原生 ARM 或 Windows。
  既有部署证据保留历史日期，不重标为本轮成功。
- 验收矩阵仍为 157 项：153 项有既有通过证据、4 项 not-run；
  本轮新增回归不是重新执行整个长期验收目录的声明。
- 整数防线阻止新的非法写入，不自动修复可能由旧版本写入的负数记录；
  本轮没有读取或修改用户实际业务数据。
