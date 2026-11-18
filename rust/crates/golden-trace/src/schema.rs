//! Fixed header, logical body generations, and decoded commands for the Soldier range profile.
use serde_json::Value;

use crate::{Invalid, require};

pub(super) const SLOTS: [&str; 2] = ["bot/0", "player/0"];
pub(super) const HEADER: &[&str] = &[
    "kind",
    "schema",
    "profile",
    "build",
    "role",
    "source_rev",
    "reference_rev",
    "adapter_rev",
    "runtime_sha256",
    "profile_sha256",
    "platform",
    "toolchain",
    "scenario",
    "seed",
    "quanta_us",
    "schedule_sha256",
];

pub(super) fn exact_fields(row: &Value, names: &[&str]) -> bool {
    row.as_object().is_some_and(|obj| {
        obj.len() == names.len() && names.iter().all(|key| obj.contains_key(*key))
    })
}

fn is_hex(v: &Value, width: usize) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == width
            && s.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}

fn token(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        (1..=80).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
    })
}

pub(super) fn uint(v: &Value, max: u64) -> bool {
    v.as_u64().is_some_and(|i| i <= max)
}

pub(super) fn sint(v: &Value, min: i64, max: i64) -> bool {
    v.as_i64().is_some_and(|i| (min..=max).contains(&i))
}

pub(super) fn finite_bits(v: &Value) -> bool {
    is_hex(v, 16)
        && v.as_str()
            .and_then(|s| u64::from_str_radix(s, 16).ok())
            .is_some_and(|bits| f64::from_bits(bits).is_finite())
}

pub(super) fn items(v: &Value, n: usize, predicate: impl Fn(&Value) -> bool) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() == n && a.iter().all(predicate))
}

fn captured_setup(s: &Value) -> bool {
    let spawn = &s["bot_spawner"];
    let mut fields = vec![
        "bot_spawner",
        "bot_spawner_slot",
        "bot_roster_size",
        "initial_room_tick",
        "first_scheduled_room_tick",
        "setup_delivery_ticks",
        "initial_owner_delivery_phase",
        "host_random_model",
        "host_initial_seed",
        "player_vm_random_model",
        "player_vm_initial_seed",
    ];
    if s["initial_owner_delivery_phase"] == "after-possession-ack" {
        fields.extend([
            "startup_acks",
            "setup_elapsed_us",
            "owner_component_start_frame",
        ]);
        if !confirmed_startup(s) {
            return false;
        }
    }
    exact_fields(s, &fields)
        && exact_fields(spawn, &["team", "hero", "position_bits", "yaw"])
        && uint(&spawn["team"], u8::MAX.into())
        && is_hex(&spawn["hero"], 16)
        && items(&spawn["position_bits"], 3, finite_bits)
        && sint(&spawn["yaw"], i32::MIN.into(), i32::MAX.into())
        && [
            "bot_spawner_slot",
            "bot_roster_size",
            "initial_room_tick",
            "first_scheduled_room_tick",
            "setup_delivery_ticks",
        ]
        .iter()
        .all(|k| uint(&s[k], u32::MAX.into()))
        && s["bot_spawner_slot"].as_u64() < s["bot_roster_size"].as_u64()
        && matches!(
            s["initial_owner_delivery_phase"].as_str(),
            Some("before-first-send" | "after-setup-ack" | "after-possession-ack")
        )
        && token(&s["host_random_model"])
        && token(&s["player_vm_random_model"])
        && (if s["host_random_model"] == "os-entropy-transport-only" {
            s["host_initial_seed"].is_null()
        } else {
            s["host_initial_seed"].as_u64().is_some()
        })
        && s["player_vm_initial_seed"].as_u64().is_some()
}

fn confirmed_startup(s: &Value) -> bool {
    let (Some(start), Some(first), Some(ticks), Some(component), Some(acks)) = (
        s["initial_room_tick"].as_u64(),
        s["first_scheduled_room_tick"].as_u64(),
        s["setup_delivery_ticks"].as_u64(),
        s["owner_component_start_frame"].as_u64(),
        s["startup_acks"].as_object(),
    ) else {
        return false;
    };
    let Some(cut) = first.checked_sub(1) else {
        return false;
    };
    let required = ["bot_create", "owner_create", "owner_possession"];
    start.checked_add(ticks) == Some(cut)
        && uint(&s["setup_elapsed_us"], u64::MAX)
        && (start..=cut).contains(&component)
        && required.iter().all(|name| acks.contains_key(*name))
        && acks["owner_create"]
            .as_u64()
            .is_some_and(|t| component <= t)
        && required
            .iter()
            .any(|name| acks[*name].as_u64() == Some(cut))
        && acks.iter().all(|(name, value)| {
            (required.contains(&name.as_str())
                || matches!(name.as_str(), "owner_full_tree" | "owner_first_delta"))
                && value.as_u64().is_some_and(|t| (start..=cut).contains(&t))
        })
}

