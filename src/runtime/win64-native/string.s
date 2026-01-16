# ==============================================================================
# BASIC Runtime: String Functions (Win64 Native - Pure Win32 API)
# ==============================================================================
#
# String manipulation functions. Uses HeapAlloc instead of malloc.
# Keeps UCRT functions: strtod, sprintf, memcpy, memcmp
#
# String Representation:
#   BASIC strings are (pointer, length) pairs. They are NOT null-terminated
#   internally.
#
# String Return Convention:
#   - rax = pointer to string data
#   - rdx = length in bytes
#
# Memory Management:
#   - Substring functions return pointers into original string (no allocation)
#   - String concatenation uses HeapAlloc(GetProcessHeap(), 0, size)
#
# Win64 ABI:
#   - Args: rcx, rdx, r8, r9 (then stack)
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#   - 32-byte shadow space required before calls
# ==============================================================================

# String length constants
.equ CHR_RESULT_LEN, 1          # CHR$() always returns 1 character

.data
_str_buf: .skip 64          # Buffer for STR$() conversion
_chr_buf: .skip 2           # Buffer for CHR$()

.text

# ------------------------------------------------------------------------------
# _rt_val - Convert string to number (VAL function)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = pointer to string
#   rdx = length (ignored - strtod reads until non-numeric)
#
# Returns:
#   xmm0 = parsed double value
# ------------------------------------------------------------------------------
.globl _rt_val
_rt_val:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # Shadow space
    xor rdx, rdx            # endptr = NULL
    call strtod             # returns double in xmm0
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_str - Convert number to string (STR$ function)
# ------------------------------------------------------------------------------
# Arguments:
#   xmm0 = number to convert (double)
#
# Returns:
#   rax = pointer to string (_str_buf)
#   rdx = length of string
# ------------------------------------------------------------------------------
.globl _rt_str
_rt_str:
    push rbp
    mov rbp, rsp
    sub rsp, 48             # Shadow space + alignment

    # sprintf(buffer, "%g", value)
    lea rcx, [rip + _str_buf]
    lea rdx, [rip + _fmt_float]
    movsd xmm2, xmm0        # value in xmm2
    movq r8, xmm0           # also in r8 for varargs
    call sprintf

    # Calculate result length
    lea rax, [rip + _str_buf]
    mov rcx, rax            # save ptr
    xor rdx, rdx            # length counter
.Lstr_len:
    cmp BYTE PTR [rax + rdx], 0
    je .Lstr_done
    inc rdx
    jmp .Lstr_len
.Lstr_done:
    mov rax, rcx            # restore ptr
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_chr - Convert ASCII code to single character (CHR$ function)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = ASCII code (0-255)
#
# Returns:
#   rax = pointer to string (_chr_buf)
#   rdx = 1 (length)
# ------------------------------------------------------------------------------
.globl _rt_chr
_rt_chr:
    push rbp
    mov rbp, rsp
    lea rax, [rip + _chr_buf]
    mov BYTE PTR [rax], cl
    mov BYTE PTR [rax + 1], 0
    mov rdx, CHR_RESULT_LEN
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_left - Extract leftmost characters (LEFT$ function)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = source string pointer
#   rdx = source string length
#   r8  = number of characters to extract
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
# ------------------------------------------------------------------------------
.globl _rt_left
_rt_left:
    mov rax, rcx
    cmp r8, rdx
    cmova r8, rdx
    mov rdx, r8
    ret

# ------------------------------------------------------------------------------
# _rt_right - Extract rightmost characters (RIGHT$ function)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = source string pointer
#   rdx = source string length
#   r8  = number of characters to extract
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
# ------------------------------------------------------------------------------
.globl _rt_right
_rt_right:
    cmp r8, rdx
    cmova r8, rdx
    mov rax, rcx
    add rax, rdx
    sub rax, r8
    mov rdx, r8
    ret

