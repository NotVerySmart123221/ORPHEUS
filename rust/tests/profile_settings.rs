use std::{fs, path::PathBuf};

use orpheus::{
    accounts::{Accounts, Profile, load_or_create_profile, save_profile},
    jam_codec::{Schemas, Value},
    lobby_player_messages::{SOCIAL_OUT, SettingsRequest, settings},
    retail_templates::{fields_to_json, restore_fields},
};
use serde_json::{Value as Json, json};

fn fixture_path() -> PathBuf {
    let dir = std::env::var_os("ORPHEUS_BEHAVIOR_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/behavior"));
    dir.join("profile-settings-v1.json")
}

fn test_schemas() -> Schemas {
    Schemas::from_json(
        r#"{"1A9879A4":{"20802":[
            {"type":14,"off":120,"count":0,"array":false,"fields":[
                {"type":2,"off":0,"count":0,"array":false},
                {"type":2,"off":4,"count":0,"array":false}
            ]},
            {"type":14,"off":264,"count":0,"array":false,"fields":[
                {"type":0,"off":4,"count":0,"array":false}
            ]},
            {"type":14,"off":269,"count":0,"array":false,"fields":[
                {"type":0,"off":0,"count":0,"array":false},
                {"type":2,"off":5,"count":0,"array":false}
            ]},
            {"type":14,"off":288,"count":0,"array":false,"fields":[
                {"type":2,"off":0,"count":0,"array":false},
                {"type":0,"off":4,"count":0,"array":false},
                {"type":0,"off":7,"count":0,"array":false}
            ]},
            {"type":14,"off":304,"count":0,"array":true,"fields":[
                {"type":1,"off":0,"count":0,"array":false},
                {"type":1,"off":8,"count":0,"array":false}
            ]}
        ]}}"#,
    )
    .unwrap()
}

#[test]
fn settings_cases_survive_save_and_reload() {
    let contract: Json =
        serde_json::from_str(&fs::read_to_string(fixture_path()).unwrap()).unwrap();
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["case_id"], "profile-settings.persistence.v1");
    assert_eq!(contract["target_build"], 104319);
    assert_eq!(contract["evidence"], "synthetic-reference");

    let schemas = test_schemas();
    let dir = tempfile::tempdir().unwrap();

    let expected_cases = &contract["expected"]["cases"];
    for (index, case) in contract["input"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let name = case["name"].as_str().unwrap();
        let expected = &expected_cases[name];
        let profile_dir = dir.path().join(format!("case_{index}"));
        fs::create_dir_all(&profile_dir).unwrap();
        let profile_path = profile_dir.join("account.json");

        let mut initial = case["initial_profile"].clone();
        initial["unlocks_reset_v2"] = Json::Bool(true);
        let profile = Profile::from_json(initial).unwrap();
        save_profile(&profile, &profile_path).unwrap();

        let accounts = Accounts::new(&profile_dir, dir.path().join("template.json")).unwrap();
        let account = accounts.get("account").unwrap();

        for packet in case["input_packets"].as_array().unwrap() {
            let msg_id = u32::try_from(packet["msg_id"].as_u64().unwrap()).unwrap();
            let payload = packet["payload"].as_object().unwrap().clone();
            let fields = restore_fields(payload).unwrap();
            let request = SettingsRequest::read(&(SOCIAL_OUT, msg_id, fields))
                .unwrap_or_else(|| panic!("{name}: failed to parse {msg_id}"));
            request.apply(&account).unwrap();
        }

        let reloaded = load_or_create_profile(&profile_path).unwrap();
        assert_eq!(
            Json::Object(reloaded.settings.clone()),
            expected["settings"],
            "{name}: saved settings mismatch"
        );

        let mut built = settings(&schemas, &reloaded).unwrap();
        // This cross-language fixture pins the older Python baseline. The
        // newer gameplay defaults have their own contract below.
        for empty_block in [0x78u32, 0x120u32] {
            let key = format!("+0x{empty_block:X}");
            if !reloaded.settings.contains_key(&key) {
                built.insert(empty_block, Value::Struct(Default::default()));
            }
        }
        let actual_20802 = Json::Object(fields_to_json(&built).unwrap());
        assert_eq!(
            actual_20802, expected["message_20802"],
            "{name}: 20802 mismatch"
        );
    }
}

