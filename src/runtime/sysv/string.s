# ==============================================================================
# BASIC Runtime: String Functions
# ==============================================================================
#
# String manipulation functions implementing BASIC's string operations.
#
# String Representation:
#   BASIC strings are (pointer, length) pairs. They are NOT null-terminated
#   internally, which allows efficient substring operations (LEFT$, MID$, RIGHT$)
#   that return pointers into the original string without copying.
#
# String Return Convention:
#   - rax = pointer to string data
#   - rdx = length in bytes
#
# Memory Management:
#   - Substring functions (LEFT$, MID$, RIGHT$) return pointers into the original
#     string - no allocation needed
#   - String concatenation (_rt_strcat) allocates new memory via malloc
#   - Conversion functions use static buffers (_str_buf, _chr_buf)
#
# Static Buffers (from data_defs.s):
#   _str_buf  = 64 bytes  - for STR$() numeric-to-string conversion
#   _chr_buf  = 2 bytes   - for CHR$() single character + null
#
# Important: Functions using static buffers return pointers that are only
# valid until the next call to the same function.
# ==============================================================================

# ------------------------------------------------------------------------------
# _rt_val - Convert string to number (VAL function)
# ------------------------------------------------------------------------------
# Parses a string as a floating-point number. Leading whitespace is skipped.
# Returns 0 if the string doesn't start with a valid number.
#
# Arguments:
#   rdi = pointer to string
#   rsi = length (currently ignored - strtod reads until non-numeric)
#
# Returns:
#   xmm0 = parsed double value
#
# Note: We pass NULL as endptr to strtod since we don't need to know where
# parsing stopped. The string should be null-terminated for strtod.
# ------------------------------------------------------------------------------
.globl _rt_val
_rt_val:
    push rbp
    mov rbp, rsp
    xor rsi, rsi            # endptr = NULL (2nd arg)
    # rdi already has string ptr (1st arg)
    call {libc}strtod       # returns double in xmm0
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_str - Convert number to string (STR$ function)
# ------------------------------------------------------------------------------
# Formats a number as a string using %g format (compact representation).
#
# Arguments:
#   xmm0 = number to convert (double)
#
# Returns:
#   rax = pointer to string (_str_buf)
#   rdx = length of string
#
# Note: Uses static buffer - result only valid until next STR$() call.
# ------------------------------------------------------------------------------
.globl _rt_str
_rt_str:
    push rbp
    mov rbp, rsp
    sub rsp, 16                     # Stack alignment
    # sprintf(buffer, "%g", value)
    lea rdi, [rip + _str_buf]       # destination buffer (1st arg)
    lea rsi, [rip + _fmt_float]     # format string (2nd arg)
    mov eax, 1                      # 1 vector register arg (xmm0)
    call {libc}sprintf
    # Calculate result length
    lea rax, [rip + _str_buf]
    mov rdx, rax                    # save ptr
    xor rcx, rcx                    # length counter
.Lstr_len:
    cmp BYTE PTR [rax + rcx], 0
    je .Lstr_done
    inc rcx
    jmp .Lstr_len
.Lstr_done:
    mov rax, rdx                    # restore ptr
    mov rdx, rcx                    # length
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_chr - Convert ASCII code to single character (CHR$ function)
# ------------------------------------------------------------------------------
# Creates a 1-character string from an ASCII code.
#
# Arguments:
#   rdi = ASCII code (0-255)
#
# Returns:
#   rax = pointer to string (_chr_buf)
#   rdx = 1 (length)
#
# Note: Uses static buffer - result only valid until next CHR$() call.
# ------------------------------------------------------------------------------
.globl _rt_chr
_rt_chr:
    push rbp
    mov rbp, rsp
    lea rax, [rip + _chr_buf]
    mov BYTE PTR [rax], dil         # store character (low byte of rdi)
    mov BYTE PTR [rax + 1], 0       # null terminate (for safety)
    mov rdx, 1                      # length = 1
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_left - Extract leftmost characters (LEFT$ function)
# ------------------------------------------------------------------------------
# Returns the first N characters of a string. If N > string length, returns
# the entire string. This is a zero-copy operation - returns pointer into
# original string.
#
# Arguments:
#   rdi = source string pointer
#   rsi = source string length
#   rdx = number of characters to extract
#
# Returns:
#   rax = pointer to start of result (same as input pointer)
#   rdx = result length (min of requested count and source length)
# ------------------------------------------------------------------------------
.globl _rt_left
_rt_left:
    mov rax, rdi            # result ptr = source ptr
    cmp rdx, rsi            # if count > length
    cmova rdx, rsi          #   count = length
    ret                     # return (ptr, count)

