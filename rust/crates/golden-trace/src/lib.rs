//! Streaming comparison for the bounded Soldier range gameplay trace profile.
//! Only validated semantic field values are retained for optional local diagnostics.
mod reader;
mod schema;
mod state;

use std::fmt;
use std::io::BufRead;

use reader::Stream;
use schema::HEADER;
use serde_json::Value;

type Invalid<T> = Result<T, &'static str>;

#[derive(Debug, PartialEq)]
pub struct Difference {
    pub tick: u32,
    pub phase: &'static str,
    pub entity: Option<String>,
    pub field: String,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
}

#[derive(Debug)]
pub struct TraceError {
    side: &'static str,
    line: u32,
    reason: &'static str,
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} line {}: {}", self.side, self.line, self.reason)
    }
}

impl std::error::Error for TraceError {}

fn require(ok: bool, reason: &'static str) -> Invalid<()> {
    if ok { Ok(()) } else { Err(reason) }
}

#[derive(Debug)]
struct Field {
    entity: String,
    name: &'static str,
    value: Value,
}

type EffectGroups = Vec<Vec<Vec<Value>>>;

struct Frame {
    tick: u32,
    fields: Vec<Field>,
    effects: EffectGroups,
}

fn difference(
    tick: u32,
    phase: &'static str,
    entity: Option<&str>,
    field: String,
    expected: Option<&Value>,
    actual: Option<&Value>,
) -> Difference {
    Difference {
        tick,
        phase,
        entity: entity.map(str::to_owned),
        field,
        expected: expected.cloned(),
        actual: actual.cloned(),
    }
}

fn effects_difference(tick: u32, left: &EffectGroups, right: &EffectGroups) -> Option<Difference> {
    for group in 0..left.len().max(right.len()) {
        let a = left.get(group).map(Vec::as_slice).unwrap_or_default();
        let b = right.get(group).map(Vec::as_slice).unwrap_or_default();
        for event in 0..a.len().max(b.len()) {
            let x = a.get(event);
            let y = b.get(event);
            let entity = x.or(y).and_then(|e| e[2].as_str());
            for (index, name) in ["recipient", "kind", "entity", "value"]
                .into_iter()
                .enumerate()
            {
                let expected = x.map(|e| &e[index]);
                let actual = y.map(|e| &e[index]);
                if expected != actual {
                    return Some(difference(
                        tick,
                        "effects",
                        entity,
                        format!("groups[{group}][{event}].{name}"),
                        expected,
                        actual,
                    ));
                }
            }
        }
    }
    None
}

fn first_difference(left: &Frame, right: &Frame) -> Option<Difference> {
    let accepted = |f: &Field| f.name == "accepted_frame";
    if let (Some(a), Some(b)) = (
        left.fields.iter().find(|f| accepted(f)),
        right.fields.iter().find(|f| accepted(f)),
    ) {
        if a.entity == b.entity && a.value != b.value {
            return Some(difference(
                left.tick,
                "acceptance",
                Some(&a.entity),
                a.name.to_owned(),
                Some(&a.value),
                Some(&b.value),
            ));
        }
    }
    // A changed slot identity is sorted before all of the optional fields it controls.
    for (a, b) in left.fields.iter().zip(&right.fields) {
        if a.value != b.value {
            return Some(difference(
                left.tick,
                "state",
                Some(&a.entity),
                a.name.to_owned(),
                Some(&a.value),
                Some(&b.value),
            ));
        }
    }
    effects_difference(left.tick, &left.effects, &right.effects)
}

/// Validate both complete traces (including all checksums) before exposing the first semantic difference.
pub fn compare<R: BufRead, S: BufRead>(
    reference: R,
    candidate: S,
) -> Result<Option<Difference>, TraceError> {
    let mut left = Stream::new(reference, "reference")?;
    let mut right = Stream::new(candidate, "candidate")?;
    for key in HEADER.iter().copied().chain(["setup"]) {
        if !["role", "source_rev", "adapter_rev", "toolchain"].contains(&key)
            && left.header[key] != right.header[key]
        {
            return Err(TraceError {
                side: "comparison",
                line: 1,
                reason: "incompatible trace headers",
            });
        }
    }
    let mut first = None;
    loop {
        match (left.next()?, right.next()?) {
            (Some(a), Some(b)) => {
                if first.is_none() {
                    first = first_difference(&a, &b);
                }
            }
            (None, None) => return Ok(first),
            _ => {
                return Err(TraceError {
                    side: "comparison",
                    line: left.line,
                    reason: "different tick counts",
                });
            }
        }
    }
}

/// Validate a standalone Python recording through its footer and scenario coverage.
pub fn validate_reference<R: BufRead>(source: R) -> Result<(), TraceError> {
    let mut reference = Stream::new(source, "reference")?;
    while reference.next()?.is_some() {}
    Ok(())
}
