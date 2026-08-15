# ==============================================================================
# BASIC Runtime: Error Reporting (Win64 Native)
# ==============================================================================
#
# One abort path for every runtime check. The message text is passed in and the
# BASIC line number is a register argument, so only a handful of message
# constants are ever needed no matter how many check sites exist.
#
# Messages are NUL-terminated and measured here rather than having their length
# passed in. A check site lives in the program's own .text, which the assembler
# sees before this file, so `mov reg, _msg_len` there is a *forward* reference:
# GAS cannot yet know the symbol is absolute and assembles a memory load from
# that address instead of an immediate. Measuring here sidesteps the ordering
# entirely.
#
# Diagnostics go to the standard error handle, so a program's real output stays
# clean and a test can still check partial output followed by an abort.
# ==============================================================================

.equ STD_ERROR_HANDLE, -12

.data
_err_buf: .skip 128
_err_fmt_line: .asciz "?%s in %lld\r\n"
_err_fmt_bare: .asciz "?%s\r\n"
_err_bytes_written: .quad 0

_err_subscript: .asciz "Subscript out of range"
_err_div0:      .asciz "Division by zero"
_err_domain:    .asciz "Illegal function call"
_err_overflow:  .asciz "Overflow"
_err_undim:     .asciz "Array used before DIM"
_err_memory:    .asciz "Out of memory"
_err_gosub:     .asciz "GOSUB stack overflow"
_err_badfile:   .asciz "Bad file number"

.text

# ------------------------------------------------------------------------------
# _rt_error - Report a runtime error and terminate
# ------------------------------------------------------------------------------
# Arguments (Win64):
#   rcx = message pointer (NUL-terminated)
#   rdx = BASIC line number, or 0 when unknown
#
# Returns: never (ExitProcess(1))
# ------------------------------------------------------------------------------
.globl _rt_error
_rt_error:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 48             # shadow space, keeps rsp 16-byte aligned

    mov rbx, rcx            # message
    mov rsi, rdx            # line number

    # sprintf(_err_buf, fmt, message [, line])
    lea rcx, [rip + _err_buf]
    test rsi, rsi
    jz .Lerr_bare
    lea rdx, [rip + _err_fmt_line]
    mov r8, rbx
    mov r9, rsi
    call sprintf
    jmp .Lerr_write
.Lerr_bare:
    lea rdx, [rip + _err_fmt_bare]
    mov r8, rbx
    call sprintf

.Lerr_write:
    mov rbx, rax            # length from sprintf
    mov ecx, STD_ERROR_HANDLE
    call GetStdHandle
    mov rcx, rax            # handle
    lea rdx, [rip + _err_buf]
    mov r8, rbx             # length
    lea r9, [rip + _err_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    mov ecx, 1
    call ExitProcess
