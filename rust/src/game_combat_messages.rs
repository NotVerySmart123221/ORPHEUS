//! Reliable combat notices shared by weapons and damage sources.
//!
//! The field layout follows ow174/game/combat.py for build 104319. Each
//! notice exposes the identity a recipient must know before it is queued.

use crate::{
    game_messages::{self, ChannelError, EncodedMessage},
    jam_codec::{Fields, Schemas, Value},
    static_ray::Vec3,
};

const RECEIVED_DAMAGE: u32 = 25000;
const HIT: u32 = 25001;
const HEAL_CONFIRM: u32 = 25002;
const DEATH: u32 = 25004;
const ELIMINATION: u32 = 25005;
const SCORE_POPUP: u32 = 25007;
const VOICE: u32 = 27500;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CombatNotice {
    Hit {
        target: u32,
        dealt: f64,
        critical: bool,
        killed: bool,
        shot_frame: u32,
    },
    HealConfirm {
        target: u32,
        amount: f64,
        shot_frame: u32,
    },
    DamageReceived {
        source: u32,
        victim: u32,
        dealt: f32,
        origin: Option<Vec3>,
        normal: Option<Vec3>,
        contact_set: u16,
        critical: bool,
    },
    Death {
        source: u32,
        killed_by: bool,
    },
    Elimination {
        killer: u32,
        victim: u32,
        loadout: u64,
        critical: bool,
    },
    ScorePopup {
        score: u16,
        label: u64,
    },
    Voice {
        anchor: u32,
        speaker: u32,
        listener: u32,
        stimulus: u64,
        line: u64,
    },
}

impl CombatNotice {
    pub(crate) fn needs_mode_ui(self) -> bool {
        matches!(
            self,
            Self::Death { .. } | Self::Elimination { .. } | Self::ScorePopup { .. }
        )
    }

    pub fn required_identity(self) -> Option<u32> {
        match self {
            Self::Hit { target, .. } | Self::HealConfirm { target, .. } => Some(target),
            Self::DamageReceived { source, .. } => Some(source),
            Self::Death { source, killed_by } => killed_by.then_some(source),
            Self::Elimination { victim, .. } => Some(victim),
            Self::ScorePopup { .. } => None,
            Self::Voice { anchor, .. } => Some(anchor),
        }
    }

    pub(crate) fn referenced_entities(self) -> impl Iterator<Item = u32> {
        let references = match self {
            Self::Hit { target, .. } | Self::HealConfirm { target, .. } => {
                [Some(target), None, None]
            }
            Self::DamageReceived { source, victim, .. } => [Some(source), Some(victim), None],
            Self::Death { source, killed_by } => [killed_by.then_some(source), None, None],
            Self::Elimination { killer, victim, .. } => [Some(killer), Some(victim), None],
            Self::ScorePopup { .. } => [None, None, None],
            Self::Voice {
                anchor,
                speaker,
                listener,
                ..
            } => [Some(anchor), Some(speaker), Some(listener)],
        };
        references
            .into_iter()
            .flatten()
            .filter(|&entity| entity != 0 && entity != u32::MAX)
    }

