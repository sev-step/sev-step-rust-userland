use anyhow::{bail, Context, Result};

pub trait VirtToPhysResolver {
    fn get_phys(&mut self, virt: usize) -> Result<usize>;
}

///LinuxPageMap uses /proc/self/pagemap to translate virtual to physical addresses.
/// Requires root rights
pub struct LinuxPageMap {
    pagemap_wrapper: pagemap::PageMap,
}

impl LinuxPageMap {
    pub fn new() -> Result<LinuxPageMap> {
        let pid = std::process::id();
        let res = LinuxPageMap {
            pagemap_wrapper: pagemap::PageMap::new(pid as u64)
                .with_context(|| "failed to open pagemap")?,
        };
        Ok(res)
    }
}

impl VirtToPhysResolver for LinuxPageMap {
    fn get_phys(&mut self, virt: usize) -> Result<usize> {
        //calc virtual address of page containing ptr_to_start
        let vaddr_start_page = virt & !0xFFF;
        let vaddr_end_page = vaddr_start_page + 4095;

        //query pagemap
        let memory_region =
            pagemap::MemoryRegion::from((vaddr_start_page as u64, vaddr_end_page as u64));
        let entry = self
            .pagemap_wrapper
            .pagemap_region(&memory_region)
            .context(format!(
                "failed to query pagemap for memory region {:?}",
                memory_region
            ))?;
        if entry.len() != 1 {
            bail!(
                "Got {} pagemap entries for virtual address 0x{:x}, expected exactly one",
                entry.len(),
                virt
            )
        }

        let pfn = entry[0].pfn().context(format!(
            "failed to get PFN for pagemap entry {:?}",
            entry[0]
        ))?;
        if pfn == 0 {
            bail!(
                "Got invalid PFN 0 for virtual address 0x{:x}. Are we root?",
                virt,
            )
        }
        let phys_addr = (pfn << 12) | ((virt as u64) & 0xFFF);

        Ok(phys_addr as usize)
    }
}
