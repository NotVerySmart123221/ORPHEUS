use super::*;
use crate::bits::BitReader;

#[test]
fn authored_mode_variables_preserve_values_and_select_the_requested_mode() {
    let graphs = GraphStore::from_json(include_bytes!(
        "../../../../fixtures/behavior/mode-presentation-variables-v1.json"
    ))
    .unwrap();
    let expected = Variables::from([
        (17, Value::Bool(true)),
        (18, Value::Bool(false)),
        (33, Value::Integer(3)),
        (44, Value::Asset(0x0d80_0000_0000_0ffe)),
    ]);
    assert_eq!(
        authored_mode_variables(&graphs, 0x0230_0000_0000_0ffe),
        Some(expected)
    );
    assert_eq!(
        authored_mode_variables(&graphs, 0x0230_0000_0000_0ffd),
        Some(Variables::from([(17, Value::Bool(false))]))
    );
    assert!(authored_mode_variables(&graphs, 0x0230_0000_0000_0ffc).is_none());
}

#[test]
#[ignore = "requires a prepared 104319 Practice Range pack"]
fn prepared_practice_mode_frame_exposes_authored_nameplate_variables() {
    let runtime = PathBuf::from(std::env::var_os("ORPHEUS_RUNTIME_DIR").unwrap());
    let assets = SceneAssets::load(runtime).unwrap();
    let scene = assets.prepare(GameMap::practice_range()).unwrap();
    let frame = scene.mode_ui();
    let mut reader = BitReader::with_range(frame.as_bytes(), 0, frame.bit_len()).unwrap();

    // Read the parentless presentation descriptor and the entity-wide variable list.
    assert_eq!(reader.bits(2).unwrap(), 0);
    assert_eq!(reader.w_u16().unwrap(), 1);
    assert_eq!(reader.bits(16).unwrap(), 0x0cb6);
    assert_eq!(reader.bits(4).unwrap(), 0);
    assert_eq!(reader.w_u16().unwrap(), 0);
    assert!(reader.bit().unwrap());
    let expected = [(9525, true), (9539, true), (27534, false)];
    for (id, value) in expected {
        assert_eq!(reader.w_var().unwrap(), id);
        assert_eq!(reader.bits(4).unwrap(), 0, "boolean value tag");
        assert_eq!(reader.bit().unwrap(), value);
        assert!(!reader.bit().unwrap(), "no variable bindings");
    }
    assert_eq!(reader.w_var().unwrap(), 0);
}
