//! Deterministic directions for authored weapon spread.

use std::{f64::consts::TAU, sync::OnceLock};

use crate::{
    script_expr::{EvalError, to_float, to_int},
    script_graph::Node,
    script_init::ReportedShot,
    script_wire::Value,
    static_ray::{Vec3, norm},
};
use serde_json::Value as Json;

const TABLE_SIZE: usize = 16_384;
const TABLE_MASK: usize = TABLE_SIZE - 1;

fn uniform_table() -> &'static [u32] {
    static TABLE: OnceLock<Vec<u32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut words = [1u32; 624];
        for index in 1..words.len() {
            let last = words[index - 1];
            words[index] = 1_812_433_253u32
                .wrapping_mul(last ^ (last >> 30))
                .wrapping_add(index as u32);
        }
        if (words[0] ^ (words[3] & 0x13c9_e684)).count_ones() % 2 == 0 {
            words[0] ^= 1;
        }
        let mut state = [0u128; 156];
        for (block, lanes) in state.iter_mut().zip(words.chunks_exact(4)) {
            *block = lanes.iter().enumerate().fold(0, |block, (lane, &word)| {
                block | (u128::from(word) << (lane * 32))
            });
        }
        let masks = [0xdfff_ffef, 0xddfe_cb7f, 0xbffa_ffff, 0xbfff_fff6];
        let mut table = Vec::with_capacity(TABLE_SIZE);
        while table.len() < TABLE_SIZE {
            let [mut previous, mut last] = [state[154], state[155]];
            for index in 0..state.len() {
                let current = state[index];
                let ahead = state[(index + 122) % state.len()];
                let (right, left) =
                    masks
                        .into_iter()
                        .enumerate()
                        .fold((0, 0), |(right, left), (lane, mask)| {
                            let shift = lane * 32;
                            let next_right = ((ahead >> shift) as u32 >> 11) & mask;
                            let next_left = ((last >> shift) as u32) << 18;
                            (
                                right | (u128::from(next_right) << shift),
                                left | (u128::from(next_left) << shift),
                            )
                        });
                let result = current ^ (current << 8) ^ (previous >> 8) ^ right ^ left;
                state[index] = result;
                previous = last;
                last = result;
            }
            let remaining = TABLE_SIZE - table.len();
            table.extend(
                state
                    .iter()
                    .rev()
                    .flat_map(|block| (0..4).rev().map(move |lane| (block >> (lane * 32)) as u32))
                    .take(remaining),
            );
        }
        table
    })
}

