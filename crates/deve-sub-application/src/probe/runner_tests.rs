//! B-14 audit-required tests: failure injection and bounded-concurrency
//! memory bound.
//!
//! 1. `failure_injection_writes_failed_status` — when the run repo returns a
//!    storage error mid-run, the outer wrapper writes `Failed` terminal
//!    status so no run is left in `Running`.
//! 2. `ten_thousand_nodes_bounded_concurrency` — 10k nodes are probed with
//!    `buffer_unordered(concurrency)`; at most `concurrency` futures are
//!    polled simultaneously, proving the runner does not spawn 10k tasks.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;

use deve_sub_domain::NodeChainEntry;
use deve_sub_domain::source::SourceError;
use deve_sub_domain::source::{
    ImportResult, NodeFilter, NodePoolEntry, NodePoolRepository, ReconcileInput, ReconcileResult,
};
use deve_sub_domain::{
    Authentication, DomainName, Endpoint, ErrorClass, Host, LatencyProbe, LatencyRecord,
    LatencyRecordRepository, LatencyResult, Node, NodeSource, ProbeError, ProbeRun,
    ProbeRunRepository, ProbeRunResult, ProbeRunStatus, ProbeType, ProtocolConfig, ProtocolKind,
    RegionAssignment, RegionMethod, TrojanConfig, UdpCapability,
};
use deve_sub_kernel::{NodeId, ProbeRunId, Timestamp};

use super::{RunnerConfig, RunnerDeps, execute_probe_run};

// ---------------------------------------------------------------------------
// Mock probe: counts concurrent invocations to prove bounded concurrency.
// ---------------------------------------------------------------------------

struct CountingProbe {
    max_concurrent: Arc<AtomicUsize>,
    current_concurrent: Arc<AtomicUsize>,
}

impl CountingProbe {
    fn new(max_concurrent: Arc<AtomicUsize>, current_concurrent: Arc<AtomicUsize>) -> Self {
        Self {
            max_concurrent,
            current_concurrent,
        }
    }
}

#[async_trait]
impl LatencyProbe for CountingProbe {
    async fn probe(&self, _node: &Node, _timeout: Duration) -> LatencyResult {
        let cur = self.current_concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        let prev_max = self.max_concurrent.load(Ordering::SeqCst);
        if cur > prev_max {
            self.max_concurrent.store(cur, Ordering::SeqCst);
        }
        // Yield to allow other futures to run concurrently.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        self.current_concurrent.fetch_sub(1, Ordering::SeqCst);
        LatencyResult {
            node_id: _node.id,
            rtt_ms: Some(10),
            error_class: ErrorClass::Ok,
        }
    }
}

// ---------------------------------------------------------------------------
// Mock node pool: returns a node for any ID.
// ---------------------------------------------------------------------------

fn make_test_node(id: NodeId) -> Node {
    Node {
        id,
        display_name: "test".to_owned(),
        protocol: ProtocolKind::Trojan,
        config: ProtocolConfig::Trojan(TrojanConfig {
            packet_encoding: None,
        }),
        endpoint: Endpoint {
            host: Host::Domain(DomainName::new("example.com".to_owned())),
            port: 443,
        },
        authentication: Authentication::Password {
            password: "TEST".to_owned(),
        },
        transport: None,
        tls: None,
        udp: UdpCapability {
            supported: None,
            xudp: None,
        },
        multiplex: None,
        obfuscation: None,
        congestion: None,
        chain: None,
        source: NodeSource {
            source_label: "test".to_owned(),
            raw_uri: None,
            imported_at: Timestamp::now(),
        },
        tags: vec![],
        region: RegionAssignment {
            method: RegionMethod::Auto,
            value: None,
        },
        extras: BTreeMap::new(),
    }
}

struct StubPool;

