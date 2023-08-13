//! Rust wrappers for the SEV STEP IOCTLs in the "linux/kvm.h" header
//! The behavior of the ioctls is documented in the kernel header.
use crate::types::{sev_step_param_t, track_all_pages_t, track_page_param_t, usp_init_poll_api_t};
use nix::{self, errno::Errno, libc};

/// Convert all status codes but `0` to an error value
/// The `nix` crate only treats `-1` as an error which does not
/// reflect the semantics of our ioctls
fn map_result(r: nix::Result<nix::libc::c_int>) -> nix::Result<nix::libc::c_int> {
    match r {
        Ok(0) => Ok(0),
        Ok(v) => Err(Errno::from_i32(v)),
        Err(e) => Err(e),
    }
}
mod internal {
    use crate::types::{
        sev_step_param_t, track_all_pages_t, track_page_param_t, usp_init_poll_api_t,
    };

    const KVMIO: u8 = 0xAE;
    // API Management

    nix::ioctl_readwrite!(init_api, KVMIO, 0xf, usp_init_poll_api_t);
    nix::ioctl_none!(close_api, KVMIO, 0x10);

    // Page Tracking

    nix::ioctl_readwrite!(track_page, KVMIO, 0xb, track_page_param_t);
    nix::ioctl_readwrite!(track_all_pages, KVMIO, 0xc, track_all_pages_t);

    nix::ioctl_readwrite!(untrack_all_pages, KVMIO, 0xd, track_all_pages_t);
    nix::ioctl_readwrite!(untrack_page, KVMIO, 0xe, track_page_param_t);

    // Single Stepping

    nix::ioctl_readwrite!(start_stepping, KVMIO, 0x11, sev_step_param_t);
    nix::ioctl_none!(stop_stepping, KVMIO, 0x12);

    // Cache Attack

    // Misc
}

pub unsafe fn init_api(
    fd: libc::c_int,
    data: *mut usp_init_poll_api_t,
) -> nix::Result<libc::c_int> {
    map_result(internal::init_api(fd, data))
}

pub unsafe fn close_api(fd: libc::c_int) -> nix::Result<libc::c_int> {
    map_result(internal::close_api(fd))
}

pub unsafe fn track_page(
    fd: libc::c_int,
    data: *mut track_page_param_t,
) -> nix::Result<libc::c_int> {
    map_result(internal::track_page(fd, data))
}

pub unsafe fn track_all_pages(
    fd: libc::c_int,
    data: *mut track_all_pages_t,
) -> nix::Result<libc::c_int> {
    map_result(internal::track_all_pages(fd, data))
}

pub unsafe fn untrack_all_pages(
    fd: libc::c_int,
    data: *mut track_all_pages_t,
) -> nix::Result<libc::c_int> {
    map_result(internal::untrack_all_pages(fd, data))
}

pub unsafe fn untrack_page(
    fd: libc::c_int,
    data: *mut track_page_param_t,
) -> nix::Result<libc::c_int> {
    map_result(internal::untrack_page(fd, data))
}

pub unsafe fn start_stepping(
    fd: libc::c_int,
    data: *mut sev_step_param_t,
) -> nix::Result<libc::c_int> {
    map_result(internal::start_stepping(fd, data))
}

pub unsafe fn stop_stepping(fd: libc::c_int) -> nix::Result<libc::c_int> {
    map_result(internal::stop_stepping(fd))
}
