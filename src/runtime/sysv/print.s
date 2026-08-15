# ==============================================================================
# BASIC Runtime: Print Functions
# ==============================================================================
#
# Output functions for the BASIC PRINT statement. All functions use libc printf
# for actual output, which handles buffering and platform differences.
#
# Format strings are defined in data_defs.s:
#   _fmt_str     = "%.*s"    - precision-limited string (ptr, len)
#   _fmt_int     = "%ld"     - long integer
#   _fmt_float   = "%g"      - floating point (compact representation)
#   _fmt_char    = "%c"      - single character
#   _fmt_newline = "\n"      - newline
#
# All functions follow System V AMD64 ABI:
#   - Callee-saved: rbx, rbp, r12-r15
#   - Caller-saved: rax, rcx, rdx, rsi, rdi, r8-r11, xmm0-xmm15
#   - Return values: rax (int), xmm0 (float)
#
# The {libc} placeholder is replaced with "_" on macOS, "" on Linux.
# ==============================================================================

# ------------------------------------------------------------------------------
# _rt_print_string - Print a string with explicit length
# ------------------------------------------------------------------------------
# BASIC strings are (ptr, len) pairs, not null-terminated. We use printf's
# precision specifier "%.*s" which takes the length as an argument.
#
# Arguments:
#   rdi = pointer to string data (char*)
#   rsi = string length (size_t)
#
# Returns: nothing
#
# printf call: printf("%.*s", length, pointer)
#   rdi = format string
#   rsi = precision (string length)
#   rdx = string pointer
# ------------------------------------------------------------------------------
.globl _rt_print_string
_rt_print_string:
    push rbp
    mov rbp, rsp
    # An unassigned string variable is (NULL, 0): .bss and the frame-zeroing
    # prologue both leave it that way. Printing nothing is the correct result,
    # and returning early avoids passing NULL to printf, which is undefined.
    test rsi, rsi
    jz .Lprint_str_done
    # Rearrange arguments for printf("%.*s", len, ptr)
    mov rdx, rdi        # ptr → rdx (3rd arg to printf)
    # rsi already has len (2nd arg to printf, as precision)
    lea rdi, [rip + _fmt_str]   # format string → rdi (1st arg)
    xor eax, eax        # no vector registers used (required for varargs)
    call {libc}printf
.Lprint_str_done:
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_char - Print a single ASCII character
# ------------------------------------------------------------------------------
# Used by PRINT with semicolon/comma separators and CHR$() output.
#
# Arguments:
#   rdi = character code (int, 0-255)
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_char
_rt_print_char:
    push rbp
    mov rbp, rsp
    mov rsi, rdi        # char → rsi (2nd arg)
    lea rdi, [rip + _fmt_char]  # format → rdi (1st arg)
    xor eax, eax        # no vector registers
    call {libc}printf
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_newline - Print a newline character
# ------------------------------------------------------------------------------
# Called at end of PRINT statement unless suppressed with ; or ,
#
# Arguments: none
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_newline
_rt_print_newline:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _fmt_newline]
    xor eax, eax
    call {libc}printf
    leave
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
# _rt_print_float - Print a DOUBLE (or an untyped numeric value)
# ------------------------------------------------------------------------------
# Arguments: xmm0 = value        Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_print_float
_rt_print_float:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _fmt_g_table]
    xor esi, esi
    call _rt_fmt_double
    lea rdi, [rip + _fmt_str]   # "%.*s"
    mov rsi, rax                # length
    lea rdx, [rip + _num_buf]
    xor eax, eax
    call {libc}printf
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
    lea rdi, [rip + _fmt_g_single_table]
    mov esi, 1
    call _rt_fmt_double
    lea rdi, [rip + _fmt_str]
    mov rsi, rax
    lea rdx, [rip + _num_buf]
    xor eax, eax
    call {libc}printf
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_gosub_overflow - Handle GOSUB stack overflow error
# ------------------------------------------------------------------------------
# Called when the GOSUB return stack is exhausted. Prints an error message
# and terminates the program with exit code 1.
#
# Arguments: none
# Returns: never (calls exit)
# ------------------------------------------------------------------------------
.globl _rt_gosub_overflow
_rt_gosub_overflow:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _gosub_overflow_msg]
    xor eax, eax
    call {libc}printf
    mov edi, 1              # exit code 1
    call {libc}exit

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
