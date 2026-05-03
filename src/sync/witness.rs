//! Deterministic JSONL chunk witnesses for sync pipelines.
//!
//! This module is intentionally pure: it reads JSONL bytes and produces a
//! stable witness without touching files, paths, storage, or git state. The
//! serial import/export path can keep its existing behavior while future
//! parallel sync work uses these witnesses to prove unchanged chunks.

use crate::util::hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead};

/// Schema marker for JSONL Merkle witnesses.
pub const JSONL_WITNESS_SCHEMA_VERSION: &str = "br.jsonl-witness.v1";

const ROOT_DOMAIN: &[u8] = b"br:jsonl-witness:root:v1\0";
const CHUNK_DOMAIN: &[u8] = b"br:jsonl-witness:chunk:v1\0";
const FIELD_SEPARATOR: &[u8] = b"\0";

/// Merkle-style witness for an exact JSONL byte stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonlMerkleWitness {
    pub schema_version: String,
    pub chunk_size_lines: usize,
    pub line_count: usize,
    pub byte_count: u64,
    pub root_hash: String,
    pub chunks: Vec<JsonlChunkWitness>,
}

/// Witness metadata for one contiguous JSONL line chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonlChunkWitness {
    pub index: usize,
    pub start_line: usize,
    pub line_count: usize,
    pub byte_count: u64,
    pub hash: String,
    pub first_line_hash: Option<String>,
    pub last_line_hash: Option<String>,
}

/// Drift summary between two JSONL Merkle witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use = "comparison summaries identify which chunks can be reused safely"]
pub struct JsonlWitnessComparison {
    pub schema_versions_match: bool,
    pub chunk_size_lines_match: bool,
    pub root_hashes_match: bool,
    pub drift_detected: bool,
    pub base_line_count: usize,
    pub candidate_line_count: usize,
    pub base_byte_count: u64,
    pub candidate_byte_count: u64,
    pub base_chunk_count: usize,
    pub candidate_chunk_count: usize,
    pub comparable_chunk_count: usize,
    pub unchanged_chunks: usize,
    pub changed_chunks: usize,
    pub added_chunks: usize,
    pub removed_chunks: usize,
    pub unchanged_byte_count: u64,
    pub changed_base_byte_count: u64,
    pub changed_candidate_byte_count: u64,
    pub added_byte_count: u64,
    pub removed_byte_count: u64,
    pub safe_reuse_prefix_chunks: usize,
    pub first_changed_chunk_index: Option<usize>,
}

/// Candidate-ordered reuse plan for a future parallel JSONL sync pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use = "reuse plans identify which chunks can be copied or rebuilt"]
pub struct JsonlWitnessReusePlan {
    pub comparison: JsonlWitnessComparison,
    pub actions: Vec<JsonlChunkReuseStep>,
}

/// One chunk action in a JSONL witness reuse plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonlChunkReuseStep {
    pub action: JsonlChunkReuseAction,
    pub base_index: Option<usize>,
    pub candidate_index: Option<usize>,
    pub start_line: usize,
    pub line_count: usize,
    pub byte_count: u64,
}

/// Conservative work class for one witness chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonlChunkReuseAction {
    ReuseUnchanged,
    RebuildCandidate,
    ReadAdded,
    DropRemoved,
}

