use std::sync::{Arc, Mutex};

use crate::{
    assembly_target::AssemblyTarget,
    req_resp::{InitAssemblyTargetReq, InitAssemblyTargetResp},
    virt_to_phys::{self, VirtToPhysResolver},
};

use anyhow::{bail, Context};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::{debug, error};

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
    //TODO: think about if AssemblyTarget is Sync. If so, we could drop the mutex
    pub assembly_target: Option<Arc<Mutex<AssemblyTarget>>>,
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
    state.assembly_target = Some(Arc::new(Mutex::new(prog)));

    debug!("Sending response {}", resp);
    Ok(resp)
}

pub async fn run_assembly_target_handler(
    State(state): State<Arc<Mutex<ServerState>>>,
) -> Result<(), AppError> {
    match run_assembly_target(state) {
        Ok(_) => Ok(()),
        Err(e) => {
            error!("run_assembly_target failed with {:?}", e);
            Err(AppError::from(e))
        }
    }
}

fn run_assembly_target(state: Arc<Mutex<ServerState>>) -> Result<(), anyhow::Error> {
    let state = match state.lock() {
        Ok(v) => v,
        Err(e) => bail!("failed to aquire state lock {}", e),
    };

    match &state.assembly_target {
        Some(prog_mutex) => match prog_mutex.lock() {
            Ok(prog) => unsafe { prog.run() },
            Err(_) => todo!(),
        },
        None => bail!("assembly target not initialized"),
    }

    Ok(())
}
