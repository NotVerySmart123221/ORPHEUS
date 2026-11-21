use std::{io, sync::Arc};

use orpheus_statescript::{
    script_graph::GraphStore,
    script_init::{Component, EmptyWorld, MAX_INIT_WORK, player_roots},
    script_wire::{Value, Variables},
};
use serde::Deserialize;
use serde_json::{Value as Json, json};

const BODY_KEY: &str = "0x040000000000AF12";
const BODY: u64 = 0x0400_0000_0000_af12;
const MODES: [u64; 2] = [0x0230_0000_0000_af10, 0x0230_0000_0000_af11];
const RECEIVER: u32 = 621;
const INHIBIT: u32 = 622;
const PARENT: u32 = 624;
const DAMAGE: u64 = 0x0240_0000_0000_ca80;
const ADMISSION: u32 = 713;
const MARKER: u32 = 714;

#[derive(Deserialize)]
struct Contract {
    schema_version: u8,
    case_id: String,
    evidence: String,
    input: Input,
    expected: Expected,
}

#[derive(Deserialize)]
struct Input {
    bundle: Json,
    damage: f64,
    nested_mount: NestedMount,
}

#[derive(Deserialize)]
struct NestedMount {
    graph: Json,
    parent: Json,
}

#[derive(Deserialize)]
struct Expected {
    closed_marker: Option<f64>,
    open_marker: f64,
}

fn fixture() -> Contract {
    let contract: Contract = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-charge-compatibility-v1.json"
    ))
    .unwrap();
    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.case_id, "script-charge-compatibility.synthetic.v1");
    assert_eq!(contract.evidence, "synthetic-rust");
    contract
}

fn load(bundle: &Json) -> io::Result<GraphStore> {
    GraphStore::from_json(&serde_json::to_vec(bundle).unwrap())
}

fn profile(bundle: &mut Json) -> &mut Json {
    &mut bundle["bodies"][BODY_KEY]["charge_compatibility"]
}

fn nested_bundle(input: &Input) -> Json {
    let mut bundle = input.bundle.clone();
    let mount = &input.nested_mount;
    bundle["graphs"]
        .as_array_mut()
        .unwrap()
        .push(mount.graph.clone());
    bundle["bodies"][BODY_KEY]["graphs"][0]["graph"] = mount.graph["index"].clone();
    profile(&mut bundle)["parent"] = mount.parent.clone();
    bundle
}

fn damage_parameters(amount: f64) -> Variables {
    Variables::from([
        (0x13, Value::Float(amount)),
        (0x15, Value::Entity(655)),
        (0x3d, Value::Array(Vec::new())),
        (0x624, Value::Array(Vec::new())),
    ])
}

fn check_rejected(bundle: &Json, label: &str) {
    match load(bundle) {
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{label}: {error}"),
        Ok(_) => panic!("accepted {label}"),
    }
}

#[test]
fn charge_compatibility_loads_one_public_root_with_an_optional_inhibit() {
    let contract = fixture();
    let graphs = Arc::new(load(&contract.input.bundle).unwrap());
    let definition = &graphs.bodies()[&BODY];
    let compatibility = definition.charge_compatibility.as_ref().unwrap();
    assert_eq!(compatibility.modes.len(), MODES.len());
    assert!(MODES.iter().all(|mode| compatibility.modes.contains(mode)));
    assert_eq!(compatibility.receiver_graph, RECEIVER);
    assert_eq!(compatibility.admission_variable, ADMISSION);
    assert_eq!(compatibility.damage_message, DAMAGE);
    assert!(compatibility.parent.is_none());
    let inhibit = compatibility.inhibit.as_ref().unwrap();
    assert_eq!((inhibit.graph, inhibit.state), (INHIBIT, 0));

    let roots = player_roots(definition, &graphs).unwrap();
    let [root] = roots.as_slice() else {
        panic!("expected one public receiver root");
    };
    assert_eq!(root.graph, RECEIVER);
    assert!(!root.is_server_only());
    let receiver = root.id;
    let mut world = EmptyWorld;
    let mut body = Component::start(
        Arc::clone(&graphs),
        654,
        Variables::new(),
        roots,
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let parameters = damage_parameters(contract.input.damage);
    assert_eq!(
        body.queue_message(receiver, DAMAGE, parameters.clone())
            .unwrap(),
        1
    );
    body.run_now(&mut world).unwrap();
    assert_eq!(
        body.snapshot().unwrap().variables.get(&MARKER),
        contract.expected.closed_marker.map(Value::Float).as_ref()
    );
    body.set_entity_variable(ADMISSION, Value::Bool(true), &mut world)
        .unwrap();
    assert_eq!(body.queue_message(receiver, DAMAGE, parameters).unwrap(), 1);
    body.run_now(&mut world).unwrap();
    let snapshot = body.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&MARKER],
        Value::Float(contract.expected.open_marker)
    );
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );

    let mut optional = contract.input.bundle;
    profile(&mut optional)
        .as_object_mut()
        .unwrap()
        .remove("inhibit");
    let loaded = load(&optional).unwrap();
    assert!(
        loaded.bodies()[&BODY]
            .charge_compatibility
            .as_ref()
            .unwrap()
            .inhibit
            .is_none()
    );
    for (field, mount) in [("manager", json!(RECEIVER)), ("weapons", json!([RECEIVER]))] {
        let mut bundle = optional.clone();
        bundle["bodies"][BODY_KEY]["graphs"] = json!([]);
        bundle["bodies"][BODY_KEY][field] = mount;
        assert!(load(&bundle).is_ok(), "public {field} mount");
    }
    optional["bodies"][BODY_KEY]
        .as_object_mut()
        .unwrap()
        .remove("charge_compatibility");
    assert!(
        load(&optional).unwrap().bodies()[&BODY]
            .charge_compatibility
            .is_none()
    );
}

