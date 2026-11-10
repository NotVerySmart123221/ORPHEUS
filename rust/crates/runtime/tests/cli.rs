use std::process::Command;

#[test]
fn help_and_argument_errors_keep_the_packer_cli_contract() {
    let packer = env!("CARGO_BIN_EXE_prepare_server_runtime");
    let help = Command::new(packer).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("prepare_server_runtime --source DIR --destination DIR")
    );

    let invalid = Command::new(packer).arg("--unknown").output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("Unknown option --unknown")
    );
}
