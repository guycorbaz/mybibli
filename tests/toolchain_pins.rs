//! Story 18-2 (#506) — the pinned versions agree with each other.
//!
//! The Rust version lives in `rust-toolchain.toml`, the Dockerfile builder
//! image and the `toolchain:` inputs of the workflows; the Node major in
//! the Dockerfile `css` stage and `node-version` in the workflows; the
//! Alpine minor in every stage tag. A bump that forgets one of them builds
//! the image, or runs CI, with another version than the one tested; this
//! test fails instead. It also refuses a floating stage image (`latest`, a
//! bare `-alpine` suffix, a version that is not `x.y.z`) and a Tailwind
//! range. The audit workflow is the one deliberate exception: it runs on
//! `stable` (it builds only cargo-audit, whose minimum Rust may move past
//! our pin) and says so with `RUSTUP_TOOLCHAIN: stable`.

use std::fs;
use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every workflow file, by repository-relative path.
fn workflows() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut out: Vec<(String, String)> = fs::read_dir(&dir)
        .expect("workflow directory")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .map(|p| {
            let rel = format!(".github/workflows/{}", p.file_name().unwrap().to_string_lossy());
            let text = fs::read_to_string(&p).unwrap();
            (rel, text)
        })
        .collect();
    out.sort();
    out
}

fn is_x_y_z(v: &str) -> bool {
    v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty() && p.parse::<u32>().is_ok())
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

/// One `FROM` line's image reference, digest stripped.
#[derive(Debug, PartialEq)]
struct Stage {
    image: String,
}

/// The image of every `FROM [--flag…] <image>[@digest] [AS <name>]` line,
/// case-insensitive; a `FROM <earlier stage>` is skipped.
fn from_images(dockerfile: &str) -> Vec<Stage> {
    let mut stage_names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for line in dockerfile.lines().map(str::trim) {
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("FROM")) {
            continue;
        }
        let words: Vec<&str> = words.filter(|w| !w.starts_with("--")).collect();
        let Some(reference) = words.first() else { continue };
        let image = reference.split('@').next().unwrap().to_string();
        if let Some(i) = words.iter().position(|w| w.eq_ignore_ascii_case("AS"))
            && let Some(name) = words.get(i + 1)
        {
            stage_names.push(name.to_string());
        }
        if stage_names[..stage_names.len().saturating_sub(1)].contains(&image) {
            continue;
        }
        out.push(Stage { image });
    }
    out
}

/// `(version, alpine_minor)` of a stage tag: `rust:1.2.3-alpine3.24` →
/// `("1.2.3", Some("3.24"))`; `alpine:3.24.4` → `("3.24.4", Some("3.24"))`;
/// a bare `-alpine` suffix → `None`.
fn split_tag(image: &str) -> (String, String, Option<String>) {
    let (name, tag) = image
        .split_once(':')
        .unwrap_or_else(|| panic!("`FROM {image}` has no tag"));
    if name == "alpine" {
        let minor = tag.split('.').take(2).collect::<Vec<_>>().join(".");
        return (name.to_string(), tag.to_string(), Some(minor));
    }
    match tag.split_once("-alpine") {
        Some((version, rest)) => {
            let minor = (!rest.is_empty()).then(|| rest.to_string());
            (name.to_string(), version.to_string(), minor)
        }
        None => (name.to_string(), tag.to_string(), None),
    }
}

fn stage_version(stages: &[Stage], name: &str) -> String {
    let stage = stages
        .iter()
        .find(|s| s.image.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("no `FROM {name}:` stage in {stages:?}"));
    split_tag(&stage.image).1
}

/// Every value of a `<key>: <value>` line: inline comment and quotes
/// stripped.
fn yaml_values(yaml: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}:");
    yaml.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix(prefix.as_str()))
        .map(|v| {
            let v = v.split(" #").next().unwrap().trim();
            v.trim_matches(|c| c == '"' || c == '\'').to_string()
        })
        .collect()
}

#[test]
fn the_rust_version_is_the_same_everywhere() {
    let channel = toolchain_channel(&read("rust-toolchain.toml"));
    assert!(is_x_y_z(&channel), "rust-toolchain.toml must pin x.y.z, got {channel:?}");

    let stages = from_images(&read("Dockerfile"));
    assert_eq!(stage_version(&stages, "rust"), channel, "Dockerfile builder image");

    let mut pinned = 0;
    for (path, text) in workflows() {
        let values = yaml_values(&text, "toolchain");
        if path.ends_with("/audit.yml") {
            assert!(
                values.iter().all(|v| v == "stable")
                    && yaml_values(&text, "RUSTUP_TOOLCHAIN") == ["stable"],
                "{path}: the audit job runs on stable, with RUSTUP_TOOLCHAIN: stable"
            );
            continue;
        }
        assert!(
            !text.contains("dtolnay/rust-toolchain@") || !values.is_empty(),
            "{path}: dtolnay/rust-toolchain without a toolchain input"
        );
        for v in values {
            assert_eq!(v, channel, "{path}: toolchain input");
            pinned += 1;
        }
    }
    assert!(pinned >= 2, "the _gates.yml Rust jobs pass the pinned toolchain");
}