#[test]
fn charge_compatibility_loads_a_sole_child_damage_receiver() {
    let contract = fixture();
    let bundle = nested_bundle(&contract.input);
    let graphs = Arc::new(load(&bundle).unwrap());
    let definition = &graphs.bodies()[&BODY];
    let compatibility = definition.charge_compatibility.as_ref().unwrap();
    assert_eq!(compatibility.receiver_graph, RECEIVER);
    let parent = compatibility.parent.as_ref().unwrap();
    assert_eq!((parent.graph, parent.state), (PARENT, 0));
    let roots = player_roots(definition, &graphs).unwrap();
    let [root] = roots.as_slice() else {
        panic!("expected one public parent root");
    };
    assert_eq!(root.graph, PARENT);
    assert!(!root.is_server_only());
    let parent_instance = root.id;
    let mut world = EmptyWorld;
    let mut body = Component::start(
        Arc::clone(&graphs),
        654,
        Variables::from([(ADMISSION, Value::Bool(true))]),
        roots,
        0,
        &mut world,
        MAX_INIT_WORK,
    )
    .unwrap();
    let snapshot = body.snapshot().unwrap();
    let mut receivers = snapshot
        .instances
        .iter()
        .filter(|(_, instance)| instance.graph == RECEIVER);
    let (&receiver, child) = receivers.next().expect("active receiver child");
    assert_eq!(child.parent, Some((parent_instance, 0)));
    assert!(receivers.next().is_none());
    assert_eq!(
        body.queue_message(receiver, DAMAGE, damage_parameters(contract.input.damage))
            .unwrap(),
        1
    );
    body.run_now(&mut world).unwrap();
    let snapshot = body.snapshot().unwrap();
    assert_eq!(
        snapshot.variables[&MARKER],
        Value::Float(contract.expected.open_marker)
    );
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );

    let mut alias = bundle;
    alias["graphs"][2]["nodes"][1]["class"] = json!("STU_37D754C8");
    assert!(
        load(&alias).is_ok(),
        "stock SubScript class uses the same mount shape"
    );
}

#[test]
fn charge_compatibility_rejects_mismatched_or_duplicate_child_mounts() {
    let original = nested_bundle(&fixture().input);
    let mut missing_parent = original.clone();
    profile(&mut missing_parent)
        .as_object_mut()
        .unwrap()
        .remove("parent");
    check_rejected(&missing_parent, "missing parent declaration");

    let mut wrong_parent = original.clone();
    profile(&mut wrong_parent)["parent"]["graph"] = json!("0x058000000000026E");
    check_rejected(&wrong_parent, "parent graph without a public mount");
    let mut unmatched_parent = original.clone();
    profile(&mut unmatched_parent)["parent"]["state"] = json!(1);
    check_rejected(&unmatched_parent, "missing parent slot");

    let mut wrong_class = original.clone();
    wrong_class["graphs"][2]["nodes"][1]["class"] = json!("STUStatescriptState");
    check_rejected(&wrong_class, "parent slot without a SubScript state");
    let mut other_child = original.clone();
    other_child["graphs"][2]["nodes"][1]["fields"]["m_36F7B64C"]["m_36F7B64C"]["m_C974E369"]["m_graph"] =
        json!("0x058000000000026E");
    check_rejected(&other_child, "parent slot mounting a different graph");

    let mut wide_child = original.clone();
    let wide_asset = json!("0x058000000001026D");
    wide_child["graphs"][0]["index"] = json!(0x0001_026d);
    wide_child["graphs"][2]["nodes"][1]["fields"]["m_36F7B64C"]["m_36F7B64C"]["m_C974E369"]["m_graph"] =
        wide_asset.clone();
    profile(&mut wide_child)["receiver_graph"] = wide_asset;
    let error = load(&wide_child).expect_err("wide receiver rejected");
    assert!(
        error.to_string().contains("SubScript index range"),
        "{error}"
    );

    let mut extra_root = original;
    extra_root["bodies"][BODY_KEY]["weapons"] = json!([RECEIVER]);
    check_rejected(
        &extra_root,
        "receiver mounted as both a child and a public root",
    );
}

