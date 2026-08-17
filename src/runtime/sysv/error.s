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
# RESUME's own failures. Never trapped -- a handler that trapped them would be
# re-entered by its own RESUME, forever.
_err_resnoerr: .asciz "RESUME without error"
_err_resproc:  .asciz "RESUME cannot return into a SUB or FUNCTION"

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


# Error-trapping state. Always defined, never conditional: _rt_error is
# assembled into every program and its preamble reads these, so a program with
# no ON ERROR would otherwise fail to link. Zero means "not trapping", which is
# what .bss gives for free.
.bss
.p2align 3
_err_handler: .skip 8       # where to jump, 0 = trapping off
_err_active:  .skip 8       # nonzero while a handler runs
_err_code:    .skip 8       # what ERR returns
_err_line:    .skip 8       # the line now running; codegen stores it per statement
_err_erl:     .skip 8       # what ERL returns: _err_line as it was when trapped
_err_stmt:    .skip 8       # index of the module-level statement now running
_err_resume:  .skip 8       # _err_stmt as it was when trapped; -1 = not resumable
_err_depth:   .skip 8       # procedure nesting, so an error inside one is known
# rsp, rbx, rbp, r12, r13, r14, r15 -- and rdi, rsi on Win64, where they are
# callee-saved too. Sized the same in both trees so the offsets agree.
_err_ctx:     .skip 80

.text

# _rt_trap_capture - Record what a trapped error must restore
#
# Called once from main's prologue when the program contains ON ERROR. Taking
# it here rather than in codegen keeps the register set -- which differs
# between the ABIs -- in the tree that knows about it.
#
# The values saved are the ones the C runtime handed main, because main's
# prologue has not touched a callee-saved register yet. Restoring them on a
# trap therefore leaves main's eventual `leave; ret` exactly as clean as it is
# without trapping.
#
# Arguments: none      Returns: nothing
.globl _rt_trap_capture
_rt_trap_capture:
    lea rax, [rsp + 8]      # the caller's rsp, past our return address
    mov QWORD PTR [rip + _err_ctx + 0], rax
    mov QWORD PTR [rip + _err_ctx + 8], rbx
    mov QWORD PTR [rip + _err_ctx + 16], rbp
    mov QWORD PTR [rip + _err_ctx + 24], r12
    mov QWORD PTR [rip + _err_ctx + 32], r13
    mov QWORD PTR [rip + _err_ctx + 40], r14
    mov QWORD PTR [rip + _err_ctx + 48], r15
    ret

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
# Returns: never unless a handler is armed, in which case it does not return
# *here* either -- it abandons every frame between this one and main and jumps
# to the handler.
.globl _rt_error
_rt_error:
    # Trapping only when ON ERROR armed one and no handler is already running:
    # GW-BASIC does not trap an error raised inside a handler, which is also
    # what stops handler -> error -> handler from looping forever.
    mov rax, QWORD PTR [rip + _err_handler]
    test rax, rax
    jz _rt_fatal
    cmp QWORD PTR [rip + _err_active], 0
    jne _rt_fatal

    # ERR is the number this message carries. Unknown text reports 0 rather
    # than inventing a code.
    lea rcx, [rip + _err_codes]
.Ltrap_scan:
    mov rdx, QWORD PTR [rcx]
    test rdx, rdx
    jz .Ltrap_unknown
    cmp rdx, rdi
    je .Ltrap_found
    add rcx, 16
    jmp .Ltrap_scan
.Ltrap_found:
    mov rdx, QWORD PTR [rcx + 8]
    jmp .Ltrap_store
.Ltrap_unknown:
    xor edx, edx
.Ltrap_store:
    mov QWORD PTR [rip + _err_code], rdx
    # Snapshot the line as well. The handler is ordinary module-level code, so
    # its own statements overwrite _err_line the moment it starts running --
    # measured: a handler on line 100 reported ERL 100 for an error on line 30.
    mov rdx, QWORD PTR [rip + _err_line]
    mov QWORD PTR [rip + _err_erl], rdx

    # And what RESUME would go back to. The handler is ordinary module-level
    # code, so its own statements overwrite _err_stmt the moment it starts;
    # -1 when the error came from inside a procedure, whose frame the unwind
    # below discards, leaving nothing for a bare RESUME to return to.
    mov rdx, QWORD PTR [rip + _err_stmt]
    cmp QWORD PTR [rip + _err_depth], 0
    je .Ltrap_resumable
    mov rdx, -1
.Ltrap_resumable:
    mov QWORD PTR [rip + _err_resume], rdx
    mov QWORD PTR [rip + _err_depth], 0
    mov QWORD PTR [rip + _err_active], 1

    # Abandon every frame between here and main. ERL is already set: codegen
    # stores the line at each statement, which is the only way the file
    # helpers can report one -- seven of their error sites have no line to
    # pass and say so in their own comments.
    mov rbx, QWORD PTR [rip + _err_ctx + 8]
    mov rbp, QWORD PTR [rip + _err_ctx + 16]
    mov r12, QWORD PTR [rip + _err_ctx + 24]
    mov r13, QWORD PTR [rip + _err_ctx + 32]
    mov r14, QWORD PTR [rip + _err_ctx + 40]
    mov r15, QWORD PTR [rip + _err_ctx + 48]
    mov rsp, QWORD PTR [rip + _err_ctx + 0]
    jmp rax

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
