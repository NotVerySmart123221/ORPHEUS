//! Runtime catalog conventions shared by the packer and the server.

use serde_json::Value;

mod rulesets;

pub use rulesets::{ModeRulesets, RuleValue};

pub const MAX_GRAPH_BYTES: u64 = 128 * 1024 * 1024;
pub const TARGET_TAG_TYPE: u64 = 0x0d60;

pub fn parse_guid(text: &str) -> Option<u64> {
    let text = trim_python_whitespace(text);
    let text = text.strip_prefix('+').unwrap_or(text);
    let (digits, radix) = match text.as_bytes() {
        [b'0', b'x' | b'X', ..] => (&text[2..], 16),
        [b'0', b'o' | b'O', ..] => (&text[2..], 8),
        [b'0', b'b' | b'B', ..] => (&text[2..], 2),
        _ => (text, 10),
    };
    let digits = if radix == 10 {
        digits
    } else {
        digits.strip_prefix('_').unwrap_or(digits)
    };
    if digits.starts_with('+') || digits.split('_').any(str::is_empty) {
        return None;
    }
    let digits = digits.replace('_', "");
    if radix == 10 && digits.starts_with('0') && digits.bytes().any(|byte| byte != b'0') {
        return None;
    }
    u64::from_str_radix(&digits, radix).ok()
}

/// Trim the same whitespace characters as the Python catalog and profile reader.
pub fn trim_python_whitespace(text: &str) -> &str {
    text.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{001c}'..='\u{001f}')
    })
}

/// Parse the canonical GUID spelling used by graph assets.
pub fn asset_guid(value: &Value) -> Option<u64> {
    let text = value.as_str()?;
    let hex = text.strip_prefix("0x")?;
    if hex.len() == 16
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'A'..=b'F'))
    {
        parse_guid(text)
    } else {
        None
    }
}
