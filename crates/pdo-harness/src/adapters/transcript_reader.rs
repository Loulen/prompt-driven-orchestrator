//! Reading a transcript file into its normalized index
//! ([`crate::domain::transcript`]), once.
//!
//! The reader keeps one [`TranscriptIndex`] per file and, on each read, feeds
//! it **only the bytes appended since the last read** (a `seek` to where the
//! index stopped). Turning pages, polling a live session or opening a tool
//! call never re-reads the whole file: a page is cut from the index in memory,
//! and a tool call's detail reads back its two lines by their byte spans. A
//! file that shrank or was replaced (another inode) is indexed afresh.
//!
//! The memo is bounded ([`TranscriptReader::new`]'s capacity): past it, the
//! least recently read file is dropped and re-indexed if read again.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::domain::probes;
use crate::domain::transcript::{
    tool_detail, IndexSearch, Needle, Span, ToolCallDetail, TranscriptIndex, TranscriptPage,
};

/// How many transcripts the daemon's reader keeps indexed: enough for every
/// node iteration of a large Run, so a search over the whole Run reads each
/// file once, then only what it gains.
const DAEMON_CAPACITY: usize = 64;

struct Indexed {
    index: TranscriptIndex,
    /// The file's identity when it was first indexed: a new one restarts.
    identity: u64,
    /// Every byte this entry read from the file, the measure of its cost.
    bytes_read: AtomicU64,
    last_read: AtomicU64,
}

type Key = (String, PathBuf);

/// A memo of transcript indexes, keyed by harness and file.
///
/// Each index sits behind a read-write lock: only bringing it up to date
/// writes; pages, tool calls and searches read it side by side.
pub struct TranscriptReader {
    capacity: usize,
    entries: Mutex<HashMap<Key, Arc<RwLock<Indexed>>>>,
    clock: AtomicU64,
}

impl TranscriptReader {
    pub fn new(capacity: usize) -> Self {
        TranscriptReader {
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
            clock: AtomicU64::new(0),
        }
    }