/// Build a deterministic witness over exact JSONL bytes read from `reader`.
///
/// Lines are counted by `BufRead::read_until(b'\n')`, so newline bytes are part
/// of the line hash when present and the final non-newline-terminated line is
/// still counted.
pub fn build_jsonl_merkle_witness<R: BufRead>(
    mut reader: R,
    chunk_size_lines: usize,
) -> io::Result<JsonlMerkleWitness> {
    if chunk_size_lines == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "chunk_size_lines must be greater than zero",
        ));
    }

    let mut chunks = Vec::new();
    let mut line_count = 0_usize;
    let mut byte_count = 0_u64;
    let mut chunk_builder = ChunkBuilder::new(0, 0)?;
    let mut line = Vec::new();

    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            break;
        }

        if chunk_builder.line_count == chunk_size_lines {
            chunks.push(chunk_builder.finish());
            chunk_builder = ChunkBuilder::new(chunks.len(), line_count)?;
        }

        line_count = line_count
            .checked_add(1)
            .ok_or_else(|| io_invalid_data("JSONL line count overflowed usize"))?;
        byte_count = checked_add_bytes(byte_count, bytes_read, "JSONL byte count")?;
        chunk_builder.push_line(&line, bytes_read)?;
    }

    if chunk_builder.line_count > 0 {
        chunks.push(chunk_builder.finish());
    }

    let root_hash = compute_root_hash(chunk_size_lines, line_count, byte_count, &chunks)?;

    Ok(JsonlMerkleWitness {
        schema_version: JSONL_WITNESS_SCHEMA_VERSION.to_string(),
        chunk_size_lines,
        line_count,
        byte_count,
        root_hash,
        chunks,
    })
}

/// Build a conservative, candidate-ordered chunk reuse plan.
///
/// Emitting actions appear in candidate JSONL order, so workers may process
/// reusable and rebuild chunks independently while the coordinator emits by
/// `candidate_index`. `DropRemoved` actions are appended after emitting actions
/// because they have no candidate output position.
pub fn plan_jsonl_witness_reuse(
    base: &JsonlMerkleWitness,
    candidate: &JsonlMerkleWitness,
) -> JsonlWitnessReusePlan {
    let comparison = compare_jsonl_merkle_witnesses(base, candidate);
    let witnesses_are_comparable =
        comparison.schema_versions_match && comparison.chunk_size_lines_match;
    let comparable_chunk_count = if witnesses_are_comparable {
        base.chunks.len().min(candidate.chunks.len())
    } else {
        0
    };
    let mut actions = Vec::with_capacity(base.chunks.len().max(candidate.chunks.len()));

    if witnesses_are_comparable {
        for (index, (base_chunk, candidate_chunk)) in
            base.chunks.iter().zip(&candidate.chunks).enumerate()
        {
            let action = if chunks_match_for_reuse(base_chunk, candidate_chunk) {
                JsonlChunkReuseAction::ReuseUnchanged
            } else {
                JsonlChunkReuseAction::RebuildCandidate
            };
            actions.push(candidate_chunk_step(
                action,
                Some(index),
                index,
                candidate_chunk,
            ));
        }
    }

    for (offset, chunk) in candidate
        .chunks
        .get(comparable_chunk_count..)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        actions.push(candidate_chunk_step(
            if witnesses_are_comparable {
                JsonlChunkReuseAction::ReadAdded
            } else {
                JsonlChunkReuseAction::RebuildCandidate
            },
            None,
            comparable_chunk_count + offset,
            chunk,
        ));
    }

    for (offset, chunk) in base
        .chunks
        .get(comparable_chunk_count..)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        actions.push(base_chunk_step(
            JsonlChunkReuseAction::DropRemoved,
            comparable_chunk_count + offset,
            chunk,
        ));
    }

    JsonlWitnessReusePlan {
        comparison,
        actions,
    }
}

