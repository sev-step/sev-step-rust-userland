use std::fmt::Display;

use iced_x86::Instruction;
use serde::{Deserialize, Serialize};

use crate::assembly_target::page_ping_ponger::PagePingPongVariant;

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
