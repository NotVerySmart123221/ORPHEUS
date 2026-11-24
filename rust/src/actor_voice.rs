//! Private actor voice selection data. Audio delivery stays in the room queue.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

use crate::runtime::format::parse_guid;
use serde::{Deserialize, Deserializer, de::Error};

const MAX_CATALOG_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Guid(u64);

impl<'de> Deserialize<'de> for Guid {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let text = String::deserialize(input)?;
        parse_guid(&text)
            .filter(|&id| id != 0)
            .map(Self)
            .ok_or_else(|| D::Error::custom("invalid actor voice GUID"))
    }
}

type VoiceSet = BTreeMap<Guid, Vec<Guid>>;

pub(crate) struct VoiceOptions<'a> {
    stimulus: Guid,
    lines: &'a [Guid],
}

impl VoiceOptions<'_> {
    pub(crate) fn choose(&self, choose: &mut dyn FnMut(usize) -> usize) -> Option<(u64, u64)> {
        let index = if self.lines.len() > 1 {
            choose(self.lines.len())
        } else {
            0
        };
        Some((self.stimulus.0, self.lines.get(index)?.0))
    }
}

#[derive(Deserialize)]
pub(crate) struct ActorVoiceCatalog {
    criteria: BTreeMap<Guid, Vec<Guid>>,
    heroes: BTreeMap<Guid, Guid>,
    #[serde(default)]
    skins: BTreeMap<Guid, BTreeMap<Guid, Guid>>,
    sets: BTreeMap<Guid, VoiceSet>,
    #[serde(default)]
    equipped: BTreeMap<Guid, Guid>,
}

impl ActorVoiceCatalog {
    pub(crate) fn load(path: &Path) -> io::Result<Option<Self>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_CATALOG_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "actor voice catalog exceeds the size limit",
            ));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(crate) fn options(&self, hero: u64, skin: u64, criterion: u64) -> Option<VoiceOptions<'_>> {
        self.options_for(hero, skin, self.criteria.get(&Guid(criterion))?)
    }

    pub(crate) fn stimulus_options(
        &self,
        hero: u64,
        skin: u64,
        stimulus: u64,
    ) -> Option<VoiceOptions<'_>> {
        self.options_for(hero, skin, &[Guid(stimulus)])
    }

    pub(crate) fn equipped_options(
        &self,
        hero: u64,
        skin: u64,
        effect: u64,
    ) -> Option<VoiceOptions<'_>> {
        self.options_for(
            hero,
            skin,
            std::slice::from_ref(self.equipped.get(&Guid(effect))?),
        )
    }

    fn options_for(&self, hero: u64, skin: u64, requested: &[Guid]) -> Option<VoiceOptions<'_>> {
        let base = self.heroes.get(&Guid(hero))?;
        let selected = self
            .skins
            .get(&Guid(skin))
            .and_then(|map| map.get(base))
            .unwrap_or(base);
        let lines_by_stimulus = self.sets.get(selected)?;
        requested.iter().find_map(|&stimulus| {
            let lines = lines_by_stimulus.get(&stimulus)?;
            (!lines.is_empty()).then_some(VoiceOptions { stimulus, lines })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_skin_voice_set_ordered_stimuli_and_variant() {
        use serde_json::{Value, json};

        let fixture: Value =
            serde_json::from_str(include_str!("../../fixtures/behavior/player-voice-v1.json"))
                .unwrap();
        let selection = &fixture["rust_selection"];
        let input = &fixture["input"];
        let mut data = input["fixtures"].clone();
        let text = |key| selection[key].as_str().unwrap();
        let key = text("criterion");
        let hero = input["hero"].as_str().unwrap();
        let skin = input["skin"].as_str().unwrap();
        let base = data["heroes"][hero].as_str().unwrap().to_owned();
        let selected = data["skins"][skin][&base].as_str().unwrap().to_owned();
        let stimulus = data["criteria"][key][0].as_str().unwrap().to_owned();
        data["criteria"][key]
            .as_array_mut()
            .unwrap()
            .insert(0, json!(text("unavailable_stimulus")));
        data["sets"][&selected][&stimulus]
            .as_array_mut()
            .unwrap()
            .push(json!(text("alternate_line")));
        data["sets"][&base] = json!({(stimulus.clone()): [text("stock_line")]});
        data["equipped"] = json!({(text("equipped_effect")): stimulus});
        let catalog: ActorVoiceCatalog = serde_json::from_value(data).unwrap();
        let guid = |value| parse_guid(value).unwrap();
        assert_eq!(
            catalog
                .options(guid(hero), guid(skin), guid(key))
                .unwrap()
                .choose(&mut |count| {
                    assert_eq!(count, 2);
                    1
                }),
            Some((guid(&stimulus), guid(text("alternate_line"))))
        );
        assert_eq!(
            catalog
                .options(guid(hero), 0, guid(key))
                .unwrap()
                .choose(&mut |_| panic!("one line needs no draw")),
            Some((guid(&stimulus), guid(text("stock_line"))))
        );
        assert!(catalog.options(1, guid(skin), guid(key)).is_none());
        assert_eq!(
            catalog
                .equipped_options(guid(hero), guid(skin), guid(text("equipped_effect")))
                .unwrap()
                .choose(&mut |_| 1),
            Some((guid(&stimulus), guid(text("alternate_line"))))
        );
        assert!(
            catalog
                .equipped_options(guid(hero), guid(skin), 1)
                .is_none()
        );
        assert_eq!(
            catalog
                .stimulus_options(guid(hero), guid(skin), guid(&stimulus))
                .unwrap()
                .choose(&mut |_| 1),
            Some((guid(&stimulus), guid(text("alternate_line"))))
        );
        assert!(
            catalog
                .stimulus_options(guid(hero), guid(skin), 1)
                .is_none()
        );
    }

    #[test]
    #[ignore = "requires the private 104319 actor voice catalog"]
    fn prepared_actor_voice_catalog_contains_playable_actor_criteria() {
        use std::path::PathBuf;

        let root = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
        let catalog = ActorVoiceCatalog::load(&root.join(crate::runtime::ACTOR_VOICE_CATALOG))
            .unwrap()
            .unwrap();
        let playable = |hero: Guid| {
            catalog
                .criteria
                .keys()
                .any(|criterion| catalog.options(hero.0, 0, criterion.0).is_some())
        };
        assert!(
            catalog
                .heroes
                .keys()
                .copied()
                .filter(|&hero| playable(hero))
                .count()
                > 1
        );
    }
}
