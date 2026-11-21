//! Career statistics and the mode-rules message sent at login.
//!
//! Ports `ow174/content/career.py`. The retail stat catalog remains in the
//! separately prepared templates. Only the two career stat lists are rebuilt.

use std::io;

use indexmap::IndexMap;

use crate::{
    accounts::{Accounts, Profile},
    catalog::ItemDb,
    collection::Collection,
    jam_codec::{
        Fields,
        Value::{self, Array, Blob, Bool, Float64, Signed, Struct, Unsigned},
    },
    lobby_menu::explicit_hero,
    lobby_player_messages::{SOCIAL_OUT, endorsement, summary},
    lobby_ranked::{LeaderboardPlaces, Queue, Ranked, Season, wins_of},
    lobby_session::PreparedLogin,
    lobby_transport::LobbyMessage,
    retail_templates::{LOBBY, RetailTemplates},
};

pub const MODE_RULES: u32 = 0xb9dc_a497;
pub const RULES: u32 = 27202;
pub(crate) const PROFILES: u32 = 0xd555_0262;
pub(crate) const NAME_QUERY: u32 = 0x46dc_9706;
pub(crate) const NAME_REPLY: u32 = 0x981d_6a52;
pub(crate) const OWL_POLL: u32 = 0xeb45_ad29;
pub(crate) const OWL_LIVE: u32 = 0x713d_7589;
pub(crate) const CAREER_PROFILE_REQ: u32 = 22206;
pub(crate) const FULL_PROFILE: u32 = 20807;
pub(crate) const PROFILE_STATUS: u32 = 39001;
pub(crate) const PROFILE_SUMMARY: u32 = 39002;
pub(crate) const NAME_QUERY_REQ: u32 = 58202;
pub(crate) const NAME_QUERY_RES: u32 = 58301;
pub(crate) const OWL_POLL_REQ: u32 = 42600;
pub(crate) const OWL_LIVE_RES: u32 = 43200;

pub(crate) struct CareerProfileRequest {
    request_id: Fields,
    target: Fields,
}

impl CareerProfileRequest {
    pub(crate) fn read((crc, id, fields): &LobbyMessage) -> Option<Self> {
        if (*crc, *id) != (SOCIAL_OUT, CAREER_PROFILE_REQ) {
            return None;
        }
        let Struct(target) = fields.get(&0x88)? else {
            return None;
        };
        let request_id = match fields.get(&0x78) {
            Some(Struct(id)) => id.clone(),
            _ => target.clone(),
        };
        Some(Self {
            request_id,
            target: target.clone(),
        })
    }

    pub(crate) fn reply(
        &self,
        login: &PreparedLogin,
        collection: &Collection,
        ranked: &Ranked,
        competitive_cards: &[u64],
        wall_time: u64,
    ) -> io::Result<Vec<LobbyMessage>> {
        let target_lo = self
            .target
            .get(&0x0)
            .and_then(|value| match value {
                Unsigned(id) => u32::try_from(*id).ok(),
                _ => None,
            })
            .unwrap_or(0);
        if target_lo != login.identity.account_lo {
            return Ok(vec![(
                PROFILES,
                PROFILE_STATUS,
                Fields::from([(0x78, Struct(self.target.clone())), (0x88, Unsigned(1))]),
            )]);
        }

        let profile = login.account.profile();
        let season = ranked.season(&profile);
        let stats = Array(career_stats(
            &profile,
            collection.heroes(),
            collection.items(),
            season,
        )?);
        let progression = collection.progression(&profile)?;
        let prog_body = progression
            .get(&0x78)
            .cloned()
            .unwrap_or_else(|| Struct(Fields::new()));
        let catalog = collection.hero_catalog(&profile)?;
        let catalog_entries = catalog
            .get(&0x80)
            .cloned()
            .unwrap_or_else(|| Array(Vec::new()));
        let cards = ranked
            .cards(&profile, competitive_cards, &LeaderboardPlaces::new())?
            .into_iter()
            .map(Struct)
            .collect();
        let body = Fields::from([
            (0x00, prog_body),
            (0x80, catalog_entries),
            (0x98, Struct(Fields::from([(0x00, Array(cards))]))),
            (0xb0, stats.clone()),
            (0xc8, stats),
            (0xe0, Struct(endorsement(profile.endorsement_level))),
            (0x100, Struct(login.identity.account())),
            (0x110, Array(Vec::new())),
            (0x128, Signed(0)),
            (0x12c, Signed(0)),
            (0x130, Bool(false)),
            (0x138, Value::String(profile.player_name.clone())),
            (0x160, Value::String(String::new())),
        ]);
        let full = Fields::from([
            (0x78, Struct(self.request_id.clone())),
            (0x88, Bool(false)),
            (0x90, Struct(body)),
            (0x218, Blob(Vec::new())),
        ]);
        let portrait_frame = collection.portrait_frame(&profile);
        let summary_msg = summary(&profile, login.identity, portrait_frame, wall_time);
        Ok(vec![
            (LOBBY, FULL_PROFILE, full),
            (PROFILES, PROFILE_SUMMARY, summary_msg),
        ])
    }
}

