# DATA/READ support functions

# double _rt_read_number()
.globl _rt_read_number
_rt_read_number:
    push rbp
    mov rbp, rsp
    mov rax, QWORD PTR [rip + _data_ptr]
    shl rax, 4
    lea rcx, [rip + _data_table]
    add rcx, rax
    mov rax, QWORD PTR [rcx]
    cmp rax, 2
    je .Lread_str_as_num
    movsd xmm0, QWORD PTR [rcx + 8]
    cmp rax, 0
    jne .Lread_num_done
    mov rax, QWORD PTR [rcx + 8]
    cvtsi2sd xmm0, rax
.Lread_num_done:
    inc QWORD PTR [rip + _data_ptr]
    leave
    ret
.Lread_str_as_num:
    mov rdi, QWORD PTR [rcx + 8]
    xor rsi, rsi
    call {libc}strtod
    inc QWORD PTR [rip + _data_ptr]
    leave
    ret

# (ptr, len) = _rt_read_string()
.globl _rt_read_string
_rt_read_string:
    push rbp
    mov rbp, rsp
    mov rax, QWORD PTR [rip + _data_ptr]
    shl rax, 4
    lea rcx, [rip + _data_table]
    add rcx, rax
    mov rax, QWORD PTR [rcx + 8]
    mov rdi, rax
    call {libc}strlen
    mov rdx, rax
    mov rax, QWORD PTR [rip + _data_ptr]
    shl rax, 4
    lea rcx, [rip + _data_table]
    add rcx, rax
    mov rax, QWORD PTR [rcx + 8]
    inc QWORD PTR [rip + _data_ptr]
    leave
    ret

# void _rt_restore(int rdi)
.globl _rt_restore
_rt_restore:
    mov QWORD PTR [rip + _data_ptr], rdi
    ret
