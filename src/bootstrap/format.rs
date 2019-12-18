//! Runs rustfmt on the repository.

use crate::Build;
use std::process::Command;
use ignore::WalkBuilder;
use std::path::Path;
use build_helper::t;

fn rustfmt(build: &Build, path: &Path, check: bool) {
    let rustfmt_path = build.config.initial_rustfmt.as_ref().unwrap_or_else(|| {
        eprintln!("./x.py fmt is not supported on this channel");
        std::process::exit(1);
    });

    let mut cmd = Command::new(&rustfmt_path);
    // avoid the submodule config paths from coming into play,
    // we only allow a single global config for the workspace for now
    cmd.arg("--config-path").arg(&build.src.canonicalize().unwrap());
    cmd.arg("--unstable-features");
    cmd.arg("--skip-children");
    if check {
        cmd.arg("--check");
    }
    cmd.arg(&path);
    let cmd_debug = format!("{:?}", cmd);
    let status = cmd.status().expect("executing rustfmt");
    assert!(status.success(), "running {} successful", cmd_debug);
}

#[derive(serde::Deserialize)]
struct RustfmtConfig {
    ignore: Vec<String>,
}

pub fn format(build: &Build, check: bool) {
    let mut builder = ignore::types::TypesBuilder::new();
    builder.add_defaults();
    builder.select("rust");
    let matcher = builder.build().unwrap();

    let rustfmt_config = t!(std::fs::read_to_string(build.src.join("rustfmt.toml")));
    let rustfmt_config: RustfmtConfig = t!(toml::from_str(&rustfmt_config));
    let mut ignore_fmt = ignore::overrides::OverrideBuilder::new(&build.src);
    for ignore in rustfmt_config.ignore {
        ignore_fmt.add(&format!("!{}", ignore)).expect(&ignore);
    }
    let ignore_fmt = ignore_fmt.build().unwrap();

    let walker = WalkBuilder::new(&build.src)
        .types(matcher)
        .overrides(ignore_fmt)
        .build();
    for entry in walker {
        let entry = t!(entry);
        if entry.file_type().map_or(false, |t| t.is_file()) {
            rustfmt(build, &entry.path(), check);
        }
    }
}
