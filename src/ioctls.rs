//! Rust wrappers for the SEV STEP IOCTLs in the "linux/kvm.h" header
//! The behavior of the ioctls is documented in the kernel header.
use crate::types::usp_init_poll_api_t;
use nix;

const KVMIO: u8 = 0xAE;

nix::ioctl_readwrite!(kvm_usp_init_poll_api, KVMIO, 0xf, usp_init_poll_api_t);
nix::ioctl_none!(kvm_usp_close_poll_api, KVMIO, 0x10);
