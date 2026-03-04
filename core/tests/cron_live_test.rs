//! Live integration test: proves recurring cron jobs fire more than once.
//!
//! Run with:
//!   cargo test --package mofaclaw-core --test cron_live_test -- --nocapture
//!
//! Expected output:
//!   Job fired! Count: 1
//!   Job fired! Count: 2
//!   Job fired! Count: 3
//!   ...
//!   ✅ Recurring job fired 3 times in 7 seconds (bug #47 is FIXED)

use mofaclaw_core::cron::service::CronService;
use mofaclaw_core::cron::types::CronSchedule;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_recurring_job_fires_multiple_times() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store_path = temp_dir.path().join("live_test_jobs.json");

    // Counter to track how many times the callback fires
    let fire_count = Arc::new(AtomicU32::new(0));
    let fire_count_clone = Arc::clone(&fire_count);

    let callback: mofaclaw_core::cron::service::CronCallback = Arc::new(move |job| {
        let counter = Arc::clone(&fire_count_clone);
        Box::pin(async move {
            let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
            eprintln!("  🔥 Job '{}' fired! Count: {}", job.name, count);
            Ok(Some(format!("Executed #{}", count)))
        })
    });

    let service = CronService::new(store_path).with_callback(callback);
    service.start().await.unwrap();

    // Add a job that fires every 2 seconds
    let job = service
        .add_job(
            "Live Test Job".to_string(),
            CronSchedule::every(2), // every 2 seconds
            "Testing recurring execution".to_string(),
            false,
            None,
            None,
        )
        .await;

    eprintln!(
        "\n  Added job '{}' (id: {}), waiting 7 seconds...\n",
        job.name, job.id
    );

    // Wait 7 seconds — should fire at least 3 times (at ~2s, ~4s, ~6s)
    tokio::time::sleep(Duration::from_secs(7)).await;

    let total_fires = fire_count.load(Ordering::SeqCst);
    eprintln!("\n  === RESULT ===");
    eprintln!("  Total fires in 7 seconds: {}", total_fires);

    if total_fires >= 3 {
        eprintln!(
            "  ✅ Recurring job fired {} times — bug #47 is FIXED!",
            total_fires
        );
    } else if total_fires == 1 {
        eprintln!("  ❌ Job only fired once — bug #47 is NOT fixed (timer never re-armed)");
    } else {
        eprintln!("  ⚠️  Job fired {} times (expected ≥3)", total_fires);
    }

    // Verify the job state was updated
    let jobs = service.list_jobs(true).await;
    assert_eq!(jobs.len(), 1);
    let j = &jobs[0];
    eprintln!("\n  Job state after test:");
    eprintln!("    last_status:    {:?}", j.state.last_status);
    eprintln!("    last_run_at_ms: {:?}", j.state.last_run_at_ms);
    eprintln!("    next_run_at_ms: {:?}", j.state.next_run_at_ms);
    eprintln!("    last_error:     {:?}", j.state.last_error);

    service.stop().await;

    // The key assertion: must fire at least 3 times in 7 seconds
    assert!(
        total_fires >= 3,
        "REGRESSION: recurring job only fired {} time(s) in 7s (expected ≥3). Bug #47 not fixed!",
        total_fires
    );
}
