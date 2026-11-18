//! Bounded canonical JSONL input, per-tick digests, and complete stream checksums.
use std::io::{BufRead, Read};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::schema::{Slot, check_header, check_inputs, exact_fields};
use crate::state::{read_effects, read_state};
use crate::{Frame, Invalid, TraceError, require};

const MAX_LINE: u64 = 256 * 1024;
const MAX_TICKS: u32 = 1_000_000;

fn read_line<R: BufRead>(source: &mut R) -> Invalid<(Value, Vec<u8>)> {
    let mut line = Vec::new();
    source
        .take(MAX_LINE + 1)
        .read_until(b'\n', &mut line)
        .map_err(|_| "read failed")?;
    require(line.len() as u64 <= MAX_LINE, "line too long")?;
    let raw = line
        .strip_suffix(b"\n")
        .ok_or("missing row or LF terminator")?;
    let mut v: Value = serde_json::from_slice(raw).map_err(|_| "invalid JSON row")?;
    v.sort_all_objects();
    // Canonical re-encoding also rejects duplicate object keys, escapes and trailing whitespace.
    require(
        serde_json::to_vec(&v).map_err(|_| "invalid JSON row")? == raw,
        "noncanonical JSON row",
    )?;
    Ok((v, line))
}

fn prefix_bytes(row: &Value, keys: &[&str]) -> Vec<u8> {
    let projected: Map<String, Value> = keys
        .iter()
        .map(|&k| (k.to_owned(), row[k].clone()))
        .collect();
    serde_json::to_vec(&projected).expect("parsed JSON values serialize")
}

pub(super) struct Stream<R> {
    reader: R,
    pub(super) header: Value,
    side: &'static str,
    pub(super) line: u32,
    count: u32,
    slots: [Slot; 2],
    last_command: Option<(u32, u32)>,
    trace_hash: Sha256,
    schedule_hash: Sha256,
}

fn hash_matches(hash: &Sha256, value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| s == format!("{:x}", hash.clone().finalize()))
}

impl<R: BufRead> Stream<R> {
    pub(super) fn new(mut reader: R, side: &'static str) -> Result<Self, TraceError> {
        let error = |reason| TraceError {
            side,
            line: 1,
            reason,
        };
        let (header, bytes) = read_line(&mut reader).map_err(error)?;
        check_header(&header, side).map_err(error)?;
        let mut trace_hash = Sha256::new();
        trace_hash.update(&bytes);
        Ok(Self {
            reader,
            header,
            side,
            line: 1,
            count: 0,
            slots: [Slot::default(); 2],
            last_command: None,
            trace_hash,
            schedule_hash: Sha256::new(),
        })
    }

    pub(super) fn next(&mut self) -> Result<Option<Frame>, TraceError> {
        self.line += 1;
        self.read_next().map_err(|reason| TraceError {
            side: self.side,
            line: self.line,
            reason,
        })
    }

    fn read_next(&mut self) -> Invalid<Option<Frame>> {
        let (row, bytes) = read_line(&mut self.reader)?;
        if row["kind"] == "end" {
            require(
                exact_fields(&row, &["kind", "last_tick", "ticks", "sha256"])
                    && self.count > 0
                    && row["ticks"] == self.count
                    && row["last_tick"] == self.count - 1,
                "incorrect end tick or count",
            )?;
            require(
                hash_matches(&self.trace_hash, &row["sha256"]),
                "file checksum mismatch",
            )?;
            require(
                hash_matches(&self.schedule_hash, &self.header["schedule_sha256"]),
                "schedule checksum mismatch",
            )?;
            require(
                self.reader
                    .fill_buf()
                    .map_err(|_| "read failed")?
                    .is_empty(),
                "trailing data after end",
            )?;
            if self.side == "reference" {
                require(
                    self.slots.iter().all(|slot| slot.generation != 0)
                        && self.last_command.is_some(),
                    "reference missing required scenario coverage",
                )?;
            }
            return Ok(None);
        }
        require(
            exact_fields(
                &row,
                &[
                    "kind",
                    "tick",
                    "elapsed_us",
                    "missed",
                    "inputs",
                    "state",
                    "effects",
                    "digest",
                ],
            ) && row["kind"] == "tick"
                && self.count < MAX_TICKS
                && row["tick"] == self.count,
            "missing, duplicate, unordered or unknown tick",
        )?;
        let quanta = self.header["quanta_us"]
            .as_u64()
            .ok_or("invalid tick quantum")?;
        require(
            row["elapsed_us"] == u64::from(self.count) * quanta && row["missed"] == 0,
            "invalid deterministic clock",
        )?;
        let before = self.slots;
        let fields = read_state(&row["state"], &mut self.slots)?;
        // The candidate receives the offered schedule even if its own player never spawns.
        let reference_player = (self.side == "reference").then_some((before[1], self.slots[1]));
        check_inputs(&row["inputs"], reference_player, &mut self.last_command)?;
        let effects = read_effects(&row["effects"], self.slots)?;
        let digest = Sha256::digest(prefix_bytes(&row, &["effects", "state"]));
        require(
            row["digest"]
                .as_str()
                .is_some_and(|s| s == format!("{digest:x}")),
            "tick digest mismatch",
        )?;
        self.trace_hash.update(bytes);
        self.schedule_hash.update(prefix_bytes(
            &row,
            &["elapsed_us", "inputs", "missed", "tick"],
        ));
        self.schedule_hash.update(b"\n");
        self.count += 1;
        Ok(Some(Frame {
            tick: self.count - 1,
            fields,
            effects,
        }))
    }
}