pub(super) fn check_header(h: &Value, role: &str) -> Invalid<()> {
    let fields_valid = match h["schema"].as_str() {
        Some("orpheus.golden.v2") => {
            let names: Vec<_> = HEADER
                .iter()
                .copied()
                .filter(|k| *k != "seed")
                .chain(["setup"])
                .collect();
            exact_fields(h, &names) && captured_setup(&h["setup"])
        }
        Some("orpheus.golden.v1") => {
            let fields = if let Some(setup) = h.get("setup") {
                exact_fields(
                    setup,
                    &["bot_spawner_slot", "initial_room_tick", "random_model"],
                ) && uint(&setup["bot_spawner_slot"], u32::MAX.into())
                    && uint(&setup["initial_room_tick"], u32::MAX.into())
                    && token(&setup["random_model"])
                    && h.as_object().is_some_and(|m| {
                        m.len() == HEADER.len() + 1 && HEADER.iter().all(|k| m.contains_key(*k))
                    })
            } else {
                exact_fields(h, HEADER)
            };
            fields && uint(&h["seed"], u32::MAX.into())
        }
        _ => false,
    };
    require(
        fields_valid
            && h["kind"] == "header"
            && h["role"] == role
            && h["profile"] == "soldier-range-v1"
            && h["build"] == 104319
            && h["scenario"] == "soldier-range",
        "incorrect header or profile",
    )?;
    require(
        ["reference_rev", "source_rev", "adapter_rev"]
            .iter()
            .all(|k| is_hex(&h[k], 40))
            && ["runtime_sha256", "profile_sha256", "schedule_sha256"]
                .iter()
                .all(|k| is_hex(&h[k], 64)),
        "invalid provenance hash",
    )?;
    require(
        role != "reference" || h["source_rev"] == h["reference_rev"],
        "unbound reference source",
    )?;
    require(
        token(&h["platform"])
            && token(&h["toolchain"])
            && uint(&h["quanta_us"], u32::MAX.into())
            && h["quanta_us"] != 0,
        "invalid model settings",
    )
}

pub(super) fn identity(key: &str) -> Option<(usize, u32)> {
    let (slot, suffix) = key.split_once("/body/")?;
    let index = SLOTS.iter().position(|s| *s == slot)?;
    let generation = suffix.parse::<u32>().ok()?;
    (generation != 0 && generation.to_string() == suffix).then_some((index, generation))
}

#[derive(Default, Copy, Clone)]
pub(super) struct Slot {
    pub(super) generation: u32,
    present: bool,
}

impl Slot {
    pub(super) fn set(&mut self, next: Option<u32>) -> Invalid<()> {
        if let Some(generation) = next {
            if !self.present || generation != self.generation {
                require(
                    self.generation.checked_add(1) == Some(generation),
                    "missing or reused body generation",
                )?;
            }
            self.generation = generation;
        }
        self.present = next.is_some();
        Ok(())
    }

    fn contains(self, generation: u32) -> bool {
        self.present && self.generation == generation
    }
}

pub(super) fn check_inputs(
    inputs: &Value,
    reference_player: Option<(Slot, Slot)>,
    last: &mut Option<(u32, u32)>,
) -> Invalid<()> {
    let inputs = inputs.as_array().ok_or("invalid input list")?;
    require(inputs.len() <= 1, "too many inputs per tick")?;
    let Some(c) = inputs.first() else {
        return Ok(());
    };
    require(
        exact_fields(
            c,
            &[
                "entity", "frame", "forward", "right", "yaw", "pitch", "buttons", "action",
                "extra", "click",
            ],
        ),
        "invalid command fields",
    )?;
    let (slot, generation) = c["entity"]
        .as_str()
        .and_then(identity)
        .ok_or("invalid command target")?;
    require(
        slot == 1
            && reference_player.is_none_or(|(before, after)| {
                before.contains(generation) || after.contains(generation)
            }),
        "command targets absent player",
    )?;
    let frame = c["frame"]
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .ok_or("invalid command frame")?;
    if let Some((previous, n)) = *last {
        require(
            generation > previous || (generation == previous && n.wrapping_add(1) == frame),
            "noncontiguous command frames",
        )?;
    }
    *last = Some((generation, frame));
    require(
        ["forward", "right"]
            .iter()
            .all(|k| sint(&c[k], i8::MIN.into(), i8::MAX.into()))
            && ["yaw", "pitch"]
                .iter()
                .all(|k| sint(&c[k], i32::MIN.into(), i32::MAX.into()))
            && uint(&c["action"], u8::MAX.into())
            && uint(&c["extra"], u16::MAX.into()),
        "invalid command value",
    )?;
    // Both decoders read 24 button bits. SUBFRAME alone determines whether a click is present.
    let buttons = c["buttons"]
        .as_u64()
        .filter(|&mask| mask <= 0x00ff_ffff)
        .ok_or("invalid command buttons")?;
    if buttons & 0x10000 == 0 {
        return require(c["click"].is_null(), "click sample without subframe flag");
    }
    require(
        c["click"].as_array().is_some_and(|click| {
            click.len() == 3
                && uint(&click[0], u8::MAX.into())
                && click[1..]
                    .iter()
                    .all(|v| sint(v, i16::MIN.into(), i16::MAX.into()))
        }),
        "invalid click sample",
    )
}
