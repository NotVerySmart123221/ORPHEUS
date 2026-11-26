//! Live On-Fire scoring, TAB scoreboard & medals, end-of-match commendation cards,
//! XP/level progression, and career profile summary generation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    game_stats::{
        BRONZE, DEATHS, ELIMINATIONS, GOLD, HEALING, HERO_DAMAGE, Medals, OBJECTIVE_KILLS,
        OBJECTIVE_TIME, PlayerStats, SILVER, StatsCatalog, on_fire_component,
    },
    world::{EntityUpdate, Operation},
};

pub const ON_FIRE_THRESHOLD: f32 = 250.0;
pub const ON_FIRE_EXIT_THRESHOLD: f32 = 200.0;
pub const ON_FIRE_MAX_SCORE: f32 = 350.0;
pub const ON_FIRE_DECAY_PER_SEC: f32 = 5.0;
pub const ON_FIRE_IDLE_DELAY_SECS: f64 = 3.0;
pub const XP_PER_LEVEL: u64 = 20_000;

/// Retail elimination score popup label GUID (`25007`).
pub const SCORE_LABEL_ELIMINATION: u64 = 0x0de0_0000_0000_0001;
pub const SCORE_LABEL_OBJECTIVE: u64 = 0x0de0_0000_0000_0002;
pub const SCORE_LABEL_HEAL: u64 = 0x0de0_0000_0000_0003;

#[derive(Clone, Debug)]
pub struct OnFireState {
    pub score: f32,
    pub threshold: f32,
    pub on_fire: bool,
    pub last_gain_secs: f64,
    pub total_on_fire_secs: f64,
}

impl Default for OnFireState {
    fn default() -> Self {
        Self {
            score: 0.0,
            threshold: ON_FIRE_THRESHOLD,
            on_fire: false,
            last_gain_secs: 0.0,
            total_on_fire_secs: 0.0,
        }
    }
}

impl OnFireState {
    /// Add fire points and return `true` if the player just transitioned into On-Fire.
    pub fn add_score(&mut self, points: f32, now_secs: f64) -> bool {
        if !points.is_finite() || points <= 0.0 {
            return false;
        }
        let was_on_fire = self.on_fire;
        self.score = (self.score + points).min(ON_FIRE_MAX_SCORE);
        self.last_gain_secs = now_secs;
        if self.score >= self.threshold {
            self.on_fire = true;
        }
        !was_on_fire && self.on_fire
    }

    /// Decay idle fire score and return `true` if the visual state changed.
    pub fn tick(&mut self, dt_secs: f64, now_secs: f64) -> bool {
        if dt_secs <= 0.0 || !dt_secs.is_finite() {
            return false;
        }
        let prev_score = self.score;
        let prev_fire = self.on_fire;
        if self.on_fire {
            self.total_on_fire_secs += dt_secs;
        }
        if self.score > 0.0 && now_secs - self.last_gain_secs >= ON_FIRE_IDLE_DELAY_SECS {
            self.score = (self.score - ON_FIRE_DECAY_PER_SEC * dt_secs as f32).max(0.0);
            if self.on_fire && self.score < ON_FIRE_EXIT_THRESHOLD {
                self.on_fire = false;
            }
        }
        self.on_fire != prev_fire || (self.score - prev_score).abs() >= 1.0
    }
}

#[derive(Clone, Debug)]
pub struct TrackedPlayerBoard {
    pub owner: u32,
    pub team: u8,
    pub hero: u64,
    pub name: String,
    pub stats: PlayerStats,
    pub on_fire: OnFireState,
    pub final_blows: u32,
    pub solo_kills: u32,
    pub critical_hits: u32,
    pub dirty: bool,
    pub last_publish_tick: u32,
}

