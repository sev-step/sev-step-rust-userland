extern crate bindgen;

use anyhow::{Context, Result};

use std::env::{self};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=./sev_step_lib/environment.sh");

    const ENV_KERNEL_HEADERS: &'static str = "KERNEL_HEADERS";

    /*besides making usage more convenient, this is also crucial for the rust language
    server to pick up the env var, allowing more in-depth analysis*/
    dotenv::from_path("./environment.sh").context("failed to load env file")?;

    let header_path = env::var(ENV_KERNEL_HEADERS)
        .with_context(|| format!("Failed to get {} env var", ENV_KERNEL_HEADERS))?;

    /*for the following build process we need a wrapper.h
    file with includes to all c headers. We generate this filed
    based on the paths/configs provided in the ENV vars
     */
    let mut wrapper_h_file =
        File::create("wrapper.h").context("failed to create warapper.h file")?;
    write!(
        wrapper_h_file,
        r#"#include "{}""#,
        Path::new(&header_path)
            .join("linux/sev-step/sev-step.h")
            .to_str()
            .expect("failed to build path to sev-step.h header")
    )
    .context("failed to add sev-step.h path to wrapper.h")?;

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        .clang_arg(format!("-I{}", header_path))
        // The input header we would like to generate
        // bindings for.
        .header("wrapper.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .rustified_enum("kvm_page_track_mode")
        .rustified_enum("usp_event_type_t")
        .rustified_enum("vmsa_register_name_t")
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .with_context(|| {
            format!(
                "Failed to build bindings to C API definitions. Include path is {}",
                header_path
            )
        })?;

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .with_context(|| format!("Failed to create bindings.rs file at {:?}", out_path))?;

    Ok(())
}