#[test]
fn charge_compatibility_rejects_malformed_profiles_and_mounts() {
    let original = fixture().input.bundle;
    for (label, field, value) in [
        ("empty modes", "modes", json!([])),
        (
            "duplicate modes",
            "modes",
            json!(["0x023000000000AF10", "0x023000000000AF10"]),
        ),
        (
            "wrong mode resource type",
            "modes",
            json!(["0x024000000000AF10"]),
        ),
        (
            "invalid receiver graph",
            "receiver_graph",
            json!("0x058000010000026D"),
        ),
        ("zero admission variable", "admission_variable", json!(0)),
        (
            "unbound admission variable",
            "admission_variable",
            json!(715),
        ),
        (
            "wrong message type",
            "damage_message",
            json!("0x0A4000000000CA80"),
        ),
        (
            "unbound damage receiver",
            "damage_message",
            json!("0x024000000000CA81"),
        ),
        (
            "unknown inhibit state",
            "inhibit",
            json!({"graph":"0x058000000000026E", "state":1}),
        ),
    ] {
        let mut bundle = original.clone();
        profile(&mut bundle)[field] = value;
        check_rejected(&bundle, label);
    }

    let mut unknown_ability = original.clone();
    unknown_ability["graphs"][1]["nodes"][0]["class"] = json!("STUStatescriptState");
    check_rejected(&unknown_ability, "inhibit pointing to an ordinary state");

    let mut mount = original.clone();
    mount["bodies"][BODY_KEY]["manager"] = json!(RECEIVER);
    check_rejected(&mount, "duplicate public receiver root");
    mount["bodies"][BODY_KEY]["manager"] = Json::Null;
    mount["bodies"][BODY_KEY]["server_graphs"] = json!([{"graph": RECEIVER, "m_75408640": 0}]);
    check_rejected(&mount, "receiver also mounted privately");
    mount["bodies"][BODY_KEY]["graphs"] = json!([]);
    check_rejected(&mount, "private receiver mount");
    mount["bodies"][BODY_KEY]["server_graphs"] = json!([]);
    check_rejected(&mount, "unmounted receiver");
}

#[test]
fn charge_compatibility_requires_ordinary_distinct_damage_bindings() {
    let original = fixture().input.bundle;
    let mut client = original.clone();
    client["graphs"][0]["nodes"][0]["server_only"] = json!(false);
    client["graphs"][0]["nodes"][0]["client_only"] = json!(true);
    check_rejected(&client, "client-only damage receiver");
    let mut directed = original.clone();
    directed["graphs"][0]["nodes"][0]["room_message_entry"] = json!(true);
    check_rejected(&directed, "room-only damage receiver");

    let mut duplicate = original.clone();
    let graph = &mut duplicate["graphs"][0];
    let mut second = graph["nodes"][0].clone();
    for field in ["pos", "uid", "rid"] {
        second[field] = json!(3);
    }
    graph["nodes"].as_array_mut().unwrap().push(second);
    graph["entries"].as_array_mut().unwrap().push(json!(3));
    graph["items"] = json!(4);
    graph["header"]["m_nodesBitCount"] = json!(3);
    check_rejected(&duplicate, "duplicate ordinary damage receiver");

    for (label, field, value) in [
        (
            "duplicate damage slot",
            "m_B5051BCE",
            json!("0x0A40000000000013"),
        ),
        (
            "wrong damage slot type",
            "m_B5051BCE",
            json!("0x0240000000000624"),
        ),
        (
            "duplicate damage output",
            "m_out_Var",
            json!({"$":"STUConfigVarDynamic", "m_identifier":81, "m_60DB8F99":0}),
        ),
        (
            "invalid damage output",
            "m_out_Var",
            json!({"$":"STUConfigVarInt", "m_value":84}),
        ),
    ] {
        let mut bundle = original.clone();
        bundle["graphs"][0]["nodes"][0]["fields"]["m_params"][3][field] = value;
        check_rejected(&bundle, label);
    }
    let mut missing = original;
    missing["graphs"][0]["nodes"][0]["fields"]["m_params"]
        .as_array_mut()
        .unwrap()
        .pop();
    check_rejected(&missing, "missing damage slot");
}

#[test]
fn charge_compatibility_is_exclusive_with_an_authored_charge_root() {
    let mut bundle = fixture().input.bundle;
    let authored: Json = serde_json::from_str(include_str!(
        "../../../../fixtures/behavior/script-authored-charge-route-v1.json"
    ))
    .unwrap();
    let definition = &authored["input"]["bundle"]["bodies"]["0x040000000000B013"];
    bundle["graphs"]
        .as_array_mut()
        .unwrap()
        .push(authored["input"]["bundle"]["graphs"][0].clone());
    bundle["bodies"][BODY_KEY]["server_graphs"] = definition["server_graphs"].clone();
    bundle["bodies"][BODY_KEY]["authored_charge_graph"] =
        definition["authored_charge_graph"].clone();
    check_rejected(&bundle, "simultaneous charge routes");
    bundle["bodies"][BODY_KEY]
        .as_object_mut()
        .unwrap()
        .remove("charge_compatibility");
    assert!(
        load(&bundle).is_ok(),
        "the private authored route is independently valid"
    );
}
