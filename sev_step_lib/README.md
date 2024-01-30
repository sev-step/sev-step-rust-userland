# SEV-Step Rust User Space

Rust-based user space library for SEV-Step
Early draft state

For information on setting up your system for SEV-Step, see the [main repo](https://github.com/sev-step/sev-step).
Follow the steps there to build the Kernel, Qemu and OVMF and set up a VM for use with SEV.
Next, follow the [*System Setup*](https://github.com/sev-step/sev-step-userland?tab=readme-ov-file#system-setup) section
in the old SEV-Step library to configure your system for SEV-Step. You can ignore the other sections.

## Build

- Edit `KERNEL_HEADERS` in `environment.sh` to point to the `usr/include/` sub folder of the SEV-Step kernel.
- From the top level directory (i.e. not the `sev_step_lib` subdirectory) execute `cargo build --release --all-targets`

## Run

We expect that you have configured your system for SEV-Step, as described at the start of this README
This library takes care of pinning your VM to the isolated CPU core as well as setting up the frequency fixation (if needed).
For this to work, you need to edit the variables in `sev_step_lib/vm-config.toml`.

All examples require you to run the vm server inside the SEV VM. It must be reachable via the URL specified in
`vm_server_address` inside `sev_step_lib/vm-config.toml`. The binary of the server should be located at
`./target/release/server ` after performing  the build step.

The following sections assume that you are in the top level directory, not the `sev_step_lib` subdirectory

#### Integration Test Suite
The `tester` binary offers integration tests for the different parts of this framework. Run the command with `--help`
to get an overview of the available tests.
To reduce the amount of output, you can drop the `RUST_LOG` part of the command.

```bash
RUST_LOG=sev_step_lib=debug sudo -E ./target/release/tester -v ./sev_step_lib/vm-config.toml <your test selction goes here>
```

#### Run Simple Event Handling + Assembly Snippet Example
This example shows how to upload and execute an assembly snippet in the VM. The VM server informs the attacker about
the GPAs of the loaded program to allow easy tracking with SEV-Step (in a debug scenario).
As an example, we execute a simple assembly snippet with secret dependent branching. Depending on the input supplied via
the cli, SEV-Step will observe a different number of executed instructions.

It uses the more lightweight event handling ideas, drafted in `sev_step_lib/src/single_stepper.rs`.

```bash
RUST_LOG=targeted_single_stepping=debug,sev_step_lib=debug sudo -E ./target/release/examples/targeted-single-stepping --help

```


#### Run Complex Event Handling + Custom program example
This example shows how to upload a custom program to the VM and execute it with the VM server.
It uses the more complex event handling ideas from `sev_step_lib/src/event_handlers.rs`.
The attacker code uses pre-built components to track the execution state of the victim with 
page faults + single stepping until it is about to perform a memory access. Than, a custom component is used
to detect the location of that memory access.

The victim program adheres to a custom protocol for its stdout/stdin output that is documented in
the `InitCustomTargetReq` struct in `vm_server/src/req_resp.rs`. The basic idea is that the program first starts in a
"setup phase", where it can perform introspection to locate relevant memory addresses and config values for the attack.
Next, it outputs these values to its stdout, where they get picked up by the VM server. After the setup phase, the program has to wait for a special marker 
input on its stdin, before it can begin with the "payload" execution phase. The VM server provides the memory addresses
and config information back the attacker program.

```bash
RUST_LOG=complex_composition=debug,sev_step_lib=debug sudo -E ./target/release/examples/complex-composition --help
```

