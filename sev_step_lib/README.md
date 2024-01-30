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

### Concept for composeability of functionality
- Filters: Cannot consume the current event and thus cannot advance the victim's exeuction
- Subrograms: May consume the current event, advancing the victim's execution. They are required to also return an event, thus ensuring, that the victim is again in a halted state

=> in program it is probably best to use only one trait for both filters and subprogram with filters simply returning the event that they where called with

How do we communicate between filters and subprograms?

We should support muliple chains. Inside a chain, handlers are executed one after another, according to the StateMachineNextAction logic.
However given Chain A and Chain B, Chain B is only executed once Chain A has terminated. Afterwards Chain A is no longer executed there is only ever once active chain
This can be used to group attacks in compartements. We use a context struct to pass information between chains

I.e.
- Chain 1: State machine to run until a victim function is about to be exeucted. End in a paused state with an event
- Chain 2: Use 

Instead of multiple chains we could also use Filters/Subprogram that return "skip" untill a certain point is reached

Chains kind of coresspond to a "Targeted Stepper". We could build a variant that returns
its context as well as any pending event

#### Example for filters + subprogram
Use page fault sequence to detect a certain function call in a dummy program
Step the function call a given amount of steps and use page faults to detect the gpa
of a memory access

compose this as follows
- Chain 1
    - StateMachine Subprogram to detect the execution of the targeted function (stock component)
- Chain 2
    - Single Step on target Pages Filter (stock component)
    - SkipUntillNSteps Filter (new). This will advance the execution untill we are at the memory access (stock component)
    - Subprogram to detect gpa of memory access (custom "one time" component provided by the caller)

### Command Snippets

#### Run single stepping tests on itsepyc3
```bash
RUST_LOG=complex_composition=debug,sev_step_lib=debug sudo -E ./target/release/tester -v ./sev_step_lib/vm-config.toml --tests single-step-nop-slide -t 0x33
```

#### Run complex-composition example on itsepyc3
```bash
RUST_LOG=complex_composition=debug,sev_step_lib=debug sudo -E ./target/release/examples/complex-composition -v ./sev_step_lib/vm-config.toml --apic-timer-value 0x33
```