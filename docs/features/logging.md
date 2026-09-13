# 日志管理

本功能归属 M10，验收编号为 AUDIT-001/003/004/005、LOG-001。
审计清理和新请求日志需要包含本次变更的 binary/Web；现有 `v0.1.0`
镜像不包含它们。Compose 的容器日志轮转独立生效，需重建容器。

## 查询与定位

Web「审计日志」记录操作者、时间、操作代码、目标与非敏感详情。
支持操作、目标类型和 UTC 日期筛选，结束日期不包含当天；可展开详情，
按游标继续加载。节点导入、标签创建/修改/删除、节点覆盖、地区、链、
打标和批量启停现在也有审计记录。常规操作的审计写入是尽力而为：
记录失败会写运行警告，已经成功的业务操作不因此回滚。

运行日志默认 `RUST_LOG=info`，每条包含 UTC 时间与模块。请求完成事件包含
`request_id`、`method`、脱敏的 `uri`、`status`、`duration_ms`（响应头生成耗时）。
同一 ID 通过响应头 `x-request-id` 返回；客户端传入的 ID 被替换。
2xx/3xx 使用 info，4xx 使用 warn，5xx 使用 error；成功的健康检查、静态请求，
空回收和正常 checkpoint 使用 debug。每分钟资源采样保留在 info。
日志不记录请求正文、Cookie、查询参数或公开订阅路径中的凭据。
重定向后的控制台输出没有 ANSI 颜色控制码。

```sh
docker compose logs --since 1h --tail 200 --timestamps deve-sub
docker compose logs -f --tail 100 deve-sub
```

临时排障可在 `.env` 中设置 `RUST_LOG=info,deve_sub_server=debug` 后重建容器。
无效的过滤语法会使启动失败，不再静默退回 info。

## 审计日志自动回收

默认保留 **90 天**，包括升级后未显式配置的实例。启用前需要永久留档时，
先做数据库备份或设为 `0`。配置优先级：CLI > 环境变量 > 配置文件默认值。

Compose 同目录 `.env`：

```dotenv
DEVE_SUB_AUDIT_RETENTION_DAYS=90
```

原生配置文件中的等价设置：

```json
{"logging":{"audit_retention_days":90}}
```

CLI 可传 `deve-sub serve --audit-retention-days 90`。有效范围为 1–3650；
`0` 关闭自动回收。配置变更后重启服务，Web 展示的是服务端实际生效值。
后台启动时及每分钟检查，与其他历史维护共享最多十轮、十秒的预算；
审计每轮最多删除 500 条，积压会在后续轮次继续处理。不同维护分支的
错误隔离；关停会取消尚未完成的工作。时间回收不会限制高流量下最近
90 天内的总记录数，仍需监测数据库磁盘空间。

## 审计日志手动清理

管理员进入「审计日志 → 日志保留与清理」，选择保留天数，点击「预览清理」。
页面明确显示 UTC 排他截止时间、本批条数以及是否还有剩余；确认后才删除。
清理针对全部审计日志，不受下方查询筛选影响；至少保留最近一天。
需要留档时先使用现有数据库备份流程。清理是永久删除，不能通过页面撤销。

一次最多 500 条；积压较多时可再次预览确认。并发清理改变了候选集合时返回
409 并要求重新预览；最近写入、业务数据和主密钥不在清理范围。
每次实际删除与一条 `audit.cleanup` 记录共同提交，记录操作者、原因、截止时间
和条数；回执写入失败会回滚删除。超时或响应丢失时先刷新审计记录，确认结果后
再预览。手动和自动清理共用这一事务边界。

SQLite 删除后空间供数据库复用，文件不一定立即缩小；后台不执行 VACUUM。
备份文件也不会被日志回收自动删除。

## 容器运行日志的回收与手动清理

Compose 使用 Docker `local` 驱动，`max-size=10m`、`max-file=3`，压缩归档。
轮转按容量进行，最多保留三个文件；它是容器运行日志的容量策略，和数据库
审计日志的时间策略分开。参见 [Docker 官方配置说明](https://docs.docker.com/engine/logging/drivers/local/)。

需要立即清空本服务的容器运行日志时，可先导出需要留存的内容，再重建本服务：

```sh
umask 077
docker compose logs --no-color --timestamps deve-sub > deve-sub-runtime.log
docker compose up -d --no-deps --force-recreate deve-sub
```

重建会短暂中断服务，旧容器删除时其运行日志一起移除；保持原目录、Compose
项目名和 named volume，数据库、主密钥及审计记录继续保留。导出的文件由操作者
自行归档或删除。不操作 Docker 内部日志文件，不使用 `down --volumes` 清日志。

原生 systemd 安装的运行日志由主机 journald 策略管理，用
`journalctl -u deve-sub --since '1 hour ago'` 查询。应用不清理共享系统 journal；
`journalctl --vacuum-*` 不是仅删除某个服务的操作，不能作为本服务的清理按钮。
需要独立容量/时间策略时由主机管理员配置专用 journal namespace 或日志收集器。