# ------------------------------------------------------------------------------
# _rt_right - Extract rightmost characters (RIGHT$ function)
# ------------------------------------------------------------------------------
# Returns the last N characters of a string. Zero-copy operation.
#
# Arguments:
#   rdi = source string pointer
#   rsi = source string length
#   rdx = number of characters to extract
#
# Returns:
#   rax = pointer to start of result (ptr + len - count)
#   rdx = result length
# ------------------------------------------------------------------------------
.globl _rt_right
_rt_right:
    cmp rdx, rsi            # if count > length
    cmova rdx, rsi          #   count = length
    mov rax, rdi            # start with source ptr
    add rax, rsi            # point to end
    sub rax, rdx            # back up by count
    ret                     # rdx already has the count

# ------------------------------------------------------------------------------
# _rt_mid - Extract substring (MID$ function)
# ------------------------------------------------------------------------------
# Returns a substring starting at position START for COUNT characters.
# Positions are 1-based (BASIC convention). Zero-copy operation.
#
# Arguments:
#   rdi = source string pointer
#   rsi = source string length
#   rdx = start position (1-based)
#   rcx = count (-1 means "rest of string")
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
#
# Edge cases:
#   - start > length: returns empty string
#   - count > remaining: returns rest of string
#   - count < 0: returns rest of string from start position
# ------------------------------------------------------------------------------
.globl _rt_mid
_rt_mid:
    dec rdx                 # Convert to 0-based index
    cmp rdx, rsi            # If start >= length
    jae .Lmid_empty         #   return empty string
    mov rax, rdi
    add rax, rdx            # result ptr = src + start
    sub rsi, rdx            # remaining = length - start
    cmp rcx, 0              # If count < 0 (means "rest")
    jl .Lmid_rest
    cmp rcx, rsi            # If count > remaining
    cmova rcx, rsi          #   count = remaining
    mov rdx, rcx            # result length = count
    ret
.Lmid_rest:
    mov rdx, rsi            # result length = remaining
    ret
.Lmid_empty:
    mov rax, rdi            # point to original (arbitrary, length is 0)
    xor rdx, rdx            # length = 0
    ret

# ------------------------------------------------------------------------------
# _rt_instr - Find substring position (INSTR function)
# ------------------------------------------------------------------------------
# Searches for needle in haystack, optionally starting at a given position.
# Returns 1-based position of first match, or 0 if not found.
#
# Arguments:
#   rdi = haystack pointer
#   rsi = haystack length
#   rdx = needle pointer
#   rcx = needle length
#   r8  = start position (1-based)
#
# Returns:
#   rax = position (1-based) or 0 if not found
#
# Algorithm:
#   1. Adjust haystack pointer and length based on start position
#   2. At each position, use memcmp to check for match
#   3. If match found, return 1-based position
#   4. If no more room for needle, return 0
#
# Register usage (callee-saved registers for surviving memcmp calls):
#   rbx = current position (0-based)
#   r12 = current haystack pointer
#   r13 = remaining haystack length
#   r14 = needle pointer
#   r15 = needle length
# ------------------------------------------------------------------------------
.globl _rt_instr
_rt_instr:
    push rbp
    mov rbp, rsp
    # Save callee-saved registers (memcmp will clobber caller-saved ones)
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 8              # Align stack for calls (6 pushes = 48 bytes, need +8 for 16-byte alignment)
    # Move arguments to callee-saved registers
    mov r12, rdi            # haystack ptr
    mov r13, rsi            # haystack len
    mov r14, rdx            # needle ptr
    mov r15, rcx            # needle len
    mov rbx, r8             # start position (1-based)
    # Adjust for start position
    dec rbx                 # convert to 0-based
    add r12, rbx            # advance haystack ptr
    sub r13, rbx            # reduce remaining length
    # Special case: empty needle matches at current position
    test r15, r15
    jz .Linstr_at_start
