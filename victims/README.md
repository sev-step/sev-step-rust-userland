# Victims

Example programs to use with the VM server.
Run `./build_all.sh` to build.

The `dummy_victim` folder contains a bare minimum example for the stdout/stdin communication protocol expected by the
VM server

The `simple_pf_victim` contains a more complex example that shows how a program can peform introspection to obtain
the (guest) physical addresses of relevant memory locations and send them to the attacker application via the
stdout/stdin communication protocol of the VM server.