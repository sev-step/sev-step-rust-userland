use rand::distributions::{Alphanumeric, DistString};
use std::{
    env::temp_dir,
    fs::create_dir,
    io::BufReader,
    sync::{Arc, Mutex},
};

use crate::{
    assembly_target::{page_ping_ponger::PagePingPonger, AssemblyTarget, RunnableTarget},
    req_resp::{
        InitAssemblyTargetReq, InitAssemblyTargetResp, InitCustomTargetResp, InitPagePingPongerReq,
        InitPagePingPongerResp,
    },
    virt_to_phys::{self, LinuxPageMap, VirtToPhysResolver},
};

use crate::external_target::ExternalTarget;
use anyhow::{anyhow, bail, Context};
use axum::{
    body::Bytes,
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::{debug, error};
use tar::Archive;

// Make our own error that wraps `anyhow::Error`.
pub struct AppError(anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
#[derive(Clone)]
pub struct ServerState {
    pub target_programm: Option<Arc<Mutex<dyn RunnableTarget + Send>>>,
}

pub async fn init_custom_target_program_handler(
    State(state): State<Arc<Mutex<ServerState>>>,
    mut form: Multipart,
) -> Result<Json<InitCustomTargetResp>, AppError> {
    debug!("init_custom_target_program_handler: parsing form data...");
    let mut execute_cmd = None;
    let mut file_bytes = None;
    while let Some(field) = form.next_field().await? {
        let name = if let Some(v) = field.name() {
            v.to_string()
        } else {
            return Err(anyhow!("error reading name for field ").into());
        };

        if name == "execute_cmd" {
            execute_cmd = Some(field.text().await?)
        } else if name == "file_archive" {
            file_bytes = Some(field.bytes().await?)
        } else {
            return Err(anyhow!("unexpected form field {}", name).into());
        }
    }

    let execute_cmd = execute_cmd.ok_or(anyhow!("execute_cmd field is missing"))?;
    let file_bytes = file_bytes.ok_or(anyhow!("file_archive field is missing"))?;

    debug!(
        "parsed form: execute_cmd={}, uploaded file has {} bytes",
        execute_cmd,
        file_bytes.len()
    );

    match init_custom_target_program(state, execute_cmd, file_bytes) {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            error!("init_custom_target_program failed with {:?}", e);
            Err(AppError::from(e))
        }
    }
}

fn init_custom_target_program(
    state: Arc<Mutex<ServerState>>,
    execute_cmd: String,
    file_bytes: Bytes,
) -> Result<InitCustomTargetResp, anyhow::Error> {
    //unpack archive into tmp dir
    let rand_suffix = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);
    let archive_dir_path = temp_dir().join(format!("vmserver_{}", rand_suffix));
    create_dir(&archive_dir_path)?;

    debug!("unpacking uploaded file to {:?}", &archive_dir_path);

    let file_bytes = file_bytes.to_vec();
    let mut archive = Archive::new(BufReader::new(file_bytes.as_slice()));
    archive.unpack(&archive_dir_path)?;

    //TODO: make sure files are marked as executeable

    //execute "setup phase"
    let cmd_tokens: Vec<_> = execute_cmd.split(" ").collect();
    let cmd = cmd_tokens[0];
    let args = cmd_tokens
        .into_iter()
        .skip(1)
        .map(|v| v.to_string())
        .collect();
    debug!(
        "target working directory:{:?} , target command:{} , additional cli args:{:?}",
        &archive_dir_path, cmd, &args
    );
    let p = ExternalTarget::new(
        archive_dir_path
            .to_str()
            .ok_or(anyhow!("failed to convert archive_dir_path to str"))?
            .to_string(),
        cmd.to_string(),
        args,
    )?;

    let resp = InitCustomTargetResp {
        setup_output: p.get_key_value_pairs().clone(),
    };
    debug!("Captured key-value pairs: {:?}", resp.setup_output);

    let mut state = match state.lock() {
        Ok(v) => v,
        Err(e) => bail!("failed to acquire state lock {}", e),
    };

    debug!("Storing prog in global state");
    state.target_programm = Some(Arc::new(Mutex::new(p)));

    debug!("done");
    Ok(resp)
}

pub async fn init_assembly_target_handler(
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(req): Json<InitAssemblyTargetReq>,
) -> Result<Json<InitAssemblyTargetResp>, AppError> {
    match init_assembly_target(state, req) {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            error!("init_assembly_target failed with: {:?}", e);
            Err(AppError::from(e))
        }
    }
}