#[async_trait]
impl NodePoolRepository for StubPool {
    async fn reconcile(&self, _input: ReconcileInput<'_>) -> Result<ReconcileResult, SourceError> {
        unimplemented!()
    }
    async fn list_nodes(
        &self,
        _filter: &NodeFilter,
        _cursor: Option<NodeId>,
        _limit: u32,
    ) -> Result<Vec<NodePoolEntry>, SourceError> {
        unimplemented!()
    }
    async fn get_node(&self, id: NodeId) -> Result<Option<NodePoolEntry>, SourceError> {
        Ok(Some(NodePoolEntry {
            node: make_test_node(id),
            missing_from_source: false,
            is_active: true,
            revision: 1,
            created_at: Timestamp::now(),
            override_info: None,
            tags: vec![],
        }))
    }
    async fn get_nodes(&self, ids: &[NodeId]) -> Result<Vec<NodePoolEntry>, SourceError> {
        Ok(ids
            .iter()
            .map(|&id| NodePoolEntry {
                node: make_test_node(id),
                missing_from_source: false,
                is_active: true,
                revision: 1,
                created_at: Timestamp::now(),
                override_info: None,
                tags: vec![],
            })
            .collect())
    }
    async fn import_nodes(&self, _nodes: Vec<Node>) -> Result<ImportResult, SourceError> {
        unimplemented!()
    }
    async fn list_node_chains(&self) -> Result<Vec<NodeChainEntry>, SourceError> {
        Ok(Vec::new())
    }
    async fn existing_node_ids(&self, ids: &[NodeId]) -> Result<Vec<NodeId>, SourceError> {
        Ok(ids.to_vec())
    }
    async fn set_node_chain(
        &self,
        _node_id: NodeId,
        _chain: Option<&[NodeId]>,
    ) -> Result<(), SourceError> {
        unimplemented!()
    }
}

// ---------------------------------------------------------------------------
// Mock run repo: configurable to fail on update_status for failure injection.
// ---------------------------------------------------------------------------

struct StubRunRepo {
    /// When true, the next `update_status` call returns a Storage error.
    fail_on_running: std::sync::Mutex<bool>,
    /// Records every status transition.
    transitions: std::sync::Mutex<Vec<ProbeRunStatus>>,
}

