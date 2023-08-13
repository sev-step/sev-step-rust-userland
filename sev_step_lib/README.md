# SEV-Step Rust User Space

Rust-based user space library for SEV-Step

## Build

- Edit path in `environment.sh` to point to the `usr/include/` subfolder of your kernel sources

- ```bash
    source ./environment.sh
    cargo build
    ```

## Dev Notes

You need the `refactorUserspaceHeader` kernel branch. Make sure to run `make headers` to refresh the
header files (especially after coming from another branch)
