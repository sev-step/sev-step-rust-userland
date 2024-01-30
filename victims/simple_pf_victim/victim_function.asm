section .text
    global victim_fn

align 4096
victim_fn:
    nop
    nop
    ;do memory access
    mov qword [rdi], rdi
    mov rax, 0
    ret