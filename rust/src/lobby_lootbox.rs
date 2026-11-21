//! Loot box opening and gallery item purchases for the 1.74 lobby.
//!
//! Ports `ow174/services/lootbox.py`, `ow174/services/shop.py`, and the
//! progression handlers in `ow174/lobby/handlers/gallery.py`.

use std::{collections::HashMap, io};

use serde_json::Value as Json;

use crate::{
    accounts::{BOX_TYPES, Profile},
    collection::{Collection, PROGRESSION},
    jam_codec::{
        Fields,
        Value::{self, Array, Bool, Signed, Struct, Unsigned},
    },
    lobby_session::PreparedLogin,
    lobby_transport::LobbyMessage,
};

pub(crate) const PROGRESSION_OUT: u32 = 0x7f4f_46cb;
pub(crate) const OPEN_BOX: u32 = 24201;
pub(crate) const PURCHASE: u32 = 24203;
pub(crate) const OPEN_BOX_RESULT: u32 = 24305;
pub(crate) const CREDITS_UPDATED: u32 = 24307;

const SLOTS_PER_BOX: usize = 4;
const LOW_STOCK: usize = 5;

const SLOT_ODDS: [(&str, f64); 3] = [("Legendary", 0.0193), ("Epic", 0.0498), ("Rare", 0.10)];
const GUARANTEED_ODDS: [(&str, f64); 2] = [("Legendary", 0.0193), ("Epic", 0.0498)];
const CREDIT_DROPS: [(&str, (u64, i64, f64)); 3] = [
    ("Rare", (0x0250_0000_0000_088b, 50, 0.05)),
    ("Epic", (0x0250_0000_0000_088c, 150, 0.05)),
    ("Legendary", (0x0250_0000_0000_088d, 500, 0.5)),
];

pub(crate) struct OpenBoxRequest {
    pub box_id: u64,
}

impl OpenBoxRequest {
    pub(crate) fn read((crc, id, fields): &LobbyMessage) -> Option<Self> {
        if (*crc, *id) != (PROGRESSION_OUT, OPEN_BOX) {
            return None;
        }
        let Struct(box_struct) = fields.get(&0x78)? else {
            return None;
        };
        let Array(ids) = box_struct.get(&0x0)? else {
            return None;
        };
        let Value::Unsigned(box_id) = ids.first()? else {
            return None;
        };
        Some(Self { box_id: *box_id })
    }
}

pub(crate) struct PurchaseRequest {
    pub guid: u64,
}

impl PurchaseRequest {
    pub(crate) fn read((crc, id, fields): &LobbyMessage) -> Option<Self> {
        if (*crc, *id) != (PROGRESSION_OUT, PURCHASE) {
            return None;
        }
        let Value::Unsigned(guid) = fields.get(&0x78)? else {
            return None;
        };
        Some(Self { guid: *guid })
    }
}

struct Drop {
    hero: u64,
    unlock: u64,
    credits: i64,
    order: usize,
    highlight: i32,
    duplicate: bool,
    new: bool,
}

