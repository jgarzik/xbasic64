# ==============================================================================
# BASIC Runtime: Print Functions
# ==============================================================================
#
# Number formatting and program termination. The printing itself lives in
# file.s: the console is file handle 0, so PRINT and PRINT # are the same
# helpers with a different handle, and neither can drift from the other.
#
# All functions follow System V AMD64 ABI:
#   - Callee-saved: rbx, rbp, r12-r15
#   - Caller-saved: rax, rcx, rdx, rsi, rdi, r8-r11, xmm0-xmm15
#   - Return values: rax (int), xmm0 (float)
#
# The {libc} placeholder is replaced with "_" on macOS, "" on Linux, and
# {stdout} with the libc symbol holding the standard output stream.
# ==============================================================================

# ------------------------------------------------------------------------------
# _rt_platform_init - Prepare the console for output
# ------------------------------------------------------------------------------
# The console is file handle 0, so every print helper reaches it the same way
# it reaches an OPENed file. Seeding the slot is all that takes here; Windows
# has to ask the OS for its handles instead.
#
# Arguments: none      Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_platform_init
_rt_platform_init:
    mov rax, QWORD PTR [rip + {stdout}]
    mov QWORD PTR [rip + _file_handles], rax
    ret

# ------------------------------------------------------------------------------
# _rt_fmt_double - Format a number into _num_buf
# ------------------------------------------------------------------------------
# Shared by console and file output so both render numbers identically.
#
# GW-BASIC convention: a whole number is written without a decimal point. For
# fractional values, write the *shortest* decimal that reads back as the same
# value: try each format in the given table in turn and keep the first whose
# text strtod's back unchanged. A plain %g gives only 6 significant digits,
# which for a dialect whose default type is Double discards most of the value
# (1/3 became 0.333333); going straight to %.17g instead would render 3.14159
# as 3.1415899999999999.
#
# Arguments:
#   xmm0 = value (a SINGLE arrives already widened to double)
#   rdi  = pointer to a NULL-terminated table of format-string pointers
#   esi  = nonzero to compare at SINGLE precision
#
# Returns:
#   rax = length of the text in _num_buf
# ------------------------------------------------------------------------------
.globl _rt_fmt_double
_rt_fmt_double:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 16             # rsp stays 16-byte aligned across the calls below
    movsd QWORD PTR [rbp - 24], xmm0    # original value
    mov rbx, rdi            # table cursor
    mov r12d, esi           # precision flag

    # Whole number? Format as an integer.
    # Out-of-range values saturate in cvttsd2si and so fail this test, which is
    # what sends 1e300 down the floating-point path.
    cvttsd2si rax, xmm0
    cvtsi2sd xmm1, rax
    ucomisd xmm0, xmm1
    jne .Lfd_fractional
    jp .Lfd_fractional
    lea rdi, [rip + _num_buf]
    lea rsi, [rip + _fmt_int]
    mov rdx, rax
    xor eax, eax
    call {libc}sprintf
    jmp .Lfd_done

.Lfd_fractional:
    mov rsi, QWORD PTR [rbx]
    test rsi, rsi
    jz .Lfd_len             # table exhausted: keep the last attempt
    lea rdi, [rip + _num_buf]
    movsd xmm0, QWORD PTR [rbp - 24]
    mov eax, 1              # one vector register argument
    call {libc}sprintf

    # Does it read back as the same value?
    lea rdi, [rip + _num_buf]
    xor esi, esi
    call {libc}strtod
    movsd xmm1, QWORD PTR [rbp - 24]
    test r12d, r12d
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
    lea rdi, [rip + _num_buf]
    call {libc}strlen

.Lfd_done:
    # sprintf and strlen both leave the length in rax.
    add rsp, 16
    pop r12
    pop rbx
    pop rbp
    ret

# ------------------------------------------------------------------------------
# _rt_end - Terminate the program normally (END / STOP)
# ------------------------------------------------------------------------------
# Valid from any frame, including inside a SUB or FUNCTION. Emitting a plain
# `leave; ret` for END only terminates when it appears in main; inside a
# procedure it merely returns to the caller and execution continues.
#
# Uses exit() rather than _exit() so stdio buffers -- printf output and any
# FILE* opened by file.s -- are flushed.
#
# Arguments: none
# Returns: never
# ------------------------------------------------------------------------------
.globl _rt_end
_rt_end:
    push rbp
    mov rbp, rsp
    xor edi, edi            # exit code 0
    call {libc}exit
