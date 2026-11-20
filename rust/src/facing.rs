//! State-owned facing selection shared by native actor hosts.

use crate::script_init::{AiFacing, Site};

pub(crate) fn winner<'a>(
    records: impl IntoIterator<Item = (&'a Site, &'a AiFacing)>,
) -> Option<(&'a Site, &'a AiFacing)> {
    records
        .into_iter()
        .fold(None, |winner: Option<(&Site, &AiFacing)>, entry| {
            if winner.is_none_or(|(_, winner)| {
                entry.1.priority.level > winner.priority.level
                    || (entry.1.priority.level == winner.priority.level
                        && entry.1.priority.above_equals)
            }) {
                Some(entry)
            } else {
                winner
            }
        })
}

/// Pack an authored direction without the compatibility aim lock's pitch clamp.
pub(crate) fn angles(direction: [f64; 3]) -> Option<[i32; 2]> {
    if !direction.into_iter().all(f64::is_finite) {
        return None;
    }
    let scale = direction.into_iter().map(f64::abs).fold(0.0, f64::max);
    if scale == 0.0 {
        return None;
    }
    let [x, y, z] = direction.map(|axis| axis / scale);
    let units = 65_536.0 / std::f64::consts::TAU;
    let yaw = (x.atan2(z) * units).round_ties_even() as i32;
    let pitch = (-y.atan2(x.hypot(z)) * units).round_ties_even() as i32;
    Some([(yaw + 32_768).rem_euclid(65_536) - 32_768, pitch])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_facing_packs_all_axes_without_compatibility_pitch_limits() {
        for (direction, expected) in [
            ([0.0, 0.0, 1.0], [0, 0]),
            ([1.0, 0.0, 0.0], [16_384, 0]),
            ([-1.0, 0.0, 0.0], [-16_384, 0]),
            ([0.0, 0.0, -1.0], [-32_768, 0]),
            ([0.0, 1.0, 0.0], [0, -16_384]),
            ([0.0, -1.0, 0.0], [0, 16_384]),
            ([f64::MAX, 0.0, 0.0], [16_384, 0]),
        ] {
            assert_eq!(angles(direction), Some(expected));
        }
        for direction in [[0.0; 3], [f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, 0.0]] {
            assert_eq!(angles(direction), None);
        }
    }
}
