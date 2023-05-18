//! Implementation of a simple spinlock whose definition is shared between he kernel
//! and userspace part of the API to allow locking from both sides.
//!
//! The lock itself is just a simple integer. It must be initialized with [`init`](fn@init)
//! before first usage.
use std::arch::global_asm;

global_asm!(include_str!("raw_spinlock.s"));

extern "C" {
    /// Take the lock
    pub fn lock(lock: &mut i32);
    /// Release the lock
    pub fn unlock(lock: &mut i32);
}

/// Initialize the lock
pub fn init(lock: &mut i32) {
    *lock = 1;
}
