use std::{
    fs::{OpenOptions as StdOpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::Arc,
};

use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::Mutex,
};

use crate::error::{Error, Result};

const RECORD_MAGIC: [u8; 4] = *b"SNDW";
const RECORD_HEADER_BYTES: usize = 4 + 8 + 4 + 32;
const ACK_RECORD_BYTES: usize = 8 + 32;
const ACK_COMPACT_THRESHOLD_BYTES: u64 = 64 * 1024;
const DEFAULT_MAX_BYTES_PER_QUEUE: u64 = 64 * 1024 * 1024;
const DEFAULT_SEGMENT_BYTES: u64 = 4 * 1024 * 1024;
const META_MAGIC: [u8; 8] = *b"SNDSPL01";
const META_BYTES: usize = META_MAGIC.len() + 32;

#[derive(Clone, Debug)]
pub struct SpoolOptions {
    pub directory: PathBuf,
    pub max_bytes_per_queue: u64,
    pub segment_bytes: u64,
}

impl SpoolOptions {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            max_bytes_per_queue: DEFAULT_MAX_BYTES_PER_QUEUE,
            segment_bytes: DEFAULT_SEGMENT_BYTES,
        }
    }

    pub fn max_bytes_per_queue(mut self, bytes: u64) -> Self {
        self.max_bytes_per_queue = bytes;
        self
    }

    pub fn segment_bytes(mut self, bytes: u64) -> Self {
        self.segment_bytes = bytes;
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.directory.as_os_str().is_empty() {
            return Err(Error::InvalidConfiguration(
                "telemetry spool directory must not be empty".into(),
            ));
        }
        if self.segment_bytes < 64 * 1024 {
            return Err(Error::InvalidConfiguration(
                "telemetry spool segment size must be at least 64 KiB".into(),
            ));
        }
        if self.max_bytes_per_queue < self.segment_bytes {
            return Err(Error::InvalidConfiguration(
                "telemetry spool max bytes per queue must be at least one segment".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SpoolRecord {
    pub sequence: u64,
    pub payload: Vec<u8>,
}

pub(crate) struct Spool {
    kind: &'static str,
    state: Mutex<SpoolState>,
}

struct SpoolState {
    directory: PathBuf,
    options: SpoolOptions,
    segments: Vec<SegmentMeta>,
    active_file: File,
    ack_file: File,
    acknowledged_sequence: u64,
    next_sequence: u64,
    total_bytes: u64,
    poisoned: bool,
    _lock_file: std::fs::File,
}

#[derive(Clone, Debug)]
struct SegmentMeta {
    path: PathBuf,
    max_sequence: Option<u64>,
    size: u64,
}

struct SegmentScan {
    records: Vec<SpoolRecord>,
    valid_len: u64,
    max_sequence: Option<u64>,
}

impl Spool {
    pub(crate) async fn open(
        kind: &'static str,
        options: SpoolOptions,
        binding: [u8; 32],
    ) -> Result<(Arc<Self>, Vec<SpoolRecord>)> {
        options.validate()?;
        let directory = options.directory.join(kind);
        fs::create_dir_all(&directory)
            .await
            .map_err(|source| spool_io(&directory, source))?;

        let lock_path = directory.join("spool.lock");
        let lock_file = StdOpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|source| spool_io(&lock_path, source))?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(Error::SpoolLocked { path: lock_path });
            }
            Err(TryLockError::Error(source)) => {
                return Err(spool_io(&lock_path, source));
            }
        }

        ensure_binding(&directory, binding).await?;

        let ack_path = directory.join("ack.log");
        let acknowledged_sequence = read_acknowledged_sequence(&ack_path).await?;
        let mut segment_paths = list_segment_paths(&directory).await?;
        segment_paths.sort_by_key(|entry| entry.0);

        let mut segments = Vec::new();
        let mut recovered = Vec::new();
        let mut total_bytes = 0_u64;
        let mut previous_sequence = 0_u64;

        for (index, (start_sequence, path)) in segment_paths.iter().enumerate() {
            let is_last = index + 1 == segment_paths.len();
            let scan = scan_segment(path, options.segment_bytes, is_last).await?;
            if scan.records.is_empty() && !is_last {
                return Err(spool_corrupt(path, "empty non-active spool segment"));
            }
            if let Some(first) = scan.records.first()
                && first.sequence != *start_sequence
            {
                return Err(spool_corrupt(
                    path,
                    "segment file name does not match its first record sequence",
                ));
            }

            if is_last {
                let current_len = fs::metadata(path)
                    .await
                    .map_err(|source| spool_io(path, source))?
                    .len();
                if scan.valid_len < current_len {
                    let file = OpenOptions::new()
                        .write(true)
                        .open(path)
                        .await
                        .map_err(|source| spool_io(path, source))?;
                    file.set_len(scan.valid_len)
                        .await
                        .map_err(|source| spool_io(path, source))?;
                    file.sync_data()
                        .await
                        .map_err(|source| spool_io(path, source))?;
                }
            }

            for record in &scan.records {
                if record.sequence <= previous_sequence {
                    return Err(spool_corrupt(
                        path,
                        "record sequence is not strictly increasing",
                    ));
                }
                previous_sequence = record.sequence;
                if record.sequence > acknowledged_sequence {
                    recovered.push(record.clone());
                }
            }

            total_bytes = total_bytes.saturating_add(scan.valid_len);
            segments.push(SegmentMeta {
                path: path.clone(),
                max_sequence: scan.max_sequence,
                size: scan.valid_len,
            });
        }

        let max_seen = segments
            .iter()
            .filter_map(|segment| segment.max_sequence)
            .max()
            .unwrap_or(0);
        let next_sequence = max_seen
            .max(acknowledged_sequence)
            .saturating_add(1)
            .max(1);

        if segments.is_empty() {
            let path = segment_path(&directory, next_sequence);
            let file = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .await
                .map_err(|source| spool_io(&path, source))?;
            segments.push(SegmentMeta {
                path,
                max_sequence: None,
                size: 0,
            });
            drop(file);
        }

        let active_path = segments
            .last()
            .map(|segment| segment.path.clone())
            .ok_or_else(|| spool_corrupt(&directory, "missing active spool segment"))?;
        let mut active_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&active_path)
            .await
            .map_err(|source| spool_io(&active_path, source))?;
        active_file
            .seek(SeekFrom::End(0))
            .await
            .map_err(|source| spool_io(&active_path, source))?;

        let mut ack_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&ack_path)
            .await
            .map_err(|source| spool_io(&ack_path, source))?;
        ack_file
            .seek(SeekFrom::End(0))
            .await
            .map_err(|source| spool_io(&ack_path, source))?;

        let mut state = SpoolState {
            directory,
            options,
            segments,
            active_file,
            ack_file,
            acknowledged_sequence,
            next_sequence,
            total_bytes,
            poisoned: false,
            _lock_file: lock_file,
        };
        cleanup_acknowledged_segments(&mut state).await?;

        let spool = Arc::new(Self {
            kind,
            state: Mutex::new(state),
        });
        Ok((spool, recovered))
    }

    pub(crate) async fn append(&self, payload: Vec<u8>) -> Result<SpoolRecord> {
        let mut state = self.state.lock().await;
        ensure_not_poisoned(&state)?;

        let sequence = state.next_sequence;
        let frame = encode_record(sequence, &payload)?;
        let frame_len = u64::try_from(frame.len()).map_err(|_| Error::PayloadTooLarge)?;

        if state.segments.last().is_some_and(|segment| {
            segment.size > 0
                && segment.size.saturating_add(frame_len) > state.options.segment_bytes
        }) {
            roll_segment(&mut state).await?;
            cleanup_acknowledged_segments(&mut state).await?;
        }

        if state.total_bytes.saturating_add(frame_len) > state.options.max_bytes_per_queue {
            recycle_fully_acknowledged_active(&mut state).await?;
            cleanup_acknowledged_segments(&mut state).await?;
        }

        if state.total_bytes.saturating_add(frame_len) > state.options.max_bytes_per_queue {
            return Err(Error::SpoolFull {
                kind: self.kind,
                max_bytes: state.options.max_bytes_per_queue,
            });
        }

        let active_path = state
            .segments
            .last()
            .map(|segment| segment.path.clone())
            .ok_or_else(|| spool_corrupt(&state.directory, "missing active segment"))?;
        state
            .active_file
            .seek(SeekFrom::End(0))
            .await
            .map_err(|source| spool_io(&active_path, source))?;
        if let Err(source) = state.active_file.write_all(&frame).await {
            state.poisoned = true;
            return Err(spool_io(&active_path, source));
        }
        if let Err(source) = state.active_file.sync_data().await {
            state.poisoned = true;
            return Err(spool_io(&active_path, source));
        }

        let active = state
            .segments
            .last_mut()
            .ok_or_else(|| spool_corrupt(&state.directory, "missing active segment metadata"))?;
        active.size = active.size.saturating_add(frame_len);
        active.max_sequence = Some(sequence);
        state.total_bytes = state.total_bytes.saturating_add(frame_len);
        state.next_sequence = sequence.saturating_add(1);

        Ok(SpoolRecord { sequence, payload })
    }

    pub(crate) async fn commit_through(&self, sequence: u64) -> Result<()> {
        let mut state = self.state.lock().await;
        ensure_not_poisoned(&state)?;
        if sequence <= state.acknowledged_sequence {
            return Ok(());
        }
        if sequence >= state.next_sequence {
            return Err(spool_corrupt(
                &state.directory,
                "checkpoint exceeds the highest appended sequence",
            ));
        }

        let ack_path = state.directory.join("ack.log");
        let frame = encode_ack(sequence);
        state
            .ack_file
            .seek(SeekFrom::End(0))
            .await
            .map_err(|source| spool_io(&ack_path, source))?;
        if let Err(source) = state.ack_file.write_all(&frame).await {
            state.poisoned = true;
            return Err(spool_io(&ack_path, source));
        }
        if let Err(source) = state.ack_file.sync_data().await {
            state.poisoned = true;
            return Err(spool_io(&ack_path, source));
        }
        state.acknowledged_sequence = sequence;

        let ack_len = state
            .ack_file
            .metadata()
            .await
            .map_err(|source| spool_io(&ack_path, source))?
            .len();
        if ack_len >= ACK_COMPACT_THRESHOLD_BYTES {
            if let Err(source) = state.ack_file.set_len(0).await {
                state.poisoned = true;
                return Err(spool_io(&ack_path, source));
            }
            if let Err(source) = state.ack_file.seek(SeekFrom::Start(0)).await {
                state.poisoned = true;
                return Err(spool_io(&ack_path, source));
            }
            if let Err(source) = state.ack_file.write_all(&frame).await {
                state.poisoned = true;
                return Err(spool_io(&ack_path, source));
            }
            if let Err(source) = state.ack_file.sync_data().await {
                state.poisoned = true;
                return Err(spool_io(&ack_path, source));
            }
        }

        cleanup_acknowledged_segments(&mut state).await
    }
}

