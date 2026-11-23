use super::*;
use crate::script_init::Component;

#[test]
fn authored_callout_retries_with_its_own_values_in_both_body_streams() {
    assert_authored_action_retry("STU_7106C356", json!({"m_text":{"$":"STU_E7579408"}}));
}

#[test]
fn authored_effect_retries_with_its_own_values_in_both_body_streams() {
    assert_authored_action_retry(
        "STUStatescriptActionEffect",
        json!({
            "m_effect":{"$":"STUConfigVarEffect","m_effect":"0x0300000000000001","m_resourceKey":null},
            "m_3E3E203A":{"$":"STU_B5A0CAF0"},
            "m_dataFlow":[{"$":"STU_13D35171",
                "m_5A6D4FBD":{"$":"STUConfigVarDataFlow","m_5A6D4FBD":"0x0480000000000001"}}]
        }),
    );
}

fn assert_authored_action_retry(class: &str, mut fields: Json) {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/body-short-action-retry-v1.json"
    ))
    .unwrap();
    let capture = &fixture["capture"];
    let instance_slot = capture["instance_variable"].as_u64().unwrap() as u32;
    let entity_slot = capture["entity_variable"].as_u64().unwrap() as u32;
    let [first_instance, first_entity]: [i64; 2] =
        serde_json::from_value(capture["first"].clone()).unwrap();
    let [second_instance, second_entity]: [i64; 2] =
        serde_json::from_value(capture["second"].clone()).unwrap();
    let graph = 197;
    let instance = 3;
    let topic = 0x0240_0000_0000_b96f_u64;
    let row = |slot, scope| {
        json!({
            "$": "STUStatescriptRemoteSyncVar",
            "m_0D09D2D9": format!("0x0D8000000000{slot:04X}"),
            "m_56341592": scope
        })
    };
    fields["m_ADEB6E05"] = json!(1);
    fields["m_BF5B22B7"] = json!([row(instance_slot, 0), row(entity_slot, 1)]);
    if class == "STUStatescriptActionEffect" {
        fields["m_dataFlow"][0]["m_answer"] =
            json!({"$":"STUConfigVarDynamic","m_identifier":instance_slot,"m_60DB8F99":0});
    }
    let graphs = std::sync::Arc::new(
        GraphStore::from_json(
            &serde_json::to_vec(&json!({
                "version":1, "bodies":{}, "missing":[], "graphs":[{
                    "index":graph,"header":{"m_nodesBitCount":2,"m_statesBitCount":0,
                        "m_remoteSyncNodesBitCount":1,"m_syncVarsBitCount":1},
                    "fields":{},"containers":{},"items":2,"unknown_nodes":[],
                    "states":[],"entries":[0],"remote":[null,1],"sync_vars":[[instance_slot,0,0]],
                    "nodes":[
                        {"pos":0,"uid":0,"rid":0,"remote":-1,"state":null,"client_only":0,"server_only":0,
                         "class":"STUStatescriptEntryGameMessage","fields":{
                            "m_gameMessage":{"m_gameMessage":format!("0x{topic:016X}")},
                            "m_outPlug":{"$":"Plug","links":[[1,"m_inPlug"]]}
                        }},
                        {"pos":1,"uid":1,"rid":1,"remote":1,"state":null,"client_only":0,"server_only":0,
                         "class":class,"fields":fields}
                    ]
                }]
            }))
            .unwrap(),
        )
        .unwrap(),
    );
    let codecs = CodecTable::from_json(CODECS).unwrap();
    let encoder = SnapshotEncoder::new(&graphs, &codecs);
    let mut world = EmptyWorld;
    let mut component = Component::start(
        std::sync::Arc::clone(&graphs),
        ENTITY,
        Variables::new(),
        vec![Root::new(instance, graph)],
        10,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let before = component.snapshot().unwrap().clone();
    let mut send = |frame, local_value, body_value| {
        component
            .set_instance_variable(
                instance,
                instance_slot,
                Value::Integer(local_value),
                &mut world,
            )
            .unwrap();
        component
            .set_entity_variable(entity_slot, Value::Integer(body_value), &mut world)
            .unwrap();
        assert_eq!(
            component
                .queue_entity_message_from(ENTITY, topic, Variables::new())
                .unwrap(),
            1
        );
        component.advance_frame(frame, &mut world).unwrap();
        component.snapshot().unwrap().clone()
    };
    let first = send(11, first_instance, first_entity);
    let second = send(12, second_instance, second_entity);
    assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
    assert_eq!(second.frame_actions.len(), 2);

    for (view, kind) in [
        (View::Owner, DeltaKind::InitialOwner(12)),
        (View::Observer, DeltaKind::Observer { resend: false }),
    ] {
        let base = encoder.project(&before, view).unwrap();
        let first = encoder.project_next(&first, &base).unwrap();
        let second = encoder.project_next(&second, &base).unwrap();
        let stream_with_loss = || {
            let mut stream = BodyStream::new(ENTITY);
            let full = encoder.queue_full(&mut stream, &base).unwrap();
            stream.create_arrived();
            assert!(stream.arrived(full.last).advanced);
            let sent = encoder
                .queue_delta(&mut stream, &first, kind, 16)
                .unwrap()
                .unwrap();
            stream.lost(sent.last);
            stream
        };
        let mut stream = stream_with_loss();
        let mut control = stream_with_loss();
        let expected = encoder
            .queue_delta(&mut control, &first, kind, 16)
            .unwrap()
            .unwrap();
        let retry = encoder
            .queue_delta(&mut stream, &second, kind, 16)
            .unwrap()
            .unwrap();
        assert_eq!(retry.update.chunk, expected.update.chunk);
        assert!(stream.arrived(retry.last).advanced);
        let (_, baseline) = stream.base().unwrap();
        assert_eq!(baseline.acknowledged_callout(), 1);
        assert_eq!(
            baseline.variables.values[&entity_slot],
            Value::Integer(first_entity)
        );
        assert_eq!(
            baseline.instances[&instance].variables.values[&instance_slot],
            Value::Integer(first_instance)
        );
        let next = encoder
            .queue_delta(&mut stream, &second, kind, 16)
            .unwrap()
            .unwrap();
        assert!(stream.arrived(next.last).advanced);
        let (_, baseline) = stream.base().unwrap();
        assert_eq!(baseline.acknowledged_callout(), 2);
        assert_eq!(
            baseline.variables.values[&entity_slot],
            Value::Integer(second_entity)
        );
        assert_eq!(
            baseline.instances[&instance].variables.values[&instance_slot],
            Value::Integer(second_instance)
        );

        if view == View::Observer {
            let mut late = BodyStream::new(ENTITY);
            let full = encoder.queue_full(&mut late, &second).unwrap();
            late.create_arrived();
            assert!(late.arrived(full.last).advanced);
            assert_eq!(late.base().unwrap().1.acknowledged_callout(), 2);
            assert!(
                encoder
                    .queue_delta(&mut late, &second, kind, 16)
                    .unwrap()
                    .is_none()
            );
        }
    }
}