pub(crate) fn player_names_reply(
    (crc, id, fields): &LobbyMessage,
    accounts: &Accounts,
) -> io::Result<Option<LobbyMessage>> {
    if (*crc, *id) != (NAME_QUERY, NAME_QUERY_REQ) {
        return Ok(None);
    }
    let Some(Array(ids)) = fields.get(&0x78) else {
        return Ok(None);
    };
    let mut names = Vec::with_capacity(ids.len());
    for entry in ids {
        let Struct(player_id) = entry else {
            continue;
        };
        let account_lo = match player_id.get(&0x0) {
            Some(Unsigned(v)) => u32::try_from(*v).unwrap_or(0),
            _ => 0,
        };
        let name = accounts.by_id(account_lo)?.map_or_else(
            || format!("Player{}", account_lo & 0xffff),
            |acc| acc.name.clone(),
        );
        names.push(Struct(Fields::from([
            (0x00, entry.clone()),
            (0x10, Signed(0)),
            (0x18, Value::String(name)),
        ])));
    }
    Ok(Some((
        NAME_REPLY,
        NAME_QUERY_RES,
        Fields::from([(0x78, Array(names))]),
    )))
}

pub(crate) fn owl_live_reply((crc, id, _): &LobbyMessage) -> Option<LobbyMessage> {
    ((*crc, *id) == (OWL_POLL, OWL_POLL_REQ)).then(|| {
        (
            OWL_LIVE,
            OWL_LIVE_RES,
            Fields::from([(0x78, Bool(true)), (0x80, Array(Vec::new()))]),
        )
    })
}

const TIME_PLAYED: u64 = 0x0860_0000_0000_0021;
const MATCHES_WON: u64 = 0x0860_0000_0000_0039;
const SEASON_KIND: u64 = 3;
const SEASON_FLAG: u64 = 0x20;
const ROLE_QUEUES: [Queue; 3] = [Queue::Tank, Queue::Damage, Queue::Support];

pub fn mode_rules(
    templates: &RetailTemplates,
    profile: &Profile,
    heroes: &[u64],
    items: &ItemDb,
    season: Season,
) -> io::Result<Fields> {
    let mut rules = templates
        .first(MODE_RULES, RULES)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing mode-rules template"))?;
    let stats = Array(career_stats(profile, heroes, items, season)?);
    rules.insert(0x78, stats.clone());
    rules.insert(0xa8, stats);
    Ok(rules)
}

