#![allow(clippy::expect_used, reason = "spike binary, failures are fatal")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};

const BATCH_SIZE: usize = 100;
const WRITE_DURATION: Duration = Duration::from_secs(10);
const READER_COUNT: usize = 4;
const WRITER_COUNT: usize = 2;

fn open_db(path: &str) -> Connection {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .expect("open db");
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("enable WAL");
    conn.pragma_update(None, "synchronous", "NORMAL")
        .expect("set synchronous NORMAL");
    conn.pragma_update(None, "busy_timeout", 5000)
        .expect("set busy_timeout");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nodes (
            id    INTEGER PRIMARY KEY,
            name  TEXT NOT NULL,
            proto TEXT NOT NULL,
            ts    INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );
        CREATE INDEX IF NOT EXISTS idx_nodes_proto ON nodes(proto);",
    )
    .expect("create schema");
    conn
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn run_concurrent_rw(db_path: &str) {
    println!("\n=== Concurrent Read/Write Test ===");
    println!(
        "Config: {} readers, {} writers, {}s duration, batch={}",
        READER_COUNT, WRITER_COUNT, WRITE_DURATION.as_secs(), BATCH_SIZE
    );

    {
        let init = open_db(db_path);
        drop(init);
    }

    let total_reads = Arc::new(AtomicU64::new(0));
    let total_writes = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));

    let wal_path = PathBuf::from(db_path).with_extension("db-wal");
    let wal_samples: Arc<Mutex<Vec<(f64, u64)>>> = Arc::new(Mutex::new(Vec::new()));

    let wal_before = file_size(&wal_path);
    let start = Instant::now();

    let mut handles = Vec::new();

    for _ in 0..WRITER_COUNT {
        let path = db_path.to_string();
        let writes = total_writes.clone();
        let errs = errors.clone();
        handles.push(thread::spawn(move || {
            let mut conn = match Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("writer open error: {e}");
                    errs.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            let _ = conn.pragma_update(None, "busy_timeout", 5000);
            let _ = conn.pragma_update(None, "synchronous", "NORMAL");
            let mut i = 0u64;
            while start.elapsed() < WRITE_DURATION {
                let tx = match conn.transaction() {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("writer tx error: {e}");
                        errs.fetch_add(1, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                let result = (0..BATCH_SIZE).try_for_each(|j| {
                    let row = i + j as u64;
                    let name = format!("node-{row}");
                    let proto = ["Vless", "VMess", "Trojan"][row as usize % 3];
                    tx.execute(
                        "INSERT INTO nodes(name, proto) VALUES (?1, ?2)",
                        rusqlite::params![name, proto],
                    )
                    .map(|_| ())
                });
                match result {
                    Ok(_) => {
                        if let Err(e) = tx.commit() {
                            eprintln!("writer commit error: {e}");
                            errs.fetch_add(1, Ordering::Relaxed);
                        } else {
                            i += BATCH_SIZE as u64;
                            writes.fetch_add(BATCH_SIZE as u64, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        eprintln!("writer insert error: {e}");
                        let _ = tx.rollback();
                        errs.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }

    for _ in 0..READER_COUNT {
        let path = db_path.to_string();
        let reads = total_reads.clone();
        let errs = errors.clone();
        handles.push(thread::spawn(move || {
            let conn = match Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("reader open error: {e}");
                    errs.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            let _ = conn.pragma_update(None, "busy_timeout", 5000);
            while start.elapsed() < WRITE_DURATION {
                match conn.query_row(
                    "SELECT COUNT(*) FROM nodes WHERE proto = 'Vless'",
                    [],
                    |r| r.get::<_, i64>(0),
                ) {
                    Ok(_) => {
                        reads.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        eprintln!("reader error: {e}");
                        errs.fetch_add(1, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        }));
    }

    let wal_path_clone = wal_path.clone();
    let wal_samples_clone = wal_samples.clone();
    let monitor_start = start;
    handles.push(thread::spawn(move || {
        while monitor_start.elapsed() < WRITE_DURATION {
            let size = file_size(&wal_path_clone);
            let elapsed = monitor_start.elapsed().as_secs_f64();
            wal_samples_clone.lock().unwrap().push((elapsed, size));
            thread::sleep(Duration::from_millis(500));
        }
    }));

    for h in handles {
        let _ = h.join();
    }

    let elapsed = start.elapsed();
    let wal_after = file_size(&wal_path);

    let writes = total_writes.load(Ordering::Relaxed);
    let reads = total_reads.load(Ordering::Relaxed);
    let errs = errors.load(Ordering::Relaxed);

    let samples = wal_samples.lock().unwrap();
    let peak_wal = samples.iter().map(|&(_, s)| s).max().unwrap_or(0);

    println!("Duration:         {:.2}s", elapsed.as_secs_f64());
    println!("Rows inserted:    {writes}");
    println!("Write throughput:  {:.0} rows/s", writes as f64 / elapsed.as_secs_f64());
    println!("Read queries:     {reads}");
    println!("Read throughput:   {:.0} queries/s", reads as f64 / elapsed.as_secs_f64());
    println!("Errors:           {errs}");
    println!("WAL size before:  {:.1} KB", wal_before as f64 / 1024.0);
    println!("WAL peak (live):  {:.1} KB", peak_wal as f64 / 1024.0);
    println!("WAL size after:   {:.1} KB", wal_after as f64 / 1024.0);
    println!("WAL samples ({})", samples.len());
    for (t, s) in samples.iter() {
        println!("  t={t:.1}s  wal={:.1} KB", *s as f64 / 1024.0);
    }
}

fn run_batched_import(db_path: &str) {
    println!("\n=== Batched Import Throughput Test ===");

    let mut conn = open_db(db_path);

    let batch_sizes = [1, 10, 100, 500, 1000];
    let total_rows = 10_000;

    for &batch in &batch_sizes {
        conn.execute("DELETE FROM nodes", []).expect("clear");
        let start = Instant::now();
        let mut inserted = 0u64;

        while (inserted as usize) < total_rows {
            let tx = conn.transaction().expect("tx");
            let this_batch = batch.min(total_rows - inserted as usize);
            for i in 0..this_batch {
                let name = format!("node-{}", inserted + i as u64);
                tx.execute(
                    "INSERT INTO nodes(name, proto) VALUES (?1, ?2)",
                    rusqlite::params![name, "Vless"],
                )
                .expect("insert");
            }
            tx.commit().expect("commit");
            inserted += this_batch as u64;
        }

        let elapsed = start.elapsed();
        let tps = total_rows as f64 / elapsed.as_secs_f64();
        println!(
            "batch={:4}  rows={total_rows}  time={:.3}s  throughput={:.0} rows/s",
            batch,
            elapsed.as_secs_f64(),
            tps
        );
    }
}

fn run_wal_growth(db_path: &str) {
    println!("\n=== WAL Size Growth Test (sustained, multiple autocheckpoint cycles) ===");

    let mut conn = open_db(db_path);
    conn.execute("DELETE FROM nodes", []).expect("clear");

    let wal_path = PathBuf::from(db_path).with_extension("db-wal");
    let checkpoints = [1_000, 5_000, 10_000, 50_000, 100_000, 200_000, 500_000];

    let mut total_inserted = 0u64;
    for &target in &checkpoints {
        while total_inserted < target {
            let tx = conn.transaction().expect("tx");
            let remaining = (target - total_inserted) as usize;
            let this_batch = remaining.min(500);
            for i in 0..this_batch {
                let name = format!("node-{}", total_inserted + i as u64);
                tx.execute(
                    "INSERT INTO nodes(name, proto) VALUES (?1, ?2)",
                    rusqlite::params![name, "Vless"],
                )
                .expect("insert");
            }
            tx.commit().expect("commit");
            total_inserted += this_batch as u64;
        }

        let wal_size = file_size(&wal_path);
        let db_size = file_size(Path::new(db_path));
        println!(
            "rows={total_inserted:7}  db={:.1} KB  wal={:.1} KB",
            db_size as f64 / 1024.0,
            wal_size as f64 / 1024.0
        );
    }

    println!("\nForcing WAL checkpoint (TRUNCATE)...");
    let result: (i64, i64, i64) = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .expect("checkpoint");
    let wal_size = file_size(&wal_path);
    println!(
        "After checkpoint: wal={:.1} KB  (busy={}, log={}, ckpt={})",
        wal_size as f64 / 1024.0,
        result.0,
        result.1,
        result.2
    );
}

fn main() {
    let db_path = concat!(env!("CARGO_MANIFEST_DIR"), "/spike.db");

    std::fs::remove_file(db_path).ok();
    std::fs::remove_file(format!("{db_path}-wal")).ok();
    std::fs::remove_file(format!("{db_path}-shm")).ok();

    println!("Deve Sub — SQLite Concurrency Spike");
    println!("Database: {db_path}");
    println!("SQLite:   {}", rusqlite::version());

    run_concurrent_rw(db_path);
    run_batched_import(db_path);
    run_wal_growth(db_path);

    std::fs::remove_file(db_path).ok();
    std::fs::remove_file(format!("{db_path}-wal")).ok();
    std::fs::remove_file(format!("{db_path}-shm")).ok();

    println!("\n=== Spike Complete ===");
}
