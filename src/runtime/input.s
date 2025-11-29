# Input functions

# (ptr, len) = _rt_input_string()
.globl _rt_input_string
_rt_input_string:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + _input_buf]
    mov BYTE PTR [rdi], 0
    lea rsi, [rip + _input_buf]
    lea rdi, [rip + _fmt_input_str]
    xor eax, eax
    call {libc}scanf
    call {libc}getchar
    lea rax, [rip + _input_buf]
    xor rdx, rdx
.Linput_len:
    cmp BYTE PTR [rax + rdx], 0
    je .Linput_done
    inc rdx
    jmp .Linput_len
.Linput_done:
    leave
    ret

# double _rt_input_number()
.globl _rt_input_number
_rt_input_number:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rsi, [rbp - 8]
    lea rdi, [rip + _fmt_input]
    xor eax, eax
    call {libc}scanf
    call {libc}getchar
    movsd xmm0, QWORD PTR [rbp - 8]
    leave
    ret