/// Compare two JSONL witnesses and conservatively identify reusable chunks.
///
/// Chunks are considered reusable only when schema, chunk sizing, chunk index,
/// line range, byte count, and chunk hash all match. This intentionally treats
/// shifted chunks as changed so an incremental sync planner cannot hide drift
/// behind a coincidental hash match.
pub fn compare_jsonl_merkle_witnesses(
    base: &JsonlMerkleWitness,
    candidate: &JsonlMerkleWitness,
) -> JsonlWitnessComparison {
    let schema_versions_match = base.schema_version == candidate.schema_version;
    let chunk_size_lines_match = base.chunk_size_lines == candidate.chunk_size_lines;
    let root_hashes_match = base.root_hash == candidate.root_hash;
    let witnesses_are_comparable = schema_versions_match && chunk_size_lines_match;

    let mut unchanged_chunks = 0;
    let mut changed_chunks = 0;
    let mut unchanged_byte_count = 0;
    let mut changed_base_byte_count = 0;
    let mut changed_candidate_byte_count = 0;
    let mut safe_reuse_prefix_chunks = 0;
    let mut first_changed_chunk_index = None;

    let comparable_chunk_count = if witnesses_are_comparable {
        let comparable_chunk_count = base.chunks.len().min(candidate.chunks.len());
        let mut prefix_is_reusable = true;

        for (index, (base_chunk, candidate_chunk)) in
            base.chunks.iter().zip(&candidate.chunks).enumerate()
        {
            if chunks_match_for_reuse(base_chunk, candidate_chunk) {
                unchanged_chunks += 1;
                unchanged_byte_count += candidate_chunk.byte_count;
                if prefix_is_reusable {
                    safe_reuse_prefix_chunks += 1;
                }
            } else {
                changed_chunks += 1;
                changed_base_byte_count += base_chunk.byte_count;
                changed_candidate_byte_count += candidate_chunk.byte_count;
                prefix_is_reusable = false;
                first_changed_chunk_index.get_or_insert(index);
            }
        }

        comparable_chunk_count
    } else {
        0
    };

    let (added_chunks, removed_chunks, added_byte_count, removed_byte_count) =
        if witnesses_are_comparable {
            (
                candidate.chunks.len().saturating_sub(base.chunks.len()),
                base.chunks.len().saturating_sub(candidate.chunks.len()),
                sum_chunk_bytes(
                    candidate
                        .chunks
                        .get(comparable_chunk_count..)
                        .unwrap_or(&[]),
                ),
                sum_chunk_bytes(base.chunks.get(comparable_chunk_count..).unwrap_or(&[])),
            )
        } else {
            (
                candidate.chunks.len(),
                base.chunks.len(),
                sum_chunk_bytes(&candidate.chunks),
                sum_chunk_bytes(&base.chunks),
            )
        };

    if first_changed_chunk_index.is_none()
        && (!witnesses_are_comparable || added_chunks > 0 || removed_chunks > 0)
    {
        first_changed_chunk_index = Some(comparable_chunk_count);
    }

    JsonlWitnessComparison {
        schema_versions_match,
        chunk_size_lines_match,
        root_hashes_match,
        drift_detected: !(witnesses_are_comparable && root_hashes_match),
        base_line_count: base.line_count,
        candidate_line_count: candidate.line_count,
        base_byte_count: base.byte_count,
        candidate_byte_count: candidate.byte_count,
        base_chunk_count: base.chunks.len(),
        candidate_chunk_count: candidate.chunks.len(),
        comparable_chunk_count,
        unchanged_chunks,
        changed_chunks,
        added_chunks,
        removed_chunks,
        unchanged_byte_count,
        changed_base_byte_count,
        changed_candidate_byte_count,
        added_byte_count,
        removed_byte_count,
        safe_reuse_prefix_chunks,
        first_changed_chunk_index,
    }
}

fn chunks_match_for_reuse(base: &JsonlChunkWitness, candidate: &JsonlChunkWitness) -> bool {
    base.index == candidate.index
        && base.start_line == candidate.start_line
        && base.line_count == candidate.line_count
        && base.byte_count == candidate.byte_count
        && base.hash == candidate.hash
}

fn candidate_chunk_step(
    action: JsonlChunkReuseAction,
    base_index: Option<usize>,
    candidate_index: usize,
    chunk: &JsonlChunkWitness,
) -> JsonlChunkReuseStep {
    JsonlChunkReuseStep {
        action,
        base_index,
        candidate_index: Some(candidate_index),
        start_line: chunk.start_line,
        line_count: chunk.line_count,
        byte_count: chunk.byte_count,
    }
}

