# File I/O functions
# Uses libc fopen, fclose, fprintf, fscanf

# File handle table (16 FILE* pointers, index 0 unused)
.data
_file_handles: .skip 128  # 16 * 8 bytes

# Mode strings for fopen
_mode_read: .asciz "r"
_mode_write: .asciz "w"
_mode_append: .asciz "a"

# Temp buffer for null-terminated filename
_file_name_buf: .skip 1024

# Format strings for file I/O
_file_fmt_str: .asciz "%.*s"
_file_fmt_int: .asciz "%ld"
_file_fmt_float: .asciz "%g"
_file_fmt_char: .asciz "%c"
_file_fmt_newline: .asciz "\n"
_file_fmt_input: .asciz "%lf"
_file_input_buf: .skip 1024

.text

# void _rt_file_open(char* rdi, size_t rsi, int rdx, int rcx)
# rdi = filename ptr, rsi = filename len, rdx = mode (0=input,1=output,2=append), rcx = file number
.globl _rt_file_open
_rt_file_open:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14

    mov r12, rdi       # filename ptr
    mov r13, rsi       # filename len
    mov r14d, edx      # mode
    mov ebx, ecx       # file number

    # Copy filename to buffer and null-terminate
    lea rdi, [rip + _file_name_buf]
    mov rsi, r12
    mov rdx, r13
    call {libc}memcpy
    lea rax, [rip + _file_name_buf]
    mov BYTE PTR [rax + r13], 0  # null terminate

    # Select mode string
    cmp r14d, 0
    je .Lmode_read
    cmp r14d, 1
    je .Lmode_write
    lea rsi, [rip + _mode_append]
    jmp .Ldo_fopen
.Lmode_read:
    lea rsi, [rip + _mode_read]
    jmp .Ldo_fopen
.Lmode_write:
    lea rsi, [rip + _mode_write]

.Ldo_fopen:
    lea rdi, [rip + _file_name_buf]
    call {libc}fopen

    # Store FILE* in handle table
    lea rcx, [rip + _file_handles]
    mov [rcx + rbx*8], rax

    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# void _rt_file_close(int rdi)
# rdi = file number
.globl _rt_file_close
_rt_file_close:
    push rbp
    mov rbp, rsp

    # Get FILE* from handle table
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rdi*8]
    test rdi, rdi
    jz .Lclose_done
    call {libc}fclose
.Lclose_done:
    leave
    ret

# void _rt_file_print_string(int rdi, char* rsi, size_t rdx)
# rdi = file number, rsi = string ptr, rdx = string len
.globl _rt_file_print_string
_rt_file_print_string:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8         # align stack for 16-byte boundary

    mov ebx, edi       # save file number
    mov rcx, rsi       # string ptr -> 4th arg
    mov r8, rdx        # string len -> will become 3rd arg

    # Get FILE* from handle table
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]

    # fprintf(file, "%.*s", len, ptr)
    lea rsi, [rip + _file_fmt_str]
    mov rdx, r8        # len (precision)
    # rcx already has ptr
    xor eax, eax
    call {libc}fprintf

    add rsp, 8
    pop rbx
    leave
    ret

# void _rt_file_print_float(int rdi, double xmm0)
# rdi = file number, xmm0 = value
.globl _rt_file_print_float
_rt_file_print_float:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi       # save file number

    # Check if integer
    cvttsd2si rax, xmm0
    cvtsi2sd xmm1, rax
    ucomisd xmm0, xmm1
    jne .Lfile_print_as_float

    # Print as integer
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    lea rsi, [rip + _file_fmt_int]
    cvttsd2si rdx, xmm0
    xor eax, eax
    call {libc}fprintf
    jmp .Lfile_print_float_done

.Lfile_print_as_float:
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    lea rsi, [rip + _file_fmt_float]
    mov eax, 1
    call {libc}fprintf

.Lfile_print_float_done:
    add rsp, 8
    pop rbx
    leave
    ret

# void _rt_file_print_char(int rdi, int rsi)
# rdi = file number, rsi = char
.globl _rt_file_print_char
_rt_file_print_char:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    mov ebx, edi       # save file number
    mov r12d, esi      # save char

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    lea rsi, [rip + _file_fmt_char]
    mov rdx, r12
    xor eax, eax
    call {libc}fprintf

    pop r12
    pop rbx
    leave
    ret

# void _rt_file_print_newline(int rdi)
# rdi = file number
.globl _rt_file_print_newline
_rt_file_print_newline:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8         # align stack

    mov ebx, edi       # save file number

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    lea rsi, [rip + _file_fmt_newline]
    xor eax, eax
    call {libc}fprintf

    add rsp, 8
    pop rbx
    leave
    ret

# double _rt_file_input_number(int rdi)
# rdi = file number, returns value in xmm0
.globl _rt_file_input_number
_rt_file_input_number:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi       # save file number

    # fscanf(file, "%lf", &result)
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    lea rsi, [rip + _file_fmt_input]
    lea rdx, [rbp - 16]
    xor eax, eax
    call {libc}fscanf

    movsd xmm0, QWORD PTR [rbp - 16]
    add rsp, 8
    pop rbx
    leave
    ret

# (ptr, len) _rt_file_input_string(int rdi)
# rdi = file number, returns ptr in rax, len in rdx
.globl _rt_file_input_string
_rt_file_input_string:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8         # align stack

    mov ebx, edi       # save file number

    # fgets(buffer, size, file)
    lea rdi, [rip + _file_input_buf]
    mov rsi, 1023
    lea rax, [rip + _file_handles]
    mov rdx, [rax + rbx*8]
    call {libc}fgets

    test rax, rax
    jz .Lfile_input_string_empty

    # Calculate length (strip trailing newline)
    lea rdi, [rip + _file_input_buf]
    call {libc}strlen
    mov rdx, rax       # length

    # Strip trailing newline if present
    test rdx, rdx
    jz .Lfile_input_string_done
    lea rax, [rip + _file_input_buf]
    mov cl, BYTE PTR [rax + rdx - 1]
    cmp cl, 10         # newline
    jne .Lfile_input_string_done
    dec rdx
    mov BYTE PTR [rax + rdx], 0

.Lfile_input_string_done:
    lea rax, [rip + _file_input_buf]
    add rsp, 8
    pop rbx
    leave
    ret

.Lfile_input_string_empty:
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax], 0
    xor edx, edx
    add rsp, 8
    pop rbx
    leave
    ret
