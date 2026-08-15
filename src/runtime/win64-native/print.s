# BASIC Runtime: Print Functions (Win64 Native - Pure Win32 API)
#
# Output functions using Win32 API (WriteFile) instead of libc printf.
# Uses UCRT sprintf for number formatting.
#
# Win64 ABI:
#   - Integer args: rcx, rdx, r8, r9 (then stack)
#   - 32-byte shadow space required before every call
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#

# Win32 API Constants
.equ STD_OUTPUT_HANDLE, -11

# I/O size constants
.equ SINGLE_BYTE, 1
.equ CRLF_LEN, 2


.data
_print_buffer: .skip 64          # Buffer for number formatting
_bytes_written: .quad 0          # For WriteFile output parameter
_newline_str: .ascii "\r\n"      # Windows uses CRLF

.text

# _rt_platform_init - Acquire the console handles (call once at startup)
# The console is file handle 0, so every print helper reaches it the same way
# it reaches an OPENed file. Windows has to ask the OS for the handle first;
# System V finds its standard output stream already open.
#
# Arguments: none      Returns: nothing
.globl _rt_platform_init
_rt_platform_init:
    push rbp
    mov rbp, rsp
    sub rsp, 32

    # GetStdHandle(STD_OUTPUT_HANDLE) -> _file_handles[0]
    mov ecx, STD_OUTPUT_HANDLE
    call GetStdHandle
    lea rcx, [rip + _file_handles]
    mov [rcx], rax

    call _rt_init_input

    leave
    ret


# _rt_fmt_double - Format a number into _num_buf
# Shared by console and file output so both render numbers identically.
#
# GW-BASIC convention: a whole number is written without a decimal point. For
# fractional values, write the *shortest* decimal that reads back as the same
# value, trying each format in the given table until one round-trips. A plain
# %g gives only 6 significant digits, which for a dialect whose default type is
# Double discards most of the value.
#
# This is a private helper, so it uses its own argument convention rather than
# the positional one: the value stays in xmm0 and the other two arguments take
# the first integer registers.
#
# Arguments:
#   xmm0 = value (a SINGLE arrives already widened to double)
#   rcx  = pointer to a NULL-terminated table of format-string pointers
#   rdx  = nonzero to compare at SINGLE precision
#
# Returns:
#   rax = length of the text in _num_buf
.globl _rt_fmt_double
_rt_fmt_double:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 64             # shadow space + locals; 64 realigns after 3 pushes

    movsd QWORD PTR [rbp - 32], xmm0    # original value
    mov rbx, rcx            # table cursor
    mov rsi, rdx            # precision flag

    # Whole number? Format as an integer.
    cvttsd2si rax, xmm0
    cvtsi2sd xmm1, rax
    ucomisd xmm0, xmm1
    jne .Lfd_fractional
    jp .Lfd_fractional
    lea rcx, [rip + _num_buf]
    lea rdx, [rip + _fmt_int]
    mov r8, rax
    call sprintf
    jmp .Lfd_done

.Lfd_fractional:
    mov rdx, QWORD PTR [rbx]
    test rdx, rdx
    jz .Lfd_len             # table exhausted: keep the last attempt
    lea rcx, [rip + _num_buf]
    movsd xmm2, QWORD PTR [rbp - 32]
    movq r8, xmm2
    call sprintf

    # Does it read back as the same value?
    lea rcx, [rip + _num_buf]
    xor edx, edx
    call strtod
    movsd xmm1, QWORD PTR [rbp - 32]
    test rsi, rsi
    jz .Lfd_cmp_double
    cvtsd2ss xmm0, xmm0
    cvtsd2ss xmm1, xmm1
    ucomiss xmm0, xmm1
    jmp .Lfd_cmp_done
.Lfd_cmp_double:
    ucomisd xmm0, xmm1
.Lfd_cmp_done:
    jp .Lfd_next            # unordered: keep trying
    je .Lfd_len
.Lfd_next:
    add rbx, 8
    jmp .Lfd_fractional

.Lfd_len:
    lea rcx, [rip + _num_buf]
    call lstrlenA

.Lfd_done:
    # sprintf and lstrlenA both leave the length in rax.
    add rsp, 64
    pop rsi
    pop rbx
    leave
    ret


# _rt_end - Terminate the program normally (END / STOP)
# Valid from any frame, including inside a SUB or FUNCTION. Emitting a plain
# `leave; ret` for END only terminates when it appears in main; inside a
# procedure it merely returns to the caller and execution continues.
#
# ExitProcess is safe here because this runtime writes console and file output
# with WriteFile rather than through CRT buffering.
#
# Arguments: none
# Returns: never
.globl _rt_end
_rt_end:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # Shadow space
    xor ecx, ecx            # exit code 0
    call ExitProcess
