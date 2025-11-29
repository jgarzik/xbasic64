# Math and utility functions

# double _rt_rnd(double xmm0)
.globl _rt_rnd
_rt_rnd:
    push rbp
    mov rbp, rsp
    mov rax, QWORD PTR [rip + _rng_state]
    mov rcx, rax
    shl rcx, 13
    xor rax, rcx
    mov rcx, rax
    shr rcx, 7
    xor rax, rcx
    mov rcx, rax
    shl rcx, 17
    xor rax, rcx
    mov QWORD PTR [rip + _rng_state], rax
    shr rax, 12
    mov rcx, 0x3FF0000000000000
    or rax, rcx
    movq xmm0, rax
    mov rcx, 0x3FF0000000000000
    movq xmm1, rcx
    subsd xmm0, xmm1
    leave
    ret

# double _rt_timer()
.globl _rt_timer
_rt_timer:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    xor rdi, rdi
    call {libc}time
    xor rdx, rdx
    mov rcx, 86400
    div rcx
    cvtsi2sd xmm0, rdx
    leave
    ret

# void _rt_cls()
.globl _rt_cls
_rt_cls:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _cls_seq]
    xor eax, eax
    call {libc}printf
    leave
    ret
