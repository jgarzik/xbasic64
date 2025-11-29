# ==============================================================================
# BASIC Runtime: Input Functions
# ==============================================================================
#
# Keyboard input functions for the BASIC INPUT statement. These read from stdin
# using libc scanf.
#
# Buffer and format strings (from data_defs.s):
#   _input_buf      = 1024 bytes for string input
#   _fmt_input      = "%lf"           - read double
#   _fmt_input_str  = "%1023[^\n]"    - read up to 1023 chars, stop at newline
#
# Note: scanf with %[^\n] reads until newline but does NOT consume the newline.
# We call getchar() after scanf to consume the trailing newline, preventing it
# from being read by the next INPUT statement.
#
# String Return Convention:
#   Strings are returned as (pointer, length) pairs:
#   - rax = pointer to string data
#   - rdx = length in bytes
# ==============================================================================

# ------------------------------------------------------------------------------
# _rt_input_string - Read a line of text from stdin
# ------------------------------------------------------------------------------
# Reads characters until newline (which is not included in result).
# Uses a static buffer, so the returned pointer is only valid until the next
# call to _rt_input_string.
#
# Arguments: none
#
# Returns:
#   rax = pointer to string data (in _input_buf)
#   rdx = length of string
#
# Implementation:
#   1. Clear buffer (set first byte to 0 for empty input case)
#   2. scanf("%1023[^\n]", buffer) - read up to 1023 chars
#   3. getchar() - consume the trailing newline
#   4. Calculate string length by scanning for null terminator
# ------------------------------------------------------------------------------
.globl _rt_input_string
_rt_input_string:
    push rbp
    mov rbp, rsp
    sub rsp, 16                     # Stack alignment
    # Clear buffer in case of empty input
    lea rdi, [rip + _input_buf]
    mov BYTE PTR [rdi], 0           # Empty string if scanf reads nothing
    # Read string: scanf("%1023[^\n]", buffer)
    lea rsi, [rip + _input_buf]     # destination buffer (2nd arg)
    lea rdi, [rip + _fmt_input_str] # format string (1st arg)
    xor eax, eax                    # no vector args
    call {libc}scanf
    # Consume trailing newline that scanf left behind
    call {libc}getchar
    # Calculate string length (scan for null terminator)
    lea rax, [rip + _input_buf]     # rax = start of string
    xor rdx, rdx                    # rdx = length counter
.Linput_len:
    cmp BYTE PTR [rax + rdx], 0     # check for null terminator
    je .Linput_done
    inc rdx                         # length++
    jmp .Linput_len
.Linput_done:
    # Return: rax = pointer (already set), rdx = length
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_input_number - Read a numeric value from stdin
# ------------------------------------------------------------------------------
# Reads a double-precision floating point number. BASIC's INPUT statement
# converts this to the appropriate type (Integer, Long, Single, Double).
#
# Arguments: none
#
# Returns:
#   xmm0 = the number read (double)
#
# Implementation:
#   1. scanf("%lf", &local_var) - read double into stack
#   2. getchar() - consume trailing newline
#   3. Load result into xmm0
# ------------------------------------------------------------------------------
.globl _rt_input_number
_rt_input_number:
    push rbp
    mov rbp, rsp
    sub rsp, 16                     # Space for local double + alignment
    # Read double: scanf("%lf", &result)
    lea rsi, [rbp - 8]              # address of local variable (2nd arg)
    lea rdi, [rip + _fmt_input]     # format string "%lf" (1st arg)
    xor eax, eax                    # no vector args
    call {libc}scanf
    # Consume trailing newline
    call {libc}getchar
    # Load result into xmm0
    movsd xmm0, QWORD PTR [rbp - 8]
    leave
    ret