async fn ensure_binding(directory: &Path, binding: [u8; 32]) -> Result<()> {
    let path = directory.join("meta.bin");
    match fs::read(&path).await {
        Ok(bytes) => {
            if bytes.len() != META_BYTES || bytes[..META_MAGIC.len()] != META_MAGIC {
                return Err(spool_corrupt(&path, "unsupported or corrupt spool metadata"));
            }
            if bytes[META_MAGIC.len()..] != binding {
                return Err(Error::SpoolBindingMismatch { path });
            }
            Ok(())
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            if has_existing_spool_data(directory).await? {
                return Err(spool_corrupt(
                    directory,
                    "spool metadata is missing for an existing journal; remove the dev spool before reusing it",
                ));
            }
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .await
                .map_err(|source| spool_io(&path, source))?;
            let mut bytes = Vec::with_capacity(META_BYTES);
            bytes.extend_from_slice(&META_MAGIC);
            bytes.extend_from_slice(&binding);
            file.write_all(&bytes)
                .await
                .map_err(|source| spool_io(&path, source))?;
            file.sync_data()
                .await
                .map_err(|source| spool_io(&path, source))
        }
        Err(source) => Err(spool_io(&path, source)),
    }
}

async fn has_existing_spool_data(directory: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(directory)
        .await
        .map_err(|source| spool_io(directory, source))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|source| spool_io(directory, source))?
    {
        let path = entry.path();
        let is_wal = path.extension().and_then(|value| value.to_str()) == Some("wal");
        let is_ack = path.file_name().and_then(|value| value.to_str()) == Some("ack.log");
        if !is_wal && !is_ack {
            continue;
        }
        let len = entry
            .metadata()
            .await
            .map_err(|source| spool_io(&path, source))?
            .len();
        if len > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn list_segment_paths(directory: &Path) -> Result<Vec<(u64, PathBuf)>> {
    let mut entries = fs::read_dir(directory)
        .await
        .map_err(|source| spool_io(directory, source))?;
    let mut segments = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|source| spool_io(directory, source))?
    {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("wal") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let start_sequence = stem
            .parse::<u64>()
            .map_err(|_| spool_corrupt(&path, "invalid segment file name"))?;
        segments.push((start_sequence, path));
    }
    Ok(segments)
}

