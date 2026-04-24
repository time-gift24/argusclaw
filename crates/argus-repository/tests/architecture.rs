use std::fs;
use std::path::Path;

#[test]
fn repository_does_not_define_admin_settings_storage() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();

    collect_disallowed_references(&manifest_dir.join("src"), &mut offenders);
    collect_disallowed_references(&manifest_dir.join("migrations"), &mut offenders);

    assert!(
        offenders.is_empty(),
        "admin settings storage should not exist in argus-repository: {offenders:?}"
    );
}

fn collect_disallowed_references(dir: &Path, offenders: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("directory should be readable") {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();

        if path.is_dir() {
            collect_disallowed_references(&path, offenders);
            continue;
        }

        let contents = fs::read_to_string(&path).expect("file should be readable");
        if contents.contains("AdminSettings") || contents.contains("admin_settings") {
            offenders.push(path.display().to_string());
        }
    }
}
