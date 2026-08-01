# Core Entity-Relationship Model

## Scope

This document defines the conceptual entity model for Deve Sub. The physical
schema source of truth is the `migrations/` directory. This diagram is the
conceptual guide; when they disagree, migrations prevail and this diagram must
be updated.

## Core ER diagram

The diagram covers the core entities required by the product spine. Not all
planned entities appear here; see `entity-catalog.md` for the full registry.

```mermaid
erDiagram
    USER ||--o{ SESSION : has
    USER ||--o{ RECOVERY_CODE : has
    USER ||--o| TOTP_SECRET : has
    USER ||--o{ SUBSCRIPTION : owns
    USER ||--o{ AUDIT_LOG : performs

    SOURCE ||--o{ SOURCE_SNAPSHOT : produces
    SOURCE_SNAPSHOT ||--o{ SOURCE_ITEM : contains
    SOURCE_ITEM }o--o| NODE : parses_to

    NODE ||--o| NODE_OVERRIDE : may_have
    NODE ||--o{ NODE_SOURCE_BINDING : bound_by
    SOURCE ||--o{ NODE_SOURCE_BINDING : binds
    NODE }o--o{ TAG : tagged

    TEMPLATE ||--o{ TEMPLATE_VERSION : versioned

    SUBSCRIPTION ||--o{ SUBSCRIPTION_VERSION : generates
    SUBSCRIPTION ||--o{ SUBSCRIPTION_TOKEN : has
    SUBSCRIPTION ||--o{ TEMPORARY_LINK : has
    SUBSCRIPTION }o--|{ TEMPLATE : uses
    SUBSCRIPTION }o--|{ COMPATIBILITY_PROFILE : targets

    JOB {
        ulid id PK
        string job_type
        string status
        string payload_json
        datetime created_at
        datetime started_at
        datetime completed_at
        string error
        bool cancel_requested
    }

    AUDIT_LOG {
        ulid id PK
        ulid actor_id FK
        string action
        string target_type
        string target_id
        string details_json
        datetime created_at
    }

    OUTBOX_EVENT {
        ulid id PK
        string aggregate_type
        ulid aggregate_id
        string event_type
        string payload_json
        datetime created_at
        datetime processed_at
    }

    USER {
        ulid id PK
        string username
        string password_hash
        string role
        bool enabled
        datetime expires_at
        int traffic_quota
        datetime created_at
    }

    SESSION {
        ulid id PK
        ulid user_id FK
        string token_hash
        datetime created_at
        datetime expires_at
        bool revoked
    }

    RECOVERY_CODE {
        ulid id PK
        ulid user_id FK
        string code_hash
        datetime used_at
    }

    TOTP_SECRET {
        ulid id PK
        ulid user_id FK
        string secret_encrypted
        bool enabled
    }

    SOURCE {
        ulid id PK
        string name
        string source_type
        string url
        string http_method
        string headers_encrypted
        bool auto_update
        int update_interval_secs
        bool enabled
        bool keep_on_fail
        datetime created_at
    }

    SOURCE_SNAPSHOT {
        ulid id PK
        ulid source_id FK
        int version
        datetime fetched_at
        string etag
        int node_count
        bool is_active
    }

    SOURCE_ITEM {
        ulid id PK
        ulid snapshot_id FK
        string raw_uri
        string parse_status
    }

    NODE {
        ulid id PK
        string display_name
        string protocol_kind
        string host
        int port
        string protocol_config_json
        string tls_json
        string udp_capability
        int revision
        string status
        bool missing_from_source
        datetime created_at
    }

    NODE_OVERRIDE {
        ulid id PK
        ulid node_id FK
        string display_name
        string region
        bool enabled
        string sni
        bool skip_cert_verify
        string fingerprint
        int sort_order
    }

    NODE_SOURCE_BINDING {
        ulid id PK
        ulid node_id FK
        ulid source_id FK
        string raw_uri
    }

    TAG {
        ulid id PK
        string name
        string color
    }

    TEMPLATE {
        ulid id PK
        string name
        string description
        int current_version
        datetime created_at
    }

    TEMPLATE_VERSION {
        ulid id PK
        ulid template_id FK
        int version
        string content_yaml
        ulid created_by FK
        datetime created_at
    }

    SUBSCRIPTION {
        ulid id PK
        string name
        string slug
        ulid owner_id FK
        string profile
        ulid template_id FK
        string node_selection_mode
        string filter_conditions_json
        int traffic_limit
        datetime expires_at
        bool enabled
        datetime created_at
    }

    SUBSCRIPTION_VERSION {
        ulid id PK
        ulid subscription_id FK
        int version
        string content_hash
        datetime generated_at
        bool is_active
    }

    SUBSCRIPTION_TOKEN {
        ulid id PK
        ulid subscription_id FK
        string token_hash
        string short_code
        datetime created_at
        datetime expires_at
        datetime rotation_grace_until
        int request_count
        datetime last_request_at
    }

    TEMPORARY_LINK {
        ulid id PK
        ulid subscription_id FK
        string token_hash
        datetime expires_at
        int request_count
    }

    COMPATIBILITY_PROFILE {
        ulid id PK
        string name
        string target_client
        string min_tested_version
        string supported_protocols
        string output_format
        string incompatibility_policy
    }
```

## Notes

- `NODE.chains_to` is a self-referential relationship (chain proxy targeting
  another node or group). It is omitted from the diagram to avoid rendering
  issues; see `docs/plan/05-protocol-engine.md` §"Override" and the spec §9.3.
- `JOB` is a standalone entity; jobs are created by application commands and
  tracked independently. Job types include source refresh, node test,
  subscription generation, and probe sync.
- `AUDIT_LOG` and `OUTBOX_EVENT` are infrastructure entities that do not
  participate in business relationships but are persisted alongside domain
  state.
- Encrypted fields (headers, cookies, TOTP secrets) are stored as ciphertext;
  the master key comes from a file or secret mount, never from the database.
- Token fields (`token_hash`) store HMAC-SHA256 digests, never plaintext tokens.
