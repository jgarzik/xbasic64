# Print functions

# void _rt_print_string(char* rdi, size_t rsi)
.globl _rt_print_string
_rt_print_string:
    push rbp
    mov rbp, rsp
    mov rdx, rdi        # ptr (3rd arg)
    mov rsi, rsi        # len (2nd arg)
    lea rdi, [rip + _fmt_str]
    xor eax, eax
    call {libc}printf
    leave
    ret

# void _rt_print_char(int rdi)
.globl _rt_print_char
_rt_print_char:
    push rbp
    mov rbp, rsp
    mov rsi, rdi
    lea rdi, [rip + _fmt_char]
    xor eax, eax
    call {libc}printf
    leave
    ret

# void _rt_print_newline()
.globl _rt_print_newline
_rt_print_newline:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _fmt_newline]
    xor eax, eax
    call {libc}printf
    leave
    ret

# void _rt_print_float(double xmm0)
.globl _rt_print_float
_rt_print_float:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    cvttsd2si rax, xmm0
    cvtsi2sd xmm1, rax
    ucomisd xmm0, xmm1
    jne .Lprint_as_float
    mov rsi, rax
    lea rdi, [rip + _fmt_int]
    xor eax, eax
    call {libc}printf
    jmp .Lprint_float_done
.Lprint_as_float:
    lea rdi, [rip + _fmt_float]
    mov eax, 1
    call {libc}printf
.Lprint_float_done:
    leave
    ret
