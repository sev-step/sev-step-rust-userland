//! Userspace client for the SEV STEP kernel API.
//!
//! The API is stateful, in the sense that you must initialize the connection before
//! you can use any of the API functions and close it when you are done. Currently, there
//! may be only one API connection/client at a time. The connection is automatically closed
//! when the [`SevStep`](struct@SevStep) struct, representing the connection, is dropped.
//!
//!
use crate::{
    ioctls, raw_spinlock,
    types::{
        kvm_page_track_mode, sev_step_event_t, sev_step_param_t, sev_step_partial_vmcb_save_area_t,
        shared_mem_region_t, track_all_pages_t, track_page_param_t, usp_event_type_t,
        usp_init_poll_api_t, usp_page_fault_event_t, vmsa_register_name_t,
        SEV_STEP_SHARED_MEM_BYTES,
    },
};
use anyhow::{anyhow, Context, Result as AhwResult};
use core::slice;
use crossbeam::channel::{bounded, Receiver, TryRecvError};
use log::{debug, error, warn};
use std::{fs::File, os::fd::AsRawFd, time::Instant};
use std::{mem, process};
use std::{thread, time::Duration};
use thiserror::Error;
use SevStepError::MultiStep;

#[derive(Error, Debug)]
pub enum SevStepError {
    #[error("failed to execute trigger function : {source}")]
    TriggerFailed {
        #[source]
        source: anyhow::Error,
    },
    #[error("operation timed out")]
    Timeout,
    #[error(
        "page tracking error, gpa=0x{:x}, mode={:?}, message={} : {}",
        gpa,
        tracking_mode,
        message,
        source
    )]
    PageTracking {
        #[source]
        source: anyhow::Error,
        gpa: u64,
        tracking_mode: kvm_page_track_mode,
        message: String,
    },
    #[error("multi step")]
    MultiStep { event: SevStepEvent },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[repr(C, align(4096))]
///Page aligned array of size `SEV_STEP_SHARED_MEM_BYTES`. This is only
/// a custom type so that we can use repr C to achieve the alignment
struct AlignedSevStepBuf([u8; SEV_STEP_SHARED_MEM_BYTES as usize]);

///Main context struct for interacting with the SEV STEP API.
///Will automatically close the connection to kernel space when dropped
pub struct SevStep<'a> {
    _raw_shared_mem: AlignedSevStepBuf,
    shared_mem_region: &'a mut shared_mem_region_t,
    kvm: File,
    ///if we receive something on this channel, abort any blocking operations
    abort: Receiver<()>,
    ///If true, Abort with [`MultiStep`] if a multi step is encountered
    error_on_multi_step: bool,
}

impl<'a> Drop for SevStep<'a> {
    ///Free internal resources and close connection with kernel counterpart. This may fail however,
    /// errors are only logged.
    fn drop(&mut self) {
        if let Err(e) = self.stop_stepping() {
            error!("Failed to stop stepping: {}", e)
        }
        unsafe {
            if let Err(e) = ioctls::close_api(self.kvm.as_raw_fd()) {
                error!("Error closing API: {}", e);
            }
        }
    }
}

impl<'a> SevStep<'a> {
    ///Initiate the SevStep API. There may be only one instance open at a time.
    /// # Arguments
    /// - `decrypt_vmsa` : if true, try to decrypt register state. Requires SEV VM to run in debug mod
    /// - `abort` : The SEV STEP API has some blocking functions. Sending a signal and the `abort` channel will abort these blocking functions with an error
    /// - `error_on_multi_step` : if true, abort with [`MultiStep`] if a multi step is detected throughout
    /// the lifetime of this API connection
    pub fn new(
        decrypt_vmsa: bool,
        abort: Receiver<()>,
        error_on_multi_step: bool,
    ) -> Result<Self, SevStepError> {
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
            ioctls::init_api(kvm.as_raw_fd(), &mut params).context("init_api ioctl failed")?;
        }

