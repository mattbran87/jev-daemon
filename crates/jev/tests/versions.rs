//! One version for the crates and the Claude Code plugin (spec section 8).

use serde_json::Value;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/{path}"))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn version(text: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = text.split('.').map(|part| part.parse().unwrap()).collect();
    assert_eq!(parts.len(), 3, "{text} is not major.minor.patch");
    (parts[0], parts[1], parts[2])
}

#[test]
fn the_plugin_version_is_the_crate_version() {
    let manifest: Value =
        serde_json::from_str(&read("plugins/claude-code/.claude-plugin/plugin.json")).unwrap();
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_mod_needs_no_jev_newer_than_this_one() {
    let text = read("plugins/claude-code/hooks/text.ts");
    let line = text
        .lines()
        .find(|line| line.starts_with("export const MIN_JEV"))
        .expect("text.ts declares MIN_JEV");
    let inside = &line[line.find('[').unwrap() + 1..line.find(']').unwrap()];
    let min = version(&inside.replace(", ", ".").replace(',', "."));
    assert!(
        min <= version(env!("CARGO_PKG_VERSION")),
        "MIN_JEV {min:?} is newer than jev {}",
        env!("CARGO_PKG_VERSION")
    );
}
