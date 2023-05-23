use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use self::req_resp_ds::*;

mod req_resp_ds;

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
    let res: SingleStepVictimInitResp =
        client.post("http://httpbin.org").json(&p).send()?.json()?;

    Ok(res)
}
