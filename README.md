# SEV-Step Rust User Space

Rust-based user space library for SEV-Step

## Build

- Edit path in `environment.sh` to point to the `usr/include/` subfolder of your kernel sources

- ```bash
    source ./environment.sh
    cargo build
    ```

## Dev Notes

You need the `refactorUserspaceHeader` kernel branch. Rebuilding the kernel module does
not seem enough after switching branches. I guess we need to re-generate the headers.
Not sure if there is an extra target for that. Building the full kernel fixes this in any case.