fn init_assembly_target(
    state: Arc<Mutex<ServerState>>,
    req: InitAssemblyTargetReq,
) -> Result<InitAssemblyTargetResp, anyhow::Error> {
    let prog = AssemblyTarget::new(req.code, req.required_mem_bytes)
        .context("failed to instantiate supplied program")?;

    let mut pagemap_parser = virt_to_phys::LinuxPageMap::new()?;

    debug!("translate code_vaddr to paddr");
    let code_paddr = pagemap_parser
        .get_phys(prog.get_code_vaddr())
        .context(format!(
            "failed to translate 0x{:x} to phys addr",
            prog.get_code_vaddr()
        ))?;
    debug!("translating data_buffer to paddr");
    let data_buffer_paddr = pagemap_parser
        .get_phys(prog.get_data_buffer_vaddr())
        .context(format!(
            "failed to translate 0x{:x} to phys addr",
            prog.get_data_buffer_vaddr()
        ))?;

    debug!("building response");
    let resp = InitAssemblyTargetResp {
        code_vaddr: prog.get_code_vaddr(),
        code_paddr,
        data_buffer_vaddr: prog.get_data_buffer_vaddr(),
        data_buffer_paddr,
        data_buffer_bytes: req.required_mem_bytes,
        instructions_with_rip: prog.get_instr_with_rip().clone(),
    };

    debug!("aquiring state lock");
    let mut state = match state.lock() {
        Ok(v) => v,
        Err(e) => bail!("failed to aquire state lock {}", e),
    };

    debug!("Storing prog in global state");
    state.target_programm = Some(Arc::new(Mutex::new(prog)));

    debug!("Sending response {}", resp);
    Ok(resp)
}

pub async fn run_target_handler(
    State(state): State<Arc<Mutex<ServerState>>>,
) -> Result<(), AppError> {
    match run_target(state) {
        Ok(_) => Ok(()),
        Err(e) => {
            error!("run_target_handler failed with {:?}", e);
            Err(AppError::from(e))
        }
    }
}

fn run_target(state: Arc<Mutex<ServerState>>) -> Result<(), anyhow::Error> {
    let state = match state.lock() {
        Ok(v) => v,
        Err(e) => bail!("failed to aquire state lock {}", e),
    };

    match &state.target_programm {
        Some(prog_mutex) => match &mut prog_mutex.lock() {
            Ok(prog) => {
                debug!("Running target program");
                unsafe { prog.run()? }
            }
            Err(e) => bail!("Failed to get target program : {:?}", e),
        },
        None => bail!("target program not initialized"),
    }

    debug!("run_target handler done");
    Ok(())
}

pub async fn init_page_ping_ponger_handler(
    State(state): State<Arc<Mutex<ServerState>>>,
    Json(req): Json<InitPagePingPongerReq>,
) -> Result<Json<InitPagePingPongerResp>, AppError> {
    match init_page_ping_ponger(state, req) {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            error!("init_page_ping_ponger failed with {:?}", e);
            Err(AppError::from(e))
        }
    }
}

pub fn init_page_ping_ponger(
    state: Arc<Mutex<ServerState>>,
    req: InitPagePingPongerReq,
) -> Result<InitPagePingPongerResp, anyhow::Error> {
    let p = PagePingPonger::new(&req.variant, req.rounds).context(format!(
        "failed to instantiate {:?} ping ponger with {} rounds",
        req.variant, req.rounds
    ))?;

    let mut pagemap = LinuxPageMap::new()?;
    let page_paddrs = p
        .get_vaddrs()
        .iter()
        .map(|v| {
            pagemap
                .get_phys(*v)
                .context(format!("failed to resolve {} to padddr", *v))
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .context("failed to resolve vaddrs to paddrs")?;
    let page_paddrs: [usize; 2] = match page_paddrs.len() {
        2 => [page_paddrs[0], page_paddrs[1]],
        v => bail!("expected page_paddrs to have length 2 but got {}", v),
    };

    let resp = InitPagePingPongerResp {
        page_vaddrs: p.get_vaddrs(),
        page_paddrs: page_paddrs,
        variant: req.variant,
    };
    debug!("aquiring state lock");
    let mut state = match state.lock() {
        Ok(v) => v,
        Err(e) => bail!("failed to aquire state lock {}", e),
    };

    state.target_programm = Some(Arc::new(Mutex::new(p)));

    Ok(resp)
}
