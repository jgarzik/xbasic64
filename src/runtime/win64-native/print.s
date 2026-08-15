# ==============================================================================
# BASIC Runtime: Print Functions (Win64 Native - Pure Win32 API)
# ==============================================================================
#
# Output functions using Win32 API (WriteFile) instead of libc printf.
# Uses UCRT sprintf for number formatting.
#
# Win64 ABI:
#   - Integer args: rcx, rdx, r8, r9 (then stack)
#   - 32-byte shadow space required before every call
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#
# ==============================================================================

# Win32 API Constants
.equ STD_OUTPUT_HANDLE, -11

# I/O size constants
.equ SINGLE_BYTE, 1
.equ CRLF_LEN, 2

.data
_stdout_handle: .quad 0
_print_buffer: .skip 64          # Buffer for number formatting
_bytes_written: .quad 0          # For WriteFile output parameter
# Current output column, so TAB(n) knows how far to advance.
_print_col: .quad 0
_newline_str: .ascii "\r\n"      # Windows uses CRLF

.text

# ------------------------------------------------------------------------------
# _rt_init_console - Initialize stdout handle (call once at startup)
# ------------------------------------------------------------------------------
.globl _rt_init_console
_rt_init_console:
    push rbp
    mov rbp, rsp
    sub rsp, 32

    # GetStdHandle(STD_OUTPUT_HANDLE)
    mov ecx, STD_OUTPUT_HANDLE
    call GetStdHandle
    lea rcx, [rip + _stdout_handle]
    mov [rcx], rax

    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_string - Print a string with explicit length
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = pointer to string data
#   rdx = string length
# ------------------------------------------------------------------------------
.globl _rt_print_string
_rt_print_string:
    push rbp
    mov rbp, rsp
    sub rsp, 48             # Shadow space + stack args
    mov QWORD PTR [rbp - 8], rdx    # remember the length for the column tracker

    # An unassigned string variable is (NULL, 0): .bss and the frame-zeroing
    # prologue both leave it that way. Printing nothing is the correct result,
    # and returning early avoids handing WriteFile a NULL buffer.
    test rdx, rdx
    jz .Lprint_str_done

    # Save args
    mov r8, rdx             # length → r8 (3rd arg for WriteFile)
    mov rdx, rcx            # buffer → rdx (2nd arg for WriteFile)

    # Get stdout handle
    lea rax, [rip + _stdout_handle]
    mov rcx, [rax]          # handle → rcx (1st arg)

    # WriteFile(handle, buffer, length, &bytesWritten, NULL)
    lea r9, [rip + _bytes_written]  # &bytesWritten → r9 (4th arg)
    mov QWORD PTR [rsp + 32], 0     # NULL → 5th arg (stack)
    call WriteFile
    mov rax, QWORD PTR [rbp - 8]
    add QWORD PTR [rip + _print_col], rax