#[test]
fn gameplay_defaults_preserve_saved_options() {
    let contract: Json = serde_json::from_str(include_str!(
        "../../fixtures/behavior/profile-gameplay-defaults-v1.json"
    ))
    .unwrap();
    assert_eq!(contract["schema_version"], 1);
    let schemas = test_schemas();
    for case in contract["cases"].as_array().unwrap() {
        let profile = Profile::from_json(json!({"settings": case["settings"]})).unwrap();
        let message = settings(&schemas, &profile).unwrap();
        let bytes = schemas.encode(0x1a98_79a4, 20802, &message).unwrap();
        let decoded = schemas.decode(0x1a98_79a4, 20802, &bytes, true).unwrap();
        let gameplay = &decoded[&0x120];
        assert_eq!(
            gameplay,
            &Value::Struct(restore_fields(case["expected"].as_object().unwrap().clone()).unwrap()),
            "{}",
            case["name"],
        );
    }
}

#[test]
fn from_json_rejects_out_of_range_and_non_integer_keys() {
    let invalid_keys = [
        json!(-2_147_483_649i64),
        json!(4_294_967_296i64),
        json!(true),
        json!("not-a-number"),
        json!(""),
    ];
    for bad_key in invalid_keys {
        let result = Profile::from_json(json!({
            "settings": { "+0x130": [{ "+0x0": 1, "+0x8": bad_key }] }
        }));
        assert!(result.is_err(), "expected error for key {bad_key}");
    }

    let bool_val = Profile::from_json(json!({
        "settings": { "+0x130": [{ "+0x0": false, "+0x8": 1 }] }
    }));
    assert!(bool_val.is_err());

    let wrapped = Profile::from_json(json!({
        "settings": { "+0x130": [{ "+0x0": 1u64 << 63, "+0x8": 1 }] }
    }))
    .unwrap();
    assert_eq!(
        wrapped.settings["+0x130"],
        json!([{ "+0x0": i64::MIN, "+0x8": 1 }])
    );
}

#[test]
fn invalid_22204_packet_does_not_mutate_or_save_profile() {
    let dir = tempfile::tempdir().unwrap();
    let profile_path = dir.path().join("account.json");
    let mut initial = Profile::default();
    initial
        .settings
        .insert("+0x130".into(), json!([{ "+0x0": 5, "+0x8": 14_725_178 }]));
    save_profile(&initial, &profile_path).unwrap();

    let accounts = Accounts::new(dir.path(), dir.path().join("template.json")).unwrap();
    let account = accounts.get("account").unwrap();

    let payload = restore_fields(
        json!({ "+0x78": [{ "+0x0": 99, "+0x8": 4_294_967_296i64 }] })
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap();
    let request = SettingsRequest::read(&(SOCIAL_OUT, 22204, payload)).unwrap();
    assert!(request.apply(&account).is_err());

    let reloaded = load_or_create_profile(&profile_path).unwrap();
    assert_eq!(
        reloaded.settings["+0x130"],
        json!([{ "+0x0": 5, "+0x8": 14_725_178 }])
    );
}

#[test]
fn load_skips_invalid_setting_entry_without_losing_profile() {
    let dir = tempfile::tempdir().unwrap();
    let profile_path = dir.path().join("account.json");
    fs::write(
        &profile_path,
        serde_json::to_vec(&json!({
            "player_name": "Keeper",
            "level": 250,
            "unlocks_reset_v2": true,
            "settings": {
                "+0x130": [
                    { "+0x0": 5, "+0x8": "0x00E0B03A" },
                    { "+0x0": 99, "+0x8": "bad-key" }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let reloaded = load_or_create_profile(&profile_path).unwrap();
    assert_eq!(reloaded.player_name, "Keeper");
    assert_eq!(reloaded.level, 250);
    assert_eq!(
        reloaded.settings["+0x130"],
        json!([{ "+0x0": 5, "+0x8": 14_725_178 }])
    );
}