    /// The daemon's reader, shared by every request.
    pub fn shared() -> &'static TranscriptReader {
        static READER: OnceLock<TranscriptReader> = OnceLock::new();
        READER.get_or_init(|| TranscriptReader::new(DAEMON_CAPACITY))
    }

    /// The page of at most `limit` entries from `after` on, after indexing
    /// what the file gained since the last read.
    pub fn page(
        &self,
        harness: &str,
        path: &Path,
        after: usize,
        limit: usize,
    ) -> io::Result<TranscriptPage> {
        self.with_index(harness, path, |index| {
            index.page(Some(harness.to_string()), after, limit)
        })
    }

    /// The detail of the tool call at `index`, read back from its two lines;
    /// `None` when that entry is not a tool call.
    pub fn tool_detail(
        &self,
        harness: &str,
        path: &Path,
        index: usize,
    ) -> io::Result<Option<ToolCallDetail>> {
        let Some(spans) = self.with_index(harness, path, |i| i.tool_spans(index))? else {
            return Ok(None);
        };
        let mut file = File::open(path)?;
        let call = read_span(&mut file, spans.call)?;
        let result = spans
            .result
            .map(|span| read_span(&mut file, span))
            .transpose()?;
        if let Some(entry) = self.entry(harness, path) {
            let entry = entry.read().unwrap_or_else(|e| e.into_inner());
            entry.bytes_read.fetch_add(
                spans.call.len + spans.result.map_or(0, |s| s.len),
                Ordering::Relaxed,
            );
        }
        Ok(tool_detail(
            &spans.call_id,
            &call,
            result.as_deref(),
            |line| probes::transcript_records(harness, line),
        ))
    }

    /// The entries of the file holding `needle`, after indexing what the file
    /// gained since the last read: every one counted, the first `keep` with
    /// their excerpt. `cancelled` stops it between entries.
    ///
    /// A search never evicts a memoized index: past the memo's capacity, a file
    /// not memoized yet is indexed for this search only.
    pub fn search(
        &self,
        harness: &str,
        path: &Path,
        needle: &Needle,
        keep: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<IndexSearch> {
        self.read_index(harness, path, false, |index| {
            index.search(needle, keep, cancelled)
        })
    }

    /// Run `f` on the file's index, brought up to date first — the seam every
    /// reader of a normalized transcript goes through (a page, a search).
    pub fn with_index<R>(
        &self,
        harness: &str,
        path: &Path,
        f: impl FnOnce(&TranscriptIndex) -> R,
    ) -> io::Result<R> {
        self.read_index(harness, path, true, f)
    }

    /// Every byte read from `path` so far for `harness`, or `None` when it is
    /// not indexed: what a read costs, measured.
    pub fn bytes_read(&self, harness: &str, path: &Path) -> Option<u64> {
        let entry = self.entry(harness, path)?;
        let entry = entry.read().unwrap_or_else(|e| e.into_inner());
        Some(entry.bytes_read.load(Ordering::Relaxed))
    }

    fn entry(&self, harness: &str, path: &Path) -> Option<Arc<RwLock<Indexed>>> {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries
            .get(&(harness.to_string(), path.to_path_buf()))
            .cloned()
    }

    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// `f` on the up-to-date index of the file. `evict`: a file not memoized
    /// yet takes the place of the least recently read one when the memo is
    /// full; otherwise it is indexed for this call only.
    fn read_index<R>(
        &self,
        harness: &str,
        path: &Path,
        evict: bool,
        f: impl FnOnce(&TranscriptIndex) -> R,
    ) -> io::Result<R> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        let identity = file_identity(&metadata);
        let len = metadata.len();
        let now = self.tick();
        let Some(entry) = self.memo_entry(harness, path, identity, now, evict) else {
            let mut index = TranscriptIndex::new();
            let mut chunk = Vec::with_capacity(len as usize);
            file.take(len).read_to_end(&mut chunk)?;
            index.feed(&chunk, |line| probes::transcript_records(harness, line));
            return Ok(f(&index));
        };
        let stale = {
            let indexed = entry.read().unwrap_or_else(|e| e.into_inner());
            indexed.last_read.store(now, Ordering::Relaxed);
            indexed.identity != identity || len != indexed.index.parsed_to()
        };
        if stale {
            let mut indexed = entry.write().unwrap_or_else(|e| e.into_inner());
            if indexed.identity != identity || len < indexed.index.parsed_to() {
                indexed.index = TranscriptIndex::new();
                indexed.identity = identity;
            }
            let from = indexed.index.parsed_to();
            if len > from {
                file.seek(SeekFrom::Start(from))?;
                let mut chunk = Vec::with_capacity((len - from) as usize);
                file.take(len - from).read_to_end(&mut chunk)?;
                indexed
                    .bytes_read
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                indexed
                    .index
                    .feed(&chunk, |line| probes::transcript_records(harness, line));
            }
        }
        let indexed = entry.read().unwrap_or_else(|e| e.into_inner());
        Ok(f(&indexed.index))
    }

    fn memo_entry(
        &self,
        harness: &str,
        path: &Path,
        identity: u64,
        now: u64,
        evict: bool,
    ) -> Option<Arc<RwLock<Indexed>>> {
        let key = (harness.to_string(), path.to_path_buf());
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if !entries.contains_key(&key) && entries.len() >= self.capacity {
            if !evict {
                return None;
            }
            let oldest = entries
                .iter()
                .min_by_key(|(_, e)| {
                    e.try_read()
                        .map(|e| e.last_read.load(Ordering::Relaxed))
                        .unwrap_or(u64::MAX)
                })
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }
        Some(
            entries
                .entry(key)
                .or_insert_with(|| {
                    Arc::new(RwLock::new(Indexed {
                        index: TranscriptIndex::new(),
                        identity,
                        bytes_read: AtomicU64::new(0),
                        last_read: AtomicU64::new(now),
                    }))
                })
                .clone(),
        )
    }
}