.Linstr_loop:
    # Check if enough room for needle
    cmp r13, r15
    jb .Linstr_not_found    # not enough chars remaining
    # Compare: memcmp(haystack_pos, needle, needle_len)
    mov rdi, r12            # current position in haystack
    mov rsi, r14            # needle
    mov rdx, r15            # needle length
    call {libc}memcmp
    test eax, eax
    jz .Linstr_found        # memcmp returns 0 if equal
    # Not found at this position, advance
    inc r12                 # next position
    dec r13                 # one less char remaining
    inc rbx                 # increment position counter
    jmp .Linstr_loop
.Linstr_found:
    mov rax, rbx
    add rax, 1              # convert to 1-based
    jmp .Linstr_done
.Linstr_at_start:
    # Empty needle: return current position
    mov rax, rbx
    add rax, 1              # convert to 1-based
    jmp .Linstr_done
.Linstr_not_found:
    xor rax, rax            # return 0
.Linstr_done:
    # Restore stack and callee-saved registers
    add rsp, 8              # Restore stack alignment
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_strcat - Concatenate two strings (+ operator)
# ------------------------------------------------------------------------------
# Creates a new string by concatenating left and right strings.
# Allocates new memory for the result using malloc.
#
# Arguments:
#   rdi = left string pointer
#   rsi = left string length
#   rdx = right string pointer
#   rcx = right string length
#
# Returns:
#   rax = pointer to new string (malloc'd)
#   rdx = total length
#
# Memory: Caller is responsible for eventually freeing the result.
# In practice, BASIC programs don't free strings (simple memory model).
#
# Register usage:
#   r12 = left ptr (saved)
#   r13 = left len
#   r14 = right ptr
#   r15 = right len
# ------------------------------------------------------------------------------
.globl _rt_strcat
_rt_strcat:
    push rbp
    mov rbp, rsp
    push r12
    push r13
    push r14
    push r15
    sub rsp, 16             # Allocate aligned space for temp storage

    # Save arguments in callee-saved registers
    mov r12, rdi            # left ptr
    mov r13, rsi            # left len
    mov r14, rdx            # right ptr
    mov r15, rcx            # right len

    # Allocate memory: malloc(left_len + right_len + 1)
    # +1 for null terminator (for safety, though we track length)
    lea rdi, [rsi + rcx + 1]
    call {libc}malloc       # returns ptr in rax

    # Copy left string: memcpy(result, left, left_len)
    mov QWORD PTR [rsp], rax    # save result ptr (aligned)
    mov rdi, rax            # dest = malloc result
    mov rsi, r12            # src = left ptr
    mov rdx, r13            # len = left len
    call {libc}memcpy

    # Copy right string: memcpy(result + left_len, right, right_len)
    mov rdi, QWORD PTR [rsp]    # restore result ptr
    add rdi, r13            # dest = result + left_len
    mov rsi, r14            # src = right ptr
    mov rdx, r15            # len = right len
    call {libc}memcpy

    # Null terminate (for safety)
    mov rax, QWORD PTR [rsp]    # restore result ptr
    lea rcx, [r13 + r15]    # total length
    mov BYTE PTR [rax + rcx], 0

    # Return: rax = ptr (already set), rdx = total length
    mov rdx, rcx

    add rsp, 16             # Deallocate temp storage
    pop r15
    pop r14
    pop r13
    pop r12
    leave
    ret