        //success
        Ok(SevStep {
            _raw_shared_mem: raw_shared_mem,
            shared_mem_region,
            kvm,
            abort,
            error_on_multi_step,
        })
    }

    /// Track a single page of the VM with the given mode
    /// # Arguments
    /// * `gpa` - Guest Physical address of the page to track. Must be page aligned
    /// * `track_mode` - Tracking mode
    pub fn track_page(
        &self,
        gpa: u64,
        track_mode: kvm_page_track_mode,
    ) -> Result<(), SevStepError> {
        let mut p = track_page_param_t {
            gpa,
            track_mode: track_mode as i32,
        };
        match unsafe { ioctls::track_page(self.kvm.as_raw_fd(), &mut p) } {
            Ok(_) => Ok(()),
            Err(e) => Err(SevStepError::PageTracking {
                source: e.into(),
                gpa,
                tracking_mode: track_mode,
                message: "track_page failed".to_string(),
            }),
        }
    }

    /// Untrack a single page of the VM that was previously tracked with the given mode
    /// If you already got a page fault event for a page, it is automatically untracked
    /// See [`track_page`](Self::track_page) for parameter description
    pub fn untrack_page(
        &self,
        gpa: u64,
        track_mode: kvm_page_track_mode,
    ) -> Result<(), SevStepError> {
        let mut p = track_page_param_t {
            gpa,
            track_mode: track_mode as i32,
        };
        unsafe {
            ioctls::untrack_page(self.kvm.as_raw_fd(), &mut p)
                .context("untrack page ioctl failed")?;
        }

        Ok(())
    }

    /// Tracks all of the VM's memory pages with the given mode
    pub fn track_all_pages(&self, track_mode: kvm_page_track_mode) -> Result<(), SevStepError> {
        let mut p = track_all_pages_t {
            track_mode: track_mode as i32,
        };

        unsafe {
            ioctls::track_all_pages(self.kvm.as_raw_fd(), &mut p)
                .context("track all pages ioctl failed")?;
        }

        Ok(())
    }

    /// Untrack all of the VM's memory pages if they where previously tracked with the given
    /// mode
    pub fn untrack_all_pages(&self, track_mode: kvm_page_track_mode) -> Result<(), SevStepError> {
        let mut p = track_all_pages_t {
            track_mode: track_mode as i32,
        };

        unsafe {
            ioctls::untrack_all_pages(self.kvm.as_raw_fd(), &mut p)
                .context("untrack all pages ioctl failed")?;
        }

        Ok(())
    }

    pub fn start_stepping(
        &self,
        timer_value: u32,
        target_gpa: &mut [u64],
        flush_tlb: bool,
    ) -> Result<(), SevStepError> {
        let mut p = sev_step_param_t {
            tmict_value: timer_value,
            gpas_target_pages: target_gpa.as_mut_ptr(),
            gpas_target_pages_len: target_gpa.len() as u64,
            do_tlb_flush_before_each_step: flush_tlb,
        };

        unsafe {
            ioctls::start_stepping(self.kvm.as_raw_fd(), &mut p)
                .context("start stepping ioctl failed")?;
        }

        Ok(())
    }

    pub fn stop_stepping(&self) -> Result<(), SevStepError> {
        unsafe {
            ioctls::stop_stepping(self.kvm.as_raw_fd()).context("stop stepping ioctls failed")?;
        }
        Ok(())
    }

    /// Check if there is a new event. The Result only indicates whether we were
    /// able to check for an event. The option inside the result indicates if there was an
    /// event
    pub fn poll_event(&mut self) -> Result<Option<Event>, SevStepError> {
        unsafe {
            raw_spinlock::lock(&mut self.shared_mem_region.spinlock);
        }
        if 0 == self.shared_mem_region.have_event {
            unsafe {
                raw_spinlock::unlock(&mut self.shared_mem_region.spinlock);
            }
            return Ok(None);
        }

        //if we are here, we hold the lock and there was and event
        let result;
        match self.shared_mem_region.event_type {
            usp_event_type_t::PAGE_FAULT_EVENT => {
                let e: *const usp_page_fault_event_t =
                    self.shared_mem_region.event_buffer.as_ptr() as *const usp_page_fault_event_t;
                result = Event::PageFaultEvent(PageFaultEvent::from_c_struct(e));
            }
            usp_event_type_t::SEV_STEP_EVENT => {
                result = Event::StepEvent(SevStepEvent::from_raw_event_buffer(
                    &self.shared_mem_region.event_buffer,
                ));
            }
        }

        unsafe { raw_spinlock::unlock(&mut self.shared_mem_region.spinlock) }
        Ok(Some(result))
    }

    ///Execute `target_trigger` (in background) and block until we receive an event
    /// or the optional `timeout` expires.
    pub fn block_untill_event<F>(
        &mut self,
        target_trigger: F,
        timeout: Option<Duration>,
    ) -> Result<Event, SevStepError>
    where
        F: FnOnce() -> AhwResult<()>,
        F: Send + 'static,
    {
        let (s, trigger_result) = bounded(1);
        thread::spawn(move || s.send(target_trigger()));

        let start_timestamp = Instant::now();
        let mut trigger_finished = false;
        loop {
            //check if caller requested abort
            match self.abort.try_recv() {
                Ok(()) => return Err(SevStepError::Other(anyhow!("received abort signal"))),
                Err(TryRecvError::Empty) => (),
                Err(e) => {
                    return Err(SevStepError::Other(anyhow!(
                        "error checking abort channel : {}",
                        e
                    )))
                }
            }

            //abort if trigger function failed
            if !trigger_finished {
                match trigger_result.try_recv() {
                    Ok(_) => {
                        debug!("trigger finished successfully");
                        trigger_finished = true
                    }
                    Err(TryRecvError::Empty) => (),
                    Err(e) => return Err(SevStepError::TriggerFailed { source: e.into() }),
                }
            }

            //check for event
            unsafe {
                raw_spinlock::lock(&mut self.shared_mem_region.spinlock);
            }
            if 1 == self.shared_mem_region.have_event {
                break;
            }
            unsafe {
                raw_spinlock::unlock(&mut self.shared_mem_region.spinlock);
            }

            //abort if optional event timeout passed
            if timeout.is_some_and(|v| start_timestamp.elapsed() > v) {
                warn!("block_until_event_timed out");
                return Err(SevStepError::Timeout);
            }
        }

        //if we are here, we hold the lock and there was and event
        let result;
        match self.shared_mem_region.event_type {
            usp_event_type_t::PAGE_FAULT_EVENT => {
                let e: *const usp_page_fault_event_t =
                    self.shared_mem_region.event_buffer.as_ptr() as *const usp_page_fault_event_t;
                result = Event::PageFaultEvent(PageFaultEvent::from_c_struct(e));
            }
            usp_event_type_t::SEV_STEP_EVENT => {
                let step_event =
                    SevStepEvent::from_raw_event_buffer(&self.shared_mem_region.event_buffer);

                if self.error_on_multi_step && step_event.retired_instructions > 1 {
                    unsafe { raw_spinlock::unlock(&mut self.shared_mem_region.spinlock) }
                    return Err(MultiStep { event: step_event });
                }
                result = Event::StepEvent(step_event);
            }
        }

        unsafe { raw_spinlock::unlock(&mut self.shared_mem_region.spinlock) }
        Ok(result)
    }

    /// Signal to the kernel space, that we are done with the latest event and that
    /// the VM can resume its execution
    pub fn ack_event(&mut self) {
        unsafe {
            raw_spinlock::lock(&mut self.shared_mem_region.spinlock);
        }

        self.shared_mem_region.event_acked = 1;
        self.shared_mem_region.have_event = 0;

        unsafe {
            raw_spinlock::unlock(&mut self.shared_mem_region.spinlock);
        }
    }
}

