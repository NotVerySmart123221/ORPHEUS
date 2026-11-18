//! Required slot/body state and ordered gameplay delivery groups.
use serde_json::Value;

use crate::schema::{SLOTS, Slot, finite_bits, identity, items, sint, uint};
use crate::{EffectGroups, Field, Invalid, require};

const BOT_FIELDS: &[&str] = &["alive", "health_bits", "position_bits"];
const PLAYER_FIELDS: &[&str] = &[
    "accepted_frame",
    "aim",
    "alive",
    "ammo",
    "health_bits",
    "position_bits",
    "reloading",
];

fn field_value(field: &str, v: &Value) -> bool {
    match field {
        "accepted_frame" => v.is_null() || uint(v, u32::MAX.into()),
        "aim" => items(v, 2, |x| sint(x, i32::MIN.into(), i32::MAX.into())),
        "alive" | "reloading" => v.is_boolean(),
        "ammo" => uint(v, u16::MAX.into()),
        "health_bits" => finite_bits(v),
        "position_bits" => items(v, 3, finite_bits),
        _ => false,
    }
}

pub(super) fn read_state(state: &Value, slots: &mut [Slot; 2]) -> Invalid<Vec<Field>> {
    let rows = state.as_array().ok_or("invalid state rows")?;
    let mut fields = Vec::new();
    let mut input = rows.iter();
    // Fixed slots and their fields are already ordered by (entity, field).
    for (i, slot) in SLOTS.iter().enumerate() {
        let row = input
            .next()
            .and_then(Value::as_array)
            .ok_or("missing slot row")?;
        require(
            row.len() == 3 && row[0] == *slot && row[1] == "body",
            "missing or unordered slot row",
        )?;
        fields.push(Field {
            entity: (*slot).to_owned(),
            name: "body",
            value: row[2].clone(),
        });
        if row[2].is_null() {
            slots[i].set(None)?;
            continue;
        }
        let body = row[2].as_str().ok_or("invalid slot identity")?;
        let (index, generation) = identity(body).ok_or("invalid body identity")?;
        require(i == index, "body belongs to another slot")?;
        slots[i].set(Some(generation))?;
        for &name in if i == 0 { BOT_FIELDS } else { PLAYER_FIELDS } {
            let row = input
                .next()
                .and_then(Value::as_array)
                .ok_or("missing body field")?;
            require(
                row.len() == 3 && row[0] == body && row[1] == name && field_value(name, &row[2]),
                "invalid or unordered body field",
            )?;
            fields.push(Field {
                entity: body.to_owned(),
                name,
                value: row[2].clone(),
            });
        }
    }
    require(input.next().is_none(), "extra state rows")?;
    Ok(fields)
}

pub(super) fn read_effects(effects: &Value, slots: [Slot; 2]) -> Invalid<EffectGroups> {
    let groups = effects.as_array().ok_or("invalid effect groups")?;
    require(groups.len() <= 64, "too many effect groups")?;
    groups
        .iter()
        .map(|g| {
            let events = g.as_array().ok_or("invalid effect group")?;
            require(
                (1..=64).contains(&events.len()),
                "invalid effect group length",
            )?;
            events
                .iter()
                .map(|v| {
                    let e = v.as_array().ok_or("invalid effect row")?;
                    require(
                        e.len() == 4 && e[0] == "owner",
                        "invalid effect recipient or row",
                    )?;
                    let (index, generation) = e[2]
                        .as_str()
                        .and_then(identity)
                        .ok_or("invalid effect target")?;
                    require(
                        generation <= slots[index].generation,
                        "unobserved effect target",
                    )?;
                    let valid = match e[1].as_str() {
                        Some("create" | "retire") => e[3].is_null(),
                        Some("health") => finite_bits(&e[3]),
                        Some("ammo") => uint(&e[3], u16::MAX.into()),
                        _ => false,
                    };
                    require(valid, "invalid effect kind or value")?;
                    Ok(e.clone())
                })
                .collect()
        })
        .collect()
}
