# SEV-Step Enhanced Userland Prototype


## Overview
This is an experimental library to explore simplifying prototyping single stepping attacks
by introducing high level abstractions. It currently lacks the cache attacks capabilities of the
regular SEV-Step userland library. See the [main SEV-Step repo](https://github.com/sev-step/sev-step) for more information on SEV-Step.

Currently, we explore two main ideas to simplify attack prototyping
1) Enhanced VM Server  
2) Re-Usable Attack Components

### Re-Usable Attack Components
While writing the original SEV-Step code, I noticed that the attack program got quite complex, quite fast. Furthermore,
the different attack programs shared a lot of similar phases/concepts.

Thus, this library tries break down the attack code into different phases, building on the idea of HTTP middleware.
An attack consists of a chain of "event handlers" that consume the page fault and single stepping events.
This makes the attack code more structured and also allows us to introduce ready-to-use handlers for common scenarios
like "Execute the program until this page fault sequence occurred.


There a currently two design drafts for event handler.
The first one is more lightweight and defined in `sev_step_lib/src/single_stepper.rs`. It is used by the
`sev_step_lib/examples/targeted-single-stepping` example.
In this design approach the event handlers are not supposed to consume events. They can return commands like "Next", "Skip"
or "Abort" to control if the next handler in the chain should get executed.

The second design drafts supports more complex behaviour. It is defined in `sev_step_lib/src/event_handlers.rs`
and used by the example in `sev_step_lib/examples/complex-composition`.
In this design approach the event handlers are allowed to consume events, allowing us to execute "sub-programs".
I.e. one event handler will consume all events until a certain page fault pattern has been observed before moving on
to the next handler. Each handler must ensure that the VM is in a paused state before passing control to the next handler.
However, the exact interface to pass control from one handler to the other still needs more work.

### Enhanced VM Server
This component is solely intended for rapidly prototyping attacks. One main issues in this area is that with confidential
VMs like SEV, the attacker and the victim don't share the same process. However, for attack prototyping we would like
the ability to
- Start/Stop the target program
- Supply our attack program with GPA's of relevant memory locations, to avoid complex page fault tracking during the prototyping phase

The naive way to achieve this, is to start attacker and victim in two different terminal sessions, requiring manual input
to synchronise them or share state.

The idea of the VM Server is to provide an HTTP API that allows the attacker code to upload and run binaries inside the VM.
The VM Server also enables the attacker code to obtain information about relevant GPA's/the memory layout of the victim.

While the original SEV-Step library also came with such a component, it required the victim code to be backed into the
VM Server at build time, requiring to rebuild and restart the server frequently.
This version can accept new victim programs at runtime. Especially, it can execute small assembly as well as full-fledged programs.

## Content
The repo is structured into three projects with their own READMEs

1) `sev_step_lib` : Library to use the SEV-Step API to write single stepping attacks
2) `vm_server` : Runs inside the VM. Intended for rapid prototyping. Can be used by the SEV-Step library to execute code inside the VM and to obtain information about relevant GPAs of the executed code.
3) `victims` : Examples for victim programs use with the SEV-Step library and the vm server.