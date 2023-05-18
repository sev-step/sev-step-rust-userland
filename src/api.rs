//! Userspace client for the SEV STEP kernel API.
//!
//! The API is stateful, in the sense that you must initialize the connection before
//! you can use any of the API functions and close it when you are done. Currently, there
//! may be only one API connection/client at a time. The connection is automatically closed
//! when the [`SevStep`](struct@SevStep) struct, representing the connection, is dropped.
//!
//! To create a new connection use
//! ```
//! //pass true if your VM is running in debug mode, else false
//! let api_ctx = SevStep::new(true)
//! ```
use crate::{
    ioctls, raw_spinlock,
    types::{shared_mem_region_t, usp_init_poll_api_t, SEV_STEP_SHARED_MEM_BYTES},
};
use anyhow::{bail, Context, Result};
use log::error;
use std::{fs::File, os::fd::AsRawFd};
use std::{mem, process};

#[repr(C, align(4096))]
///Page aligned array of size `SEV_STEP_SHARED_MEM_BYTES`. This is only
/// a custom type so that we can use repr C to achieve the alignment
struct AlignedSevStepBuf([u8; SEV_STEP_SHARED_MEM_BYTES as usize]);

///Main context struct for interacting with the SEV STEP API.
///Will automatically close the connection to kernel space when dropped
pub struct SevStep<'a> {
    raw_shared_mem: AlignedSevStepBuf,
    shared_mem_region: &'a mut shared_mem_region_t,
    kvm: File,
}

impl<'a> Drop for SevStep<'a> {
    ///Free internal resources an close connection with kernel counterpart. This may fail however,
    /// errors are only logged.
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = ioctls::kvm_usp_close_poll_api(self.kvm.as_raw_fd()) {
                error!("Error closing API: {}", e);
            }
        }
    }
}

impl<'a> SevStep<'a> {
    ///Initiate the SevStep API. There may be only one instance open at a time.
    pub fn new(decrypt_vmsa: bool) -> Result<Self> {
        //alloc buffer
        let mut raw_shared_mem: AlignedSevStepBuf =
            AlignedSevStepBuf([0; SEV_STEP_SHARED_MEM_BYTES as usize]);

        //create shared_mem_region_t "view" into buffer
        assert!(SEV_STEP_SHARED_MEM_BYTES as usize >= mem::size_of::<shared_mem_region_t>());
        let shared_mem_ptr = raw_shared_mem.0.as_mut_ptr();
        let shared_mem_region;
        unsafe {
            shared_mem_region = (shared_mem_ptr as *mut shared_mem_region_t)
                .as_mut()
                .unwrap();
        }

        //init shared_mem_region
        raw_spinlock::init(&mut shared_mem_region.spinlock);
        shared_mem_region.event_acked = 1;
        shared_mem_region.have_event = 0;

        //call api init ioctl
        let mut params = usp_init_poll_api_t {
            pid: process::id() as i32,
            user_vaddr_shared_mem: shared_mem_ptr as u64,
            decrypt_vmsa,
        };
        let kvm = File::open("/dev/kvm").context("failed to open kvm file")?;
        unsafe {
            let msg = "Failed to issue kvm_usp_init_poll_api ioctl";
            match ioctls::kvm_usp_init_poll_api(kvm.as_raw_fd(), &mut params) {
                Ok(0) => (),
                Ok(code) => bail!("{} : {}", msg, code),
                Err(e) => bail!("{} : {}", msg, e),
            }
        }

        //success
        Ok(SevStep {
            raw_shared_mem,
            shared_mem_region,
            kvm,
        })
    }
}