async fn scan_segment(
    path: &Path,
    segment_limit: u64,
    allow_truncated_tail: bool,
) -> Result<SegmentScan> {
    let bytes = fs::read(path)
        .await
        .map_err(|source| spool_io(path, source))?;
    let mut offset = 0_usize;
    let mut records = Vec::new();
    let mut max_sequence = None;

    while offset < bytes.len() {
        let remaining = bytes.len() - offset;
        if remaining < RECORD_HEADER_BYTES {
            if allow_truncated_tail {
                break;
            }
            return Err(spool_corrupt(path, "truncated record header"));
        }
        if bytes[offset..offset + 4] != RECORD_MAGIC {
            return Err(spool_corrupt(path, "record magic mismatch"));
        }

        let sequence = u64::from_le_bytes(
            bytes[offset + 4..offset + 12]
                .try_into()
                .map_err(|_| spool_corrupt(path, "invalid record sequence"))?,
        );
        let payload_len_u32 = u32::from_le_bytes(
            bytes[offset + 12..offset + 16]
                .try_into()
                .map_err(|_| spool_corrupt(path, "invalid record length"))?,
        );
        let payload_len = usize::try_from(payload_len_u32)
            .map_err(|_| spool_corrupt(path, "record length does not fit this platform"))?;
        let frame_len = RECORD_HEADER_BYTES.saturating_add(payload_len);
        let frame_len_u64 = u64::try_from(frame_len)
            .map_err(|_| spool_corrupt(path, "record frame length overflow"))?;
        if frame_len_u64 > segment_limit.saturating_add(RECORD_HEADER_BYTES as u64) {
            return Err(spool_corrupt(path, "record length exceeds spool segment limit"));
        }
        if remaining < frame_len {
            if allow_truncated_tail {
                break;
            }
            return Err(spool_corrupt(path, "truncated record payload"));
        }

        let expected_hash = &bytes[offset + 16..offset + 48];
        let payload = &bytes[offset + RECORD_HEADER_BYTES..offset + frame_len];
        let actual_hash = Sha256::digest(payload);
        if expected_hash != &actual_hash[..] {
            return Err(spool_corrupt(path, "record checksum mismatch"));
        }

        records.push(SpoolRecord {
            sequence,
            payload: payload.to_vec(),
        });
        max_sequence = Some(sequence);
        offset += frame_len;
    }

    Ok(SegmentScan {
        records,
        valid_len: u64::try_from(offset)
            .map_err(|_| spool_corrupt(path, "segment length overflow"))?,
        max_sequence,
    })
}

