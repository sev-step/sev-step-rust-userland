use std::arch::asm;

use super::{AssemblyTarget, RunnableTarget};
use anyhow::{Context, Result};
use iced_x86::code_asm::*;
use log::debug;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter};

/// Describes which kind of access a [`PagePingPonger`] performs
/// to it's two pages
#[derive(Serialize, Deserialize, Debug, EnumIter, Display)]
pub enum PagePingPongVariant {
    READ,
    WRITE,
    EXEC,
}

///Program that alternates accessing two page aligned pages using one of the access types from [`PagePingPongVariant`] as the access type
pub struct PagePingPonger {
    code: AssemblyTarget,
    page_vaddrs: Vec<usize>,
}

unsafe impl Send for PagePingPonger {}

impl PagePingPonger {
    /// # Arguments
    /// * `mode` : specifies the access type
    /// * `rounds` : One rounds consists of reading once from both pages
    ///
    pub fn new(mode: &PagePingPongVariant, rounds: u32) -> Result<PagePingPonger> {
        let (code, page_vaddrs) = match mode {
            PagePingPongVariant::READ => {
                let mut a =
                    CodeAssembler::new(64).context("failed to instantiate CodeAssembler")?;

                const DATA_BUFFER_BYTES: usize = 2 * 4096;

                for i in 0..rounds {
                    a.mov(rsi, qword_ptr(rdi))
                        .context(format!("failed to add {}th read from first page", i))?;
                    a.mov(rsi, qword_ptr(rdi + 4096))
                        .context(format!("failed to add {}th read from second page", i))?;
                }
                a.ret().context("failed to add final add")?;

                let code = AssemblyTarget::new(a.take_instructions(), DATA_BUFFER_BYTES)
                    .context("failed to assemble")?;

                let page_vaddrs = vec![
                    code.get_data_buffer_vaddr(),
                    code.get_data_buffer_vaddr() + 4096,
                ];

                (code, page_vaddrs)
            }
            PagePingPongVariant::WRITE => {
                let mut a =
                    CodeAssembler::new(64).context("failed to instantiate CodeAssembler")?;

                const DATA_BUFFER_BYTES: usize = 2 * 4096;

                for i in 0..rounds {
                    a.mov(qword_ptr(rdi), 42)
                        .context(format!("failed to add {}th write to first page", i))?;
                    a.mov(qword_ptr(rdi + 4096), 42)
                        .context(format!("failed to add {}th write to second page", i))?;
                }
                a.ret().context("failed to add final add")?;

                let code = AssemblyTarget::new(a.take_instructions(), DATA_BUFFER_BYTES)
                    .context("failed to assemble")?;

                let page_vaddrs = vec![
                    code.get_data_buffer_vaddr(),
                    code.get_data_buffer_vaddr() + 4096,
                ];
                (code, page_vaddrs)
            }
            PagePingPongVariant::EXEC => unsafe {
                let mut a =
                    CodeAssembler::new(64).context("failed to instantiate CodeAssembler")?;

                debug!(
                    "aligned_target_fn1 is at vaddr 0x{:x}, aligned_target_fn2 is at vaddr 0x{:x}",
                    aligned_target_fn1 as u64, aligned_target_fn2 as u64
                );
                for i in 0..rounds {
                    a.call(aligned_target_fn1 as u64).context(format!(
                        "failed to add {}th call to 0x{:x}",
                        i, aligned_target_fn1 as u64
                    ))?;
                    a.call(aligned_target_fn2 as u64).context(format!(
                        "failed to add {}th call to 0x{:x}",
                        i, aligned_target_fn2 as u64
                    ))?;
                }
                a.ret().context("failed to add final add")?;

                let code =
                    AssemblyTarget::new(a.take_instructions(), 0).context("failed to assemble")?;
                let page_vaddrs = vec![aligned_target_fn1 as usize, aligned_target_fn2 as usize];

                (code, page_vaddrs)
            },
        };

        Ok(PagePingPonger { code, page_vaddrs })
    }

    /// returns the virtual addresses of the two pages that are accessed. The first
    /// page is accessed first
    pub fn get_vaddrs(&self) -> [usize; 2] {
        [self.page_vaddrs[0], self.page_vaddrs[1]]
    }
}

impl RunnableTarget for PagePingPonger {
    unsafe fn run(&mut self) -> Result<()> {
        self.code.run()
    }

    unsafe fn stop(self) -> Result<()> {
        Ok(())
    }
}

/// A page aligned function that is never inlined and does nothing
/// Used by the `EXEC` variant of the [`PagePingPonger`]
#[inline(never)]
fn aligned_target_fn1() {
    unsafe {
        asm!(".align 4096", "nop");
    };
}

/// A page aligned function that is never inligned and does nothing
/// Used by the `EXEC` variant of the [`PagePingPonger`]
#[inline(never)]
fn aligned_target_fn2() {
    unsafe {
        asm!(".align 4096", "nop", "nop");
    };
}

#[cfg(test)]
mod tests {
    use super::{PagePingPongVariant, PagePingPonger};
    use crate::assembly_target::RunnableTarget;
    use anyhow::{Context, Result};
    use strum::IntoEnumIterator;
    #[test]
    /// We cannot check much here. But at least we can make sure that the JIT code
    /// compiles and does not crash at runtime
    fn run_all_ping_pongers() -> Result<()> {
        for variant in PagePingPongVariant::iter() {
            let mut p = PagePingPonger::new(&variant, 10)
                .context(format!("failed to init {} ping ponger", variant))?;
            unsafe { p.run() };
        }

        Ok(())
    }
}