#[derive(Clone)]
/// Each entry represents a single "probe". The exact semantics depends on the used
/// cache attack. E.g. for the default prime+probe attack, a "probe" is the result
/// of accessing a single eviction set entry and `way count` consecutive probes represent
/// the state of a full cache set.
///
/// `timing_probes` contains access time data for the given probe while `perf_counter_probes`
/// contains the diff of the configured perf counter before and after accessing the probe.
/// Both arrays are guaranteed to have the same amount of entries
#[derive(Debug)]
pub struct CacheTrace {
    pub timing_probes: Vec<u64>,
    pub perf_counter_probes: Vec<u64>,
}
/// Events generated by activating single stepping.
#[derive(Clone, Debug)]
pub struct SevStepEvent {
    /// Amount of instructions executed by the VM in this step event.
    pub retired_instructions: u32,
    register_values: Option<sev_step_partial_vmcb_save_area_t>,
    /// If a cache attack was requested prior to this step event, this will hold the resulting
    /// data
    pub cache_trace: Option<CacheTrace>,
}

impl SevStepEvent {
    /// If the VM runs in debug mode, this allows read access to its register file
    pub fn get_register(&self, name: vmsa_register_name_t) -> Option<u64> {
        self.register_values
            .map(|v| v.register_values[name as usize])
    }
    pub fn get_cache_trace(&self) -> Option<&CacheTrace> {
        return self.cache_trace.as_ref();
    }