impl TrackedPlayerBoard {
    pub fn new(owner: u32, team: u8, hero: u64, name: String) -> Self {
        Self {
            owner,
            team,
            hero,
            name,
            stats: PlayerStats::default(),
            on_fire: OnFireState::default(),
            final_blows: 0,
            solo_kills: 0,
            critical_hits: 0,
            dirty: true,
            last_publish_tick: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommendationCard {
    pub card_id: usize,
    pub owner: u32,
    pub player_name: String,
    pub hero: u64,
    pub category: &'static str,
    pub stat_guid: u64,
    pub value: f64,
    pub votes: u32,
}

impl CommendationCard {
    pub fn epic(&self) -> bool {
        self.votes >= 5
    }

    pub fn legendary(&self) -> bool {
        self.votes >= 10
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchXpBreakdown {
    pub time_played_xp: u64,
    pub match_finish_xp: u64,
    pub win_bonus_xp: u64,
    pub medal_bonus_xp: u64,
    pub consecutive_match_xp: u64,
    pub total_xp: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CareerMatchRecord {
    pub mode_name: String,
    pub won: bool,
    pub duration_secs: f64,
    pub eliminations: u64,
    pub deaths: u64,
    pub hero_damage: u64,
    pub healing: u64,
    pub objective_kills: u64,
    pub objective_time_secs: u64,
    pub final_blows: u64,
    pub gold_medals: u32,
    pub silver_medals: u32,
    pub bronze_medals: u32,
    pub xp: MatchXpBreakdown,
}

#[derive(Default)]
pub struct MatchScoreboard {
    players: BTreeMap<u32, TrackedPlayerBoard>,
    cards: Vec<CommendationCard>,
    card_voters: HashMap<u32, usize>,
    /// Tracks `(victim_wire -> (attacker_owner -> recent_damage_sum))` for elimination share.
    damage_credit: HashMap<u32, HashMap<u32, f64>>,
    match_start_secs: Option<f64>,
}

impl MatchScoreboard {
    pub fn register_player(
        &mut self,
        owner: u32,
        team: u8,
        hero: u64,
        name: String,
        now_secs: f64,
    ) {
        self.match_start_secs.get_or_insert(now_secs);
        let entry = self
            .players
            .entry(owner)
            .or_insert_with(|| TrackedPlayerBoard::new(owner, team, hero, name.clone()));
        entry.team = team;
        if hero != 0 {
            entry.hero = hero;
        }
        if !name.is_empty() {
            entry.name = name;
        }
        entry.dirty = true;
    }

    pub fn player(&self, owner: u32) -> Option<&TrackedPlayerBoard> {
        self.players.get(&owner)
    }

    pub fn record_damage(
        &mut self,
        attacker_owner: u32,
        victim_wire: u32,
        amount: f64,
        critical: bool,
        now_secs: f64,
    ) {
        if !amount.is_finite() || amount <= 0.0 {
            return;
        }
        self.match_start_secs.get_or_insert(now_secs);
        if let Some(board) = self.players.get_mut(&attacker_owner) {
            board.stats.add(HERO_DAMAGE, amount);
            if critical {
                board.critical_hits = board.critical_hits.saturating_add(1);
            }
            board.dirty = true;
        }
        *self
            .damage_credit
            .entry(victim_wire)
            .or_default()
            .entry(attacker_owner)
            .or_default() += amount;
    }

    pub fn record_heal(&mut self, healer_owner: u32, amount: f64, now_secs: f64) -> bool {
        if !amount.is_finite() || amount <= 0.0 {
            return false;
        }
        self.match_start_secs.get_or_insert(now_secs);
        if let Some(board) = self.players.get_mut(&healer_owner) {
            board.stats.add(HEALING, amount);
            board.dirty = true;
            return board.on_fire.add_score((amount / 5.0) as f32, now_secs);
        }
        false
    }

    /// Record an elimination, crediting the killer + all assisting damage dealers,
    /// returning `(owner, fire_points_awarded, ignited_on_fire)` tuples.
    pub fn record_elimination(
        &mut self,
        killer_owner: u32,
        victim_wire: u32,
        on_objective: bool,
        now_secs: f64,
    ) -> Vec<(u32, u16, bool)> {
        let mut credits = self.damage_credit.remove(&victim_wire).unwrap_or_default();
        credits.entry(killer_owner).or_insert(1.0);
        let total_dmg: f64 = credits.values().copied().sum::<f64>().max(1.0);
        let contributors = credits.len();
        let mut awarded = Vec::new();
        for (owner, dealt) in credits {
            let Some(board) = self.players.get_mut(&owner) else {
                continue;
            };
            board.stats.add(ELIMINATIONS, 1.0);
            if on_objective {
                board.stats.add(OBJECTIVE_KILLS, 1.0);
            }
            if owner == killer_owner {
                board.final_blows = board.final_blows.saturating_add(1);
                if contributors == 1 {
                    board.solo_kills = board.solo_kills.saturating_add(1);
                }
            }
            let share = ((dealt / total_dmg) * 100.0).clamp(15.0, 100.0);
            let bonus = if on_objective { 25.0 } else { 0.0 };
            let points = (share + bonus).round().clamp(1.0, 150.0) as u16;
            let ignited = board.on_fire.add_score(f32::from(points), now_secs);
            board.dirty = true;
            awarded.push((owner, points, ignited));
        }
        awarded
    }

    pub fn record_death(&mut self, victim_owner: u32, victim_wire: u32) {
        self.damage_credit.remove(&victim_wire);
        if let Some(board) = self.players.get_mut(&victim_owner) {
            board.stats.add(DEATHS, 1.0);
            board.dirty = true;
        }
    }

    pub fn record_objective_time(&mut self, owner: u32, dt_secs: f64, now_secs: f64) -> bool {
        if dt_secs <= 0.0 || !dt_secs.is_finite() {
            return false;
        }
        if let Some(board) = self.players.get_mut(&owner) {
            board.stats.add(OBJECTIVE_TIME, dt_secs);
            board.dirty = true;
            return board.on_fire.add_score((dt_secs * 10.0) as f32, now_secs);
        }
        false
    }

    pub fn tick_on_fire(&mut self, dt_secs: f64, now_secs: f64) {
        for board in self.players.values_mut() {
            if board.on_fire.tick(dt_secs, now_secs) {
                board.dirty = true;
            }
        }
    }

    /// Build dirty Component 64 (`GMP_STATS`) + On-Fire `EntityUpdate`s for each
    /// player controller entity, throttled cleanly so TAB and On-Fire stay live.
    pub fn collect_board_updates(
        &mut self,
        catalog: &StatsCatalog,
        mode: u64,
        tick: u32,
        min_interval_ticks: u32,
    ) -> Vec<(u32, EntityUpdate)> {
        let mut by_team: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
        for (&owner, board) in &self.players {
            by_team.entry(board.team).or_default().push(owner);
        }
        let mut medals_by_owner: HashMap<u32, Medals> = HashMap::new();
        for owners in by_team.values() {
            let team_stats: Vec<&PlayerStats> = owners
                .iter()
                .filter_map(|owner| self.players.get(owner).map(|b| &b.stats))
                .collect();
            let team_medals = catalog.medals(&team_stats, mode);
            for (&owner, medals) in owners.iter().zip(team_medals) {
                medals_by_owner.insert(owner, medals);
            }
        }
        let mut updates = Vec::new();
        for (&owner, board) in &mut self.players {
            if !board.dirty || tick.saturating_sub(board.last_publish_tick) < min_interval_ticks {
                continue;
            }
            board.dirty = false;
            board.last_publish_tick = tick;
            let stats_comp = catalog.component(
                &board.stats,
                mode,
                (board.hero != 0).then_some(board.hero),
                medals_by_owner.get(&owner),
            );
            let fire_comp = on_fire_component(
                Some(board.on_fire.score),
                Some(board.on_fire.threshold),
                Some(board.on_fire.on_fire),
            );
            updates.push((
                owner,
                EntityUpdate {
                    entity: owner,
                    operation: Some(Operation::Update(vec![stats_comp, fire_comp])),
                    ..EntityUpdate::default()
                },
            ));
        }
        updates
    }

    /// Generate up to 4 end-of-match commendation cards (one per category, distinct players when possible).
    pub fn generate_commendation_cards(&mut self) -> &[CommendationCard] {
        self.cards.clear();
        self.card_voters.clear();
        let categories: [(&'static str, u64); 4] = [
            ("Eliminations", ELIMINATIONS),
            ("Hero Damage Done", HERO_DAMAGE),
            ("Healing Done", HEALING),
            ("Objective Time", OBJECTIVE_TIME),
        ];
        let mut featured: HashSet<u32> = HashSet::new();
        for (category, stat_guid) in categories {
            let best = self
                .players
                .values()
                .filter(|b| !featured.contains(&b.owner) && b.stats.value(stat_guid) > 0.0)
                .max_by(|a, b| {
                    a.stats
                        .value(stat_guid)
                        .total_cmp(&b.stats.value(stat_guid))
                })
                .or_else(|| {
                    self.players
                        .values()
                        .filter(|b| b.stats.value(stat_guid) > 0.0)
                        .max_by(|a, b| {
                            a.stats
                                .value(stat_guid)
                                .total_cmp(&b.stats.value(stat_guid))
                        })
                });
            if let Some(winner) = best {
                featured.insert(winner.owner);
                let card_id = self.cards.len();
                self.cards.push(CommendationCard {
                    card_id,
                    owner: winner.owner,
                    player_name: winner.name.clone(),
                    hero: winner.hero,
                    category,
                    stat_guid,
                    value: winner.stats.value(stat_guid),
                    votes: 0,
                });
            }
        }
        &self.cards
    }

    pub fn vote_card(&mut self, voter_owner: u32, card_id: usize) -> Option<&CommendationCard> {
        if card_id >= self.cards.len() {
            return None;
        }
        if let Some(prev) = self.card_voters.insert(voter_owner, card_id) {
            if prev == card_id {
                return self.cards.get(card_id);
            }
            if let Some(old_card) = self.cards.get_mut(prev) {
                old_card.votes = old_card.votes.saturating_sub(1);
            }
        }
        let card = self.cards.get_mut(card_id)?;
        card.votes = card.votes.saturating_add(1);
        Some(card)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_career_record(
        &self,
        catalog: &StatsCatalog,
        mode: u64,
        mode_name: &str,
        owner: u32,
        won: bool,
        consecutive: bool,
        now_secs: f64,
    ) -> Option<CareerMatchRecord> {
        let board = self.players.get(&owner)?;
        let team_owners: Vec<u32> = self
            .players
            .values()
            .filter(|b| b.team == board.team)
            .map(|b| b.owner)
            .collect();
        let team_stats: Vec<&PlayerStats> = team_owners
            .iter()
            .filter_map(|o| self.players.get(o).map(|b| &b.stats))
            .collect();
        let all_medals = catalog.medals(&team_stats, mode);
        let my_medals = team_owners
            .iter()
            .position(|&o| o == owner)
            .and_then(|idx| all_medals.get(idx));
        let (mut gold, mut silver, mut bronze) = (0u32, 0u32, 0u32);
        if let Some(medals) = my_medals {
            for &m in medals.values() {
                match m {
                    GOLD => gold += 1,
                    SILVER => silver += 1,
                    BRONZE => bronze += 1,
                    _ => {}
                }
            }
        }
        let duration_secs = (now_secs - self.match_start_secs.unwrap_or(now_secs)).max(1.0);
        let time_played_xp = (duration_secs * 3.4).round() as u64;
        let match_finish_xp = 250;
        let win_bonus_xp = if won { 500 } else { 0 };
        let medal_bonus_xp = if gold > 0 {
            150
        } else if silver > 0 {
            100
        } else if bronze > 0 {
            50
        } else {
            0
        };
        let consecutive_match_xp = if consecutive { 200 } else { 0 };
        let total_xp =
            time_played_xp + match_finish_xp + win_bonus_xp + medal_bonus_xp + consecutive_match_xp;
        Some(CareerMatchRecord {
            mode_name: mode_name.to_owned(),
            won,
            duration_secs,
            eliminations: board.stats.value(ELIMINATIONS).round() as u64,
            deaths: board.stats.value(DEATHS).round() as u64,
            hero_damage: board.stats.value(HERO_DAMAGE).round() as u64,
            healing: board.stats.value(HEALING).round() as u64,
            objective_kills: board.stats.value(OBJECTIVE_KILLS).round() as u64,
            objective_time_secs: board.stats.value(OBJECTIVE_TIME).round() as u64,
            final_blows: u64::from(board.final_blows),
            gold_medals: gold,
            silver_medals: silver,
            bronze_medals: bronze,
            xp: MatchXpBreakdown {
                time_played_xp,
                match_finish_xp,
                win_bonus_xp,
                medal_bonus_xp,
                consecutive_match_xp,
                total_xp,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_fire_ignites_at_250_and_decays_after_idle_window() {
        let mut fire = OnFireState::default();
        assert!(!fire.add_score(100.0, 1.0));
        assert!(!fire.on_fire);
        assert!(fire.add_score(160.0, 2.0));
        assert!(fire.on_fire);
        assert!(!fire.tick(2.0, 4.0));
        assert!(fire.on_fire);
        assert!(fire.tick(15.0, 20.0));
        assert!(!fire.on_fire);
        assert!(fire.score < ON_FIRE_EXIT_THRESHOLD);
    }

    #[test]
    fn scoreboard_tracks_stats_medals_cards_voting_and_xp() {
        let catalog = StatsCatalog::from_json(
            r#"{
                "player": {"default": [["0x0860000000000025",0],["0x08600000000004B8",0],["0x08600000000001D1",0],["0x0860000000000029",1]], "modes": {}},
                "heroes": {},
                "stats": {}
            }"#,
        )
        .unwrap();
        let mut board = MatchScoreboard::default();
        board.register_player(1, 0, 0x10, "Alice".into(), 0.0);
        board.register_player(2, 0, 0x20, "Bob".into(), 0.0);
        board.record_damage(1, 99, 150.0, true, 1.0);
        board.record_damage(2, 99, 50.0, false, 1.2);
        let awards = board.record_elimination(1, 99, true, 1.5);
        assert_eq!(awards.len(), 2);
        board.record_heal(2, 400.0, 2.0);
        let updates = board.collect_board_updates(&catalog, 0, 62, 10);
        assert_eq!(updates.len(), 2);
        let cards = board.generate_commendation_cards();
        assert!(!cards.is_empty());
        for voter in 1..=5 {
            board.vote_card(voter, 0);
        }
        assert!(board.cards[0].epic());
        let career = board
            .build_career_record(&catalog, 0, "quickplay", 1, true, true, 120.0)
            .unwrap();
        assert_eq!(career.eliminations, 1);
        assert_eq!(career.gold_medals, 2);
        assert!(career.xp.total_xp >= 1000);
    }
}