fn base_chunk_step(
    action: JsonlChunkReuseAction,
    base_index: usize,
    chunk: &JsonlChunkWitness,
) -> JsonlChunkReuseStep {
    JsonlChunkReuseStep {
        action,
        base_index: Some(base_index),
        candidate_index: None,
        start_line: chunk.start_line,
        line_count: chunk.line_count,
        byte_count: chunk.byte_count,
    }
}

fn sum_chunk_bytes(chunks: &[JsonlChunkWitness]) -> u64 {
    chunks.iter().map(|chunk| chunk.byte_count).sum()
}

struct ChunkBuilder {
    index: usize,
    start_line: usize,
    line_count: usize,
    byte_count: u64,
    hasher: Sha256,
    first_line_hash: Option<String>,
    last_line_hash: Option<String>,
}

impl ChunkBuilder {
    fn new(index: usize, start_line: usize) -> io::Result<Self> {
        let mut hasher = Sha256::new();
        hasher.update(CHUNK_DOMAIN);
        hasher.update(index_to_bytes(index)?);
        hasher.update(index_to_bytes(start_line)?);

        Ok(Self {
            index,
            start_line,
            line_count: 0,
            byte_count: 0,
            hasher,
            first_line_hash: None,
            last_line_hash: None,
        })
    }

    fn push_line(&mut self, line: &[u8], bytes_read: usize) -> io::Result<()> {
        let line_hash = sha256_hex(line);

        if self.first_line_hash.is_none() {
            self.first_line_hash = Some(line_hash.clone());
        }
        self.last_line_hash = Some(line_hash);

        self.hasher.update(index_to_bytes(self.line_count)?);
        self.hasher.update(length_to_bytes(bytes_read)?);
        self.hasher.update(line);
        self.line_count = self
            .line_count
            .checked_add(1)
            .ok_or_else(|| io_invalid_data("chunk line count overflowed usize"))?;
        self.byte_count = checked_add_bytes(self.byte_count, bytes_read, "chunk byte count")?;
        Ok(())
    }

    fn finish(self) -> JsonlChunkWitness {
        let hash = hex_encode(&self.hasher.finalize());

        JsonlChunkWitness {
            index: self.index,
            start_line: self.start_line,
            line_count: self.line_count,
            byte_count: self.byte_count,
            hash,
            first_line_hash: self.first_line_hash,
            last_line_hash: self.last_line_hash,
        }
    }
}

