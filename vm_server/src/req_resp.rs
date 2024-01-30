use std::{collections::HashMap, fmt::Display};

use iced_x86::Instruction;
use serde::{Deserialize, Serialize};

use crate::assembly_target::page_ping_ponger::PagePingPongVariant;

/// The uploaded program must adhere to the following interface on stdin/stdout
/// After starting the binary via `execute_cmd`, it may do some arbitrary setup. To indicate that it
/// is done with the setup phase, it must output `VMSERVER::SETUP_DONE` on a single line to stdout.
/// Afterwards, it must wait for `VMSERVER::START` on stdin before it is allowed to start any payload logic.
/// To send the start command, the [`run_target`]  API enpoint must be called.
/// During the setup phase, it may write arbitrary data to stdout. In order to send information (like a memory address)
/// back to the client it may output lines of the format `VMSERVER::VAR <NAME> <VALUE>`. Both `<NAME>` and `<VALUE`> may not
/// contain any whitespaces. The tuples (<NAME>,<VALUE>) are send back to the calling client as part of [`InitCustomTargetResp`]
///
#[derive(Deserialize, Debug)]
pub struct InitCustomTargetReq {
    ///Path to folder containing all files for the custom binary that should get executed
    pub folder_path: String,
    ///Command to execute the custom binary, assuming the current working directory is a at `folder_path`. You can
    /// also supply cli args.
    pub execute_cmd: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct InitCustomTargetResp {
    ///Key value pairs recorded during the setup phase. See comment on [`InitCustomTargetReq`] for a desription
    pub setup_output: HashMap<String, String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct InitPagePingPongerReq {
    ///selects the type of access that should be performed
    pub variant: PagePingPongVariant,
    ///selects the number of rounds. One round consists of one access to each of the two pages accessed by the ping ponger
    pub rounds: u32,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct InitPagePingPongerResp {
    ///virtual addresses of the two pages accessed by the ping ponger
    pub page_vaddrs: [usize; 2],
    /// physical addresses for `page_vaddrs`
    pub page_paddrs: [usize; 2],
    ///Same as in request. Just for convenience
    pub variant: PagePingPongVariant,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct InitAssemblyTargetReq {
    pub code: Vec<Instruction>,
    //code requires to be called with ptr to a page aligned buffer
    //of this size
    pub required_mem_bytes: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InitAssemblyTargetResp {
    ///Virtual address where the code from the request has been placed
    /// Guaranteed to be page aligned
    pub code_vaddr: usize,
    ///Physical address for `code_vaddr`
    pub code_paddr: usize,
    ///Virtual address of the data buffer supplied to the code in rdi
    pub data_buffer_vaddr: usize,
    ///Physical address for `data_buffer_vaddr`
    pub data_buffer_paddr: usize,
    ///Same as in the request. Just for convenience
    pub data_buffer_bytes: usize,
    /// Instructions from the request with their final RIP value. Substract
    /// `code_vaddr` to get the expected offsets inside the code page.
    pub instructions_with_rip: Vec<Instruction>,
}

impl Display for InitAssemblyTargetResp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,
                "InitAssemblyTargetResp(code_vaddr=0x{:x}, code_paddr=0x{:x}, data_buffer_vaddr=0x{:x}, data_buffer_paddr=0x{:x}, data_buffer_bytes=0x{:x})",self.code_vaddr,self.code_paddr,self.data_buffer_vaddr,self.data_buffer_paddr,self.data_buffer_bytes
            )
    }
}