async fn read_acknowledged_sequence(path: &Path) -> Result<u64> {
    let bytes = match fs::read(path).await {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(source) => return Err(spool_io(path, source)),
    };
    let mut acknowledged = 0_u64;
    let mut offset = 0_usize;
    while bytes.len().saturating_sub(offset) >= ACK_RECORD_BYTES {
        let sequence = u64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .map_err(|_| spool_corrupt(path, "invalid ACK sequence"))?,
        );
        let expected = &bytes[offset + 8..offset + ACK_RECORD_BYTES];
        let actual = Sha256::digest(sequence.to_le_bytes());
        if expected != &actual[..] {
            break;
        }
        acknowledged = acknowledged.max(sequence);
        offset += ACK_RECORD_BYTES;
    }
    Ok(acknowledged)
}

fn encode_record(sequence: u64, payload: &[u8]) -> Result<Vec<u8>> {
    let payload_len = u32::try_from(payload.len()).map_err(|_| Error::PayloadTooLarge)?;
    let mut frame = Vec::with_capacity(RECORD_HEADER_BYTES.saturating_add(payload.len()));
    frame.extend_from_slice(&RECORD_MAGIC);
    frame.extend_from_slice(&sequence.to_le_bytes());
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&Sha256::digest(payload));
    frame.extend_from_slice(payload);
    Ok(frame)
}

