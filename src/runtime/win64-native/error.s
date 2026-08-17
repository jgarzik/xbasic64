# BASIC Runtime: Error Reporting (Win64 Native)
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

.equ STD_ERROR_HANDLE, -12

.data
_err_fmt_line: .asciz "?%s in %lld\r\n"
_err_fmt_bare: .asciz "?%s\r\n"

_err_subscript: .asciz "Subscript out of range"
_err_div0:      .asciz "Division by zero"
_err_domain:    .asciz "Illegal function call"
_err_overflow:  .asciz "Overflow"
_err_undim:     .asciz "Array used before DIM"
_err_memory:    .asciz "Out of memory"
_err_gosub:     .asciz "GOSUB stack overflow"
_err_badfile:   .asciz "Bad file number"
_err_badmode:   .asciz "Bad file mode"
_err_fieldovf:  .asciz "FIELD overflow"
_err_permission: .asciz "Permission denied"
_err_notfound: .asciz "File not found"
_err_alreadyopen: .asciz "File already open"
_err_pastend: .asciz "Input past end of file"

_err_unprintable: .asciz "Unprintable error"

# GW-BASIC's own error numbers, so that a listing's `IF ERR = 53` means what it
# meant in 1983. Pairs of (message, number), terminated by a zero message.
#
# A table rather than a third argument to _rt_error: that argument would have to
# be threaded through thirteen call sites in this tree alone, plus every
# trampoline codegen emits, and a helper here may take only four arguments
# because Win64 passes only four in registers. Errors are not a hot path, so a
# linear scan on the way to exit costs nothing.
#
# Searched by number as well as by message, for the ERROR statement -- so where
# two messages share a number the first one listed is the one ERROR n raises.
.p2align 3
_err_codes:
    .quad _err_domain,      5
    .quad _err_overflow,    6
    .quad _err_memory,      7
    .quad _err_gosub,       7
    .quad _err_subscript,   9
    .quad _err_undim,       9
    .quad _err_div0,        11
    .quad _err_fieldovf,    50
    .quad _err_badfile,     52
    .quad _err_notfound,    53
    .quad _err_badmode,     54
    .quad _err_alreadyopen, 55
    .quad _err_pastend,     62
    .quad _err_permission,  70
    .quad 0, 0

# Zero-filled scratch, so .bss rather than .data -- see data_defs.s. The
# .text below restores the section for the code that follows.
.bss
.p2align 3
_err_buf: .skip 128
_err_bytes_written: .skip 8

.text

# _rt_error_num - Raise the error with GW-BASIC number `n` (ERROR statement)
#
# Arguments (Win64):
#   rcx = error number
#   rdx = BASIC line number, or 0 when unknown
#
# Returns: never -- tail-calls _rt_error with the matching message.
.globl _rt_error_num
_rt_error_num:
    lea rax, [rip + _err_codes]
.Lnum_scan:
    mov r8, QWORD PTR [rax]         # message, or 0 at the end
    test r8, r8
    jz .Lnum_unknown
    cmp QWORD PTR [rax + 8], rcx
    je .Lnum_found
    add rax, 16
    jmp .Lnum_scan
.Lnum_found:
    mov rcx, r8
    jmp _rt_error
.Lnum_unknown:
    lea rcx, [rip + _err_unprintable]
    jmp _rt_error

# _rt_error - Report a runtime error and terminate
# Arguments (Win64):
#   rcx = message pointer (NUL-terminated)
#   rdx = BASIC line number, or 0 when unknown
#
# Returns: never (ExitProcess(1))
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
