//! Build script for `beads_rust`.
//!
//! Uses vergen-gix to embed build information into the binary.

use std::path::Path;
use vergen_gix::{BuildBuilder, CargoBuilder, Emitter, GixBuilder, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = BuildBuilder::default().build_timestamp(true).build()?;
    let cargo = CargoBuilder::default().target_triple(true).build()?;
    let rustc = RustcBuilder::default().semver(true).build()?;

    let mut emitter = Emitter::default();
    emitter
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&rustc)?;

    if Path::new(".git/HEAD").is_file() {
        let gix = GixBuilder::default()
            .branch(true)
            .sha(true)
            .commit_timestamp(true)
            .dirty(true)
            .build()?;
        emitter.add_instructions(&gix)?;
    }

    emitter.emit()?;

    Ok(())
}
