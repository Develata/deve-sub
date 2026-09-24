# 文档导航

Deve Sub 的文档按用途和权威层级组织。首次部署请先看用户路径；需要修改实现时，
再从工程蓝图、类型契约与验收记录追溯。

## 按用途查找

| 我需要…… | 从这里开始 |
|---|---|
| 安装或升级当前稳定版 `v0.1.3` | [README 快速开始](../README.md#quick-start) · [部署与更新](features/deployment.md) · [Release](https://github.com/Develata/deve-sub/releases/tag/v0.1.3) |
| 第一次创建订阅 | [节点整理](features/node-organization.md) · [模板与规则](features/subscription-templates.md) · [认证与订阅链接](features/authentication-and-links.md) |
| 备份、恢复或排查日志 | [备份与恢复](guides/backup-restore.md) · [日志管理](features/logging.md) |
| 判断功能是否已验证 | [验收矩阵](acceptance/matrix.tsv) · [验收方法](acceptance/gates.md) · [2026-09-24 全局审查报告](report/2026-09-24-global-review.md) · [2026-09-22 质量与部署证据](report/2026-09-22-global-quality.md) |
| 接入 API 或参与开发 | [OpenAPI](openapi/openapi.json) · [开发指南](guides/development.md) · [贡献指南](guides/contributing.md) |

## 工程阅读顺序

1. [`AGENTS.md`](../AGENTS.md) — 仓库治理、工作流程与 Git 策略
2. [`plan/00-engineering-constitution.md`](plan/00-engineering-constitution.md)
3. [`plan/01-terminology.md`](plan/01-terminology.md)
4. [`plan/04-workspace-layout.md`](plan/04-workspace-layout.md)
5. [`contracts/module-boundaries.md`](contracts/module-boundaries.md)
6. [`tasks/execution-roadmap.md`](tasks/execution-roadmap.md)
7. [`overview/architecture.md`](overview/architecture.md)
8. [`coverage-matrix.md`](coverage-matrix.md)
9. 对应里程碑的计划、功能、契约与验收文档

## 文档结构

| 路径 | 主要内容 |
|---|---|
| [`plan/`](plan/) | 当前工程蓝图与里程碑。 |
| [`contracts/`](contracts/) | 精确的模式、CLI、接口、权限和数据契约。 |
| [`features/`](features/) | 用户可见行为与操作路径。 |
| [`acceptance/`](acceptance/) | 验收用例、执行状态与证据规则。 |
| [`tasks/`](tasks/) | 里程碑任务与交付顺序。 |
| [`overview/`](overview/) | 跨层架构概览。 |
| [`guides/`](guides/) | 开发、贡献、备份与恢复指南。 |
| [`adr/`](adr/) | 重要架构决策的历史理由。 |
| [`data-model/`](data-model/) | 概念实体模型；物理模式以 `migrations/` 为准。 |
| [`openapi/`](openapi/) | 从代码生成的 API 规范。 |
| [`product-and-architecture-spec.md`](product-and-architecture-spec.md) | 原始任务规格归档。 |

[`coverage-matrix.md`](coverage-matrix.md) 对应当前各层覆盖关系；
[`AGENTS.md`](AGENTS.md) 说明文档权威和编辑边界。

## 不在此处保存

原始探索工作区、被否决的草案、私人基础设施/备份操作和空的占位目录不放入文档树。
当前有效语义留在计划、功能、契约、验收记录和 ADR 中；旧版本由 Git 历史保留。

## 详细用户路径

- [节点管理与分类](features/node-organization.md)
- [订阅模板与 Clash 分流规则](features/subscription-templates.md)
- [并发功能验收范围](acceptance/functional-matrix.md)