# ------------------------------------------------------------------------------
# _rt_mid - Extract substring (MID$ function)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = source string pointer
#   rdx = source string length
#   r8  = start position (1-based)
#   r9  = count (-1 means "rest of string")
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
# ------------------------------------------------------------------------------
.globl _rt_mid
_rt_mid:
    dec r8                  # Convert to 0-based index
    cmp r8, rdx             # If start >= length
    jae .Lmid_empty
    mov rax, rcx
    add rax, r8             # result ptr = src + start
    sub rdx, r8             # remaining = length - start
    cmp r9, 0               # If count < 0 (means "rest")
    jl .Lmid_rest
    cmp r9, rdx
    cmova r9, rdx
    mov rdx, r9
    ret
.Lmid_rest:
    ret
.Lmid_empty:
    mov rax, rcx
    xor rdx, rdx
    ret

# ------------------------------------------------------------------------------
# _rt_instr - Find substring position (INSTR function)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = haystack pointer
#   rdx = haystack length
#   r8  = needle pointer
#   r9  = needle length
#   [rsp+40] = start position (1-based)
#
# Returns:
#   rax = position (1-based) or 0 if not found
# ------------------------------------------------------------------------------
.globl _rt_instr
_rt_instr:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    push rdi
    push rsi
    sub rsp, 40             # Shadow space + alignment

    # Get 5th argument from stack
    mov rdi, QWORD PTR [rbp + 48]

    # Move arguments to callee-saved registers
    mov r12, rcx            # haystack ptr
    mov r13, rdx            # haystack len
    mov r14, r8             # needle ptr
    mov r15, r9             # needle len
    mov rbx, rdi            # start position (1-based)

    # Adjust for start position
    dec rbx                 # convert to 0-based
    add r12, rbx            # advance haystack ptr
    sub r13, rbx            # reduce remaining length

    # Special case: empty needle
    test r15, r15
    jz .Linstr_at_start

.Linstr_loop:
    cmp r13, r15
    jb .Linstr_not_found

    # memcmp(haystack_pos, needle, needle_len)
    mov rcx, r12
    mov rdx, r14
    mov r8, r15
    call memcmp
    test eax, eax
    jz .Linstr_found

    inc r12
    dec r13
    inc rbx
    jmp .Linstr_loop

.Linstr_found:
    mov rax, rbx
    add rax, 1
    jmp .Linstr_done

.Linstr_at_start:
    mov rax, rbx
    add rax, 1
    jmp .Linstr_done

.Linstr_not_found:
    xor rax, rax

.Linstr_done:
    add rsp, 40
    pop rsi
    pop rdi
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
# Arguments:
#   rcx = left string pointer
#   rdx = left string length
#   r8  = right string pointer
#   r9  = right string length
#
# Returns:
#   rax = pointer to new string
#   rdx = total length
# ------------------------------------------------------------------------------
.globl _rt_strcat
_rt_strcat:
    push rbp
    mov rbp, rsp
    push r12
    push r13
    push r14
    push r15
    push rdi
    push rsi
    sub rsp, 48             # Shadow space (must be 0 mod 16)

    # Save arguments
    mov r12, rcx            # left ptr
    mov r13, rdx            # left len
    mov r14, r8             # right ptr
    mov r15, r9             # right len

    # Get process heap handle
    call GetProcessHeap
    mov rsi, rax            # save heap handle

    # HeapAlloc(hHeap, 0, size)
    mov rcx, rax            # hHeap
    xor rdx, rdx            # dwFlags = 0
    lea r8, [r13 + r15 + 1] # dwBytes = left_len + right_len + 1
    call HeapAlloc

    mov rdi, rax            # save result ptr

    # memcpy(result, left, left_len)
    mov rcx, rax
    mov rdx, r12
    mov r8, r13
    call memcpy

    # memcpy(result + left_len, right, right_len)
    lea rcx, [rdi + r13]
    mov rdx, r14
    mov r8, r15
    call memcpy

    # Null terminate
    lea rax, [r13 + r15]
    mov BYTE PTR [rdi + rax], 0

    # Return
    mov rdx, rax            # total length
    mov rax, rdi            # result pointer

    add rsp, 48
    pop rsi
    pop rdi
    pop r15
    pop r14
    pop r13
    pop r12
    leave
    ret