fn encode_ack(sequence: u64) -> [u8; ACK_RECORD_BYTES] {
    let sequence_bytes = sequence.to_le_bytes();
    let hash = Sha256::digest(sequence_bytes);
    let mut frame = [0_u8; ACK_RECORD_BYTES];
    frame[..8].copy_from_slice(&sequence_bytes);
    frame[8..].copy_from_slice(&hash);
    frame
}

async fn roll_segment(state: &mut SpoolState) -> Result<()> {
    let start_sequence = state.next_sequence.max(1);
    let path = segment_path(&state.directory, start_sequence);
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .await
        .map_err(|source| spool_io(&path, source))?;
    state.active_file = file;
    state.segments.push(SegmentMeta {
        path,
        max_sequence: None,
        size: 0,
    });
    Ok(())
}

async fn recycle_fully_acknowledged_active(state: &mut SpoolState) -> Result<()> {
    let fully_acknowledged = state.segments.last().is_some_and(|segment| {
        segment
            .max_sequence
            .is_some_and(|max_sequence| max_sequence <= state.acknowledged_sequence)
            && segment.size > 0
    });
    if fully_acknowledged {
        roll_segment(state).await?;
    }
    Ok(())
}

async fn cleanup_acknowledged_segments(state: &mut SpoolState) -> Result<()> {
    while state.segments.len() > 1 {
        let removable = state.segments.first().is_some_and(|segment| {
            segment
                .max_sequence
                .is_some_and(|max_sequence| max_sequence <= state.acknowledged_sequence)
        });
        if !removable {
            break;
        }
        let segment = state.segments.remove(0);
        match fs::remove_file(&segment.path).await {
            Ok(()) => {
                state.total_bytes = state.total_bytes.saturating_sub(segment.size);
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                state.total_bytes = state.total_bytes.saturating_sub(segment.size);
            }
            Err(source) => return Err(spool_io(&segment.path, source)),
        }
    }
    Ok(())
}

fn ensure_not_poisoned(state: &SpoolState) -> Result<()> {
    if state.poisoned {
        Err(spool_corrupt(
            &state.directory,
            "spool writer is poisoned after an incomplete durable write; restart to recover",
        ))
    } else {
        Ok(())
    }
}

fn segment_path(directory: &Path, start_sequence: u64) -> PathBuf {
    directory.join(format!("{start_sequence:020}.wal"))
}

fn spool_io(path: &Path, source: std::io::Error) -> Error {
    Error::SpoolIo {
        path: path.to_path_buf(),
        source,
    }
}

fn spool_corrupt(path: &Path, reason: impl Into<String>) -> Error {
    Error::SpoolCorrupt {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Spool, SpoolOptions};

    #[test]
    fn replays_only_unacknowledged_records() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("sonde-spool-test-{}", uuid::Uuid::new_v4()));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        runtime.block_on(async {
            let options = SpoolOptions::new(&root)
                .segment_bytes(64 * 1024)
                .max_bytes_per_queue(128 * 1024);
            let binding = [7_u8; 32];
            let (spool, recovered) = Spool::open("events", options.clone(), binding).await?;
            assert!(recovered.is_empty());

            let first = spool.append(br#"{\"name\":\"first\"}"#.to_vec()).await?;
            let second = spool.append(br#"{\"name\":\"second\"}"#.to_vec()).await?;
            spool.commit_through(first.sequence).await?;
            drop(spool);

            let (reopened, recovered) = Spool::open("events", options, binding).await?;
            assert_eq!(recovered.len(), 1);
            assert_eq!(recovered[0].sequence, second.sequence);
            assert_eq!(recovered[0].payload, second.payload);
            reopened.commit_through(second.sequence).await?;
            Ok::<(), crate::Error>(())
        })?;

        let _ = fs::remove_dir_all(root);
        Ok(())
    }
}
