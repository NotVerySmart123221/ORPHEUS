//! Compose local metadata into the selected graph bundle.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufReader, Read, Write},
    path::Path,
};

use flate2::{Compression, bufread::MultiGzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, value::RawValue};

use crate::{
    RuntimeError, at,
    format::{MAX_GRAPH_BYTES, ModeRulesets},
    target_tags,
};

#[derive(Deserialize)]
struct BundleProfiles {
    version: u64,
    #[serde(default)]
    mode_rulesets: Vec<Box<RawValue>>,
}

pub(super) fn rewrite(
    input: &Path,
    target_tags_xml: Option<&Path>,
    mode_rulesets: Option<&Path>,
) -> Result<Option<Vec<u8>>, RuntimeError> {
    if target_tags_xml.is_none() && mode_rulesets.is_none() {
        return Ok(None);
    }
    let decoder = MultiGzDecoder::new(BufReader::new(at(input, File::open(input))?));
    let bundle_bytes = read_limited(input, decoder)?;
    let mut profiles: BundleProfiles = at(input, decode(&bundle_bytes))?;
    if profiles.version != 1 {
        return at(
            input,
            Err(invalid("unsupported state graph bundle version")),
        );
    }

    if mode_rulesets.is_none() && profiles.mode_rulesets.is_empty() {
        let mut bundle: Value = at(input, decode(&bundle_bytes))?;
        drop(bundle_bytes);
        if let Some(xml) = target_tags_xml {
            target_tags::apply(input, xml, &mut bundle)?;
        }
        return encode(input, &bundle).map(Some);
    }

    let mut bundle: BTreeMap<String, Box<RawValue>> = at(input, decode(&bundle_bytes))?;
    drop(bundle_bytes);
    if let Some(rules_path) = mode_rulesets {
        let local_bytes = read_limited(rules_path, at(rules_path, File::open(rules_path))?)?;
        let local_profiles: Vec<Box<RawValue>> = at(rules_path, decode(&local_bytes))?;
        profiles.mode_rulesets.extend(local_profiles);
        let rules = at(
            rules_path,
            serde_json::value::to_raw_value(&profiles.mode_rulesets).map_err(invalid),
        )?;
        // Check the composed table with the same parser as GraphStore.
        at(
            rules_path,
            serde_json::from_str::<ModeRulesets>(rules.get()).map_err(invalid),
        )?;
        bundle.insert("mode_rulesets".into(), rules);
    }
    // Keep the rule tokens intact when importing XML into an already-authored pack.
    if let Some(xml) = target_tags_xml {
        apply_tags(input, xml, &mut bundle)?;
    }
    encode(input, &bundle).map(Some)
}

fn apply_tags(
    input: &Path,
    xml: &Path,
    bundle: &mut BTreeMap<String, Box<RawValue>>,
) -> Result<(), RuntimeError> {
    let mut fields = Map::new();
    for key in ["bodies", "entities", "junkenstein"] {
        if let Some(raw) = bundle.get(key) {
            fields.insert(key.to_owned(), at(input, decode(raw.get().as_bytes()))?);
        }
    }
    let mut sections = Value::Object(fields);
    target_tags::apply(input, xml, &mut sections)?;
    if let Value::Object(fields) = sections {
        for (key, value) in fields {
            let raw = at(
                input,
                serde_json::value::to_raw_value(&value).map_err(invalid),
            )?;
            bundle.insert(key, raw);
        }
    }
    Ok(())
}

fn read_limited(path: &Path, reader: impl Read) -> Result<Vec<u8>, RuntimeError> {
    let mut bytes = Vec::new();
    at(
        path,
        reader.take(MAX_GRAPH_BYTES + 1).read_to_end(&mut bytes),
    )?;
    check_size(path, &bytes)?;
    Ok(bytes)
}

fn check_size(path: &Path, bytes: &[u8]) -> Result<(), RuntimeError> {
    if bytes.len() as u64 > MAX_GRAPH_BYTES {
        return at(path, Err(invalid("graph metadata exceeds the size limit")));
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(invalid)
}

fn encode(path: &Path, bundle: &impl Serialize) -> Result<Vec<u8>, RuntimeError> {
    let bytes = at(path, serde_json::to_vec(bundle).map_err(invalid))?;
    check_size(path, &bytes)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    at(path, encoder.write_all(&bytes))?;
    at(path, encoder.finish())
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
