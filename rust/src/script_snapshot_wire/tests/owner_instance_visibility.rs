use super::*;
use crate::{
    bits::BitWriter,
    script_init::Component,
    script_stream::OWNER_SETTLE_FRAMES,
    script_wire::{Instance, Layout},
};
use std::sync::Arc;

struct Contract {
    graphs: Arc<GraphStore>,
    private: u32,
    input: u32,
    public: u32,
    alias: u32,
    base: i64,
    values: Vec<i64>,
    entity_same_id: i64,
    public_value: i64,
}

impl Contract {
    fn load() -> Self {
        let fixture: Json = serde_json::from_str(include_str!(
            "../../../../fixtures/behavior/script-owner-instance-visibility-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture["schema_version"], 1);
        assert_eq!(
            fixture["case_id"],
            "script-owner-instance-visibility.synthetic.v1"
        );
        assert_eq!(fixture["evidence"], "synthetic-rust");
        let input = &fixture["input"];
        let id = |name| u32::try_from(input[name].as_u64().unwrap()).unwrap();
        Self {
            graphs: Arc::new(
                GraphStore::from_json(&serde_json::to_vec(&input["bundle"]).unwrap()).unwrap(),
            ),
            private: id("private"),
            input: id("input"),
            public: id("public"),
            alias: id("alias"),
            base: input["base"].as_i64().unwrap(),
            values: serde_json::from_value(input["values"].clone()).unwrap(),
            entity_same_id: input["entity_same_id"].as_i64().unwrap(),
            public_value: input["public_value"].as_i64().unwrap(),
        }
    }

    fn component(&self) -> Component {
        let mut root = Root::new(1, 121);
        root.variables = Variables::from([
            (self.private, Value::Integer(self.base)),
            (self.input, Value::Integer(self.values[0])),
            (self.public, Value::Integer(self.public_value)),
        ]);
        Component::start(
            Arc::clone(&self.graphs),
            ENTITY,
            Variables::from([(self.private, Value::Integer(self.entity_same_id))]),
            vec![root],
            0,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .unwrap()
    }

    fn expected_full(&self, owner: bool, private_value: i64) -> BitWriter {
        let graph = self.graphs.graph(121).unwrap();
        let mut instance = Instance::new(1, Layout::try_from(graph).unwrap(), None);
        instance
            .presence
            .insert(1, Value::Integer(self.public_value));
        if owner {
            instance.presence.insert(0, Value::Integer(private_value));
            instance
                .variables
                .insert(self.input, Value::Integer(private_value));
        }
        crate::script_wire::remote_full(
            &[instance],
            &Variables::from([(self.private, Value::Integer(self.entity_same_id))]),
        )
        .unwrap()
    }
}

fn bytes(bits: &BitWriter) -> (usize, Vec<u8>) {
    (bits.bit_len(), bits.as_bytes().to_vec())
}

#[test]
fn owner_instance_values_follow_stack_lifetime_without_observer_wire_updates() {
    let contract = Contract::load();
    let codecs = CodecTable::from_json(CODECS).unwrap();
    let encoder = SnapshotEncoder::new(&contract.graphs, &codecs);
    let mut component = contract.component();
    let start = component.snapshot().unwrap();
    assert!(start.initialization_complete(), "{:?}", start.diagnostics);
    assert_eq!(
        start.instances[&1].variables[&contract.private],
        Value::Integer(contract.base)
    );
    let mut owner = encoder.project(start, View::Owner).unwrap();
    let mut observer = encoder.project(start, View::Observer).unwrap();
    let observer_bytes = bytes(&encoder.full(&observer).unwrap());
    assert_eq!(
        observer_bytes,
        bytes(&contract.expected_full(false, contract.values[0]))
    );
    assert_eq!(
        bytes(&encoder.full(&owner).unwrap()),
        bytes(&contract.expected_full(true, contract.values[0]))
    );
    assert_eq!(
        observer.variables.values[&contract.private],
        Value::Integer(contract.entity_same_id)
    );
    assert!(!observer.entity_exclusions.contains(&contract.private));
    for id in [contract.private, contract.input] {
        assert!(!observer.instances[&1].variables.values.contains_key(&id));
        assert!(!observer.instances[&1].variables.bindings.contains_key(&id));
    }
    let mut owner_stream = BodyStream::new(ENTITY);
    let mut observer_stream = BodyStream::new(ENTITY);
    for (stream, projected) in [
        (&mut owner_stream, &owner),
        (&mut observer_stream, &observer),
    ] {
        let full = encoder.queue_full(stream, projected).unwrap();
        stream.create_arrived();
        stream.sent(full.last);
        assert!(stream.arrived(full.last).advanced);
    }
    let initial = encoder
        .queue_delta(&mut owner_stream, &owner, DeltaKind::InitialOwner(1), 16)
        .unwrap()
        .unwrap();
    assert_eq!(owner_stream.sent(initial.last).unwrap().command_frame, 1);
    assert!(owner_stream.arrived(initial.last).advanced);
    let first_correction = 1 + OWNER_SETTLE_FRAMES;
    for (index, &value) in contract.values.iter().enumerate().skip(1) {
        let frame = first_correction + u32::try_from(2 * (index - 1)).unwrap();
        component
            .set_instance_variable(1, contract.input, Value::Integer(value), &mut EmptyWorld)
            .unwrap();
        component.advance_frame(frame, &mut EmptyWorld).unwrap();
        let snapshot = component.snapshot().unwrap();
        assert_eq!(
            snapshot.instances[&1].variables[&contract.private],
            Value::Integer(contract.base)
        );
        owner = encoder.project_next(snapshot, &owner).unwrap();
        observer = encoder.project_next(snapshot, &observer).unwrap();
        assert_eq!(
            owner.instances[&1].variables.values[&contract.private],
            Value::Integer(value)
        );
        assert_eq!(
            bytes(&encoder.full(&owner).unwrap()),
            bytes(&contract.expected_full(true, value))
        );
        assert_eq!(bytes(&encoder.full(&observer).unwrap()), observer_bytes);
        let update = encoder
            .queue_delta(&mut owner_stream, &owner, DeltaKind::Correction(frame), 16)
            .unwrap()
            .unwrap();
        let sent = owner_stream.sent(update.last).unwrap();
        assert!(
            sent.changes
                .contains(&ChangeKey::Variable(1, contract.private))
        );
        assert!(
            sent.changes
                .contains(&ChangeKey::Variable(1, contract.input))
        );
        let repeated = encoder
            .queue_delta(
                &mut owner_stream,
                &owner,
                DeltaKind::Correction(frame + 1),
                16,
            )
            .unwrap()
            .unwrap();
        assert_ne!(
            bytes(update.update.chunk.as_ref().unwrap()),
            bytes(repeated.update.chunk.as_ref().unwrap())
        );
        let resent = owner_stream.sent(repeated.last).unwrap();
        assert!(
            resent
                .changes
                .contains(&ChangeKey::Variable(1, contract.private))
        );
        assert!(owner_stream.arrived(repeated.last).advanced);
        for resend in [false, true] {
            assert!(
                encoder
                    .queue_delta(
                        &mut observer_stream,
                        &observer,
                        DeltaKind::Observer { resend },
                        16
                    )
                    .unwrap()
                    .is_none()
            );
        }
        let (_, acknowledged) = owner_stream.base().unwrap();
        let settled = encoder
            .differences(acknowledged, &owner, &BTreeSet::new(), &BTreeSet::new())
            .unwrap();
        assert!(
            !settled
                .keys
                .contains(&ChangeKey::Variable(1, contract.private))
        );
    }
    let release_frame = first_correction + u32::try_from(2 * (contract.values.len() - 1)).unwrap();
    component
        .advance_frame(release_frame, &mut EmptyWorld)
        .unwrap();
    component.debug_end_state(1, 0, &mut EmptyWorld).unwrap();
    owner = encoder
        .project_next(component.snapshot().unwrap(), &owner)
        .unwrap();
    observer = encoder
        .project_next(component.snapshot().unwrap(), &observer)
        .unwrap();
    assert_eq!(
        owner.instances[&1].variables.values[&contract.private],
        Value::Integer(contract.base)
    );
    let release = encoder
        .queue_delta(
            &mut owner_stream,
            &owner,
            DeltaKind::Correction(release_frame),
            16,
        )
        .unwrap()
        .unwrap();
    let sent = owner_stream.sent(release.last).unwrap();
    assert!(
        sent.changes
            .contains(&ChangeKey::Variable(1, contract.private))
    );
    assert!(owner_stream.arrived(release.last).advanced);
    assert_eq!(bytes(&encoder.full(&observer).unwrap()), observer_bytes);
    assert!(
        encoder
            .queue_delta(
                &mut observer_stream,
                &observer,
                DeltaKind::Observer { resend: false },
                16
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn owner_visibility_is_per_instance_and_public_aliases_keep_their_own_scope() {
    let contract = Contract::load();
    let codecs = CodecTable::from_json(CODECS).unwrap();
    let encoder = SnapshotEncoder::new(&contract.graphs, &codecs);
    let mut component = contract.component();
    let before = encoder
        .project(component.snapshot().unwrap(), View::Observer)
        .unwrap();
    component.begin_state(1, 1, &mut EmptyWorld).unwrap();
    let child = component
        .snapshot()
        .unwrap()
        .instances
        .iter()
        .find_map(|(&id, instance)| (instance.graph == 122).then_some(id))
        .unwrap();
    assert_eq!(
        component.snapshot().unwrap().instances[&child].parent,
        Some((1, 1))
    );
    component
        .set_instance_variable(child, contract.private, Value::Integer(91), &mut EmptyWorld)
        .unwrap();
    component
        .set_instance_variable(
            child,
            contract.public,
            Value::Integer(contract.public_value),
            &mut EmptyWorld,
        )
        .unwrap();
    let owner = encoder
        .project(component.snapshot().unwrap(), View::Owner)
        .unwrap();
    let observer = encoder
        .project_next(component.snapshot().unwrap(), &before)
        .unwrap();
    assert_eq!(
        owner.instances[&child].variables.values[&contract.private],
        Value::Integer(91)
    );
    assert!(
        !observer.instances[&child]
            .variables
            .values
            .contains_key(&contract.private)
    );
    assert_eq!(
        observer.instances[&child].variables.values[&contract.public],
        Value::Integer(contract.public_value)
    );
    let introduction = encoder
        .differences(&before, &observer, &BTreeSet::new(), &BTreeSet::new())
        .unwrap();
    assert!(introduction.keys.contains(&ChangeKey::Instance(child)));
    assert!(
        !introduction
            .keys
            .contains(&ChangeKey::Variable(child, contract.private))
    );
    let first_bytes = bytes(
        &encoder
            .delta_frame(
                &observer,
                &introduction,
                DeltaKind::Observer { resend: false },
                16,
            )
            .unwrap(),
    );
    let full = bytes(&encoder.full(&observer).unwrap());
    component
        .set_instance_variable(child, contract.private, Value::Integer(92), &mut EmptyWorld)
        .unwrap();
    let next = encoder
        .project_next(component.snapshot().unwrap(), &observer)
        .unwrap();
    assert_eq!(bytes(&encoder.full(&next).unwrap()), full);
    let repeated = encoder
        .differences(&before, &next, &BTreeSet::new(), &BTreeSet::new())
        .unwrap();
    assert_eq!(
        bytes(
            &encoder
                .delta_frame(&next, &repeated, DeltaKind::Observer { resend: true }, 16)
                .unwrap()
        ),
        first_bytes
    );
    component
        .set_instance_variable(
            1,
            contract.alias,
            Value::Integer(contract.values[2]),
            &mut EmptyWorld,
        )
        .unwrap();
    let alias = encoder
        .project_next(component.snapshot().unwrap(), &next)
        .unwrap();
    assert_eq!(
        alias.instances[&1].variables.values[&contract.alias],
        Value::Integer(contract.values[2])
    );
    assert_ne!(bytes(&encoder.full(&alias).unwrap()), full);
}
