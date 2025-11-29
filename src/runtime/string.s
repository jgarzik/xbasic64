# String functions

# double _rt_val(char* rdi, size_t rsi)
.globl _rt_val
_rt_val:
    push rbp
    mov rbp, rsp
    xor rsi, rsi
    call {libc}strtod
    leave
    ret

# (ptr, len) = _rt_str(double xmm0)
.globl _rt_str
_rt_str:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    lea rdi, [rip + _str_buf]
    lea rsi, [rip + _fmt_float]
    mov eax, 1
    call {libc}sprintf
    lea rax, [rip + _str_buf]
    mov rdx, rax
    xor rcx, rcx
.Lstr_len:
    cmp BYTE PTR [rax + rcx], 0
    je .Lstr_done
    inc rcx
    jmp .Lstr_len
.Lstr_done:
    mov rax, rdx
    mov rdx, rcx
    leave
    ret

# (ptr, len) = _rt_chr(int rdi)
.globl _rt_chr
_rt_chr:
    push rbp
    mov rbp, rsp
    lea rax, [rip + _chr_buf]
    mov BYTE PTR [rax], dil
    mov BYTE PTR [rax + 1], 0
    mov rdx, 1
    leave
    ret

# (ptr, len) = _rt_left(ptr rdi, len rsi, count rdx)
.globl _rt_left
_rt_left:
    mov rax, rdi
    cmp rdx, rsi
    cmova rdx, rsi
    ret

# (ptr, len) = _rt_right(ptr rdi, len rsi, count rdx)
.globl _rt_right
_rt_right:
    cmp rdx, rsi
    cmova rdx, rsi
    mov rax, rdi
    add rax, rsi
    sub rax, rdx
    ret

# (ptr, len) = _rt_mid(ptr rdi, len rsi, start rdx, count rcx)
.globl _rt_mid
_rt_mid:
    dec rdx
    cmp rdx, rsi
    jae .Lmid_empty
    mov rax, rdi
    add rax, rdx
    sub rsi, rdx
    cmp rcx, 0
    jl .Lmid_rest
    cmp rcx, rsi
    cmova rcx, rsi
    mov rdx, rcx
    ret
.Lmid_rest:
    mov rdx, rsi
    ret
.Lmid_empty:
    mov rax, rdi
    xor rdx, rdx
    ret

# int _rt_instr(ptr rdi, len rsi, ptr rdx, len rcx, start r8)
.globl _rt_instr
_rt_instr:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov r12, rdi
    mov r13, rsi
    mov r14, rdx
    mov r15, rcx
    dec r8
    add r12, r8
    sub r13, r8
    test r15, r15
    jz .Linstr_at_start
.Linstr_loop:
    cmp r13, r15
    jb .Linstr_not_found
    mov rdi, r12
    mov rsi, r14
    mov rdx, r15
    call {libc}memcmp
    test eax, eax
    jz .Linstr_found
    inc r12
    dec r13
    inc r8
    jmp .Linstr_loop
.Linstr_found:
    mov rax, r8
    add rax, 1
    jmp .Linstr_done
.Linstr_at_start:
    mov rax, r8
    add rax, 1
    jmp .Linstr_done
.Linstr_not_found:
    xor rax, rax
.Linstr_done:
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret
