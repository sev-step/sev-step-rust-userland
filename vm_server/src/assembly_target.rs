use anyhow::{bail, Context, Result};
use iced_x86::{code_asm::CodeAssembler, Decoder, DecoderOptions, Instruction};
use log::{debug, error};
use nix::{
    libc::memcpy,
    sys::mman::{self, munmap, MapFlags, ProtFlags},
};
use std::{arch::asm, ffi::c_void, num::NonZeroUsize};

pub mod page_ping_ponger;

pub trait RunnableTarget {
    unsafe fn run(&self);
}

#[derive(Clone)]
pub struct AssemblyTarget {
    code_buffer: *mut c_void,
    code_buffer_bytes: usize,

    ///instructions making up the code with their final rip values
    instructions_with_rip: Vec<Instruction>,

    data_buffer: *mut c_void,
    data_buffer_bytes: usize,
}

unsafe impl Send for AssemblyTarget {}

impl AssemblyTarget {
    /// Allocates required resources to run the code with a data buffer of the given size
    /// # Arguments
    /// * `code` : Gets assembled and loaded into page aligned, executeable memory. Is called with a pointer to page aligned memory of size at least `data_buffer_bytes`. Code is wrapped into assembly stub
    /// to guarantee C calling convections
    /// * `data_buffer_bytes` size of the data buffer. Is rounded up to be a multiple of page size
    pub fn new(code: Vec<Instruction>, data_buffer_bytes: usize) -> Result<AssemblyTarget> {
        let mut assembler = CodeAssembler::new(64)?;
        for x in code {
            assembler
                .add_instruction(x)
                .context(format!("failed to add instruction {} to assembler", x))?;
        }

        //allocate page aligned buffers for code and data. We round up sizes to be page aligned.
        //To get the required size for the code, we to one dummy assembly. Later on, we assemble
        //again with the correct ip addr
        let required_code_bytes = assembler.assemble(0)?.len();
        debug!("assembled code requires 0x{:x} bytes", required_code_bytes);
        //round up to next full page size
        let required_code_bytes = required_code_bytes + (4096 - (required_code_bytes % 4096));
        debug!(
            "going to allocate 0x{:x} bytes for code",
            required_code_bytes
        );
        let required_code_bytes =
            NonZeroUsize::new(required_code_bytes).context("required code bytes are zero")?;

        debug!("requested data buffer bytes: 0x{:x}", data_buffer_bytes);
        let data_buffer_bytes =
            NonZeroUsize::new(data_buffer_bytes + (4096 - (data_buffer_bytes % 4096)))
                .context("page aligned data buffer bytes are zero")?;
        debug!(
            "data_buffer_bytes after rounding: 0x{:x}",
            data_buffer_bytes.get()
        );

        let code_buffer;
        let data_buffer;
        unsafe {
            code_buffer = mman::mmap(
                None,
                required_code_bytes,
                ProtFlags::PROT_EXEC | ProtFlags::PROT_WRITE | ProtFlags::PROT_READ,
                MapFlags::MAP_ANON | MapFlags::MAP_PRIVATE | MapFlags::MAP_POPULATE,
                -1,
                0,
            )
            .context("failed to allocate code buffer")?;

            data_buffer = mman::mmap(
                None,
                data_buffer_bytes,
                ProtFlags::PROT_WRITE | ProtFlags::PROT_READ,
                MapFlags::MAP_ANON | MapFlags::MAP_PRIVATE | MapFlags::MAP_POPULATE,
                -1,
                0,
            )
            .context("failed to allocate data buffer")?;
        }
        if (code_buffer as u64 % 4096) != 0 {
            bail!(
                "expected code buffer to be page aligned but got {}",
                code_buffer as u64
            );
        }
        if (data_buffer as u64 % 4096) != 0 {
            bail!(
                "expected data buffer to be page aligned but got {}",
                data_buffer as u64
            );
        }

        //do final code assembly, copy code to target location and cast to c function pointer
        let code = assembler.assemble(code_buffer as u64)?;
        if code.len() > required_code_bytes.get() {
            bail!(
                "final assembly requries {} bytes but code buffer is only {}",
                code.len(),
                required_code_bytes
            );
        }
        unsafe {
            memcpy(code_buffer, code.as_ptr().cast(), code.len());
        }

        let decoder = Decoder::with_ip(64, &code, code_buffer as u64, DecoderOptions::NONE);
        let instructions_with_rip = decoder.into_iter().collect();

        Ok(AssemblyTarget {
            code_buffer,
            code_buffer_bytes: required_code_bytes.get(),
            data_buffer,
            data_buffer_bytes: data_buffer_bytes.get(),
            instructions_with_rip,
        })
    }

    ///virtual address at which the code is located
    pub fn get_code_vaddr(&self) -> usize {
        self.code_buffer as usize
    }

    ///All instructions with their final, absolute rip value set.
    /// Sbustract [`get_code_vaddr`], to get the offset inside the code buffer
    pub fn get_instr_with_rip(&self) -> &Vec<Instruction> {
        &self.instructions_with_rip
    }

    ///virtual address of the data buffer
    pub fn get_data_buffer_vaddr(&self) -> usize {
        self.data_buffer as usize
    }
}

impl RunnableTarget for AssemblyTarget {
    ///Executes the code
    unsafe fn run(&self) {
        unsafe {
            asm!(
                //save registers for arguments, except, rdi and rax, which are handled by the inout in the asm macro
                "push rsi",
                "push rdx",
                "push rcx",
                "push r8",
                "push r9",
                //registers that a well behaved x86_64 sytem v functions should leave unchanged, but we are better save then sorry
                "push rbx",
                "push rbp",
                "push r12",
                "push r13",
                "push r14",
                "push r15",
                //execute target fucntion
                "call rax",
                "pop r15",
                "pop r14",
                "pop r13",
                "pop r12",
                "pop rbp",
                "pop rbx",
                //restore registers
                "pop r9",
                "pop r8",
                "pop rcx",
                "pop rdx",
                "pop rsi",
                inout("rax") (self.code_buffer) as u64 => _,
                // 1st argument in rdi, which is caller-saved
                inout("rdi") self.data_buffer as u64 => _
            );
        }
    }
}

impl Drop for AssemblyTarget {
    fn drop(&mut self) {
        debug!(
            "Dropping AssemblyTarget with code_vaddr=0x{:x} and data_buffer=0x{:x}",
            self.code_buffer as usize, self.data_buffer as usize
        );
        unsafe {
            if let Err(e) = munmap(self.code_buffer, self.code_buffer_bytes) {
                error!(
                    "failed to munmap code buffer at vaddr 0x{:x} with len=0x{:x} : {}",
                    self.code_buffer as u64, self.code_buffer_bytes, e
                );
            }
            if let Err(e) = munmap(self.data_buffer, self.data_buffer_bytes) {
                error!(
                    "failed to munmap data buffer at vaddr 0x{:x} with len=0x{:x} : {}",
                    self.data_buffer as u64, self.data_buffer_bytes, e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, Result};
    use iced_x86::code_asm::*;

    use super::AssemblyTarget;
    use super::RunnableTarget;
    #[test]
    fn catch_caller_preserved_regs() -> Result<()> {
        let mut a = CodeAssembler::new(64)?;
        for r in [rbx, r12, r13, r14, r15] {
            a.mov(r, 42_u64)
                .context(format!("failed to add mov for register {:?}", r))?;
        }
        a.ret()?;

        let target = AssemblyTarget::new(a.take_instructions(), 0)?;

        unsafe { target.run() };

        Ok(())
    }

    #[test]
    fn access_memory_buffer() -> Result<()> {
        let mut a = CodeAssembler::new(64)?;
        let data_buffer_size = 4096;
        for _off in (0..4096).step_by(64) {
            a.mov(rsi, qword_ptr(rdi))
                .context(format!("failed add add mov for mem offset 0x{:x}", _off))?;
            a.add(rdi, 64)
                .context(format!("failed to add add to rdi at _off=0x{:x}", _off))?;
        }
        a.ret()?;

        let target = AssemblyTarget::new(a.take_instructions(), data_buffer_size)?;

        unsafe { target.run() };

        Ok(())
    }
}
