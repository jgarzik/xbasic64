# BASIC Runtime: Error Reporting
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
# Diagnostics go to stderr, so a program's real output on stdout stays clean
# and a test can still check partial output followed by an abort.

.data
_err_fmt_line: .asciz "?%s in %ld\n"
_err_fmt_bare: .asciz "?%s\n"

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

.text

# _rt_error_num - Raise the error with GW-BASIC number `n` (ERROR statement)
#
# Arguments:
#   rdi = error number
#   rsi = BASIC line number, or 0 when unknown
#
# Returns: never -- tail-calls _rt_error with the matching message.
.globl _rt_error_num
_rt_error_num:
    lea rax, [rip + _err_codes]
.Lnum_scan:
    mov rcx, QWORD PTR [rax]        # message, or 0 at the end
    test rcx, rcx
    jz .Lnum_unknown
    cmp QWORD PTR [rax + 8], rdi
    je .Lnum_found
    add rax, 16
    jmp .Lnum_scan
.Lnum_found:
    mov rdi, rcx
    jmp _rt_error
.Lnum_unknown:
    lea rdi, [rip + _err_unprintable]
    jmp _rt_error


# _rt_error - Report a runtime error and terminate
# Arguments:
#   rdi = message pointer (NUL-terminated)
#   rsi = BASIC line number, or 0 when unknown
#
# Returns: never (exit code 1)
.globl _rt_error
_rt_error:
    push rbp
    mov rbp, rsp
    push rbx
    push r12                # two pushes keep rsp 16-byte aligned for the calls

    # Hold the arguments in callee-saved registers: the fflush below would
    # otherwise clobber them, since rcx/rdx/rsi/rdi are all caller-saved.
    mov rbx, rdi            # message
    mov r12, rsi            # line number

    # Flush stdout first, so output written before the error is not lost when
    # the process exits.
    xor edi, edi
    call fflush

    mov rdi, QWORD PTR [rip + stderr]
    mov rdx, rbx            # message -> 3rd arg
    test r12, r12
    jz .Lerr_bare
    lea rsi, [rip + _err_fmt_line]
    mov rcx, r12            # line -> 4th arg
    xor eax, eax
    call fprintf
    jmp .Lerr_exit
.Lerr_bare:
    lea rsi, [rip + _err_fmt_bare]
    xor eax, eax
    call fprintf

.Lerr_exit:
    mov edi, 1
    call exit
