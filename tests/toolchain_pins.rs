//! Story 18-2 (#506) — the pinned versions agree with each other.
//!
//! The Rust version lives in three places (`rust-toolchain.toml`, the
//! Dockerfile builder image, the `toolchain:` inputs of the workflows) and
//! the Node major in two (the Dockerfile `css` stage, `node-version` in
//! `_gates.yml`). A bump that forgets one of them builds the image, or runs
//! CI, with another version than the one tested; this test fails instead.
//! It also refuses a floating stage image (`latest`, a bare `alpine`).

use std::fs;
use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The value of `channel = "…"` in `rust-toolchain.toml`.
fn toolchain_channel(toml: &str) -> String {
    toml.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("channel"))
        .and_then(|rest| rest.split('"').nth(1))
        .expect("rust-toolchain.toml has a quoted channel")
        .to_string()
}

/// The image of every `FROM <image> [AS <stage>]` line.
fn from_images(dockerfile: &str) -> Vec<String> {
    dockerfile
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("FROM "))
        .map(|rest| rest.split_whitespace().next().unwrap().to_string())
        .collect()
}

/// The version part of `<name>:<version>-alpine…` for the stage image
/// named `name`.
fn stage_version(images: &[String], name: &str) -> String {
    let image = images
        .iter()
        .find(|i| i.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("no `FROM {name}:` stage in {images:?}"));
    let tag = &image[name.len() + 1..];
    tag.split("-alpine")
        .next()
        .expect("tag has a version")
        .to_string()
}

/// Every value of a `<key>: <value>` line (quotes stripped).
fn yaml_values(yaml: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}:");
    yaml.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix(prefix.as_str()))
        .map(|v| v.trim().trim_matches('"').to_string())
        .collect()
}

#[test]
fn the_rust_version_is_the_same_everywhere() {
    let channel = toolchain_channel(&read("rust-toolchain.toml"));
    assert!(
        channel.split('.').count() == 3 && channel.split('.').all(|p| p.parse::<u32>().is_ok()),
        "rust-toolchain.toml must pin an exact x.y.z version, got {channel:?}"
    );

    let images = from_images(&read("Dockerfile"));
    assert_eq!(stage_version(&images, "rust"), channel, "Dockerfile builder image");

    for workflow in [".github/workflows/_gates.yml", ".github/workflows/audit.yml"] {
        let values = yaml_values(&read(workflow), "toolchain");
        assert!(!values.is_empty(), "{workflow}: no `toolchain:` input");
        for v in values {
            assert_eq!(v, channel, "{workflow}: toolchain input");
        }
    }
}

#[test]
fn the_node_major_of_the_css_stage_is_the_ci_one() {
    let images = from_images(&read("Dockerfile"));
    let docker_node = stage_version(&images, "node");
    let docker_major = docker_node.split('.').next().unwrap().to_string();

    let ci = yaml_values(&read(".github/workflows/_gates.yml"), "node-version");
    assert!(!ci.is_empty(), "_gates.yml: no node-version");
    for v in ci {
        assert_eq!(v, docker_major, "_gates.yml node-version vs Dockerfile node:{docker_node}");
    }
}

#[test]
fn every_stage_image_is_pinned_to_an_exact_version() {
    for image in from_images(&read("Dockerfile")) {
        let (_, tag) = image
            .split_once(':')
            .unwrap_or_else(|| panic!("`FROM {image}` has no tag"));
        assert_ne!(tag, "latest", "`FROM {image}` floats");
        let version = tag.split("-alpine").next().unwrap();
        assert_eq!(
            version.split('.').count(),
            3,
            "`FROM {image}`: the version must be x.y.z"
        );
    }
}

#[test]
fn tailwind_is_pinned_exactly_in_the_tracked_manifest() {
    let manifest = read("package.json");
    for pkg in ["\"tailwindcss\"", "\"@tailwindcss/cli\""] {
        let line = manifest
            .lines()
            .find(|l| l.trim_start().starts_with(pkg))
            .unwrap_or_else(|| panic!("package.json: no {pkg}"));
        let version = line.split('"').nth(3).expect("quoted version");
        assert!(
            version.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "package.json {pkg}: exact version expected, got {version:?}"
        );
    }
    assert!(
        read("Dockerfile").contains("RUN npm ci"),
        "the css stage installs from the lockfile"
    );
}

#[test]
fn the_helpers_read_the_shapes_they_check() {
    assert_eq!(toolchain_channel("[toolchain]\nchannel = \"1.2.3\"\n"), "1.2.3");
    let images = from_images("FROM rust:1.2.3-alpine3.24 AS b\nRUN x\nFROM alpine:3.24.4\n");
    assert_eq!(images, ["rust:1.2.3-alpine3.24", "alpine:3.24.4"]);
    assert_eq!(stage_version(&images, "rust"), "1.2.3");
    assert_eq!(stage_version(&images, "alpine"), "3.24.4");
    assert_eq!(
        yaml_values("  with:\n    toolchain: 1.2.3\n    node-version: \"24\"\n", "node-version"),
        ["24"]
    );
}