#[test]
fn the_node_major_of_the_css_stage_is_the_ci_one() {
    let stages = from_images(&read("Dockerfile"));
    let docker_node = stage_version(&stages, "node");
    let docker_major = docker_node.split('.').next().unwrap().to_string();

    let mut seen = 0;
    for (path, text) in workflows() {
        for v in yaml_values(&text, "node-version") {
            assert!(
                v == docker_major || v == docker_node,
                "{path}: node-version {v:?} vs Dockerfile node:{docker_node}"
            );
            seen += 1;
        }
    }
    assert!(seen >= 1, "no node-version in the workflows");
}

#[test]
fn every_stage_image_is_pinned_on_one_alpine_minor() {
    let stages = from_images(&read("Dockerfile"));
    assert!(stages.len() >= 3, "{stages:?}");
    let mut minors = Vec::new();
    for stage in &stages {
        let (name, version, minor) = split_tag(&stage.image);
        assert_ne!(version, "latest", "`FROM {}` floats", stage.image);
        assert!(is_x_y_z(&version), "`FROM {}`: the version must be x.y.z", stage.image);
        let minor = minor.unwrap_or_else(|| {
            panic!("`FROM {}`: name the Alpine minor (`-alpine3.NN`)", stage.image)
        });
        minors.push((name, minor));
    }
    let first = &minors[0].1;
    for (name, minor) in &minors {
        assert_eq!(minor, first, "{name} is on Alpine {minor}, not {first}: {minors:?}");
    }
}

#[test]
fn tailwind_is_pinned_exactly_in_the_tracked_manifest() {
    let manifest = read("package.json");
    let lock = read("package-lock.json");
    let mut versions = Vec::new();
    for pkg in ["tailwindcss", "@tailwindcss/cli"] {
        let key = format!("\"{pkg}\"");
        let line = manifest
            .lines()
            .find(|l| l.trim_start().starts_with(&key))
            .unwrap_or_else(|| panic!("package.json: no {pkg}"));
        let version = line.split('"').nth(3).expect("quoted version").to_string();
        assert!(is_x_y_z(&version), "package.json {pkg}: exact x.y.z expected, got {version:?}");
        // The lockfile resolves the same version.
        let entry = format!("\"node_modules/{pkg}\": {{");
        let at = lock.find(&entry).unwrap_or_else(|| panic!("package-lock.json: no {pkg}"));
        let locked = lock[at..]
            .lines()
            .find_map(|l| l.trim().strip_prefix("\"version\": \""))
            .and_then(|v| v.split('"').next())
            .unwrap();
        assert_eq!(locked, version, "package-lock.json {pkg}");
        versions.push(version);
    }
    assert_eq!(versions[0], versions[1], "tailwindcss and @tailwindcss/cli move together");
    assert!(
        read("Dockerfile").lines().map(str::trim).any(|l| l == "RUN npm ci"),
        "the css stage installs from the lockfile (`RUN npm ci`)"
    );
}

#[test]
fn the_helpers_read_the_shapes_they_check() {
    assert_eq!(toolchain_channel("[toolchain]\nchannel = \"1.2.3\"\n"), "1.2.3");
    let stages = from_images(
        "FROM --platform=$BUILDPLATFORM rust:1.2.3-alpine3.24@sha256:ab AS b\nRUN x\n\
         from alpine:3.24.4\nFROM b AS again\n",
    );
    let images: Vec<&str> = stages.iter().map(|s| s.image.as_str()).collect();
    assert_eq!(images, ["rust:1.2.3-alpine3.24", "alpine:3.24.4"]);
    assert_eq!(stage_version(&stages, "rust"), "1.2.3");
    assert_eq!(
        split_tag("node:24.1.0-alpine3.24"),
        ("node".into(), "24.1.0".into(), Some("3.24".into()))
    );
    assert_eq!(split_tag("node:24.1.0-alpine").2, None);
    assert_eq!(split_tag("alpine:3.24.4").2, Some("3.24".into()));
    assert_eq!(
        yaml_values("  toolchain: 1.2.3 # pinned\n  node-version: '24'\n", "toolchain"),
        ["1.2.3"]
    );
    assert_eq!(yaml_values("  node-version: '24'\n", "node-version"), ["24"]);
    assert!(is_x_y_z("4.3.5") && !is_x_y_z("4") && !is_x_y_z("4.x.1") && !is_x_y_z("4.3"));
}