    fn from_raw_event_buffer(raw_event_buff: &[u8]) -> SevStepEvent {
        let event;
        let mut offset = mem::size_of::<sev_step_event_t>();

        unsafe {
            event = (raw_event_buff.as_ptr() as *const sev_step_event_t)
                .as_ref()
                .unwrap();
        }

        //build CacheTrace
        let cache_trace;
        //check if we have data. *N.B.* that the actual data is in the event buffer and not
        //at the memory area pointed to by `cache_attack_perf_values` or `cache_attack_perf_values`
        if event.cache_attack_perf_values.is_null() || event.cache_attack_perf_values.is_null() {
            cache_trace = None;
        } else {
            let timings;
            let perf;
            let timing_probes: Vec<u64>;
            let perf_counter_probes: Vec<u64>;
            unsafe {
                timings = (raw_event_buff.as_ptr().add(offset) as *const u64)
                    .as_ref()
                    .unwrap();
                offset += mem::size_of::<u64>() * event.cache_attack_data_len as usize;
                timing_probes =
                    slice::from_raw_parts(timings, event.cache_attack_data_len as usize).to_vec();

                perf = (raw_event_buff.as_ptr().add(offset) as *const u64)
                    .as_ref()
                    .unwrap();
                perf_counter_probes =
                    slice::from_raw_parts(perf, event.cache_attack_data_len as usize).to_vec();
            }
            cache_trace = Some(CacheTrace {
                timing_probes,
                perf_counter_probes,
            });
        }

        //build RegisterFile
        let register_values;
        if !event.is_decrypted_vmsa_data_valid || event.decrypted_vmsa_data.failed_to_get_data {
            register_values = None;
        } else {
            register_values = Some(event.decrypted_vmsa_data);
        }

        SevStepEvent {
            retired_instructions: event.counted_instructions,
            register_values,
            cache_trace,
        }
    }
}

#[derive(Clone, Debug)]
/// Events generated by activating page tracking
pub struct PageFaultEvent {
    /// GPA at which the page fault occurred
    pub faulted_gpa: u64,
    register_values: Option<sev_step_partial_vmcb_save_area_t>,
}

impl PageFaultEvent {
    /// If the VM runs in debug mode, this allows read access to its register file
    pub fn get_register(&self, name: vmsa_register_name_t) -> Option<u64> {
        self.register_values
            .map(|v| v.register_values[name as usize])
    }

    fn from_c_struct(ptr: *const usp_page_fault_event_t) -> PageFaultEvent {
        let event;
        unsafe {
            event = ptr.as_ref().unwrap();
        }
        //build RegisterFile
        let register_values;
        if !event.is_decrypted_vmsa_data_valid || event.decrypted_vmsa_data.failed_to_get_data {
            register_values = None;
        } else {
            register_values = Some(event.decrypted_vmsa_data);
        }
        PageFaultEvent {
            faulted_gpa: event.faulted_gpa,
            register_values,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    PageFaultEvent(PageFaultEvent),
    StepEvent(SevStepEvent),
}
