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
    # Rearrange arguments for printf("%.*s", len, ptr)
    mov rdx, rdi        # ptr → rdx (3rd arg to printf)
    # rsi already has len (2nd arg to printf, as precision)
    lea rdi, [rip + _fmt_str]   # format string → rdi (1st arg)
    xor eax, eax        # no vector registers used (required for varargs)
    call {libc}printf
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
# _rt_print_float - Print a numeric value (integer or floating point)
# ------------------------------------------------------------------------------
# GW-BASIC convention: if a number is a whole number, print without decimal.
# We achieve this by:
#   1. Truncate to integer and convert back to double
#   2. Compare with original: if equal, it's a whole number
#   3. Print as integer (%ld) or float (%g) accordingly
#
# Arguments:
#   xmm0 = value to print (double)
#
# Returns: nothing
#
# Note: %g format automatically chooses between %f and %e notation and
# strips trailing zeros, giving clean output like "3.14159" not "3.141590".
# ------------------------------------------------------------------------------
.globl _rt_print_float
_rt_print_float:
    push rbp
    mov rbp, rsp
    sub rsp, 16         # Stack alignment for potential printf call
    # Check if value is a whole number
    cvttsd2si rax, xmm0     # truncate to integer
    cvtsi2sd xmm1, rax      # convert back to double
    ucomisd xmm0, xmm1      # compare original with truncated
    jne .Lprint_as_float    # if different, has fractional part
    # Print as integer (cleaner output)
    mov rsi, rax            # integer value → rsi (2nd arg)
    lea rdi, [rip + _fmt_int]
    xor eax, eax
    call {libc}printf
    jmp .Lprint_float_done
.Lprint_as_float:
    # Print as floating point - value still in xmm0
    lea rdi, [rip + _fmt_float]
    mov eax, 1              # 1 = one vector register argument (xmm0)
    call {libc}printf
.Lprint_float_done:
    leave
    ret