pub(crate) struct LootBoxEngine {
    pools: HashMap<(i64, &'static str), Vec<u64>>,
    hero_pools: HashMap<(String, &'static str), Vec<u64>>,
    products: HashMap<u64, Product>,
}

struct Product {
    price: i64,
    currency: Currency,
}

#[derive(Clone, Copy)]
enum Currency {
    Credits,
    CompPoints,
    LeagueTokens,
}

impl LootBoxEngine {
    pub(crate) fn new(collection: &Collection) -> Self {
        let mut pools: HashMap<(i64, &'static str), Vec<u64>> = HashMap::new();
        let mut hero_pools: HashMap<(String, &'static str), Vec<u64>> = HashMap::new();
        let mut products = HashMap::new();
        let items = collection.items();

        for (&guid, &entry) in collection.store() {
            let Some(unlock) = items.get(guid) else {
                continue;
            };
            if entry.price > 0 {
                let currency = if unlock.categories.iter().any(|c| c == "OWL") {
                    Currency::LeagueTokens
                } else if unlock.item_type == "WeaponSkin" {
                    Currency::CompPoints
                } else {
                    Currency::Credits
                };
                products.insert(
                    guid,
                    Product {
                        price: entry.price,
                        currency,
                    },
                );
            }
            if entry.event < 0 || !is_lootable_type(&unlock.item_type) {
                continue;
            }
            if entry.price <= 0 && unlock.item_type != "Icon" {
                continue;
            }
            if let Some(rarity) = canonical_rarity(&unlock.rarity) {
                pools.entry((entry.event, rarity)).or_default().push(guid);
                if entry.event == 0 {
                    if let Some(hero) = unlock.hero.as_deref().filter(|hero| !hero.is_empty()) {
                        hero_pools
                            .entry((hero.to_owned(), rarity))
                            .or_default()
                            .push(guid);
                    }
                }
            }
        }

        Self {
            pools,
            hero_pools,
            products,
        }
    }

    pub(crate) fn open_box(
        &self,
        login: &PreparedLogin,
        collection: &Collection,
        box_id: u64,
    ) -> io::Result<Vec<LobbyMessage>> {
        let credits = login.account.edit_profile(|profile| {
            let box_type = take_box(profile, box_id);
            let (event, always_legendary, hero_name) = box_spec(box_type);
            let rarities = roll_slot_rarities(always_legendary);
            let events = roll_slot_events(event);
            let promised = if always_legendary {
                rarities.iter().position(|&r| r == "Legendary")
            } else {
                None
            };

            let mut owned = collection.owned_set(profile);
            let mut drops = Vec::with_capacity(SLOTS_PER_BOX);
            let mut dup_credits = 0i64;
            let mut legendary_count = 0i64;

            for (order, (&rarity, slot_event)) in rarities.iter().zip(events).enumerate() {
                if rarity == "Legendary" {
                    legendary_count += 1;
                }
                if let Some((guid, amount)) =
                    roll_credits(rarity, slot_event, Some(order) == promised)
                {
                    profile.credits += amount;
                    drops.push(Drop {
                        hero: 0,
                        unlock: guid,
                        credits: 0,
                        order,
                        highlight: 0,
                        duplicate: false,
                        new: false,
                    });
                    continue;
                }

                let guid = if let Some(hero) = hero_name {
                    self.pick_hero_item(hero, rarity, |candidate| {
                        profile.unlock_all || owned.contains(&candidate)
                    })
                } else {
                    self.pick(slot_event, rarity, |candidate| {
                        profile.unlock_all || owned.contains(&candidate)
                    })
                };

                let Some(guid) = guid else {
                    profile.credits += duplicate_credits(rarity);
                    dup_credits += duplicate_credits(rarity);
                    continue;
                };

                let duplicate = profile.unlock_all || owned.contains(&guid);
                let credits_award = if duplicate {
                    duplicate_credits(rarity)
                } else {
                    0
                };
                dup_credits += credits_award;
                if !duplicate {
                    owned.insert(guid);
                    profile
                        .unlocked_items
                        .push(Json::String(format!("0x{guid:016X}")));
                }
                drops.push(Drop {
                    hero: collection.hero_for(guid).unwrap_or(0),
                    unlock: guid,
                    credits: credits_award,
                    order,
                    highlight: if slot_event != 0 { box_type } else { 0 },
                    duplicate,
                    new: !duplicate,
                });
            }

            profile.credits += dup_credits;
            increment_stat(profile, "boxes_opened", 1);
            if legendary_count > 0 {
                increment_stat(profile, "legendaries_dropped", legendary_count);
            }
            if dup_credits > 0 {
                increment_stat(profile, "credits_earned_from_duplicates", dup_credits);
            }
            if profile.loot_boxes.len() < LOW_STOCK {
                for (box_type, _, count) in BOX_TYPES {
                    if count > 0 {
                        profile.add_boxes(box_type, count)?;
                    }
                }
            }

            let items = drops
                .into_iter()
                .map(|drop| {
                    Struct(Fields::from([
                        (0x00, Unsigned(drop.hero)),
                        (0x08, Unsigned(drop.unlock)),
                        (0x10, Signed(drop.credits)),
                        (0x14, Signed(drop.order as i64)),
                        (0x18, Signed(i64::from(drop.highlight))),
                        (0x1c, Bool(drop.duplicate)),
                        (0x1d, Bool(drop.new)),
                    ]))
                })
                .collect();
            let result = Fields::from([(
                0x78,
                Struct(Fields::from([
                    (0x00, Array(items)),
                    (
                        0x18,
                        Struct(Fields::from([(
                            0x00,
                            Array(vec![Unsigned(box_id), Unsigned(0)]),
                        )])),
                    ),
                    (0x28, Signed(0)),
                ])),
            )]);
            Ok(Some((result, profile.credits)))
        })?;

        let Some((result, credits)) = credits else {
            return Ok(Vec::new());
        };
        Ok(vec![
            (PROGRESSION, OPEN_BOX_RESULT, result),
            (
                PROGRESSION,
                CREDITS_UPDATED,
                Fields::from([(0x78, Signed(credits))]),
            ),
        ])
    }

    pub(crate) fn purchase(
        &self,
        login: &PreparedLogin,
        collection: &Collection,
        guid: u64,
    ) -> io::Result<Vec<LobbyMessage>> {
        let Some(product) = self.products.get(&guid) else {
            return Ok(Vec::new());
        };
        let purchased = login.account.edit_profile(|profile| {
            if profile.unlock_all || collection.owns(profile, guid) {
                return Ok(None);
            }
            let balance = match product.currency {
                Currency::Credits => &mut profile.credits,
                Currency::CompPoints => &mut profile.comp_points,
                Currency::LeagueTokens => &mut profile.league_tokens,
            };
            if *balance < product.price {
                return Ok(None);
            }
            *balance -= product.price;
            profile
                .unlocked_items
                .push(Json::String(format!("0x{guid:016X}")));
            let pair = collection
                .items()
                .team_skin_pair(guid)
                .filter(|&other| !collection.owns(profile, other));
            if let Some(other) = pair {
                profile
                    .unlocked_items
                    .push(Json::String(format!("0x{other:016X}")));
            }
            let progression = collection.progression(profile)?;
            Ok(Some((pair, progression)))
        })?;

        let Some((pair, progression)) = purchased else {
            return Ok(Vec::new());
        };
        let mut messages = vec![collection.unlock_bought(guid)];
        if let Some(other) = pair {
            messages.push(collection.unlock_granted(other));
        }
        messages.push((PROGRESSION, 24300, progression));
        Ok(messages)
    }

    fn pick(
        &self,
        event: i64,
        rarity: &'static str,
        is_owned: impl Fn(u64) -> bool,
    ) -> Option<u64> {
        let mut candidates = Vec::new();
        if let Some(pool) = self.pools.get(&(event, rarity)) {
            candidates.push(pool);
        }
        if event != 0 {
            if let Some(pool) = self.pools.get(&(0, rarity)) {
                candidates.push(pool);
            }
        }
        if candidates.is_empty() {
            if let Some(pool) = self.pools.get(&(0, "Common")) {
                candidates.push(pool);
            }
        }
        for pool in &candidates {
            let unowned: Vec<_> = pool.iter().copied().filter(|&g| !is_owned(g)).collect();
            if !unowned.is_empty() {
                return choose(&unowned);
            }
        }
        candidates.first().and_then(|pool| choose(pool))
    }

    fn pick_hero_item(
        &self,
        hero: &str,
        rarity: &'static str,
        is_owned: impl Fn(u64) -> bool,
    ) -> Option<u64> {
        let pool = self
            .hero_pools
            .get(&(hero.to_owned(), rarity))
            .or_else(|| self.pools.get(&(0, rarity)))
            .or_else(|| self.pools.get(&(0, "Common")))?;
        let unowned: Vec<_> = pool.iter().copied().filter(|&g| !is_owned(g)).collect();
        if !unowned.is_empty() {
            choose(&unowned)
        } else {
            choose(pool)
        }
    }
}

fn is_lootable_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "Skin" | "Emote" | "VictoryPose" | "VoiceLine" | "Spray" | "HighlightIntro" | "Icon"
    )
}

fn canonical_rarity(rarity: &str) -> Option<&'static str> {
    match rarity {
        "Common" => Some("Common"),
        "Rare" => Some("Rare"),
        "Epic" => Some("Epic"),
        "Legendary" => Some("Legendary"),
        _ => None,
    }
}

fn duplicate_credits(rarity: &str) -> i64 {
    match rarity {
        "Rare" => 15,
        "Epic" => 50,
        "Legendary" => 200,
        _ => 5,
    }
}

fn box_spec(box_type: i32) -> (i64, bool, Option<&'static str>) {
    match box_type {
        1 => (1, false, None),
        2 => (2, false, None),
        3 => (3, false, None),
        4 => (4, false, None),
        5 => (5, false, None),
        6 => (6, false, None),
        7 => (0, true, None),
        9 => (6, true, None),
        10 => (0, false, Some("Wrecking Ball")),
        12 => (0, true, None),
        _ => (0, false, None),
    }
}

fn take_box(profile: &mut Profile, box_id: u64) -> i32 {
    if let Some(pos) = profile.loot_boxes.iter().position(|b| b.id == box_id) {
        profile.loot_boxes.remove(pos).box_type
    } else {
        0
    }
}

fn increment_stat(profile: &mut Profile, key: &str, delta: i64) {
    let current = profile.stats.get(key).and_then(Json::as_i64).unwrap_or(0);
    profile
        .stats
        .insert(key.into(), Json::from(current.saturating_add(delta)));
}

fn random_unit() -> f64 {
    let bits = getrandom::u64().unwrap_or(0);
    // 53 random bits give a uniform f64 in [0.0, 1.0).
    ((bits >> 11) as f64) / ((1u64 << 53) as f64)
}

fn choose(pool: &[u64]) -> Option<u64> {
    if pool.is_empty() {
        return None;
    }
    let idx = (getrandom::u64().unwrap_or(0) as usize) % pool.len();
    Some(pool[idx])
}

fn roll_rarity(guaranteed: bool, always_legendary: bool) -> &'static str {
    if always_legendary {
        return "Legendary";
    }
    let odds: &[(&str, f64)] = if guaranteed {
        &GUARANTEED_ODDS
    } else {
        &SLOT_ODDS
    };
    let mut roll = random_unit();
    for &(rarity, chance) in odds {
        if roll < chance {
            return rarity;
        }
        roll -= chance;
    }
    if guaranteed { "Rare" } else { "Common" }
}