.Lprint_str_done:
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_char - Print a single ASCII character
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = character code (0-255)
# ------------------------------------------------------------------------------
.globl _rt_print_char
_rt_print_char:
    push rbp
    mov rbp, rsp
    sub rsp, 48

    # Store char in buffer
    lea rax, [rip + _print_buffer]
    mov [rax], cl

    # Get stdout handle
    lea rax, [rip + _stdout_handle]
    mov rcx, [rax]          # handle

    # WriteFile(handle, buffer, 1, &bytesWritten, NULL)
    lea rdx, [rip + _print_buffer]
    mov r8, SINGLE_BYTE
    lea r9, [rip + _bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile
    inc QWORD PTR [rip + _print_col]

    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_newline - Print CRLF newline
# ------------------------------------------------------------------------------
.globl _rt_print_newline
_rt_print_newline:
    push rbp
    mov rbp, rsp
    sub rsp, 48

    # Get stdout handle
    lea rax, [rip + _stdout_handle]
    mov rcx, [rax]

    # WriteFile(handle, "\r\n", 2, &bytesWritten, NULL)
    lea rdx, [rip + _newline_str]
    mov r8, CRLF_LEN
    lea r9, [rip + _bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile
    mov QWORD PTR [rip + _print_col], 0

    leave
    ret

# ------------------------------------------------------------------------------
# _rt_fmt_double - Format a number into _num_buf
# ------------------------------------------------------------------------------
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
# ------------------------------------------------------------------------------
.globl _rt_fmt_double
_rt_fmt_double:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 56             # shadow space + locals, keeps rsp 16-byte aligned

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
    add rsp, 56
    pop rsi
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_float - Print a DOUBLE (or an untyped numeric value)
# ------------------------------------------------------------------------------
# Arguments: xmm0 = value        Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_float
_rt_print_float:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    lea rcx, [rip + _fmt_g_table]
    xor edx, edx
    call _rt_fmt_double
    lea rcx, [rip + _num_buf]
    mov rdx, rax
    call _rt_print_string
    add rsp, 32
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_single - Print a SINGLE
# ------------------------------------------------------------------------------
# A SINGLE carries only ~7 significant digits, so it uses a table starting at a
# shorter format and compares at 32-bit precision. Otherwise 3.14159! would
# print as 3.1415901184082: at 15 digits even a float's value round-trips.
#
# Arguments: xmm0 = value, already widened to double     Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_single
_rt_print_single:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    lea rcx, [rip + _fmt_g_single_table]
    mov edx, 1
    call _rt_fmt_double
    lea rcx, [rip + _num_buf]
    mov rdx, rax
    call _rt_print_string
    add rsp, 32
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_gosub_overflow - Handle GOSUB stack overflow error
# ------------------------------------------------------------------------------
# Called when the GOSUB return stack is exhausted. Prints an error message
# and terminates the program with exit code 1.
#
# Arguments: none
# Returns: never (calls ExitProcess)
# ------------------------------------------------------------------------------
.globl _rt_gosub_overflow
_rt_gosub_overflow:
    push rbp
    mov rbp, rsp
    sub rsp, 48

    # Get stdout handle
    lea rax, [rip + _stdout_handle]
    mov rcx, [rax]

    # WriteFile(handle, message, length, &bytesWritten, NULL)
    lea rdx, [rip + _gosub_overflow_msg]
    mov r8, _gosub_overflow_msg_len
    lea r9, [rip + _bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    # ExitProcess(1)
    mov ecx, 1
    call ExitProcess

# ------------------------------------------------------------------------------
# _rt_end - Terminate the program normally (END / STOP)
# ------------------------------------------------------------------------------
# Valid from any frame, including inside a SUB or FUNCTION. Emitting a plain
# `leave; ret` for END only terminates when it appears in main; inside a
# procedure it merely returns to the caller and execution continues.
#
# ExitProcess is safe here because this runtime writes console and file output
# with WriteFile rather than through CRT buffering.
#
# Arguments: none
# Returns: never
# ------------------------------------------------------------------------------
.globl _rt_end
_rt_end:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # Shadow space
    xor ecx, ecx            # exit code 0
    call ExitProcess

# ------------------------------------------------------------------------------
# _rt_print_spc - SPC(n): print n spaces
# ------------------------------------------------------------------------------
# Arguments: rcx = count      Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_spc
_rt_print_spc:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40
    mov rbx, rcx
.Lspc_loop:
    cmp rbx, 0
    jle .Lspc_done
    mov ecx, ' '
    call _rt_print_char
    dec rbx
    jmp .Lspc_loop
.Lspc_done:
    add rsp, 40
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_tab - TAB(n): advance to column n
# ------------------------------------------------------------------------------
# Columns are 1-based, as in GW-BASIC. If output is already at or past the
# requested column, a newline is emitted first.
#
# Arguments: rcx = target column      Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_tab
_rt_print_tab:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40

    mov rbx, rcx
    cmp rbx, 1
    jge .Ltab_have_target
    mov rbx, 1
.Ltab_have_target:
    dec rbx

    cmp rbx, QWORD PTR [rip + _print_col]
    jge .Ltab_pad
    call _rt_print_newline

.Ltab_pad:
    mov rcx, rbx
    sub rcx, QWORD PTR [rip + _print_col]
    call _rt_print_spc

    add rsp, 40
    pop rbx
    leave
    ret