impl StubRunRepo {
    fn new() -> Self {
        Self {
            fail_on_running: std::sync::Mutex::new(false),
            transitions: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn transitions(&self) -> Vec<ProbeRunStatus> {
        self.transitions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[async_trait]
impl ProbeRunRepository for StubRunRepo {
    async fn create(&self, _run: &ProbeRun) -> Result<(), ProbeError> {
        Ok(())
    }
    async fn find_by_id(&self, _id: ProbeRunId) -> Result<Option<ProbeRun>, ProbeError> {
        Ok(None)
    }
    async fn update_status(
        &self,
        _id: ProbeRunId,
        status: ProbeRunStatus,
        _results: &[ProbeRunResult],
        _completed_at: Option<Timestamp>,
    ) -> Result<(), ProbeError> {
        self.transitions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(status);
        let fail = *self
            .fail_on_running
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if fail && status == ProbeRunStatus::Running {
            return Err(ProbeError::Storage("injected failure".to_owned()));
        }
        Ok(())
    }
    async fn update_results(
        &self,
        _id: ProbeRunId,
        _results: &[ProbeRunResult],
        _completed_at: Option<Timestamp>,
    ) -> Result<(), ProbeError> {
        Ok(())
    }
    async fn recover_crashed_runs(&self) -> Result<u64, ProbeError> {
        Ok(0)
    }
    async fn delete(&self, _id: ProbeRunId) -> Result<(), ProbeError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock latency repo: counts batch_create calls.
// ---------------------------------------------------------------------------

struct StubLatencyRepo {
    batch_create_calls: Arc<AtomicUsize>,
    records_inserted: Arc<AtomicUsize>,
}

#[async_trait]
impl LatencyRecordRepository for StubLatencyRepo {
    async fn create(&self, _record: &LatencyRecord) -> Result<(), ProbeError> {
        unimplemented!()
    }
    async fn batch_create(&self, records: &[LatencyRecord]) -> Result<(), ProbeError> {
        self.batch_create_calls.fetch_add(1, Ordering::SeqCst);
        self.records_inserted
            .fetch_add(records.len(), Ordering::SeqCst);
        Ok(())
    }
    async fn list_for_node(
        &self,
        _node_id: NodeId,
        _limit: u32,
    ) -> Result<Vec<LatencyRecord>, ProbeError> {
        unimplemented!()
    }
    async fn list_recent(&self, _limit: u32) -> Result<Vec<LatencyRecord>, ProbeError> {
        unimplemented!()
    }
    async fn delete_for_run(&self, _run_id: ProbeRunId) -> Result<(), ProbeError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Test 1: failure injection writes Failed terminal status.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn failure_injection_writes_failed_status() {
    let run_repo = Arc::new(StubRunRepo::new());
    // Inject a storage failure when the runner tries to write `Running`.
    *run_repo
        .fail_on_running
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = true;

    let batch_calls = Arc::new(AtomicUsize::new(0));
    let records_inserted = Arc::new(AtomicUsize::new(0));
    let latency_repo = Arc::new(StubLatencyRepo {
        batch_create_calls: Arc::clone(&batch_calls),
        records_inserted: Arc::clone(&records_inserted),
    });

    let deps = RunnerDeps {
        probe: Arc::new(CountingProbe::new(
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        )),
        pool_repo: Arc::new(StubPool),
        run_repo: Arc::clone(&run_repo) as Arc<dyn ProbeRunRepository>,
        latency_repo: latency_repo as Arc<dyn LatencyRecordRepository>,
    };

    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let node_ids: Vec<NodeId> = (0..5).map(|_| NodeId::new()).collect();

    let result = execute_probe_run(
        ProbeRunId::new(),
        node_ids,
        ProbeType::TcpConnect,
        deps,
        cancelled,
        RunnerConfig::default(),
    )
    .await;

    // The inner function returns the storage error; the outer wrapper
    // catches it and writes Failed.
    assert!(result.is_err(), "should propagate error from inner");

    let transitions = run_repo.transitions();
    assert!(
        transitions.contains(&ProbeRunStatus::Failed),
        "Failed status must be written after error; got {transitions:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: 10k nodes probed with bounded concurrency — at most `concurrency`
// futures run simultaneously, not 10k tasks.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ten_thousand_nodes_bounded_concurrency() {
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let current_concurrent = Arc::new(AtomicUsize::new(0));

    let probe = Arc::new(CountingProbe::new(
        Arc::clone(&max_concurrent),
        Arc::clone(&current_concurrent),
    ));

    let batch_calls = Arc::new(AtomicUsize::new(0));
    let records_inserted = Arc::new(AtomicUsize::new(0));
    let latency_repo = Arc::new(StubLatencyRepo {
        batch_create_calls: Arc::clone(&batch_calls),
        records_inserted: Arc::clone(&records_inserted),
    });

    let concurrency = 16;
    let config = RunnerConfig {
        timeout: Duration::from_millis(100),
        concurrency,
    };

    let deps = RunnerDeps {
        probe,
        pool_repo: Arc::new(StubPool),
        run_repo: Arc::new(StubRunRepo::new()) as Arc<dyn ProbeRunRepository>,
        latency_repo: latency_repo as Arc<dyn LatencyRecordRepository>,
    };

    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let node_ids: Vec<NodeId> = (0..10_000).map(|_| NodeId::new()).collect();

    let result = execute_probe_run(
        ProbeRunId::new(),
        node_ids,
        ProbeType::TcpConnect,
        deps,
        cancelled,
        config,
    )
    .await;

    assert!(result.is_ok(), "run should complete: {:?}", result.err());

    let observed_max = max_concurrent.load(Ordering::SeqCst);
    assert!(
        observed_max <= concurrency,
        "concurrency must be bounded by {concurrency}, observed max {observed_max}"
    );
    assert!(observed_max > 0, "at least one probe must have run");

    // Batch insert happened exactly once with all 10k records.
    assert_eq!(
        batch_calls.load(Ordering::SeqCst),
        1,
        "batch_create called once"
    );
    assert_eq!(
        records_inserted.load(Ordering::SeqCst),
        10_000,
        "all 10k records inserted in one batch"
    );
}