fn roll_slot_rarities(always_legendary: bool) -> [&'static str; SLOTS_PER_BOX] {
    let mut rarities = [
        roll_rarity(true, always_legendary),
        roll_rarity(false, false),
        roll_rarity(false, false),
        roll_rarity(false, false),
    ];
    for i in (1..SLOTS_PER_BOX).rev() {
        let j = (getrandom::u64().unwrap_or(0) as usize) % (i + 1);
        rarities.swap(i, j);
    }
    rarities
}

fn roll_slot_events(event: i64) -> [i64; SLOTS_PER_BOX] {
    let mut events = [0; SLOTS_PER_BOX];
    if event != 0 {
        for (slot, out) in events.iter_mut().enumerate() {
            if slot == 0 || random_unit() < 0.5 {
                *out = event;
            }
        }
    }
    events
}

fn roll_credits(rarity: &str, slot_event: i64, promised: bool) -> Option<(u64, i64)> {
    if slot_event != 0 || promised {
        return None;
    }
    let (_, (guid, amount, share)) = CREDIT_DROPS.iter().find(|&&(r, _)| r == rarity)?;
    (random_unit() < *share).then_some((*guid, *amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{Accounts, LootBox};
    use crate::jam_codec::Schemas;
    use tempfile::tempdir;

    #[test]
    fn open_box_and_purchase_requests_decode_expected_payloads() {
        let open = (
            PROGRESSION_OUT,
            OPEN_BOX,
            Fields::from([(
                0x78,
                Struct(Fields::from([(
                    0x00,
                    Array(vec![Unsigned(42), Unsigned(0)]),
                )])),
            )]),
        );
        assert_eq!(OpenBoxRequest::read(&open).unwrap().box_id, 42);

        let buy = (
            PROGRESSION_OUT,
            PURCHASE,
            Fields::from([(0x78, Unsigned(0x0250_0000_0000_0123))]),
        );
        assert_eq!(
            PurchaseRequest::read(&buy).unwrap().guid,
            0x0250_0000_0000_0123
        );
    }

    #[test]
    fn open_box_refills_and_awards_drops_against_real_catalog() {
        let Ok(runtime) = std::env::var("ORPHEUS_RUNTIME_DIR") else {
            return;
        };
        let data = std::path::Path::new(&runtime).join("data");
        let schemas = Schemas::load(data.join("schemas_174.json")).unwrap();
        let templates =
            crate::retail_templates::RetailTemplates::load(data.join("retail_templates.json"))
                .unwrap();
        let items = std::sync::Arc::new(crate::catalog::ItemDb::load(&data).unwrap());
        let collection = Collection::new(&templates, items).unwrap();
        let engine = LootBoxEngine::new(&collection);

        let dir = tempdir().unwrap();
        let accounts = Accounts::new(dir.path(), dir.path().join("missing.json")).unwrap();
        let account = accounts.get("BoxTester").unwrap();
        account
            .edit_profile(|profile| {
                profile.loot_boxes = vec![LootBox {
                    id: 99,
                    box_type: 0,
                    name: "Standard".into(),
                }];
                Ok(Some(()))
            })
            .unwrap();
        let login = PreparedLogin {
            account,
            identity: crate::lobby_login::Identity::for_account(account_id_for_test()),
            tournament: false,
        };
        let messages = engine.open_box(&login, &collection, 99).unwrap();
        assert_eq!(messages.len(), 2);
        for (crc, id, fields) in &messages {
            schemas.encode(*crc, *id, fields).unwrap();
        }
        let profile = login.account.profile();
        assert!(profile.loot_boxes.len() >= 5);
        assert_eq!(profile.stats["boxes_opened"], 1);
    }

    fn account_id_for_test() -> u32 {
        crate::accounts::account_id_for("BoxTester")
    }
}
