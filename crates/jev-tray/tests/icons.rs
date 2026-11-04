//! The committed icon files (jev-tray spec section 3): every PNG that the
//! name scheme asks for exists, and each has the size its name gives.

use std::path::PathBuf;

const STATES: [&str; 2] = ["on", "off"];
const COLORS: [&str; 2] = ["black", "white"];
const SIZES: [u32; 5] = [16, 20, 24, 32, 48];
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

fn icons() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons")
}

/// The width and height from the IHDR chunk, which a PNG must put first.
fn size_of_png(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.len() >= 26, "too short for a PNG");
    assert_eq!(bytes[..8], SIGNATURE, "not a PNG");
    assert_eq!(&bytes[12..16], b"IHDR", "the first chunk is not IHDR");
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (width, height)
}

#[test]
fn every_png_has_the_size_its_name_gives() {
    for state in STATES {
        for color in COLORS {
            for size in SIZES {
                let name = format!("{state}-{color}-{size}.png");
                let bytes = std::fs::read(icons().join(&name))
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(size_of_png(&bytes), (size, size), "{name}");
                // The IHDR bit depth and color type: 8 bits, RGBA.
                assert_eq!(bytes[24..26], [8, 6], "{name} is not 8-bit RGBA");
            }
        }
    }
}

#[test]
fn no_other_png_is_in_the_folder() {
    let mut found = 0;
    for entry in std::fs::read_dir(icons()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let Some(stem) = name.strip_suffix(".png") else {
            continue;
        };
        let parts: Vec<&str> = stem.split('-').collect();
        assert_eq!(parts.len(), 3, "{name}");
        assert!(STATES.contains(&parts[0]), "{name}");
        assert!(COLORS.contains(&parts[1]), "{name}");
        assert!(
            parts[2].parse::<u32>().is_ok_and(|s| SIZES.contains(&s)),
            "{name}"
        );
        found += 1;
    }
    assert_eq!(found, STATES.len() * COLORS.len() * SIZES.len());
}

#[test]
fn both_svgs_use_the_16_grid_and_one_color() {
    for state in STATES {
        let text = std::fs::read_to_string(icons().join(format!("{state}.svg"))).unwrap();
        assert!(text.contains(r#"viewBox="0 0 16 16""#), "{state}.svg");
        assert!(text.contains("currentColor"), "{state}.svg");
        for value in paint_values(&text) {
            assert!(
                value == "currentColor" || value == "none",
                "{state}.svg has the paint {value:?}"
            );
        }
    }
}

/// Every `fill="..."` and `stroke="..."` value in an SVG text.
fn paint_values(text: &str) -> Vec<&str> {
    let mut values = Vec::new();
    for attribute in ["fill=\"", "stroke=\""] {
        for (at, _) in text.match_indices(attribute) {
            let rest = &text[at + attribute.len()..];
            let end = rest
                .find('"')
                .expect("an attribute value without its end quote");
            values.push(&rest[..end]);
        }
    }
    values
}
