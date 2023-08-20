use anyhow::{bail, Context, Result};

use reqwest::{blocking::Client, Url};
use vm_server::req_resp::{InitAssemblyTargetReq, InitAssemblyTargetResp};

use self::req_resp_ds::*;

mod req_resp_ds;

pub use req_resp_ds::{
    AccessType, PingPongPageTrackInitResp, PingPongPageTrackReq, SingleStepTarget,
    SingleStepVictimInitResp,
};

pub fn single_step_victim_init(
    basepath: &str,
    program: SingleStepTarget,
) -> Result<SingleStepVictimInitResp> {
    let url = Url::parse(basepath)?;
    let url = url.join("/single-step-victim/init")?;

    let p = SingleStepVictimInitReq {
        victim_program: program.to_string(),
    };
    let client = reqwest::blocking::Client::new();
    let res: SingleStepVictimInitResp = client.post(url).json(&p).send()?.json()?;

    Ok(res)
}

pub fn single_step_victim_start(basepath: &str, program: SingleStepTarget) -> Result<()> {
    let url = Url::parse(basepath)?;
    let url = url.join("/single-step-victim/start")?;

    let p = SingleStepVictimStartReq {
        victim_program: program.to_string(),
    };
    let client = reqwest::blocking::Client::new();
    let res = client.post(url).json(&p).send()?;
    res.error_for_status()?;

    Ok(())
}

pub fn pagetrack_victim_init(
    basepath: &str,
    args: &PingPongPageTrackReq,
) -> Result<PingPongPageTrackInitResp> {
    let url = Url::parse(basepath).context(format!("cannot parse {} as url", basepath))?;
    const SUB_URL: &'static str = "/pingpong-pagetrack/init";
    let url = url.join(&SUB_URL).context(format!(
        "failed to append {} to base URL {}",
        SUB_URL, basepath
    ))?;

    let client = reqwest::blocking::Client::new();
    let res = client
        .post(url)
        .json(args)
        .send()
        .context("error sending request")?;
    let res = res.error_for_status().context("request failed")?;

    let res = res.json().context("failed to parse body")?;
    Ok(res)
}

pub fn pagetrack_victim_start(_basepath: &str) -> Result<()> {
    let url = Url::parse(_basepath)?;
    let url = url.join("/pingpong-pagetrack/start")?;

    let client = reqwest::blocking::Client::new();
    let res = client.post(url).send()?;
    res.error_for_status()?;

    Ok(())
}

pub fn pagetrack_victim_teardown(_basepath: &str) -> Result<()> {
    let url = Url::parse(_basepath)?;
    let url = url.join("/pingpong-pagetrack/teardown")?;

    let client = reqwest::blocking::Client::new();
    let res = client.post(url).send()?;
    res.error_for_status()?;

    Ok(())
}

pub fn assembly_target_new(
    basepath: &str,
    req: &InitAssemblyTargetReq,
) -> Result<InitAssemblyTargetResp> {
    let url = Url::parse(basepath).context(format!("failed to parse {} as url", basepath))?;
    let url = url.join("/assembly-target/new")?;

    let client = Client::new();
    let resp = client
        .post(url.clone())
        .json(&req)
        .send()
        .context(format!("error sending post request to {}", url))?;

    let assembly_target_resp;
    if !resp.status().is_success() {
        bail!("server returned error {}", resp.text()?)
    } else {
        assembly_target_resp = resp
            .json::<InitAssemblyTargetResp>()
            .context("failed to parse server response")?;
    }

    Ok(assembly_target_resp)
}

pub fn assembly_target_run(basepath: &str) -> Result<()> {
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
