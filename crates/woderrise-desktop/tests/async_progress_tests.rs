use std::sync::{Arc, Mutex};
use woderrise_core::InstallStage;

#[test]
fn test_async_install_all_15_stages_stringification_and_ordering() {
    let stages = [
        InstallStage::Resolve,
        InstallStage::Compatibility,
        InstallStage::Fetch,
        InstallStage::Validate,
        InstallStage::Inspect,
        InstallStage::Plan,
        InstallStage::Conflicts,
        InstallStage::Backup,
        InstallStage::Prepared,
        InstallStage::Execute,
        InstallStage::Outcomes,
        InstallStage::Activate,
        InstallStage::Verify,
        InstallStage::Commit,
        InstallStage::Present,
    ];

    assert_eq!(stages.len(), 15);

    let stage_names: Vec<&'static str> = stages.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        stage_names,
        vec![
            "Resolve",
            "Compatibility",
            "Fetch",
            "Validate",
            "Inspect",
            "Plan",
            "Conflicts",
            "Backup",
            "Prepared",
            "Execute",
            "Outcomes",
            "Activate",
            "Verify",
            "Commit",
            "Present",
        ]
    );
}

#[test]
fn test_async_progress_percentage_clamping() {
    let clamp_pct = |val: f32| -> i32 {
        if val.is_nan() || val < 0.0 {
            0
        } else if val > 1.0 {
            100
        } else {
            (val * 100.0).round() as i32
        }
    };

    assert_eq!(clamp_pct(-0.5), 0);
    assert_eq!(clamp_pct(0.0), 0);
    assert_eq!(clamp_pct(0.154), 15);
    assert_eq!(clamp_pct(0.999), 100);
    assert_eq!(clamp_pct(1.0), 100);
    assert_eq!(clamp_pct(1.5), 100);
    assert_eq!(clamp_pct(f32::NAN), 0);
}

#[test]
fn test_async_worker_progress_channel_stream_without_deadlock() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let recorded_clone = recorded.clone();

    let (tx, rx) = std::sync::mpsc::channel();

    let worker = std::thread::spawn(move || {
        let simulated_events = [
            (InstallStage::Resolve, "Resolving manifest", 0.05),
            (InstallStage::Validate, "Validating checksums", 0.20),
            (InstallStage::Backup, "Backing up target files", 0.50),
            (InstallStage::Execute, "Applying payload", 0.75),
            (InstallStage::Commit, "Installation committed", 1.00),
        ];

        for (stage, msg, pct) in simulated_events {
            tx.send((stage, msg.to_string(), pct)).unwrap();
        }
    });

    while let Ok((stage, msg, pct)) = rx.recv() {
        recorded_clone.lock().unwrap().push((stage, msg, pct));
    }

    worker.join().unwrap();

    let events = recorded.lock().unwrap();
    assert_eq!(events.len(), 5);
    assert_eq!(events[0].0, InstallStage::Resolve);
    assert_eq!(events[4].0, InstallStage::Commit);
    assert_eq!(events[4].2, 1.00);
}
