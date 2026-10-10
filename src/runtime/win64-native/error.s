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
_stop_fmt_line: .asciz "Break in %lld\r\n"
_stop_fmt_bare: .asciz "Break\r\n"

_err_syntax:    .asciz "Syntax error"
_err_subscript: .asciz "Subscript out of range"
_err_typemismatch: .asciz "Type mismatch"
_err_div0:      .asciz "Division by zero"
_err_domain:    .asciz "Illegal function call"
_err_overflow:  .asciz "Overflow"
_err_dupdef:    .asciz "Duplicate definition"
_err_memory:    .asciz "Out of memory"
_err_gosub:     .asciz "GOSUB stack overflow"
_err_noreturn:  .asciz "RETURN without GOSUB"
_err_outofdata: .asciz "Out of DATA"
_err_badfile:   .asciz "Bad file number"
_err_badmode:   .asciz "Bad file mode"
_err_fieldovf:  .asciz "FIELD overflow"
_err_permission: .asciz "Permission denied"
_err_notfound: .asciz "File not found"
_err_alreadyopen: .asciz "File already open"
_err_pastend: .asciz "Input past end of file"
_err_badname: .asciz "Bad file name"
_err_access: .asciz "Path/File access error"
_err_pathnotfound: .asciz "Path not found"

_err_unprintable: .asciz "Unprintable error"
# RESUME's own failures. Never trapped -- a handler that trapped them would be
# re-entered by its own RESUME, forever.
_err_resnoerr: .asciz "RESUME without error"

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
    .quad _err_syntax,      2
    .quad _err_noreturn,    3
    .quad _err_outofdata,   4
    .quad _err_domain,      5
    .quad _err_overflow,    6
    .quad _err_memory,      7
    .quad _err_gosub,       7
    .quad _err_subscript,   9
    .quad _err_dupdef,      10
    .quad _err_div0,        11
    .quad _err_typemismatch, 13
    .quad _err_fieldovf,    50
    .quad _err_badfile,     52
    .quad _err_notfound,    53
    .quad _err_badmode,     54
    .quad _err_alreadyopen, 55
    .quad _err_pastend,     62
    .quad _err_badname,     64
    .quad _err_permission,  70
    .quad _err_access,      75
    .quad _err_pathnotfound, 76
    .quad 0, 0

# Zero-filled scratch, so .bss rather than .data -- see data_defs.s. The
# .text below restores the section for the code that follows.
.bss
.p2align 3
_err_buf: .skip 128
_err_bytes_written: .skip 8


# Error-trapping state. Always defined, never conditional: _rt_error is
# assembled into every program and its preamble reads these, so a program with
# no ON ERROR would otherwise fail to link. Zero means "not trapping", which is
# what .bss gives for free.
.bss
.p2align 3
_err_handler: .skip 8       # which ON ERROR target, numbered from 1; 0 = trapping off
_err_pending: .skip 8       # 1 from a trapped error until its handler runs;
                            # 2 while procedures unwind toward a RESUME label
_err_active:  .skip 8       # nonzero while a handler runs
_err_code:    .skip 8       # what ERR returns
_err_line:    .skip 8       # the line now running; codegen stores it per statement
_err_erl:     .skip 8       # what ERL returns: _err_line as it was when trapped

.text

# _rt_err - ERR: the number of the error that was trapped
#
# Arguments: none      Returns: eax = error number, 0 if none has been trapped
.globl _rt_err
_rt_err:
    mov rax, QWORD PTR [rip + _err_code]
    ret

# _rt_erl - ERL: the line the trapped error happened on
#
# Arguments: none      Returns: eax = line number, 0 if none has been trapped
.globl _rt_erl
_rt_erl:
    mov rax, QWORD PTR [rip + _err_erl]
    ret

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
# Returns: only when a handler traps the error, with _err_pending set.
# Otherwise never (exit code 1).
.globl _rt_error
_rt_error:
    # An error already on its way to the handler wins. Its helper may carry on
    # to the end of what it was doing -- printing the rest of a number to the
    # same unopened file, say -- and each step fails the same way; the first
    # failure is the one the handler hears about.
    cmp QWORD PTR [rip + _err_pending], 0
    jne .Ltrap_return

    # Trapping only when ON ERROR armed one and no handler is already running:
    # GW-BASIC does not trap an error raised inside a handler, which is also
    # what stops handler -> error -> handler from looping forever.
    #
    # Ahead of the sprintf below, which would otherwise scribble _err_buf on
    # the way to a handler that never wanted it.
    mov rax, QWORD PTR [rip + _err_handler]
    test rax, rax
    jz _rt_fatal
    cmp QWORD PTR [rip + _err_active], 0
    jne _rt_fatal

    # ERR is the number this message carries. Unknown text reports 0 rather
    # than inventing a code.
    lea r8, [rip + _err_codes]
.Ltrap_scan:
    mov r9, QWORD PTR [r8]
    test r9, r9
    jz .Ltrap_unknown
    cmp r9, rcx
    je .Ltrap_found
    add r8, 16
    jmp .Ltrap_scan
.Ltrap_found:
    mov r9, QWORD PTR [r8 + 8]
    jmp .Ltrap_store
.Ltrap_unknown:
    xor r9d, r9d
.Ltrap_store:
    mov QWORD PTR [rip + _err_code], r9
    # Snapshot the line as well. The handler is ordinary module-level code, so
    # its own statements overwrite _err_line the moment it starts running --
    # measured: a handler on line 100 reported ERL 100 for an error on line 30.
    mov r9, QWORD PTR [rip + _err_line]
    mov QWORD PTR [rip + _err_erl], r9

    mov QWORD PTR [rip + _err_active], 1

    # Return to whoever raised it. Generated code tests _err_pending after
    # every call that can fail; the statement's fault block then calls the
    # handler, on top of whatever faulted, and RESUMEs from what it returns.
    # A runtime helper that called this returns first, through its own
    # epilogue. Nothing is abandoned, so there is no register state to
    # restore.
    mov QWORD PTR [rip + _err_pending], 1
.Ltrap_return:
    ret

# _rt_fatal - Report and exit, without consulting the handler
#
# Today's whole behaviour, and still what happens when nothing is trapping.
# Called directly for the errors that must never be trapped: a handler
# re-entered by its own failing RESUME would never stop.
#
# Arguments: the same as _rt_error.
# Returns: never (exit code 1)
.globl _rt_fatal
_rt_fatal:
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
    ud2                     # exit does not return

# _rt_stop - STOP: say where, and end the program
#
# See the System V twin. Console output here is written straight through, so
# there is nothing to flush first.
#
# Arguments: rcx = BASIC line number, or 0 when unknown
# Returns: never (exit code 0)
.globl _rt_stop
_rt_stop:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40             # shadow space + the 5th WriteFile argument
    lea rdx, [rip + _stop_fmt_bare]
    test rcx, rcx
    jz .Lstop_fmt
    lea rdx, [rip + _stop_fmt_line]
.Lstop_fmt:
    mov r8, rcx             # the line, if the format wants it
    lea rcx, [rip + _err_buf]
    call sprintf
    mov rbx, rax            # length
    mov ecx, STD_ERROR_HANDLE
    call GetStdHandle
    mov rcx, rax
    lea rdx, [rip + _err_buf]
    mov r8, rbx
    lea r9, [rip + _err_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile
    add rsp, 40
    pop rbx
    leave
    jmp _rt_end
