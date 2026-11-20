use std::sync::Arc;

use crate::script_init::{ABORT_PLUG, Component, World};

use super::*;

const VOICE: &str = "STU_40E05F6B";
const PLAIN: &str = "STUStatescriptState";

struct VoiceWorld {
    context: Option<bool>,
    context_reads: usize,
    manager_reads: usize,
}

impl VoiceWorld {
    fn new(context: Option<bool>) -> Self {
        Self {
            context,
            context_reads: 0,
            manager_reads: 0,
        }
    }
}

impl World for VoiceWorld {
    fn position(&mut self, _: u32) -> Option<[f64; 3]> {
        None
    }

    fn entity_variable(&mut self, _: u32, _: u32) -> Option<Value> {
        None
    }

    fn voice_context(&mut self, site: Site) -> Option<bool> {
        assert_eq!(site.entity, ENTITY);
        self.context_reads += 1;
        self.context
    }

    fn voice_manager(&mut self, _: Site) -> Option<bool> {
        self.manager_reads += 1;
        None
    }
}

fn fixture() -> Json {
    serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/statescript-voice-empty-projection-v1.json"
    ))
    .unwrap()
}

fn start(
    fixture: &Json,
    class: &str,
    remote: bool,
    world: &mut VoiceWorld,
) -> (Arc<GraphStore>, Component) {
    let input = &fixture["input"];
    let graph_index = u32::try_from(input["graph"].as_u64().unwrap()).unwrap();
    let mut state = node(1, class, Some(0), input["dormant_fields"].clone());
    let remote_slots = if remote {
        state["remote"] = json!(0);
        json!([1])
    } else {
        json!([])
    };
    let root = graph(
        graph_index,
        vec![
            node(
                0,
                "STUStatescriptEntry",
                None,
                json!({"m_outPlug":{"$":"Plug","links":[[1,"m_beginPlug"]]}}),
            ),
            state,
            node(
                2,
                "STUStatescriptEntryGameMessage",
                None,
                json!({
                    "m_gameMessage":{"m_gameMessage":input["abort_message"]},
                    "m_outPlug":{"$":"Plug","links":[[1,ABORT_PLUG]]},
                }),
            ),
        ],
        json!([1]),
        json!([0, 2]),
        remote_slots,
        json!([]),
    );
    let bundle = json!({"version":1,"graphs":[root],"bodies":{},"missing":[]});
    let graphs = Arc::new(GraphStore::from_json(&serde_json::to_vec(&bundle).unwrap()).unwrap());
    let component = Component::start(
        Arc::clone(&graphs),
        ENTITY,
        Variables::new(),
        vec![Root::new(1, graph_index)],
        0,
        world,
        MAX_INIT_WORK,
    )
    .unwrap();
    (graphs, component)
}

fn codec_registry() -> Json {
    let mut registry: Json = serde_json::from_str(CODECS).unwrap();
    registry["classes"]["40E05F6B"] = json!({
        "name":VOICE,"codec":"none","stec":false,"status":"VERIFIED","layout":[],
    });
    registry
}

