# SQLite Concurrency Spike Report

## Summary

**Result: PASS with caveats** — WAL mode handles concurrent read/write with
zero errors. Batched transactions provide 10x throughput. WAL auto-checkpoint
stabilizes WAL size under single-connection sustained writes, but **does NOT
bound WAL size under concurrent multi-writer load** — a dedicated checkpoint
strategy is required for production.

## Configuration

| Parameter        | Value                        |
|------------------|------------------------------|
| SQLite           | 3.50.2 (bundled via rusqlite)|
| Journal mode     | WAL                          |
| Synchronous      | NORMAL                       |
| Busy timeout     | 5000ms                       |
| Writers          | 2 threads                    |
| Readers          | 4 threads                    |
| Test duration    | 10s                          |
| Batch size       | 100 rows/transaction         |

## Test 1 — Concurrent Read/Write

4 reader threads + 2 writer threads, 10s sustained load. WAL size sampled
every 500ms by a monitoring thread **during** the test (not after connection
close).

| Metric           | Value       |
|------------------|-------------|
| Duration         | 10.05s      |
| Rows inserted    | 334,800     |
| Write throughput | 33,306 rows/s |
| Read queries     | 33,129      |
| Read throughput  | 3,296 queries/s |
| Errors           | 0           |
| WAL peak (live)  | 168,385 KB (~164 MB) |
| WAL size (after) | 0.0 KB (connection-close checkpoint) |

**WAL growth during test** (sampled every 500ms while writers active):

```
t=0.0s   wal=0 KB        t=5.0s   wal=82,308 KB
t=1.0s   wal=24,479 KB   t=6.0s   wal=94,326 KB
t=2.0s   wal=41,293 KB   t=7.0s   wal=110,399 KB
t=3.0s   wal=55,359 KB   t=8.0s   wal=136,230 KB
t=4.0s   wal=68,873 KB   t=9.0s   wal=157,844 KB
```

**Finding**: WAL grows unboundedly under concurrent multi-writer load.
Default `wal_autocheckpoint=1000` (~4 MB) does NOT fire effectively when
multiple connections are actively writing — the checkpoint is deferred
because connections are in active transactions. The WAL only returns to 0 KB
when all connections close (connection-close checkpoint).

**Production implication**: A dedicated checkpoint strategy is required:
- Periodic `PRAGMA wal_checkpoint(PASSIVE)` from a background thread, or
- Lower `wal_autocheckpoint` threshold, or
- Schedule checkpoints during write-idle periods.

## Test 2 — Batched Import Throughput

10,000 rows inserted with varying batch sizes (single transaction per batch).

| Batch size | Time    | Throughput       |
|------------|---------|------------------|
| 1          | 0.392s  | 25,526 rows/s    |
| 10         | 0.082s  | 121,340 rows/s   |
| 100        | 0.052s  | 193,057 rows/s   |
| 500        | 0.041s  | 244,959 rows/s   |
| 1000       | 0.049s  | 202,899 rows/s   |

**Finding**: Batch size 500 is optimal. Single-row inserts are 10x slower.
Batch >500 shows diminishing returns (transaction overhead amortized).

## Test 3 — WAL Size Growth (sustained, multiple autocheckpoint cycles)

Single-connection sequential writes with WAL monitoring at row milestones
from 1K to 500K rows (crossing the ~4 MB autocheckpoint threshold multiple
times).

| Rows    | DB size    | WAL size   |
|---------|------------|------------|
| 1,000   | 15,272 KB  | 104.6 KB   |
| 5,000   | 15,272 KB  | 527.1 KB   |
| 10,000  | 15,272 KB  | 1,062.2 KB |
| 50,000  | 15,272 KB  | 4,047.6 KB |
| 100,000 | 15,272 KB  | 4,071.8 KB |
| 200,000 | 15,272 KB  | 4,071.8 KB |
| 500,000 | 21,924 KB  | 4,091.9 KB |

After `PRAGMA wal_checkpoint(TRUNCATE)`: WAL = 0.0 KB (busy=0, log=0, ckpt=0)

**Finding**: With a single connection doing sequential batched writes,
autocheckpoint fires at ~4 MB (1000 pages) and WAL stabilizes. The DB file
grows as checkpointed pages are merged. WAL stays bounded at ~4 MB across
500K rows (500x the autocheckpoint threshold). Manual TRUNCATE checkpoint
fully reclaims WAL space.

**Contrast with Test 1**: Autocheckpoint works for single-connection
sequential writes but NOT for concurrent multi-writer load (Test 1 showed
168 MB WAL peak). This is the key finding for production design.

## Conclusion

SQLite in WAL mode is viable for Deve Sub with a proper checkpoint strategy:
- Concurrent reads don't block writes and vice versa (0 errors under load).
- Batched imports (100-500 rows/tx) provide sufficient throughput (250k rows/s).
- WAL auto-checkpoint works for single-connection writes (stabilizes at ~4 MB).
- **WAL does NOT auto-checkpoint under concurrent multi-writer load** — a
  dedicated background checkpoint thread or scheduled checkpoint is required.
- Manual `wal_checkpoint(TRUNCATE)` fully reclaims WAL space.
