//! One version for the crates and the Claude Code plugin (spec section 8).

use serde_json::Value;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/{path}"))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn version(text: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = text
        .split('.')
        .map(|part| {
            part.trim()
                .parse()
                .unwrap_or_else(|_| panic!("{text:?} is not major.minor.patch"))
        })
        .collect();
    assert_eq!(parts.len(), 3, "{text} is not major.minor.patch");
    (parts[0], parts[1], parts[2])
}

#[test]
fn the_plugin_version_is_the_crate_version() {
    let manifest: Value =
        serde_json::from_str(&read("plugins/claude-code/.claude-plugin/plugin.json")).unwrap();
    assert_eq!(
        manifest["version"],
        env!("CARGO_PKG_VERSION"),
        "plugin.json version must equal the crate version"
    );
}

#[test]
fn the_marketplace_entry_has_no_other_version() {
    let marketplace: Value =
        serde_json::from_str(&read(".claude-plugin/marketplace.json")).unwrap();
    let entry = marketplace["plugins"]
        .as_array()
        .expect("marketplace.json has a plugins array")
        .iter()
        .find(|plugin| plugin["source"] == "./plugins/claude-code")
        .expect("marketplace.json lists the claude-code plugin");
    if let Some(version) = entry.get("version") {
        assert_eq!(
            version,
            env!("CARGO_PKG_VERSION"),
            "marketplace.json version must equal the crate version"
        );
    }
}

#[test]
fn the_mod_needs_no_jev_newer_than_this_one() {
    let text = read("plugins/claude-code/hooks/text.ts");
    let after = &text[text.find("MIN_JEV").expect("text.ts declares MIN_JEV")..];
    let open = after
        .find('[')
        .expect("MIN_JEV must be a [major, minor, patch] literal");
    let close = after
        .find(']')
        .expect("MIN_JEV must be a [major, minor, patch] literal");
    assert!(
        open < close,
        "MIN_JEV must be a [major, minor, patch] literal"
    );
    let min = version(
        &after[open + 1..close]
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("."),
    );
    assert!(
        min <= version(env!("CARGO_PKG_VERSION")),
        "MIN_JEV {min:?} is newer than jev {}",
        env!("CARGO_PKG_VERSION")
    );
}
