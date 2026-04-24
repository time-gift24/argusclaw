use std::fs;
use std::path::Path;

#[test]
fn argus_server_does_not_depend_on_argus_wing() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml = fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .expect("argus-server Cargo.toml should be readable");

    assert!(
        !cargo_toml.contains("argus-wing"),
        "argus-server must assemble its own core instead of depending on argus-wing"
    );
}

#[test]
fn argus_server_sources_do_not_import_argus_wing() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_dir = manifest_dir.join("src");
    let mut offenders = Vec::new();

    collect_argus_wing_imports(&source_dir, &mut offenders);

    assert!(
        offenders.is_empty(),
        "argus-server source files must not import argus_wing: {offenders:?}"
    );
}

#[test]
fn argus_server_sources_do_not_expose_admin_settings() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_dir = manifest_dir.join("src");
    let mut offenders = Vec::new();

    collect_disallowed_settings_references(&source_dir, &mut offenders);

    assert!(
        offenders.is_empty(),
        "argus-server should not expose admin settings API or storage: {offenders:?}"
    );
}

fn collect_argus_wing_imports(dir: &Path, offenders: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source directory should be readable") {
        let entry = entry.expect("source directory entry should be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_argus_wing_imports(&path, offenders);
            continue;
        }

        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }

        let contents = fs::read_to_string(&path).expect("source file should be readable");
        if contents.contains("argus_wing") {
            offenders.push(path.display().to_string());
        }
    }
}

fn collect_disallowed_settings_references(dir: &Path, offenders: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source directory should be readable") {
        let entry = entry.expect("source directory entry should be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_disallowed_settings_references(&path, offenders);
            continue;
        }

        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }

        let contents = fs::read_to_string(&path).expect("source file should be readable");
        if contents.contains("AdminSettings")
            || contents.contains("admin_settings")
            || contents.contains("/api/v1/settings")
        {
            offenders.push(path.display().to_string());
        }
    }
}
