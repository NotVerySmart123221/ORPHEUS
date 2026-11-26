//! Killcam & Play of the Game (POTG) highlight recording/scoring and
//! in-game team/match/group voice chat packet routing.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::static_ray::Vec3;

pub const MAX_REPLAY_FRAMES: usize = 360; // 12 seconds @ 30Hz (every 2 simulation ticks)
pub const KILLCAM_DURATION_TICKS: u32 = 62 * 7;
pub const POTG_WINDOW_TICKS: u32 = 62 * 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayActorPose {
    pub entity: u32,
    pub team: u8,
    pub hero: u64,
    pub position: Vec3,
    pub yaw: i32,
    pub pitch: i32,
    pub health: f32,
    pub alive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReplayEventKind {
    Elimination {
        killer: u32,
        victim: u32,
        critical: bool,
        environmental: bool,
        ult_shutdown: bool,
        lifesaver: bool,
        distance: f64,
    },
    HealBurst {
        healer: u32,
        amount: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayEvent {
    pub tick: u32,
    pub kind: ReplayEventKind,
}

#[derive(Clone, Debug)]
pub struct ReplayFrame {
    pub tick: u32,
    pub actors: Vec<ReplayActorPose>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PotgCategory {
    HighScore,
    Lifesaver,
    Sharpshooter,
    Shutdown,
}

impl PotgCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HighScore => "Play of the Game",
            Self::Lifesaver => "Lifesaver",
            Self::Sharpshooter => "Sharpshooter",
            Self::Shutdown => "Shutdown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct KillcamClip {
    pub victim: u32,
    pub killer: u32,
    pub start_tick: u32,
    pub end_tick: u32,
    pub frames: Vec<ReplayFrame>,
}

#[derive(Clone, Debug)]
pub struct PotgClip {
    pub star_entity: u32,
    pub star_hero: u64,
    pub category: PotgCategory,
    pub score: f64,
    pub start_tick: u32,
    pub end_tick: u32,
    pub frames: Vec<ReplayFrame>,
}

#[derive(Default)]
pub struct HighlightRecorder {
    frames: VecDeque<ReplayFrame>,
    events: VecDeque<ReplayEvent>,
    killcams: HashMap<u32, KillcamClip>,
    best_potg: Option<PotgClip>,
    last_capture_tick: u32,
}

impl HighlightRecorder {
    pub fn record_frame(&mut self, tick: u32, actors: Vec<ReplayActorPose>) {
        if tick == self.last_capture_tick || tick.saturating_sub(self.last_capture_tick) < 2 {
            return;
        }
        self.last_capture_tick = tick;
        if self.frames.len() >= MAX_REPLAY_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back(ReplayFrame { tick, actors });
        let cutoff = tick.saturating_sub(POTG_WINDOW_TICKS);
        while self.events.front().is_some_and(|e| e.tick < cutoff) {
            self.events.pop_front();
        }
    }

    pub fn record_event(&mut self, event: ReplayEvent) {
        self.events.push_back(event);
        if let ReplayEventKind::Elimination { killer, victim, .. } = event.kind {
            self.capture_killcam(victim, killer, event.tick);
            self.evaluate_potg(killer, event.tick);
        } else if let ReplayEventKind::HealBurst { healer, .. } = event.kind {
            self.evaluate_potg(healer, event.tick);
        }
    }

    pub fn capture_killcam(&mut self, victim: u32, killer: u32, death_tick: u32) {
        let start_tick = death_tick.saturating_sub(KILLCAM_DURATION_TICKS);
        let clip_frames: Vec<ReplayFrame> = self
            .frames
            .iter()
            .filter(|f| f.tick >= start_tick && f.tick <= death_tick)
            .cloned()
            .collect();
        self.killcams.insert(
            victim,
            KillcamClip {
                victim,
                killer,
                start_tick,
                end_tick: death_tick,
                frames: clip_frames,
            },
        );
    }

    fn evaluate_potg(&mut self, candidate: u32, now_tick: u32) {
        let start_tick = now_tick.saturating_sub(POTG_WINDOW_TICKS);
        let mut score = 0.0_f64;
        let mut shutdown_pts = 0.0_f64;
        let mut lifesaver_pts = 0.0_f64;
        let mut sharpshooter_pts = 0.0_f64;
        let mut kills = 0u32;

        for event in self.events.iter().filter(|e| e.tick >= start_tick) {
            match event.kind {
                ReplayEventKind::Elimination {
                    killer,
                    critical,
                    environmental,
                    ult_shutdown,
                    lifesaver,
                    distance,
                    ..
                } if killer == candidate => {
                    kills += 1;
                    let mut pts = 100.0;
                    if critical {
                        pts += 25.0;
                        if distance >= 20.0 {
                            let sharp = 40.0 + (distance - 20.0).min(40.0);
                            pts += sharp;
                            sharpshooter_pts += sharp;
                        }
                    }
                    if environmental {
                        pts *= 2.0;
                    }
                    if ult_shutdown {
                        pts += 120.0;
                        shutdown_pts += 120.0;
                    }
                    if lifesaver {
                        pts += 110.0;
                        lifesaver_pts += 110.0;
                    }
                    score += pts;
                }
                ReplayEventKind::HealBurst { healer, amount } if healer == candidate => {
                    score += amount * 0.25;
                }
                _ => {}
            }
        }
        // Multikill synergy bonus.
        if kills >= 2 {
            score += f64::from(kills - 1) * 65.0;
        }
        if score <= 0.0
            || self
                .best_potg
                .as_ref()
                .is_some_and(|best| score <= best.score)
        {
            return;
        }
        let category = if shutdown_pts >= lifesaver_pts
            && shutdown_pts >= sharpshooter_pts
            && shutdown_pts >= 100.0
        {
            PotgCategory::Shutdown
        } else if lifesaver_pts >= sharpshooter_pts && lifesaver_pts >= 100.0 {
            PotgCategory::Lifesaver
        } else if sharpshooter_pts >= 60.0 {
            PotgCategory::Sharpshooter
        } else {
            PotgCategory::HighScore
        };
        let star_hero = self
            .frames
            .back()
            .and_then(|f| f.actors.iter().find(|a| a.entity == candidate))
            .map_or(0, |a| a.hero);
        let clip_frames: Vec<ReplayFrame> = self
            .frames
            .iter()
            .filter(|f| f.tick >= start_tick && f.tick <= now_tick)
            .cloned()
            .collect();
        self.best_potg = Some(PotgClip {
            star_entity: candidate,
            star_hero,
            category,
            score,
            start_tick,
            end_tick: now_tick,
            frames: clip_frames,
        });
    }

    pub fn killcam(&self, victim: u32) -> Option<&KillcamClip> {
        self.killcams.get(&victim)
    }

    pub fn potg(&self) -> Option<&PotgClip> {
        self.best_potg.as_ref()
    }

    /// Build a frame-by-frame playback cursor for a victim's captured Killcam clip.
    pub fn start_killcam_playback(&self, victim: u32) -> Option<HighlightPlayback> {
        let clip = self.killcams.get(&victim)?;
        (!clip.frames.is_empty()).then(|| HighlightPlayback {
            camera_target: clip.killer,
            frames: clip.frames.clone(),
            cursor: 0,
        })
    }

    /// Build a frame-by-frame playback cursor for the highest-scoring Play of the Game clip.
    pub fn start_potg_playback(&self) -> Option<HighlightPlayback> {
        let clip = self.best_potg.as_ref()?;
        (!clip.frames.is_empty()).then(|| HighlightPlayback {
            camera_target: clip.star_entity,
            frames: clip.frames.clone(),
            cursor: 0,
        })
    }
}

/// Chronological 30Hz playback cursor over a recorded Killcam or POTG clip.
#[derive(Clone, Debug)]
pub struct HighlightPlayback {
    pub camera_target: u32,
    frames: Vec<ReplayFrame>,
    cursor: usize,
}

impl HighlightPlayback {
    pub fn next_frame(&mut self) -> Option<&ReplayFrame> {
        let frame = self.frames.get(self.cursor)?;
        self.cursor += 1;
        Some(frame)
    }

    pub fn remaining_frames(&self) -> usize {
        self.frames.len().saturating_sub(self.cursor)
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.frames.len()
    }
}

/// In-game voice chat channel identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VoiceChannel {
    Team(u8),
    Match,
    Group(u64),
}

#[derive(Clone, Debug, Default)]
pub struct VoicePeerState {
    pub channel: Option<VoiceChannel>,
    pub self_muted: bool,
    pub deafened: bool,
    pub muted_peers: BTreeSet<u32>,
    pub speaking_until_tick: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutedVoicePacket {
    pub sender: u32,
    pub channel: VoiceChannel,
    pub sequence: u32,
    pub recipients: Vec<u32>,
    pub payload: Vec<u8>,
}

/// Bounded, zero-allocation-leak voice chat router for team/match/group voice channels.
#[derive(Default)]
pub struct VoiceChatRouter {
    peers: BTreeMap<u32, VoicePeerState>,
}

impl VoiceChatRouter {
    pub fn join(&mut self, peer: u32, channel: VoiceChannel) {
        self.peers.entry(peer).or_default().channel = Some(channel);
    }

    pub fn leave(&mut self, peer: u32) {
        if let Some(state) = self.peers.get_mut(&peer) {
            state.channel = None;
        }
    }

    pub fn set_self_muted(&mut self, peer: u32, muted: bool) {
        self.peers.entry(peer).or_default().self_muted = muted;
    }

    pub fn set_deafened(&mut self, peer: u32, deafened: bool) {
        self.peers.entry(peer).or_default().deafened = deafened;
    }

    pub fn mute_peer(&mut self, listener: u32, target: u32, muted: bool) {
        let set = &mut self.peers.entry(listener).or_default().muted_peers;
        if muted {
            set.insert(target);
        } else {
            set.remove(&target);
        }
    }

    /// Route an encoded Opus/PCM voice frame (max 1400 bytes) to eligible peers in the sender's channel.
    pub fn route_audio(
        &mut self,
        sender: u32,
        sequence: u32,
        payload: Vec<u8>,
        tick: u32,
    ) -> Option<RoutedVoicePacket> {
        if payload.is_empty() || payload.len() > 1400 {
            return None;
        }
        let channel = {
            let state = self.peers.get_mut(&sender)?;
            if state.self_muted {
                return None;
            }
            let ch = state.channel?;
            state.speaking_until_tick = tick.saturating_add(16);
            ch
        };
        let recipients: Vec<u32> = self
            .peers
            .iter()
            .filter_map(|(&peer, state)| {
                (peer != sender
                    && state.channel == Some(channel)
                    && !state.deafened
                    && !state.muted_peers.contains(&sender))
                .then_some(peer)
            })
            .collect();
        if recipients.is_empty() {
            return None;
        }
        Some(RoutedVoicePacket {
            sender,
            channel,
            sequence,
            recipients,
            payload,
        })
    }

    pub fn active_speakers(&self, channel: VoiceChannel, tick: u32) -> Vec<u32> {
        self.peers
            .iter()
            .filter_map(|(&peer, state)| {
                (state.channel == Some(channel) && tick < state.speaking_until_tick).then_some(peer)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_recorder_captures_killcam_and_scores_potg() {
        let mut rec = HighlightRecorder::default();
        for t in (0..=100).step_by(2) {
            rec.record_frame(
                t,
                vec![
                    ReplayActorPose {
                        entity: 1,
                        team: 0,
                        hero: 0x29,
                        position: [0.0, 0.0, 0.0],
                        yaw: 0,
                        pitch: 0,
                        health: 200.0,
                        alive: true,
                    },
                    ReplayActorPose {
                        entity: 2,
                        team: 1,
                        hero: 0x6e,
                        position: [10.0, 0.0, 0.0],
                        yaw: 0,
                        pitch: 0,
                        health: 0.0,
                        alive: false,
                    },
                ],
            );
        }
        rec.record_event(ReplayEvent {
            tick: 100,
            kind: ReplayEventKind::Elimination {
                killer: 1,
                victim: 2,
                critical: true,
                environmental: false,
                ult_shutdown: true,
                lifesaver: false,
                distance: 10.0,
            },
        });
        let kc = rec.killcam(2).unwrap();
        assert_eq!(kc.killer, 1);
        assert!(!kc.frames.is_empty());
        let potg = rec.potg().unwrap();
        assert_eq!(potg.star_entity, 1);
        assert_eq!(potg.category, PotgCategory::Shutdown);
        assert!(potg.score >= 245.0);

        let mut kc_play = rec.start_killcam_playback(2).unwrap();
        assert_eq!(kc_play.camera_target, 1);
        let first_tick = kc_play.next_frame().unwrap().tick;
        let mut last_tick = first_tick;
        let mut count = 1usize;
        while let Some(frame) = kc_play.next_frame() {
            assert!(frame.tick > last_tick);
            last_tick = frame.tick;
            count += 1;
        }
        assert!(kc_play.is_finished());
        assert_eq!(kc_play.remaining_frames(), 0);
        assert_eq!(count, kc.frames.len());

        let mut potg_play = rec.start_potg_playback().unwrap();
        assert_eq!(potg_play.camera_target, 1);
        assert_eq!(potg_play.remaining_frames(), potg.frames.len());
        assert!(potg_play.next_frame().is_some());
    }

    #[test]
    fn voice_chat_router_respects_channels_mute_and_deafen() {
        let mut router = VoiceChatRouter::default();
        router.join(1, VoiceChannel::Team(0));
        router.join(2, VoiceChannel::Team(0));
        router.join(3, VoiceChannel::Team(0));
        router.join(4, VoiceChannel::Team(1));
        router.mute_peer(3, 1, true);
        let routed = router.route_audio(1, 1, vec![0xAA; 32], 10).unwrap();
        assert_eq!(routed.recipients, vec![2]);
        assert_eq!(router.active_speakers(VoiceChannel::Team(0), 15), vec![1]);
        router.set_deafened(2, true);
        assert!(router.route_audio(1, 2, vec![0xBB; 32], 12).is_none());
    }
}
