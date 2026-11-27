//! Text chat messages for the General and Match channels in retail 104319.

use crate::{
    jam_codec::{Fields, Value},
    lobby_transport::LobbyMessage,
    match_handoff::CHAT_IN,
};

pub(crate) const CHAT_OUT: u32 = 0x28a2_a1cd;
const MAX_TEXT_BYTES: usize = 4096;

pub(crate) enum ChatRequest<'a> {
    Members,
    Post { text: &'a str, flags: u64 },
}

impl<'a> ChatRequest<'a> {
    pub(crate) fn read((crc, id, fields): &'a LobbyMessage) -> Option<(&'a Fields, Self)> {
        if *crc != CHAT_OUT {
            return None;
        }
        let Value::Struct(channel) = fields.get(&0x78)? else {
            return None;
        };
        let request = match id {
            21701 => Self::Members,
            21700 => {
                let Value::String(text) = fields.get(&0x90)? else {
                    return None;
                };
                if text.len() > MAX_TEXT_BYTES || text.trim().is_empty() {
                    return None;
                }
                let Some(Value::Unsigned(flags)) = fields.get(&0xb8) else {
                    return None;
                };
                Self::Post {
                    text,
                    flags: *flags,
                }
            }
            _ => return None,
        };
        Some((channel, request))
    }
}

pub(crate) fn member(record: Fields) -> Value {
    Value::Struct(Fields::from([
        (0, Value::Struct(record)),
        (0x68, Value::Unsigned(0)),
    ]))
}

pub(crate) fn members(channel: Fields, members: Vec<Value>) -> LobbyMessage {
    (
        CHAT_IN,
        20401,
        Fields::from([
            (0x78, Value::Struct(channel)),
            (0x90, Value::Array(members)),
        ]),
    )
}

pub(crate) fn post(channel: Fields, sender: Value, text: &str, flags: u64) -> LobbyMessage {
    (
        CHAT_IN,
        20400,
        Fields::from([
            (0x78, Value::Struct(channel)),
            (0x90, sender),
            (0x100, Value::String(text.into())),
            (0x128, Value::Unsigned(flags)),
        ]),
    )
}