/// The lifetime list followed by the selected season's two career categories.
/// The roster is the collection's ordered catalog, shared with other login messages.
pub fn career_stats(
    profile: &Profile,
    heroes: &[u64],
    items: &ItemDb,
    season: Season,
) -> io::Result<Vec<Value>> {
    if heroes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty career hero catalog",
        ));
    }
    let main = explicit_hero(&profile.lobby_hero, heroes, items).unwrap_or_else(|| {
        heroes[crc32fast::hash(profile.player_name.as_bytes()) as usize % heroes.len()]
    });
    let played: IndexMap<_, _> = std::iter::once(main)
        .chain(heroes.iter().copied().filter(|&hero| hero != main))
        .zip([500., 50., 40., 30., 20.])
        .map(|(hero, hours)| (hero, vec![stat(TIME_PLAYED, hours * 3600.)]))
        .collect();

    let mut won = IndexMap::new();
    for queue in ROLE_QUEUES {
        let wins = wins_of(profile, queue)?;
        if wins == 0 {
            continue;
        }
        if let Some(hero) = played.keys().chain(heroes).copied().find(|hero| {
            items
                .hero_classes
                .get(hero)
                .and_then(Option::as_deref)
                .is_some_and(|class| class.eq_ignore_ascii_case(queue.name()))
        }) {
            *won.entry(hero).or_insert(0) += wins;
        }
    }

    let mut all_modes = played;
    let mut seasonal = IndexMap::new();
    for (hero, wins) in won {
        let wins = stat(MATCHES_WON, f64::from(wins));
        all_modes.entry(hero).or_default().push(wins.clone());
        seasonal.insert(hero, vec![wins]);
    }
    let key = (season.number << 16) | SEASON_KIND;
    Ok(vec![
        category(all_modes, 0),
        category(seasonal.clone(), key),
        category(seasonal, key | SEASON_FLAG),
    ])
}

fn stat(guid: u64, value: f64) -> Value {
    Struct(Fields::from([(0, Unsigned(guid)), (8, Float64(value))]))
}

