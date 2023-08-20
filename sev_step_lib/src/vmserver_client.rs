use anyhow::{bail, Context, Result};

use reqwest::{blocking::Client, Url};
use vm_server::req_resp::*;

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

    let client = reqwest::blocking::Client::new();
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