#[test]
fn stock_voice_with_absent_context_projects_the_empty_state_lifecycle() {
    let fixture = fixture();
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["evidence"], "synthetic-rust");
    let abort = crate::script_graph::asset_guid(&fixture["input"]["abort_message"]).unwrap();
    let codecs = CodecTable::from_json(&codec_registry().to_string()).unwrap();
    let capture = |class, remote| {
        let mut world = VoiceWorld::new(Some(false));
        let (graphs, mut component) = start(&fixture, class, remote, &mut world);
        component.advance_frame(1, &mut world).unwrap();
        let active = component.snapshot().unwrap().clone();
        assert!(active.initialization_complete(), "{:?}", active.diagnostics);
        let state = &active.instances[&1].states[&0];
        assert!(state.active);
        if class == VOICE {
            assert_eq!(state.kind, StateKind::VoiceStimulus);
            assert_eq!(world.context_reads, 1);
        }
        assert_eq!(world.manager_reads, 0);
        assert_eq!(
            component.queue_message(1, abort, Variables::new()).unwrap(),
            1
        );
        component.run_now(&mut world).unwrap();
        let retired = component.snapshot().unwrap();
        assert!(
            retired.initialization_complete(),
            "{:?}",
            retired.diagnostics
        );
        let encoder = SnapshotEncoder::new(&graphs, &codecs);

        [
            (View::Owner, DeltaKind::Correction(1), true),
            (
                View::Observer,
                DeltaKind::Observer { resend: false },
                remote,
            ),
        ]
        .map(|(view, kind, projected)| {
            let before = encoder.project(&active, view).unwrap();
            let states = &before.instances[&1].states;
            assert_eq!(states.contains_key(&0), projected);
            if let Some(state) = states.get(&0) {
                assert!(state.payload.is_empty());
            }
            let after = encoder.project_next(retired, &before).unwrap();
            assert!(after.instances[&1].states.is_empty());
            let delta = encoder
                .differences(&before, &after, &BTreeSet::new(), &BTreeSet::from([1]))
                .unwrap();
            assert_eq!(delta.keys.contains(&ChangeKey::State(1, 0)), projected);
            let removal = projected.then(|| encoder.delta_frame(&after, &delta, kind, 16).unwrap());
            (encoder.full(&before).unwrap(), removal)
        })
    };
    for remote in [false, true] {
        let voice = capture(VOICE, remote);
        let plain = capture(PLAIN, remote);
        for ((full, removal), (plain_full, plain_removal)) in voice.into_iter().zip(plain) {
            assert_eq!(full.bit_len(), plain_full.bit_len());
            assert_eq!(full.as_bytes(), plain_full.as_bytes());
            assert_eq!(removal.is_some(), plain_removal.is_some());
            if let (Some(removal), Some(plain_removal)) = (removal, plain_removal) {
                assert_eq!(removal.bit_len(), plain_removal.bit_len());
                assert_eq!(removal.as_bytes(), plain_removal.as_bytes());
            }
        }
    }
}

#[test]
fn stock_voice_projection_keeps_host_diagnostics_and_requires_a_verified_empty_codec() {
    let fixture = fixture();
    let registry = codec_registry();
    let codecs = CodecTable::from_json(&registry.to_string()).unwrap();
    for (context, issue, manager_reads) in [
        (None, "voice stimulus context", 0),
        (Some(true), "voice manager lookup", 1),
    ] {
        let mut world = VoiceWorld::new(context);
        let (graphs, component) = start(&fixture, VOICE, true, &mut world);
        let encoder = SnapshotEncoder::new(&graphs, &codecs);
        assert_eq!(world.manager_reads, manager_reads);
        for view in [View::Owner, View::Observer] {
            assert!(matches!(
                encoder.project(component.snapshot().unwrap(), view),
                Err(SnapshotError::Incomplete { diagnostics, .. })
                    if matches!(diagnostics.as_slice(), [Diagnostic {
                        issue: Issue::UnsupportedField(field), ..
                    }] if field == &issue)
            ));
        }
    }

    let (graphs, component) = start(&fixture, VOICE, true, &mut VoiceWorld::new(Some(false)));
    for (field, value) in [
        ("status", json!("UNVERIFIED")),
        ("layout", json!([{"op":"bits","f":"flag","n":1}])),
        ("codec", json!("switch")),
    ] {
        let mut invalid_registry = registry.clone();
        invalid_registry["classes"]["40E05F6B"][field] = value;
        let invalid = CodecTable::from_json(&invalid_registry.to_string()).unwrap();
        let encoder = SnapshotEncoder::new(&graphs, &invalid);
        assert!(matches!(
            encoder.project(component.snapshot().unwrap(), View::Owner),
            Err(SnapshotError::UnsupportedState { class, .. }) if class == VOICE
        ));
    }
}