fn category(stats: IndexMap<u64, Vec<Value>>, key: u64) -> Value {
    let records = stats
        .into_iter()
        .map(|(hero, stats)| Struct(Fields::from([(0, Array(stats)), (0x18, Unsigned(hero))])))
        .collect();
    Struct(Fields::from([(0, Array(records)), (0x18, Unsigned(key))]))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{collection::Collection, jam_codec::Schemas, lobby_ranked::Ranked};

    const SEASON: Season = Season {
        number: 32,
        card: 1,
        open_card: None,
    };

    fn roster() -> ItemDb {
        let mut items = ItemDb::default();
        items.hero_names.insert(6, "Reaper".into());
        items.hero_names.insert(20, "Fictional NPC".into());
        for (hero, role) in [(2, "Damage"), (5, "SUPPORT"), (6, "Damage"), (7, "Tank")] {
            items.hero_classes.insert(hero, Some(role.into()));
        }
        items
    }

    #[test]
    fn preserves_played_hero_order_and_adds_role_wins_after_time_played() {
        let items = roster();
        let heroes = [1, 2, 3, 4, 5, 6, 7];
        let mut profile = Profile {
            lobby_hero: " rEaPeR ".into(),
            ..Profile::default()
        };
        profile.wins.extend([
            ("tank".into(), 7.into()),
            ("damage".into(), 3.into()),
            ("support".into(), 4.into()),
        ]);
        profile.matches.insert("tank".into(), 5.into());
        profile.wins.insert("open".into(), 11.into());
        let got = career_stats(&profile, &heroes, &items, SEASON).unwrap();
        let all = IndexMap::from([
            (
                6,
                vec![stat(TIME_PLAYED, 1_800_000.), stat(MATCHES_WON, 3.)],
            ),
            (1, vec![stat(TIME_PLAYED, 180_000.)]),
            (2, vec![stat(TIME_PLAYED, 144_000.)]),
            (3, vec![stat(TIME_PLAYED, 108_000.)]),
            (4, vec![stat(TIME_PLAYED, 72_000.)]),
            (7, vec![stat(MATCHES_WON, 5.)]),
            (5, vec![stat(MATCHES_WON, 4.)]),
        ]);
        let seasonal = IndexMap::from([
            (7, vec![stat(MATCHES_WON, 5.)]),
            (6, vec![stat(MATCHES_WON, 3.)]),
            (5, vec![stat(MATCHES_WON, 4.)]),
        ]);
        assert_eq!(
            got,
            [
                category(all, 0),
                category(seasonal.clone(), 0x20_0003),
                category(seasonal, 0x20_0023)
            ]
        );
        profile.lobby_hero = "0X0000000000000006".into();
        assert_eq!(
            career_stats(&profile, &heroes, &items, SEASON).unwrap(),
            got
        );
    }

    #[test]
    fn clones_the_template_and_uses_a_stable_name_for_non_catalog_heroes() {
        let templates = RetailTemplates::from_reader(&br#"{"format":1,"server":[["0xB9DCA497",27202,{"+0x78":[],"+0x90":"kept","+0xA8":[]}]]}"#[..]).unwrap();
        let items = roster();
        let heroes = [1, 2, 3, 4, 5, 6, 7];
        let mut profile = Profile {
            player_name: "Fävorite 群".into(),
            lobby_hero: "random".into(),
            ..Profile::default()
        };
        let random = mode_rules(&templates, &profile, &heroes, &items, SEASON).unwrap();
        profile.lobby_hero = "Fictional NPC".into();
        let npc = mode_rules(&templates, &profile, &heroes, &items, SEASON).unwrap();
        assert_eq!(npc, random);
        assert_eq!(npc[&0x90], Value::String("kept".into()));
        assert_eq!(npc[&0x78], npc[&0xa8]);
        assert_eq!(
            templates.first(MODE_RULES, RULES).unwrap()[&0x78],
            Array(Vec::new())
        );
        assert!(career_stats(&profile, &[], &items, SEASON).is_err());
    }

    #[test]
    #[ignore = "requires the prepared 104319 retail templates and catalogs"]
    fn mode_rules_matches_three_python_104319_message_bodies() {
        let data = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap()).join("data");
        let templates = RetailTemplates::load(data.join("retail_templates.json")).unwrap();
        let items = Arc::new(ItemDb::load(&data).unwrap());
        let collection = Collection::new(&templates, Arc::clone(&items)).unwrap();
        let ranked = Ranked::new(&templates).unwrap();
        let schemas = Schemas::load(data.join("schemas_174.json")).unwrap();

        let default = Profile {
            player_name: "Mode fixture".into(),
            ..Profile::default()
        };
        let mut reaper = Profile {
            lobby_hero: " Reaper ".into(),
            season: 18,
            ..default.clone()
        };
        reaper.wins.extend([
            ("tank".into(), 5.into()),
            ("damage".into(), 13.into()),
            ("support".into(), 8.into()),
        ]);
        reaper.matches.insert("tank".into(), 4.into());
        let mut npc = Profile {
            player_name: "Fävorite 群".into(),
            lobby_hero: "B.O.B.".into(),
            season: 555,
            ..Profile::default()
        };
        npc.wins.insert("support".into(), 2.into());

        for (profile, length, digest) in [
            (
                default,
                66_272,
                "9f995f88d919b841a296a5f1b43c2cca5a53360e6ba83827168f4ee571676b4b",
            ),
            (
                reaper,
                66_728,
                "0ac88c4d182c2ab8bffb5759ffca83f61af266fa1e40bc3fe3f0f405a1306f56",
            ),
            (
                npc,
                66_416,
                "95009d091e3c450ff56124fa10f7b7a97b9d8cd452227712f75e04e80698c7fa",
            ),
        ] {
            let fields = mode_rules(
                &templates,
                &profile,
                collection.heroes(),
                &items,
                ranked.season(&profile),
            )
            .unwrap();
            let body = schemas.encode(MODE_RULES, RULES, &fields).unwrap();
            assert_eq!(body.len(), length);
            assert_eq!(format!("{:x}", Sha256::digest(body)), digest);
        }
    }

    #[test]
    fn career_profile_names_and_owl_replies_encode_with_schemas() {
        let Ok(runtime) = std::env::var("ORPHEUS_RUNTIME_DIR") else {
            return;
        };
        let data = PathBuf::from(runtime).join("data");
        let schemas = Schemas::load(data.join("schemas_174.json")).unwrap();
        let templates = RetailTemplates::load(data.join("retail_templates.json")).unwrap();
        let items = Arc::new(ItemDb::load(&data).unwrap());
        let collection = Collection::new(&templates, items).unwrap();
        let ranked = Ranked::new(&templates).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let accounts = Accounts::new(dir.path(), dir.path().join("missing.json")).unwrap();
        let account = accounts.get("ProfileTester").unwrap();
        let identity = crate::lobby_login::Identity::for_account(account.account_lo);
        let login = PreparedLogin {
            account,
            identity,
            tournament: false,
        };

        let self_req = CareerProfileRequest::read(&(
            SOCIAL_OUT,
            CAREER_PROFILE_REQ,
            Fields::from([
                (0x78, Struct(identity.account())),
                (0x88, Struct(identity.account())),
            ]),
        ))
        .unwrap();
        let self_msgs = self_req
            .reply(&login, &collection, &ranked, &[], 1_700_000_000)
            .unwrap();
        assert_eq!(self_msgs.len(), 2);
        assert_eq!(self_msgs[0].0, LOBBY);
        assert_eq!(self_msgs[0].1, FULL_PROFILE);
        assert_eq!(self_msgs[1].0, PROFILES);
        assert_eq!(self_msgs[1].1, PROFILE_SUMMARY);
        for (crc, id, fields) in &self_msgs {
            schemas.encode(*crc, *id, fields).unwrap();
        }

        let foreign_account = crate::lobby_login::Identity::for_account(identity.account_lo + 1);
        let foreign_req = CareerProfileRequest::read(&(
            SOCIAL_OUT,
            CAREER_PROFILE_REQ,
            Fields::from([
                (0x78, Struct(foreign_account.account())),
                (0x88, Struct(foreign_account.account())),
            ]),
        ))
        .unwrap();
        let foreign_msgs = foreign_req
            .reply(&login, &collection, &ranked, &[], 1_700_000_000)
            .unwrap();
        assert_eq!(foreign_msgs.len(), 1);
        assert_eq!(foreign_msgs[0].0, PROFILES);
        assert_eq!(foreign_msgs[0].1, PROFILE_STATUS);
        schemas
            .encode(foreign_msgs[0].0, foreign_msgs[0].1, &foreign_msgs[0].2)
            .unwrap();

        let names_msg = player_names_reply(
            &(
                NAME_QUERY,
                NAME_QUERY_REQ,
                Fields::from([(
                    0x78,
                    Array(vec![
                        Struct(identity.account()),
                        Struct(foreign_account.account()),
                    ]),
                )]),
            ),
            &accounts,
        )
        .unwrap()
        .unwrap();
        assert_eq!(names_msg.0, NAME_REPLY);
        assert_eq!(names_msg.1, NAME_QUERY_RES);
        schemas
            .encode(names_msg.0, names_msg.1, &names_msg.2)
            .unwrap();

        let owl_msg = owl_live_reply(&(OWL_POLL, OWL_POLL_REQ, Fields::new())).unwrap();
        assert_eq!(owl_msg.0, OWL_LIVE);
        assert_eq!(owl_msg.1, OWL_LIVE_RES);
        schemas.encode(owl_msg.0, owl_msg.1, &owl_msg.2).unwrap();
    }
}