fn compute_root_hash(
    chunk_size_lines: usize,
    line_count: usize,
    byte_count: u64,
    chunks: &[JsonlChunkWitness],
) -> io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(ROOT_DOMAIN);
    hash_field(&mut hasher, JSONL_WITNESS_SCHEMA_VERSION.as_bytes())?;
    hasher.update(index_to_bytes(chunk_size_lines)?);
    hasher.update(index_to_bytes(line_count)?);
    hasher.update(byte_count.to_le_bytes());
    hasher.update(index_to_bytes(chunks.len())?);

    for chunk in chunks {
        hasher.update(index_to_bytes(chunk.index)?);
        hasher.update(index_to_bytes(chunk.start_line)?);
        hasher.update(index_to_bytes(chunk.line_count)?);
        hasher.update(chunk.byte_count.to_le_bytes());
        hash_field(&mut hasher, chunk.hash.as_bytes())?;
        hash_optional_field(&mut hasher, chunk.first_line_hash.as_deref())?;
        hash_optional_field(&mut hasher, chunk.last_line_hash.as_deref())?;
    }

    Ok(hex_encode(&hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) -> io::Result<()> {
    hasher.update(length_to_bytes(bytes.len())?);
    hasher.update(FIELD_SEPARATOR);
    hasher.update(bytes);
    hasher.update(FIELD_SEPARATOR);
    Ok(())
}

fn hash_optional_field(hasher: &mut Sha256, value: Option<&str>) -> io::Result<()> {
    match value {
        Some(value) => {
            hasher.update([1]);
            hash_field(hasher, value.as_bytes())?;
        }
        None => hasher.update([0]),
    }
    Ok(())
}

fn checked_add_bytes(total: u64, delta: usize, label: &'static str) -> io::Result<u64> {
    let delta = length_to_u64(delta)?;
    total
        .checked_add(delta)
        .ok_or_else(|| io_invalid_data(format!("{label} overflowed u64")))
}

fn index_to_bytes(value: usize) -> io::Result<[u8; 8]> {
    length_to_bytes(value)
}

fn length_to_bytes(value: usize) -> io::Result<[u8; 8]> {
    Ok(length_to_u64(value)?.to_le_bytes())
}

fn length_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| io_invalid_data("length exceeded u64"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn io_invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn witness(input: &[u8], chunk_size_lines: usize) -> JsonlMerkleWitness {
        build_jsonl_merkle_witness(Cursor::new(input), chunk_size_lines).unwrap()
    }

    #[test]
    fn builds_deterministic_chunk_witnesses() {
        let input = b"{\"id\":\"a\"}\n{\"id\":\"b\"}\n{\"id\":\"c\"}\n";

        let first = build_jsonl_merkle_witness(Cursor::new(input), 2).unwrap();
        let second = build_jsonl_merkle_witness(Cursor::new(input), 2).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.schema_version, JSONL_WITNESS_SCHEMA_VERSION);
        assert_eq!(first.chunk_size_lines, 2);
        assert_eq!(first.line_count, 3);
        assert_eq!(first.byte_count, u64::try_from(input.len()).unwrap());
        assert_eq!(first.chunks.len(), 2);
        assert_eq!(first.chunks[0].index, 0);
        assert_eq!(first.chunks[0].start_line, 0);
        assert_eq!(first.chunks[0].line_count, 2);
        assert_eq!(first.chunks[1].index, 1);
        assert_eq!(first.chunks[1].start_line, 2);
        assert_eq!(first.chunks[1].line_count, 1);
        assert_eq!(
            first.chunks[0].first_line_hash.as_deref(),
            Some(sha256_hex(b"{\"id\":\"a\"}\n").as_str())
        );
        assert_eq!(
            first.chunks[1].last_line_hash.as_deref(),
            Some(sha256_hex(b"{\"id\":\"c\"}\n").as_str())
        );
    }

    #[test]
    fn root_changes_when_a_byte_changes() {
        let original = build_jsonl_merkle_witness(Cursor::new(b"{\"id\":\"a\"}\n"), 2).unwrap();
        let changed = build_jsonl_merkle_witness(Cursor::new(b"{\"id\":\"b\"}\n"), 2).unwrap();

        assert_ne!(original.root_hash, changed.root_hash);
        assert_ne!(original.chunks[0].hash, changed.chunks[0].hash);
    }

    #[test]
    fn root_changes_when_line_order_changes() {
        let first = build_jsonl_merkle_witness(Cursor::new(b"a\nb\n"), 2).unwrap();
        let second = build_jsonl_merkle_witness(Cursor::new(b"b\na\n"), 2).unwrap();

        assert_ne!(first.root_hash, second.root_hash);
    }

    #[test]
    fn final_line_without_newline_is_counted_and_hashed() {
        let witness = build_jsonl_merkle_witness(Cursor::new(b"a\nb"), 1).unwrap();

        assert_eq!(witness.line_count, 2);
        assert_eq!(witness.byte_count, 3);
        assert_eq!(witness.chunks.len(), 2);
        assert_eq!(
            witness.chunks[1].first_line_hash.as_deref(),
            Some(sha256_hex(b"b").as_str())
        );
        assert_eq!(
            witness.chunks[1].last_line_hash.as_deref(),
            Some(sha256_hex(b"b").as_str())
        );
    }

    #[test]
    fn zero_chunk_size_is_rejected() {
        let err = build_jsonl_merkle_witness(Cursor::new(b"a\n"), 0).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn empty_input_has_stable_empty_root() {
        let first = build_jsonl_merkle_witness(Cursor::new(Vec::new()), 2).unwrap();
        let second = build_jsonl_merkle_witness(Cursor::new(Vec::new()), 2).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.line_count, 0);
        assert_eq!(first.byte_count, 0);
        assert!(first.chunks.is_empty());
        assert!(!first.root_hash.is_empty());
    }

    #[test]
    fn comparison_reports_identical_witnesses_as_reusable() {
        let base = witness(b"a\nb\nc\n", 2);
        let candidate = witness(b"a\nb\nc\n", 2);

        let comparison = compare_jsonl_merkle_witnesses(&base, &candidate);

        assert!(comparison.schema_versions_match);
        assert!(comparison.chunk_size_lines_match);
        assert!(comparison.root_hashes_match);
        assert!(!comparison.drift_detected);
        assert_eq!(comparison.comparable_chunk_count, 2);
        assert_eq!(comparison.unchanged_chunks, 2);
        assert_eq!(comparison.changed_chunks, 0);
        assert_eq!(comparison.added_chunks, 0);
        assert_eq!(comparison.removed_chunks, 0);
        assert_eq!(comparison.unchanged_byte_count, candidate.byte_count);
        assert_eq!(comparison.safe_reuse_prefix_chunks, 2);
        assert_eq!(comparison.first_changed_chunk_index, None);
    }

    #[test]
    fn comparison_localizes_changed_chunk() {
        let base = witness(b"a\nb\nc\n", 1);
        let candidate = witness(b"a\nB\nc\n", 1);

        let comparison = compare_jsonl_merkle_witnesses(&base, &candidate);

        assert!(comparison.drift_detected);
        assert!(!comparison.root_hashes_match);
        assert_eq!(comparison.comparable_chunk_count, 3);
        assert_eq!(comparison.unchanged_chunks, 2);
        assert_eq!(comparison.changed_chunks, 1);
        assert_eq!(comparison.added_chunks, 0);
        assert_eq!(comparison.removed_chunks, 0);
        assert_eq!(comparison.safe_reuse_prefix_chunks, 1);
        assert_eq!(comparison.first_changed_chunk_index, Some(1));
        assert_eq!(
            comparison.changed_base_byte_count,
            base.chunks[1].byte_count
        );
        assert_eq!(
            comparison.changed_candidate_byte_count,
            candidate.chunks[1].byte_count
        );
    }

    #[test]
    fn comparison_reports_tail_appends_without_marking_prefix_changed() {
        let base = witness(b"a\nb\n", 1);
        let candidate = witness(b"a\nb\nc\n", 1);

        let comparison = compare_jsonl_merkle_witnesses(&base, &candidate);

        assert!(comparison.drift_detected);
        assert_eq!(comparison.comparable_chunk_count, 2);
        assert_eq!(comparison.unchanged_chunks, 2);
        assert_eq!(comparison.changed_chunks, 0);
        assert_eq!(comparison.added_chunks, 1);
        assert_eq!(comparison.removed_chunks, 0);
        assert_eq!(comparison.added_byte_count, candidate.chunks[2].byte_count);
        assert_eq!(comparison.removed_byte_count, 0);
        assert_eq!(comparison.safe_reuse_prefix_chunks, 2);
        assert_eq!(comparison.first_changed_chunk_index, Some(2));
    }

    #[test]
    fn comparison_rejects_incompatible_chunk_sizes_for_reuse() {
        let base = witness(b"a\nb\nc\n", 1);
        let candidate = witness(b"a\nb\nc\n", 2);

        let comparison = compare_jsonl_merkle_witnesses(&base, &candidate);

        assert!(comparison.drift_detected);
        assert!(!comparison.chunk_size_lines_match);
        assert_eq!(comparison.comparable_chunk_count, 0);
        assert_eq!(comparison.unchanged_chunks, 0);
        assert_eq!(comparison.changed_chunks, 0);
        assert_eq!(comparison.added_chunks, candidate.chunks.len());
        assert_eq!(comparison.removed_chunks, base.chunks.len());
        assert_eq!(comparison.safe_reuse_prefix_chunks, 0);
        assert_eq!(comparison.first_changed_chunk_index, Some(0));
    }

    #[test]
    fn reuse_plan_reuses_identical_candidate_chunks_in_order() {
        let base = witness(b"a\nb\nc\n", 1);
        let candidate = witness(b"a\nb\nc\n", 1);

        let plan = plan_jsonl_witness_reuse(&base, &candidate);

        let actions: Vec<_> = plan.actions.iter().map(|step| step.action).collect();
        assert_eq!(
            actions,
            vec![
                JsonlChunkReuseAction::ReuseUnchanged,
                JsonlChunkReuseAction::ReuseUnchanged,
                JsonlChunkReuseAction::ReuseUnchanged,
            ]
        );
        assert!(
            plan.actions
                .iter()
                .enumerate()
                .all(|(index, step)| step.candidate_index == Some(index))
        );
        assert_eq!(plan.comparison.safe_reuse_prefix_chunks, 3);
    }

    #[test]
    fn reuse_plan_separates_changed_and_added_candidate_chunks() {
        let base = witness(b"a\nb\n", 1);
        let candidate = witness(b"a\nB\nc\n", 1);

        let plan = plan_jsonl_witness_reuse(&base, &candidate);

        let actions: Vec<_> = plan.actions.iter().map(|step| step.action).collect();
        assert_eq!(
            actions,
            vec![
                JsonlChunkReuseAction::ReuseUnchanged,
                JsonlChunkReuseAction::RebuildCandidate,
                JsonlChunkReuseAction::ReadAdded,
            ]
        );
        assert_eq!(plan.actions[1].base_index, Some(1));
        assert_eq!(plan.actions[1].candidate_index, Some(1));
        assert_eq!(plan.actions[2].base_index, None);
        assert_eq!(plan.actions[2].candidate_index, Some(2));
    }

    #[test]
    fn reuse_plan_appends_removed_base_chunks_after_candidate_actions() {
        let base = witness(b"a\nb\nc\n", 1);
        let candidate = witness(b"a\n", 1);

        let plan = plan_jsonl_witness_reuse(&base, &candidate);

        let actions: Vec<_> = plan.actions.iter().map(|step| step.action).collect();
        assert_eq!(
            actions,
            vec![
                JsonlChunkReuseAction::ReuseUnchanged,
                JsonlChunkReuseAction::DropRemoved,
                JsonlChunkReuseAction::DropRemoved,
            ]
        );
        assert_eq!(plan.actions[0].candidate_index, Some(0));
        assert_eq!(plan.actions[1].base_index, Some(1));
        assert_eq!(plan.actions[1].candidate_index, None);
        assert_eq!(plan.actions[2].base_index, Some(2));
    }

    #[test]
    fn reuse_plan_rebuilds_all_candidate_chunks_when_chunk_sizes_differ() {
        let base = witness(b"a\nb\nc\n", 1);
        let candidate = witness(b"a\nb\nc\n", 2);

        let plan = plan_jsonl_witness_reuse(&base, &candidate);

        let actions: Vec<_> = plan.actions.iter().map(|step| step.action).collect();
        assert_eq!(
            actions,
            vec![
                JsonlChunkReuseAction::RebuildCandidate,
                JsonlChunkReuseAction::RebuildCandidate,
                JsonlChunkReuseAction::DropRemoved,
                JsonlChunkReuseAction::DropRemoved,
                JsonlChunkReuseAction::DropRemoved,
            ]
        );
        assert!(plan.actions[0].base_index.is_none());
        assert_eq!(plan.actions[0].candidate_index, Some(0));
        assert_eq!(plan.actions[2].base_index, Some(0));
        assert_eq!(plan.comparison.comparable_chunk_count, 0);
    }
}
