# 并发功能验收矩阵

机器证据绑定在 `tests/acceptance/matrix.yaml`；此处解释测试维度及其限度。
本轮主代理负责实现和真实执行，后端一致性与 UI/验收两条只读审查负责复核。
范围是 M4/M5/M6/M7 中已复现的管理操作，以及 M10 的日志查询、清理与回收。日志索引迁移和 Compose 轮转各有单独验收；本矩阵不执行远端发布。

## 执行与隔离

```sh
cargo build --locked -p deve-sub-cli --all-features --bin deve-sub
npm ci --prefix apps/web
npm run --prefix apps/web build:css
bash scripts/build-web-release.sh
cd tests/e2e
npm ci
DEVE_SUB_BINARY=../../target/debug/deve-sub npm run test:functional
```

配置 `functional.config.ts` 使用 4 workers、0 retries。每个测试独占服务进程、
临时数据库、密钥、管理员会话及回环端口；只有同一竞争用例内的并发请求共享
目标。借用已有 server-lifecycle 的启动检查、有限退出和 finally 清理。结果按
run UUID 和 test/project 分目录保留 JSON、失败 trace、截图及服务日志；fixture
凭据只用于隔离测试，不能用于真实部署或发布截图。

单请求 10 秒，浏览器动作 10 秒、导航 15 秒，单例 60 秒、整组 600 秒。
实际 Web Fetch 使用 30 秒 AbortSignal。并行运行不等于证明所有线程交错：
API 竞争用真实 Promise.all，链路用例重复 8 轮；源配置交错使用 Notify 屏障，
UI 乱序用受控响应释放顺序，不依赖固定 sleep。

## 场景

API 行各运行一次；浏览器行在桌面 Chromium 与 Pixel 5 视口各运行一次，
共 36 项（8 API + 14 × 2 浏览器）。每项检查最终状态或用户实际可执行的操作，
不能只把 HTTP 请求发出当成通过。

| 场景标识 | 绑定 | 维度/操作 | 必须成立的结果 |
|---|---|---|---|
| FUNC-TAG-INPUT | NODE-005 | 空白、Unicode、重复成员、缺失引用 | 规范化正确，错误为 400/404，失败无部分写入 |
| FUNC-TAG-RACE | NODE-005 | 同节点并发添加，替换失败，移除，重复目标 | 保留并集；替换整批回滚；移除不动其他标签 |
| FUNC-TAG-IDENTITY | NODE-005 | 同名并发创建、重命名、删除与赋值竞争 | 同名仅一个成功；ID/成员保留；删除后无悬挂关系 |
| FUNC-CHAIN-RACE | NODE-017/018 | A→B 与 B→A 并发写入 | 一个成功、一个冲突，最终图无环 |
| FUNC-NODE-FILTER | NODE-004/006 | 并发启停与地区修改、筛选 | 两字段均保留；计数按唯一节点；筛选与有效值一致 |
| FUNC-IMPORT-RACE | NODE-001/003 | 四次同时导入同样节点 | 相同身份，池内数量不增殖，无 SQLite 517 错误 |
| FUNC-TAG-MODES | NODE-005 | Web 批量移除与空替换 | 移除保留其余标签，空替换清空 |
| FUNC-TAG-EDIT | NODE-005 | 打开已有成员并清空 | 预选与存储一致，清空真正持久化 |
| FUNC-TAG-MANAGE | NODE-005/UI-009 | 创建、改名/色标、批量添加、筛选、删除 | 列表与持久成员一致，色标可见，页面不横向溢出 |
| FUNC-OVERRIDE | NODE-010 | 重开覆盖编辑器，只改名称 | 其他覆盖字段完整保留 |
| FUNC-TEMPLATE-ROLLBACK | GEN-004 | v2 回滚确认到 v1 | 实际 POST 成功，活动版本及内容恢复 |
| FUNC-SUB-CREATE | OUT-016 | Web 默认表单创建订阅 | 201，返回链接可获取内容 |
| FUNC-PAGINATION | SRC-001/GEN-001/OUT-016 | 21 源、51 模板和订阅 | 后续记录可操作，模板选项完整 |
| FUNC-SOURCE-JOBS | SRC-002/013 | A/B 同时刷新，A先成功、B后失败 | 忙碌状态独立，失败后列表和重试入口保留 |
| FUNC-REQUEST-TIMEOUT | UI-009 | 节点请求不返回 | 实际 AbortSignal 到期后显示错误，结束等待 |
| FUNC-LIST-ORDER | SRC-013 | 新列表成功后旧列表失败 | 旧错误不能隐藏当前数据 |
| FUNC-TEMPLATE-ORDER | GEN-003 | A加载挂起、关闭、B加载完成、再释放A | B编辑器及实际保存均保持B内容 |

`refresh_source/hardening.rs::refresh_failure_preserves_concurrent_source_edits_and_current_policy`
另以可控屏障覆盖 SRC-005：抓取失败前更新源名称/URL/周期和两种 keep_on_fail
策略，失败后新字段保留，enabled 按当前策略处理。

## 证据边界

标签、覆盖、分页、模板和订阅使用真实 CLI、REST、SQLite 与生产 WASM。
仅 FUNC-SOURCE-JOBS 的刷新/任务传输被控制，用于 UI 状态验证；真实刷新失败和
并发配置保护由 Rust 集成测试补充。超时和乱序测试同样只控制传输，使用真实
页面代码。订阅获取成功不等于已导入真实代理客户端。

既有 Playwright 生命周期、鉴权、主题、键盘、移动端和 10,000 节点套件继续保留。
协议兼容、长时间 soak、Docker/arm64 和其他浏览器引擎不由本组测试代替。
最终执行结果记录在 [gates.md](gates.md)，不把历史 `pass` 当成本轮运行结果。


## 日志生命周期补充（M10）

| 场景标识 | 绑定 | 并发与边界 | 必须成立的结果 |
|---|---|---|---|
| audit001_ui_old_response_cannot_replace_new_filter | AUDIT-001 | 持有初次响应，先应用新筛选，再释放旧响应并等待渲染 | 新列表不被旧结果覆盖 |
| audit004_ui_preview_cancel_confirm_and_receipt | AUDIT-004 | 关闭自动回收，独立种入3条100天前事件；桌面与手机 | 预览不删除、取消不删除、改变范围使预览失效、确认后保留回执 |
| audit004_ui_concurrent_cleanup_requires_new_preview | AUDIT-004 | Web持有预览，另一个API客户端先清理 | Web收到409要求重新预览，只有一条回执 |
| audit005_runtime_retention_reclaims_only_expired_events | AUDIT-005 | 配置0、环境90，启动真实后台维护 | 环境覆盖生效，旧日志删除，当前登录记录保留 |
| log001_default_logs_correlate_results_and_redact_secrets | LOG-001、AUDIT-003 | 公开订阅200/304/404和节点/标签操作 | 关联ID可查，状态/耗时可见，秘密、查询、Cookie和ANSI不入日志 |

存储集成测试另外覆盖501条探针与500条删除上限、并发确认+25次当前写入、
回执插入故障回滚、锁等待取消后重试、排他时间边界、0禁用、索引查询计划和迁移备份恢复。
CLI回归注入审计回执故障，确认过期outbox仍回收。Docker实测以独立标签标记容器，
无网络/端口/磁盘挂载，镜像声明卷替换为tmpfs防止匿名卷遗留，写入45000条合成记录验证轮转，最后只删除本次容器。
