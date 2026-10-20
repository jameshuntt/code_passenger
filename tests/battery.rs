// Copyright © 2026 1o. All rights reserved.

//! James's battery rule in code_passenger: under 20% and not charging, a proof's cargo lines do not start; the
//! transform is refused with the kit's code, and nothing is written. The battery is given (`CUSTODIAN_BATTERY`) to the
//! program it runs, so no other test runs under it.

mod common;

#[test]
fn a_dying_battery_refuses_the_proof_before_any_cargo_line_and_writes_nothing() {
    let d = common::fresh_copy("batch_ws");
    let root = d.path();
    let spec = root.join(".specs/port.toml");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(&spec, "name = \"port\"\nobjective = \"o\"\noperation = \"replace_body\"\ntarget = \"rules::port\"\n\n[scope]\nwrite = [\"crate:rules\"]\n\n[function]\nname = \"port\"\n\n[[function.body]]\npiece = \"value\"\nvalue = \"Ok(n)\"\n").unwrap();
    let before = common::read(&root.join("rules/src/lib.rs"));
    let place = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_code-passenger"))
        .args(["--root", root.to_str().unwrap(), "transform", "--spec", spec.to_str().unwrap(), "--write"])
        .env("CUSTODIAN_BATTERY", "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t11%; discharging; 0:15 remaining present: true\n")
        .env("CODE_PASSENGER_PROOF_DIR", place.path())
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_ne!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("[CUST0042]") && said.contains("11%, not charging"), "{said}");
    assert_eq!(common::read(&root.join("rules/src/lib.rs")), before, "nothing written");
    assert!(!place.path().join("target").exists(), "no cargo line ran");
}