fn read_span(file: &mut File, span: Span) -> io::Result<String> {
    file.seek(SeekFrom::Start(span.offset))?;
    let mut bytes = Vec::with_capacity(span.len as usize);
    file.take(span.len).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(unix)]
fn file_identity(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
fn file_identity(_metadata: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::registry::{CLAUDE, PI};
    use crate::domain::transcript::TranscriptEntry;
    use std::io::Write;

    const PI_FIXTURE: &str = include_str!("../../tests/fixtures/transcript/pi.jsonl");

    fn claude_line(i: usize) -> String {
        if i.is_multiple_of(2) {
            serde_json::json!({
                "type": "assistant", "uuid": format!("a{i}"), "timestamp": "2026-10-19T10:00:00Z",
                "message": {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "x".repeat(400)},
                    {"type": "tool_use", "id": format!("t{i}"), "name": "Bash", "input": {"command": format!("echo {i}")}}
                ]}
            })
        } else {
            serde_json::json!({
                "type": "user", "uuid": format!("u{i}"),
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": format!("t{}", i - 1), "content": "y".repeat(800)}
                ]}
            })
        }
        .to_string()
    }

    fn long_transcript(lines: usize) -> String {
        let mut text = String::new();
        for i in 0..lines {
            text.push_str(&claude_line(i));
            text.push('\n');
        }
        text
    }

    /// The cost criterion, measured on a long transcript: opening the tab and
    /// turning every page reads the file once; a live append reads only what
    /// was appended; a tool detail reads its two lines.
    #[test]
    fn pages_of_a_long_transcript_read_the_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.jsonl");
        let text = long_transcript(6_000);
        std::fs::write(&path, &text).unwrap();
        let reader = TranscriptReader::new(4);

        let started = std::time::Instant::now();
        let first = reader.page(CLAUDE, &path, 0, 50).unwrap();
        let first_read = started.elapsed();
        assert_eq!(first.total, 3_000);
        assert_eq!(first.entries.len(), 50);
        assert_eq!(reader.bytes_read(CLAUDE, &path), Some(text.len() as u64));

        let started = std::time::Instant::now();
        let mut after = first.next;
        let mut pages = 1;
        while let Some(from) = after {
            let page = reader.page(CLAUDE, &path, from as usize, 50).unwrap();
            after = page.next;
            pages += 1;
        }
        let other_pages = started.elapsed();
        assert_eq!(pages, 60);
        assert_eq!(
            reader.bytes_read(CLAUDE, &path),
            Some(text.len() as u64),
            "turning 59 more pages read nothing more from the file"
        );
        eprintln!(
            "{} bytes: first page {first_read:?}, the 59 others {other_pages:?}",
            text.len()
        );

        let mut appended = String::new();
        for i in 6_000..6_002 {
            appended.push_str(&claude_line(i));
            appended.push('\n');
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(appended.as_bytes())
            .unwrap();
        let live = reader.page(CLAUDE, &path, 3_000, 50).unwrap();
        assert_eq!(live.total, 3_001);
        assert_eq!(live.entries.len(), 1);
        assert_eq!(
            reader.bytes_read(CLAUDE, &path),
            Some((text.len() + appended.len()) as u64),
            "a live poll reads only the appended bytes"
        );

        let before = reader.bytes_read(CLAUDE, &path).unwrap();
        let detail = reader.tool_detail(CLAUDE, &path, 7).unwrap().unwrap();
        assert_eq!(detail.output.as_deref(), Some("y".repeat(800).as_str()));
        let spent = reader.bytes_read(CLAUDE, &path).unwrap() - before;
        assert!(
            spent < 2_000,
            "a tool detail reads its two lines, not the file ({spent})"
        );
    }

    /// A search reads the file once; searching again, as one types, reads
    /// only what the file gained.
    #[test]
    fn a_search_reads_the_file_once_then_what_it_gains() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.jsonl");
        let text = long_transcript(2_000);
        std::fs::write(&path, &text).unwrap();
        let reader = TranscriptReader::new(4);
        let never = || false;

        let found = reader
            .search(CLAUDE, &path, &Needle::new("ECHO 1").unwrap(), 5, &never)
            .unwrap();
        // The even commands from 10 on that start with a 1: 1x, 1xx, 1xxx.
        assert_eq!(found.matched, 5 + 50 + 500);
        assert_eq!(found.hits.len(), 5);
        assert_eq!(reader.bytes_read(CLAUDE, &path), Some(text.len() as u64));

        let again = reader
            .search(CLAUDE, &path, &Needle::new("echo 12").unwrap(), 5, &never)
            .unwrap();
        assert_eq!(again.matched, 1 + 5 + 50);
        assert_eq!(
            reader.bytes_read(CLAUDE, &path),
            Some(text.len() as u64),
            "a second search read nothing more"
        );
    }

    #[test]
    fn a_torn_tail_is_indexed_once_the_line_is_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pi.jsonl");
        let (head, tail) = PI_FIXTURE.split_at(PI_FIXTURE.len() - 30);
        std::fs::write(&path, head).unwrap();
        let reader = TranscriptReader::new(4);
        let partial = reader.page(PI, &path, 0, 100).unwrap();
        assert_eq!(partial.total, 4);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(tail.as_bytes())
            .unwrap();
        let whole = reader.page(PI, &path, 0, 100).unwrap();
        assert_eq!(whole.total, 5);
        assert!(matches!(
            whole.entries.last(),
            Some(TranscriptEntry::Message { text, .. }) if text == "Only to typing."
        ));
    }

    #[test]
    fn a_rewritten_file_is_indexed_afresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pi.jsonl");
        std::fs::write(&path, PI_FIXTURE).unwrap();
        let reader = TranscriptReader::new(4);
        assert_eq!(reader.page(PI, &path, 0, 100).unwrap().total, 5);
        let first_line = PI_FIXTURE.lines().take(3).collect::<Vec<_>>().join("\n") + "\n";
        std::fs::write(&path, first_line).unwrap();
        assert_eq!(reader.page(PI, &path, 0, 100).unwrap().total, 1);
    }

    #[test]
    fn a_missing_file_is_an_error_the_caller_reads_as_empty() {
        let reader = TranscriptReader::new(4);
        let err = reader
            .page(PI, Path::new("/nonexistent/pdo/transcript.jsonl"), 0, 10)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_search_past_the_memo_s_capacity_evicts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let reader = TranscriptReader::new(1);
        let viewed = dir.path().join("viewed.jsonl");
        let other = dir.path().join("other.jsonl");
        std::fs::write(&viewed, PI_FIXTURE).unwrap();
        std::fs::write(&other, PI_FIXTURE).unwrap();
        reader.page(PI, &viewed, 0, 1).unwrap();
        let found = reader
            .search(PI, &other, &Needle::new("typing").unwrap(), 10, &|| false)
            .unwrap();
        assert_eq!(found.matched, 1, "the file is searched all the same");
        assert!(
            reader.bytes_read(PI, &viewed).is_some(),
            "the viewed transcript stays"
        );
        assert!(reader.bytes_read(PI, &other).is_none());
    }

    #[test]
    fn the_memo_drops_the_least_recently_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let reader = TranscriptReader::new(2);
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| {
                let path = dir.path().join(format!("{i}.jsonl"));
                std::fs::write(&path, PI_FIXTURE).unwrap();
                path
            })
            .collect();
        reader.page(PI, &paths[0], 0, 1).unwrap();
        reader.page(PI, &paths[1], 0, 1).unwrap();
        reader.page(PI, &paths[0], 0, 1).unwrap();
        reader.page(PI, &paths[2], 0, 1).unwrap();
        assert!(reader.bytes_read(PI, &paths[0]).is_some());
        assert!(reader.bytes_read(PI, &paths[1]).is_none());
        assert!(reader.bytes_read(PI, &paths[2]).is_some());
    }
}
