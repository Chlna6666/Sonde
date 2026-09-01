use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use crate::{
    Error,
    spool::{Spool, SpoolOptions},
};

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sonde-spool-{name}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn options(root: &Path) -> SpoolOptions {
    SpoolOptions::new(root)
        .segment_bytes(64 * 1024)
        .max_bytes_per_queue(128 * 1024)
}

fn runtime() -> Result<tokio::runtime::Runtime, std::io::Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

#[test]
fn rejects_binding_mismatch() -> Result<(), Box<dyn std::error::Error>> {
    let root = test_root("binding");
    runtime()?.block_on(async {
        let (spool, _) = Spool::open("events", options(&root), [1_u8; 32]).await?;
        spool.append(br#"{"name":"first"}"#.to_vec()).await?;
        drop(spool);

        let error = match Spool::open("events", options(&root), [2_u8; 32]).await {
            Ok(_) => return Err(crate::Error::InvalidConfiguration("expected binding mismatch".into())),
            Err(error) => error,
        };
        assert!(matches!(error, Error::SpoolBindingMismatch { .. }));
        Ok::<(), crate::Error>(())
    })?;
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn rejects_second_live_writer() -> Result<(), Box<dyn std::error::Error>> {
    let root = test_root("lock");
    runtime()?.block_on(async {
        let (first, _) = Spool::open("events", options(&root), [3_u8; 32]).await?;
        let error = match Spool::open("events", options(&root), [3_u8; 32]).await {
            Ok(_) => return Err(crate::Error::InvalidConfiguration("expected spool lock".into())),
            Err(error) => error,
        };
        assert!(matches!(error, Error::SpoolLocked { .. }));
        drop(first);
        Ok::<(), crate::Error>(())
    })?;
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn repairs_truncated_active_segment_tail() -> Result<(), Box<dyn std::error::Error>> {
    let root = test_root("tail");
    let binding = [4_u8; 32];
    let first_payload = br#"{"name":"first"}"#.to_vec();
    let second_payload = br#"{"name":"second"}"#.to_vec();

    runtime()?.block_on(async {
        let (spool, _) = Spool::open("events", options(&root), binding).await?;
        let first = spool.append(first_payload.clone()).await?;
        let second = spool.append(second_payload.clone()).await?;
        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        drop(spool);
        Ok::<(), crate::Error>(())
    })?;

    let segment = root
        .join("events")
        .join("00000000000000000001.wal");
    let valid_len = fs::metadata(&segment)?.len();
    let mut file = OpenOptions::new().append(true).open(&segment)?;
    file.write_all(b"SNDWpartial-tail")?;
    file.sync_all()?;
    drop(file);
    assert!(fs::metadata(&segment)?.len() > valid_len);

    runtime()?.block_on(async {
        let (spool, recovered) = Spool::open("events", options(&root), binding).await?;
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].payload, first_payload);
        assert_eq!(recovered[1].payload, second_payload);
        assert_eq!(fs::metadata(&segment).await?.len(), valid_len);
        drop(spool);
        Ok::<(), crate::Error>(())
    })?;

    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn enforces_per_queue_spool_capacity() -> Result<(), Box<dyn std::error::Error>> {
    let root = test_root("capacity");
    runtime()?.block_on(async {
        let constrained = SpoolOptions::new(&root)
            .segment_bytes(64 * 1024)
            .max_bytes_per_queue(64 * 1024);
        let (spool, _) = Spool::open("logs", constrained, [5_u8; 32]).await?;

        spool.append(vec![b'a'; 40 * 1024]).await?;
        let error = spool.append(vec![b'b'; 40 * 1024]).await.expect_err("second record must exceed spool capacity");
        assert!(matches!(
            error,
            Error::SpoolFull {
                kind: "logs",
                max_bytes: 65_536
            }
        ));
        Ok::<(), crate::Error>(())
    })?;
    let _ = fs::remove_dir_all(root);
    Ok(())
}
