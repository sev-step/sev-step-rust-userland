use std::num::ParseIntError;

use std::{env::temp_dir, fs::File};

use anyhow::{bail, Context, Result};

use reqwest::{
    blocking::{multipart::Form, Client},
    Url,
};
use tar::Builder;
use vm_server::req_resp::*;

/// Helper function to parse a string that might have hex prefix "0x" to u64
pub fn parse_hex_str(v: &str) -> Result<u64, ParseIntError> {
    u64::from_str_radix(v.strip_prefix("0x").unwrap_or(v), 16)
}

/// Prepare the VM server to execute an arbitrary, binary. The binary must adhere
/// to the communication protocol documented in the `InitCustomTargetReq` struct.
/// This allows the VM server to provide you with GPA's and other relevant information to quickly
/// prototype an attack. To start the program, use the `run_target_program` API call.
pub fn new_custom_target(
    basepath: &str,
    args: &InitCustomTargetReq,
) -> Result<InitCustomTargetResp> {
    let url = Url::parse(basepath).context(format!("cannot parse {} as url", basepath))?;
    const SUB_URL: &'static str = "/custom-target/new";
    let url = url.join(&SUB_URL).context(format!(
        "failed to append {} to base URL {}",
        SUB_URL, basepath
    ))?;

    //create temporary file for archive, and add all files from `args.folder_path` to it
    let archive_dir = temp_dir();
    let archive_file_path = archive_dir.join("vmserver_upload.tar");
    let archive_file = File::create(archive_dir.join("vmserver_upload.tar"))?;
    let mut archive = Builder::new(archive_file);
    archive.append_dir_all("./", &args.folder_path)?;
    drop(archive.into_inner()?);

    let form = Form::new()
        .text("execute_cmd", args.execute_cmd.clone())
        .file("file_archive", archive_file_path)?;

    let client = reqwest::blocking::Client::new();
    client
        .post(url)
        .multipart(form)
        .send()
        .context("error sending request")?
        .error_for_status()
        .context("server returned error code")?
        .json()
        .context("failed to parse body")
}

pub fn new_page_ping_ponger(
    basepath: &str,
    args: &InitPagePingPongerReq,
) -> Result<InitPagePingPongerResp> {
    let url = Url::parse(basepath).context(format!("cannot parse {} as url", basepath))?;
    const SUB_URL: &'static str = "/page-ping-ponger/new";
    let url = url.join(&SUB_URL).context(format!(
        "failed to append {} to base URL {}",
        SUB_URL, basepath
    ))?;

    let client = Client::new();
    client
        .post(url)
        .json(args)
        .send()
        .context("error sending request")?
        .error_for_status()
        .context("server returned error code")?
        .json()
        .context("failed to parse body")
}

pub fn new_assembly_target(
    basepath: &str,
    req: &InitAssemblyTargetReq,
) -> Result<InitAssemblyTargetResp> {
    let url = Url::parse(basepath).context(format!("failed to parse {} as url", basepath))?;
    let url = url.join("/assembly-target/new")?;

    let client = Client::new();
    client
        .post(url.clone())
        .json(&req)
        .send()
        .context(format!("error sending post request to {}", url))?
        .error_for_status()
        .context("server returned error code")?
        .json()
        .context("failed to parse body")
}

pub fn run_target_program(basepath: &str) -> Result<()> {
    let url = Url::parse(basepath).context(format!("failed to parse {} as url", basepath))?;
    let url = url.join("/run-target")?;

    let client = Client::new();
    let resp = client
        .post(url.clone())
        .send()
        .context(format!("error sending post request to {}", url))?;
    match resp.status().is_success() {
        true => Ok(()),
        false => bail!("server returned error {}", resp.text()?),
    }
}
