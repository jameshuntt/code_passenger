//! The two writers custodian routes to: `dep add | remove` (a crate's
//! dependency through toml_edit, optionally through the workspace table)
//! and `attr place | remove` (one attribute line on an item at klepto's
//! location, by the gates' placement rule). Fixture: tests/fixtures/gate/.

mod common;

use code_passenger::commands::edit;
use code_passenger::error::PassengerError;
use code_passenger::lines;
use common::*;
use std::ffi::OsString;
use std::path::Path;

const APP_LIB: &str = "app/src/lib.rs";

fn words(v: &[&str]) -> Vec<OsString> {
    v.iter().map(OsString::from).collect()
}

#[test]
fn dep_add_and_remove_edit_the_manifest_and_keep_its_formatting() {
    let copy = fresh_copy("gate");
    let root = copy.path();
    let e = edit::dep_add(root, "app", "serde@1.0", None, &[], false).unwrap();
    assert!(e.written && e.said == "added serde to app", "{}", e.said);
    // a path dependency that is there (cargo metadata reads it on the next run)
    std::fs::create_dir_all(root.join("vendor/maintenance/src")).unwrap();
    std::fs::write(root.join("vendor/maintenance/Cargo.toml"), "[package]\nname = \"maintenance\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\ncheck = []\nrunner = []\n").unwrap();
    std::fs::write(root.join("vendor/maintenance/src/lib.rs"), "").unwrap();
    let e = edit::dep_add(root, "app", "maintenance", Some("../vendor/maintenance"), &["check".into(), "runner".into()], false).unwrap();
    assert!(e.written);
    let again = edit::dep_add(root, "app", "serde@1.0", None, &[], false).unwrap();
    assert!(!again.written && again.said.contains("nothing to do"));
    let up = edit::dep_add(root, "app", "serde@1.0.200", None, &["derive".into()], false).unwrap();
    assert_eq!(up.said, "updated serde in app");
    assert_eq!(
        read(&root.join("app/Cargo.toml")),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nbeta = { path = \"../beta\" }\nserde = { version = \"1.0.200\", features = [\"derive\"] }\nmaintenance = { path = \"../vendor/maintenance\", features = [\"check\", \"runner\"] }\n\n[features]\ndefault = []\n# a dashed feature name: `feature-probe extra-io` must find it (crate names are `_`, features keep `-`)\nextra-io = []\n"
    );

    // through the workspace table: the version at the root, `workspace = true` in the crate
    let e = edit::dep_add(root, "beta", "big-bad-bool@0.1.1", None, &[], true).unwrap();
    assert_eq!(e.edits.files.len(), 2);
    assert_eq!(read(&root.join("Cargo.toml")), "[workspace]\nresolver = \"2\"\nmembers = [\"app\", \"beta\"]\n\n[workspace.dependencies]\nbig-bad-bool = \"0.1.1\"\n");
    assert!(read(&root.join("beta/Cargo.toml")).ends_with("[dependencies]\nbig-bad-bool = { workspace = true }\n"));
    edit::dep_add(root, "app", "big-bad-bool", None, &["x".into()], true).expect("the table holds it: no version needed");
    assert!(read(&root.join("app/Cargo.toml")).contains("big-bad-bool = { workspace = true, features = [\"x\"] }"));
    let r = edit::dep_remove(root, "beta", "big-bad-bool", true).unwrap();
    assert!(r.said.contains("[workspace.dependencies] keeps it (app still name it)"), "{}", r.said);
    let r = edit::dep_remove(root, "app", "big-bad-bool", true).unwrap();
    assert!(r.said.ends_with(", and from [workspace.dependencies]"), "{}", r.said);
    assert!(!read(&root.join("Cargo.toml")).contains("big-bad-bool"));

    // refusals
    let e = edit::dep_add(root, "app", "tokio", None, &[], false).unwrap_err();
    assert!(matches!(e, PassengerError::DepVersion { .. }) && e.to_string().starts_with("[CP0060]"), "{e}");
    let e = edit::dep_add(root, "nosuch", "tokio@1", None, &[], false).unwrap_err();
    assert!(matches!(e, PassengerError::UnknownName { .. }), "{e}");
    std::fs::write(root.join("app/Cargo.toml"), read(&root.join("app/Cargo.toml")).replace("extra-io = []", "extra-io = [\"serde/std\"]")).unwrap();
    let e = edit::dep_remove(root, "app", "serde", false).unwrap_err();
    assert!(matches!(e, PassengerError::DepInUse { .. }) && e.to_string().contains("extra-io"), "{e}");
    let single = fresh_copy("gate");
    let e = edit::dep_add(&single.path().join("beta"), "beta", "x@1", None, &[], true);
    assert!(e.is_ok(), "a member's root is its workspace's: {e:?}");
}

#[test]
fn attr_place_and_remove_follow_the_gates_placement_rule() {
    let copy = fresh_copy("gate");
    let root = copy.path();
    let lib = root.join(APP_LIB);
    // a non-cfg attribute goes above the item's first attribute, after its docs
    edit::attr_place(root, Path::new(APP_LIB), "app::unix_beta", "#[sensitivity(secret)]").unwrap();
    assert!(read(&lib).contains("/// Already under a cfg: the gate must MERGE, not stack a second attribute.\n#[sensitivity(secret)]\n#[cfg(unix)]\npub fn unix_beta()"), "{}", read(&lib));
    // a cfg merges into the item's own
    edit::attr_place(root, Path::new(APP_LIB), "app::unix_beta", "#[cfg(feature = \"b\")]").unwrap();
    assert!(read(&lib).contains("#[sensitivity(secret)]\n#[cfg(all(unix, feature = \"b\"))]\npub fn unix_beta()"), "{}", read(&lib));
    // a cfg on an item with none goes right above the declaration; `crate::` is the file's package
    edit::attr_place(root, Path::new(APP_LIB), "crate::plain", "#[cfg(feature = \"b\")]").unwrap();
    assert!(read(&lib).contains("/// Nothing to do with beta: stays as it is.\n#[cfg(feature = \"b\")]\npub fn plain()"), "{}", read(&lib));
    let same = edit::attr_place(root, Path::new(APP_LIB), "app::unix_beta", "#[sensitivity(secret)]").unwrap();
    assert!(!same.written && same.said.contains("nothing to do"));
    let gone = edit::attr_remove(root, Path::new(APP_LIB), "app::unix_beta", "#[sensitivity(secret)]").unwrap();
    assert!(gone.written);
    assert!(read(&lib).contains("stack a second attribute.\n#[cfg(all(unix, feature = \"b\"))]\npub fn unix_beta()"));

    let e = edit::attr_place(root, Path::new("app/src/util.rs"), "app::uses_beta", "#[x]").unwrap_err();
    assert!(matches!(e, PassengerError::AttrItem { .. }) && e.to_string().contains("it is in app/src/lib.rs"), "{e}");
    let e = edit::attr_place(root, Path::new(APP_LIB), "app::plain", "pub fn x() {}").unwrap_err();
    assert!(matches!(e, PassengerError::AttrLine { .. }) && e.to_string().starts_with("[CP0064]"), "{e}");
}

#[test]
fn custodians_lines_for_the_writers_are_declared() {
    let d = lines::read(&words(&["dep", "add", "app", "maintenance@0.1.0", "--features", "check,runner", "--workspace-table"])).unwrap();
    assert_eq!(d.display(), "code-passenger dep add app maintenance@0.1.0 --features check,runner --workspace-table");
    let d = lines::read(&words(&["--root", "ws", "dep", "remove", "app", "maintenance"])).unwrap();
    assert_eq!(d.display(), "code-passenger dep --root ws remove app maintenance");
    let a = lines::read(&words(&["attr", "place", "--file", "src/lib.rs", "--item", "app::f", "--line", "#[sensitivity(secret)]"])).unwrap();
    assert_eq!(a.display(), "code-passenger attr place --file src/lib.rs --item app::f --line #[sensitivity(secret)]");
    assert!(lines::read(&words(&["attr", "place", "--file", "x.rs", "--item", "a"])).is_err(), "--line is needed");
    assert!(lines::read(&words(&["dep", "upgrade", "app", "x"])).is_err());

    // through the binary, with its receipt
    let copy = fresh_copy("gate");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_code-passenger")).args(["dep", "add", "app", "serde@1.0", "--root"]).arg(copy.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("written or nothing to do (exit 0), receipt sha256:"));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_code-passenger")).arg("--root").arg(copy.path()).args(["attr", "place", "--file", APP_LIB, "--item", "app::nope", "--line", "#[x]"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("[CP0063]"));
}
