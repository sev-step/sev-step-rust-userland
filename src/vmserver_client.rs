use anyhow::{Context, Result};

use reqwest::Url;

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
