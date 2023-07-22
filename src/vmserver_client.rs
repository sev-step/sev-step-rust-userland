use anyhow::{Result};

use reqwest::Url;


use self::req_resp_ds::*;

mod req_resp_ds;

pub use req_resp_ds::{SingleStepTarget, SingleStepVictimInitResp};

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