fn cross([ax, ay, az]: Vec3, [bx, by, bz]: Vec3) -> Vec3 {
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn spread_index(shot: &crate::script_init::ShotEvent, state: u16, pellet: usize) -> usize {
    let seed = (shot.entity as u16)
        .wrapping_add(shot.instance.wrapping_mul(23))
        .wrapping_add(state.wrapping_mul(7))
        .wrapping_add(shot.activation & 63);
    let key = (usize::from(seed) * 131 + shot.ordinal as usize * 23 + pellet * 3) & TABLE_MASK;
    uniform_table()[key] as usize & TABLE_MASK
}

fn rotate(vector: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    let parallel: f64 = axis.into_iter().zip(vector).map(|(a, v)| a * v).sum();
    let perpendicular = cross(axis, vector);
    std::array::from_fn(|i| {
        vector[i] * cos + perpendicular[i] * sin + axis[i] * parallel * (1.0 - cos)
    })
}

/// The uniform cone uses the volley state slot and a distinct seed per pellet.
fn direct_cone_direction(
    forward: Vec3,
    spread_degrees: f32,
    shot: &crate::script_init::ShotEvent,
    pellet: usize,
) -> Vec3 {
    if spread_degrees == 0.0 {
        return forward;
    }
    let table = uniform_table();
    let index = spread_index(shot, shot.raw_state, pellet);
    let angle = f64::from((table[index] as f32 * std::f32::consts::TAU) * (1.0 / 4_294_967_296.0));
    let radians = spread_degrees * (std::f64::consts::PI / 180.0) as f32;
    let radius = (table[(index + 1) & TABLE_MASK] as f32 * radians) * (1.0 / 4_294_967_296.0);
    let forward = forward.map(|axis| f64::from(axis as f32));
    let right = cross(forward, [0.0, 1.0, 0.0]);
    let squared = right
        .into_iter()
        .map(|axis| f64::from((axis * axis) as f32))
        .sum::<f64>() as f32;
    let right = if squared >= 1.175_494_35e-36 {
        right.map(|axis| axis / f64::from(squared).sqrt())
    } else {
        [1.0, 0.0, 0.0]
    };
    let axis = rotate(right, forward, angle);
    rotate(forward, axis, f64::from(radius)).map(|value| f64::from(value as f32))
}

pub(crate) fn for_each_authored_hitscan_direction(
    node: &Node,
    shot: &ReportedShot,
    forward: Vec3,
    mut evaluate: impl FnMut(&Json) -> Result<Value, EvalError>,
    mut emit: impl FnMut(Vec3),
) {
    let launch = node.field("m_5EC1EB38");
    if launch.is_some_and(|launch| launch["$"] == "STU_613BDDB0") {
        if let Some(directions) =
            authored_ring_directions(&node.fields, forward, &shot.event, node.pos, &mut evaluate)
        {
            directions.into_iter().for_each(emit);
        }
        return;
    }
    if let Some(fan) = launch.filter(|launch| launch["$"] == "STU_1145C42E") {
        let count = node
            .field("m_numProjectilesPerShot")
            .filter(|value| !value.is_null())
            .map_or(1, |value| {
                evaluate(value).map_or(1, |value| to_int(&value, 1))
            });
        let spread = fan
            .get("m_spread")
            .filter(|value| !value.is_null())
            .and_then(|value| evaluate(value).ok())
            .map_or(0.0, |value| to_float(&value));
        let roll_deg = fan
            .get("m_A2A9890A")
            .filter(|value| !value.is_null())
            .and_then(|value| evaluate(value).ok())
            .map_or(90.0, |value| to_float(&value));
        if (1..=32).contains(&count) {
            let count = count as usize;
            if count == 1 || !spread.is_finite() || spread <= 0.0 || spread >= 180.0 {
                emit(forward);
                return;
            }
            let right = cross(forward, [0.0, 1.0, 0.0]);
            let rlen = norm(right);
            let right = if rlen > 1e-6 {
                right.map(|axis| axis / rlen)
            } else {
                [1.0, 0.0, 0.0]
            };
            let up = cross(right, forward);
            // Roll = 90 deg rotates the fan plane into the horizontal (yaw) plane.
            let roll = roll_deg.to_radians();
            let axis = std::array::from_fn(|i| up[i] * roll.sin() + right[i] * roll.cos());
            let alen = norm(axis);
            let axis = if alen > 1e-6 {
                axis.map(|a| a / alen)
            } else {
                up
            };
            let half = spread.to_radians() * 0.5;
            for pellet in 0..count {
                let frac = (pellet as f64) / ((count - 1) as f64);
                let angle = -half + spread.to_radians() * frac;
                let dir = rotate(forward, axis, angle).map(|v| f64::from(v as f32));
                emit(dir);
            }
        }
        return;
    }
    let mut degrees = |key: &str| {
        node.field(key)
            .filter(|value| !value.is_null())
            .map(|value| evaluate(value).map(|value| to_float(&value) as f32))
            .unwrap_or(Ok(0.0))
    };
    let (Ok(low), Ok(high)) = (
        degrees("m_5EC1EB38.m_7F0FED02"),
        degrees("m_5EC1EB38.m_429A6C85"),
    ) else {
        emit(forward);
        return;
    };
    let spread = match shot.normalized_spread {
        Some(normalized) => low + (high - low) * normalized,
        None if low > 0.0 && (high - low).abs() <= 1.0e-6 => low,
        None => {
            emit(forward);
            return;
        }
    };
    if !(0.0..90.0).contains(&spread) {
        emit(forward);
        return;
    }
    let count = launch
        .filter(|launch| launch["$"] == "STU_239B571E" && launch["m_E2FD6F04"].is_null())
        .and_then(|_| node.field("m_numProjectilesPerShot"))
        .filter(|value| !value.is_null())
        .map_or(1, |value| {
            evaluate(value).map_or(0, |value| to_int(&value, 0))
        });
    if (1..=32).contains(&count) {
        for pellet in 0..count as usize {
            emit(direct_cone_direction(forward, spread, &shot.event, pellet));
        }
    }
}

pub(crate) fn authored_ring_directions(
    fields: &Json,
    forward: [f64; 3],
    shot: &crate::script_init::ShotEvent,
    position: u16,
    evaluate: &mut impl FnMut(&Json) -> Result<Value, EvalError>,
) -> Option<Vec<[f64; 3]>> {
    let launch = &fields["m_5EC1EB38"];
    if launch["$"] != "STU_613BDDB0" {
        return None;
    }
    let cone = to_float(&evaluate(&launch["m_spread"]).ok()?);
    let count = to_int(&evaluate(&fields["m_numProjectilesPerShot"]).ok()?, 0);
    if !(0.0..90.0).contains(&cone) || !(1..=32).contains(&count) {
        return None;
    }
    let rings: Vec<_> = launch["m_0D30E40C"]
        .as_array()?
        .iter()
        .map(|row| {
            let radius = to_float(&evaluate(&row["m_6425FFB9"]).ok()?);
            let size = to_int(&evaluate(&row["m_B19BD35D"]).ok()?, 0);
            ((0.0..=1.0).contains(&radius) && (1..=count).contains(&size))
                .then_some((radius, size as usize))
        })
        .collect::<Option<_>>()?;
    (rings.iter().map(|(_, size)| size).sum::<usize>() == count as usize)
        .then(|| ring_directions(forward, cone.to_radians(), &rings, shot, position).collect())
}
fn ring_directions<'a>(
    forward: Vec3,
    cone: f64,
    rings: &'a [(f64, usize)],
    shot: &crate::script_init::ShotEvent,
    node: u16,
) -> impl Iterator<Item = Vec3> + use<'a> {
    let table = uniform_table();
    let phase = f64::from(table[spread_index(shot, node, 0)]) * TAU / 4_294_967_296.0;
    let right = cross(forward, [0.0, 1.0, 0.0]);
    let length = norm(right);
    let right = if length == 0.0 {
        [1.0, 0.0, 0.0]
    } else {
        right.map(|axis| axis / length)
    };
    let up = cross(right, forward);
    rings.iter().flat_map(move |&(radius, count)| {
        let (sin, cos) = (cone * radius).sin_cos();
        (0..count).map(move |pellet| {
            let (phase_sin, phase_cos) = (phase + TAU * pellet as f64 / count as f64).sin_cos();
            std::array::from_fn(|axis| {
                f64::from(
                    (forward[axis] * cos + (up[axis] * phase_cos + right[axis] * phase_sin) * sin)
                        as f32,
                )
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_init::ShotEvent;
    use sha2::{Digest, Sha256};

    #[test]
    fn constant_cone_without_normalized_spread_uses_authored_angle() {
        let node: Node = serde_json::from_value(serde_json::json!({
            "pos": 1,
            "uid": 0,
            "rid": 0,
            "remote": -1,
            "state": null,
            "class": "STUStatescriptStateWeaponVolley",
            "client_only": 0,
            "server_only": 0,
            "fields": {
                "m_5EC1EB38": {
                    "$": "STU_239B571E",
                    "m_7F0FED02": {"$": "STUConfigVarFloat", "m_value": 6.0},
                    "m_429A6C85": {"$": "STUConfigVarFloat", "m_value": 6.0}
                }
            }
        }))
        .unwrap();
        let shot = ReportedShot {
            event: ShotEvent {
                entity: 0xa000_0101,
                instance: 1,
                graph: 1,
                raw_state: 1,
                frame: 10,
                time_ms: 160,
                activation: 1,
                activation_ms: 160,
                ordinal: 0,
            },
            presentation: Vec::new(),
            presentation_states: Vec::new(),
            normalized_spread: None,
        };
        let forward = [0.0, 0.0, 1.0];
        let mut dirs = Vec::new();
        for_each_authored_hitscan_direction(
            &node,
            &shot,
            forward,
            |v| crate::script_expr::evaluate(v, &mut crate::script_expr::EmptyContext),
            |dir| dirs.push(dir),
        );
        assert_eq!(dirs.len(), 1);
        assert_ne!(dirs[0], forward);
        assert!(dirs[0][2] >= 6.0_f64.to_radians().cos() - 1e-6);
    }

    #[test]
    fn direct_launch_cone_matches_the_reference_using_the_raw_state_slot() {
        let fixture: Json = serde_json::from_str(include_str!(
            "../../fixtures/behavior/direct-weapon-cone-v1.json"
        ))
        .unwrap();
        let source = &fixture["input"];
        let mut shot = ShotEvent {
            entity: source["entity"].as_u64().unwrap() as u32,
            instance: source["instance"].as_u64().unwrap() as u16,
            graph: 42,
            raw_state: source["raw_state"].as_u64().unwrap() as u16,
            frame: 0,
            time_ms: 0,
            activation: source["activation"].as_u64().unwrap() as u16,
            activation_ms: 0,
            ordinal: 0,
        };
        let forward = serde_json::from_value(source["forward"].clone()).unwrap();
        for row in source["rounds"].as_array().unwrap() {
            shot.ordinal = row["ordinal"].as_u64().unwrap() as u32;
            let degrees = row["degrees"].as_f64().unwrap() as f32;
            for (pellet, key) in ["bits", "second_pellet_bits"].into_iter().enumerate() {
                let bits: [String; 3] = serde_json::from_value(row[key].clone()).unwrap();
                assert_eq!(
                    direct_cone_direction(forward, degrees, &shot, pellet)
                        .map(|axis| (axis as f32).to_bits()),
                    bits.map(|hex| u32::from_str_radix(&hex, 16).unwrap())
                );
            }
        }
        assert_eq!(direct_cone_direction(forward, 0.0, &shot, 0), forward);
    }

    #[test]
    fn reaper_rings_match_the_reference_table_and_directions() {
        let mut digest = Sha256::new();
        for word in uniform_table() {
            digest.update(word.to_le_bytes());
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "4f99ff210621528fff6737582c39f0ed5464cdf1c9980f03d503dcf311b356f6"
        );
        let shot = ShotEvent {
            entity: 0xc000_1000,
            instance: 10,
            graph: 0x0019,
            raw_state: 6,
            frame: 0,
            time_ms: 0,
            activation: 1,
            activation_ms: 0,
            ordinal: 0,
        };
        let directions: Vec<_> = ring_directions(
            [0.0, 0.0, 1.0],
            6.0_f64.to_radians(),
            &[(0.0, 1), (0.25, 5), (0.5, 6), (1.0, 8)],
            &shot,
            10,
        )
        .collect();
        assert_eq!(directions.len(), 20);
        for (index, expected) in [
            (0, [0, 0, 0x3f80_0000]),
            (1, [0x3ca8_4a87, 0xbc84_e73d, 0x3f7f_e98b]),
            (6, [0x3d28_3bc4, 0xbd04_db95, 0x3f7f_a62f]),
            (12, [0x3da8_00be, 0xbd84_acf8, 0x3f7e_98fd]),
            (19, [0x3c47_d74d, 0xbdd4_9ca2, 0x3f7e_98fd]),
        ] {
            assert_eq!(
                directions[index].map(|axis| (axis as f32).to_bits()),
                expected
            );
        }
    }

    #[test]
    fn fan_spread_emits_symmetric_horizontal_directions() {
        let node: Node = serde_json::from_value(serde_json::json!({
            "pos": 29,
            "uid": 0,
            "rid": 0,
            "remote": -1,
            "state": null,
            "class": "STUStatescriptStateWeaponVolley",
            "client_only": 0,
            "server_only": 0,
            "fields": {
                "m_numProjectilesPerShot": {"$": "STUConfigVarInt", "m_value": 3},
                "m_5EC1EB38": {
                    "$": "STU_1145C42E",
                    "m_spread": {"$": "STUConfigVarFloat", "m_value": 9.0},
                    "m_A2A9890A": {"$": "STUConfigVarFloat", "m_value": 90.0}
                }
            }
        }))
        .unwrap();
        let shot = ReportedShot {
            event: ShotEvent {
                entity: 0xa000_0101,
                instance: 1,
                graph: 0x0304,
                raw_state: 29,
                frame: 10,
                time_ms: 160,
                activation: 1,
                activation_ms: 160,
                ordinal: 0,
            },
            presentation: Vec::new(),
            presentation_states: Vec::new(),
            normalized_spread: None,
        };
        let mut dirs = Vec::new();
        for_each_authored_hitscan_direction(
            &node,
            &shot,
            [0.0, 0.0, 1.0],
            |v| crate::script_expr::evaluate(v, &mut crate::script_expr::EmptyContext),
            |dir| dirs.push(dir),
        );
        assert_eq!(dirs.len(), 3);
        // Center direction straight ahead, left and right symmetric at ±4.5° yaw (9° total fan)
        assert!(dirs[1][0].abs() < 1e-6 && (dirs[1][2] - 1.0).abs() < 1e-6);
        assert!((dirs[0][0] + dirs[2][0]).abs() < 1e-6);
        assert!((dirs[0][0].abs() - 4.5_f64.to_radians().sin()).abs() < 1e-5);
        assert!(dirs[0][1].abs() < 1e-6 && dirs[2][1].abs() < 1e-6);
    }
}
