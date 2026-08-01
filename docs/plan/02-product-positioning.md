# 02 — Product Positioning

## Scope

This chapter defines what Deve Sub is, what it is not, and the core business
spine. Product behavior details live in `docs/features/`; typed boundaries
live in `docs/contracts/`.

## Product

Deve Sub is a self-hosted proxy subscription infrastructure manager. It
aggregates subscription sources and single nodes, normalizes them into a
unified pool, and generates client-specific subscription outputs with user
authorization, traffic control, and expiry enforcement.

## Architecture form

Modular monolith with hexagonal layering and lightweight CQRS. Not a
microservice collection. Single binary `deve-sub` with subcommands. Thin web
UI renders server-owned state and dispatches typed intent.

## Core spine

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

## Configuration centralization

Product name, logo, and site title are centrally configured. No hardcoded
scattering across components.

## Reference note

Features and workflows may reference the external project miaomiaowu for
information architecture and usage patterns, but Deve Sub is an independent
implementation: no copied source code, logos, illustrations, or brand assets;
no pixel-level cloning.

## Authority

- Product behavior: `docs/features/`
- Typed boundaries: `docs/contracts/`
- Acceptance proof: `docs/acceptance/`
