//! Deterministic rotation of the 104319 authored concentric pellet rings.

use std::{f64::consts::TAU, sync::OnceLock};

use crate::{
    script_expr::{EvalError, to_float},
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

fn spread_index(shot: &crate::script_init::ShotEvent, state: u16) -> usize {
    let seed = (shot.entity as u16)
        .wrapping_add(shot.instance.wrapping_mul(23))
        .wrapping_add(state.wrapping_mul(7))
        .wrapping_add(shot.activation & 63);
    let key = (usize::from(seed) * 131 + shot.ordinal as usize * 23) & TABLE_MASK;
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

/// The single-projectile uniform cone uses the volley state slot, not its node position.
fn direct_cone_direction(
    forward: Vec3,
    spread_degrees: f32,
    shot: &crate::script_init::ShotEvent,
) -> Vec3 {
    if spread_degrees == 0.0 {
        return forward;
    }
    let table = uniform_table();
    let index = spread_index(shot, shot.raw_state);
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

pub(crate) fn authored_hitscan_direction(
    node: &Node,
    shot: &ReportedShot,
    forward: Vec3,
    mut evaluate: impl FnMut(&Json) -> Result<Value, EvalError>,
) -> Vec3 {
    let Some(normalized) = shot.normalized_spread else {
        return forward;
    };
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
        return forward;
    };
    let spread = low + (high - low) * normalized;
    if (0.0..90.0).contains(&spread) {
        direct_cone_direction(forward, spread, &shot.event)
    } else {
        forward
    }
}

pub(crate) fn ring_directions<'a>(
    forward: Vec3,
    cone: f64,
    rings: &'a [(f64, usize)],
    shot: &crate::script_init::ShotEvent,
    node: u16,
) -> impl Iterator<Item = Vec3> + use<'a> {
    let table = uniform_table();
    let phase = f64::from(table[spread_index(shot, node)]) * TAU / 4_294_967_296.0;
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
            let bits: [String; 3] = serde_json::from_value(row["bits"].clone()).unwrap();
            assert_eq!(
                direct_cone_direction(forward, degrees, &shot).map(|axis| (axis as f32).to_bits()),
                bits.map(|hex| u32::from_str_radix(&hex, 16).unwrap())
            );
        }
        assert_eq!(direct_cone_direction(forward, 0.0, &shot), forward);
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
}