    pub fn encode(self, schemas: &Schemas) -> Result<EncodedMessage, ChannelError> {
        let entity = |id: u32| Value::Struct(Fields::from([(0, Value::Unsigned(u64::from(id)))]));
        let (id, fields) = match self {
            Self::Hit {
                target,
                dealt,
                critical,
                killed,
                shot_frame,
            } => (
                HIT,
                Fields::from([
                    (
                        0x78,
                        Value::Struct(Fields::from([
                            (0, entity(target)),
                            (
                                4,
                                Value::Unsigned(u64::from(dealt.round_ties_even().max(1.0) as u32)),
                            ),
                            (
                                8,
                                Value::Unsigned(u64::from(critical) | (u64::from(killed) << 1)),
                            ),
                        ])),
                    ),
                    (0x84, Value::Unsigned(u64::from(shot_frame))),
                ]),
            ),
            Self::HealConfirm {
                target,
                amount,
                shot_frame,
            } => (
                HEAL_CONFIRM,
                Fields::from([
                    (
                        0x78,
                        Value::Struct(Fields::from([
                            (0, entity(target)),
                            (
                                4,
                                Value::Unsigned(
                                    u64::from(amount.round_ties_even().max(1.0) as u32),
                                ),
                            ),
                            (8, Value::Unsigned(0)),
                        ])),
                    ),
                    (0x84, Value::Unsigned(u64::from(shot_frame))),
                ]),
            ),
            Self::DamageReceived {
                source,
                victim,
                dealt,
                origin,
                normal,
                contact_set,
                critical,
            } => {
                let vector = |xyz: Option<Vec3>| {
                    let [x, y, z] = xyz
                        .map(|xyz| xyz.map(|value| value as f32))
                        .filter(|xyz| xyz.iter().all(|value| value.is_finite()))
                        .unwrap_or_default()
                        .map(Value::Float32);
                    Value::Struct(Fields::from([(0, x), (4, y), (8, z)]))
                };
                (
                    RECEIVED_DAMAGE,
                    Fields::from([
                        (0x78, Value::Signed(1)),
                        (0x7c, entity(source)),
                        (0x80, entity(victim)),
                        // The contact effect turns the outward normal inward.
                        (0x84, vector(normal)),
                        (0x90, vector(None)),
                        // The direction indicator subtracts the view position.
                        (0x9c, vector(origin)),
                        (
                            0xa8,
                            Value::Array(vec![
                                Value::Unsigned(u64::from(critical)),
                                Value::Unsigned(0),
                                Value::Unsigned(0),
                            ]),
                        ),
                        (0xc0, Value::Float32(dealt)),
                        (0xc4, Value::Unsigned(0)),
                        (0xc8, Value::Unsigned(0)),
                        (0xca, Value::Unsigned(u64::from(contact_set))),
                        (0xcc, Value::Float32(0.0)),
                        (0xd0, Value::Bool(false)),
                        (
                            0xd8,
                            Value::Array(vec![Value::Float32(dealt), Value::Float32(1.0)]),
                        ),
                        (0xf0, Value::Array(Vec::new())),
                        (0x108, Value::Bool(false)),
                    ]),
                )
            }
            Self::Death { source, killed_by } => (
                DEATH,
                Fields::from([
                    (0x78, Value::Unsigned(2 * u64::from(killed_by))),
                    (0x7c, entity(source)),
                ]),
            ),
            Self::Elimination {
                killer,
                victim,
                loadout,
                critical,
            } => (
                ELIMINATION,
                Fields::from([
                    (0x78, Value::Unsigned(loadout)),
                    (0x80, entity(killer)),
                    (0x84, entity(victim)),
                    (0x88, Value::Unsigned(0)),
                    (0x89, Value::Bool(critical)),
                ]),
            ),
            Self::ScorePopup { score, label } => (
                SCORE_POPUP,
                Fields::from([
                    (0x78, Value::Unsigned(u64::from(score))),
                    (0x80, Value::Unsigned(label)),
                ]),
            ),
            Self::Voice {
                speaker,
                listener,
                stimulus,
                line,
                ..
            } => (
                VOICE,
                Fields::from([
                    (0x78, entity(speaker)),
                    (0x7c, entity(listener)),
                    (0x80, entity(u32::MAX)),
                    (0x88, Value::Unsigned(stimulus)),
                    (0x90, Value::Unsigned(line)),
                    (0x98, Value::Unsigned(0)),
                    (0xa0, Value::Unsigned(0)),
                ]),
            ),
        };
        game_messages::encode(schemas, id, &fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the separately prepared 104319 game schemas"]
    fn combat_notice_messages_match_python() {
        const KILLER: u32 = 0xa000_0101;
        const TARGET: u32 = 0xa004_0010;
        let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let schemas = Schemas::load(root.join("data/game_schemas_174.json")).unwrap();
        let cases: [(CombatNotice, Option<u32>, &[u8], usize); 4] = [
            (
                CombatNotice::Hit {
                    target: TARGET,
                    dealt: 12.5,
                    critical: true,
                    killed: true,
                    shot_frame: 1042,
                },
                Some(TARGET),
                b"\x02\x01\x10\x00\x04\xa0\x0c\x00\x00\x00\x03\x12\x04\x00\x00",
                120,
            ),
            (
                CombatNotice::Elimination {
                    killer: KILLER,
                    victim: TARGET,
                    loadout: 0,
                    critical: false,
                },
                Some(TARGET),
                b"\x02\x05\x00\x00\x00\x00\x00\x00\x00\x00\x01\x01\x00\xa0\x10\x00\x04\xa0\x00\x00",
                153,
            ),
            (
                CombatNotice::Death {
                    source: KILLER,
                    killed_by: true,
                },
                Some(KILLER),
                b"\x02\x04\x02\x01\x01\x00\xa0",
                56,
            ),
            (
                CombatNotice::Death {
                    source: TARGET,
                    killed_by: false,
                },
                None,
                b"\x02\x04\x00\x10\x00\x04\xa0",
                56,
            ),
        ];
        for (notice, identity, expected, expected_bits) in cases {
            assert_eq!(notice.required_identity(), identity);
            let encoded = notice.encode(&schemas).unwrap();
            assert_eq!(encoded.as_bytes(), expected);
            assert_eq!(encoded.bit_len(), expected_bits);
        }
    }

    #[test]
    #[ignore = "requires the separately prepared 104319 game schemas"]
    fn elimination_frame_resumes_at_exact_bit() {
        use crate::{
            bits::BitReader,
            game_messages::read_channel,
            world::{EntityUpdate, Operation, frame},
        };

        let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let schemas = Schemas::load(root.join("data/game_schemas_174.json")).unwrap();
        let elim = CombatNotice::Elimination {
            killer: 0xa000_0101,
            victim: 0xc000_1005,
            loadout: 0,
            critical: false,
        }
        .encode(&schemas)
        .unwrap();
        assert_eq!(elim.bit_len(), 153);
        let destroy = EntityUpdate {
            entity: 0xc000_1005,
            operation: Some(Operation::Destroy),
            ..EntityUpdate::default()
        };
        let payload = frame(6906, 6905, &[(0, elim)], &[], &[&destroy]).unwrap();
        let mut reader = BitReader::new(&payload);
        assert_eq!(reader.var_a().unwrap(), 6906);
        let channel = read_channel(&mut reader, &schemas).unwrap();
        assert_eq!(channel.reliable.len(), 1);
        assert_eq!(channel.reliable[0].message.id, ELIMINATION);
        assert_eq!(reader.position(), 217);
        assert_eq!(reader.var_a().unwrap(), 6905);
        assert_eq!(reader.bits(4).unwrap(), 0);
        assert!(!reader.bit().unwrap());
        assert!(reader.bit().unwrap());
        assert!(!reader.bit().unwrap());
        assert_eq!(reader.bits(7).unwrap(), 1);
        assert_eq!(reader.entity_id().unwrap(), Some(0xc000_1005));
    }

    #[test]
    #[ignore = "requires the separately prepared 104319 game schemas"]
    fn boss_intro_voice_notices_match_python() {
        let root = std::path::PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let schemas = Schemas::load(root.join("data/game_schemas_174.json")).unwrap();
        let anchor = 0xc000_1000;
        for (speaker, stimulus, line, expected) in [
            (
                crate::world::GAME_MODE_ENTITY,
                0x0ee0_0000_0000_06b5,
                0x0760_0000_0000_8cb1,
                "0300100000d0010100a0ffffffffb50600000000e00eb18c000000006007000000000000000000000000",
            ),
            (
                anchor,
                0x0ee0_0000_0000_06bd,
                0x0760_0000_0000_8a2b,
                "0300001000c0010100a0ffffffffbd0600000000e00e2b8a000000006007000000000000000000000000",
            ),
        ] {
            let notice = CombatNotice::Voice {
                anchor,
                speaker,
                listener: 0xa000_0101,
                stimulus,
                line,
            };
            assert_eq!(notice.required_identity(), Some(anchor));
            assert!(!notice.needs_mode_ui());
            let encoded = notice.encode(&schemas).unwrap();
            assert_eq!(encoded.bit_len(), 336);
            let encoded: String = encoded
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(encoded, expected);
        }
    }
}
