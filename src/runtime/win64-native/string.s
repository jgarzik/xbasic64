# BASIC Runtime: String Functions (Win64 Native - Pure Win32 API)
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
#   - Conversion functions (STR$, CHR$, HEX$, OCT$) format into a scratch
#     buffer and return a heap copy of it, so that two of them in one
#     expression cannot alias; see the System V tree for the failure
#
# Win64 ABI:
#   - Args: rcx, rdx, r8, r9 (then stack)
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#   - 32-byte shadow space required before calls

# String length constants
.equ CHR_RESULT_LEN, 1          # CHR$() always returns 1 character

.data
_fmt_hex: .asciz "%llX"
_fmt_oct: .asciz "%llo"

# Zero-filled scratch, so .bss rather than .data -- see data_defs.s. The
# .text below restores the section for the code that follows.
.bss
.p2align 3
_str_buf: .skip 64          # Scratch for HEX$()/OCT$(); never returned
_chr_buf: .skip 2           # Scratch for CHR$(); never returned

.text

# _rt_val - Convert string to number (VAL function)
# Arguments:
#   rcx = pointer to string
#   rdx = length (ignored - strtod reads until non-numeric)
#
# Returns:
#   xmm0 = parsed double value
.globl _rt_val
_rt_val:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # Shadow space
    xor rdx, rdx            # endptr = NULL
    call strtod             # returns double in xmm0
    leave
    ret

# _rt_str - Convert number to string (STR$ function)
#
# Renders exactly what PRINT would render, by going through the same helper;
# see the System V tree for why the old sprintf("%g") was wrong.
#
# Arguments:
#   xmm0 = number to convert (double)
#
# Returns:
#   rax = pointer to a fresh heap copy of the text
#   rdx = length of string
.globl _rt_str
_rt_str:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # shadow space
    lea rcx, [rip + _fmt_g_table]
    xor edx, edx
    call _rt_fmt_double
    lea rcx, [rip + _num_buf]
    mov rdx, rax            # length
    add rsp, 32
    leave
    jmp _rt_strdup          # the caller may hold another such result

# _rt_str_single - STR$ of a SINGLE
#
# A SINGLE carries only ~7 significant digits, so it uses the shorter table for
# the same reason PRINT does.
#
# Arguments: xmm0 = value, already widened to double
# Returns:   rax = pointer to a fresh heap copy, rdx = length
.globl _rt_str_single
_rt_str_single:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # shadow space
    lea rcx, [rip + _fmt_g_single_table]
    mov edx, 1
    call _rt_fmt_double
    lea rcx, [rip + _num_buf]
    mov rdx, rax
    add rsp, 32
    leave
    jmp _rt_strdup

# _rt_chr - Convert ASCII code to single character (CHR$ function)
# Arguments:
#   rcx = ASCII code (0-255)
#
# Returns:
#   rax = pointer to a fresh heap copy of the character
#   rdx = 1 (length)
.globl _rt_chr
_rt_chr:
    push rbp
    mov rbp, rsp
    lea rax, [rip + _chr_buf]
    mov BYTE PTR [rax], cl          # read cl before rcx becomes the buffer
    mov BYTE PTR [rax + 1], 0
    mov rcx, rax
    mov rdx, CHR_RESULT_LEN
    leave
    jmp _rt_strdup                  # the caller may hold another such result

# _rt_left - Extract leftmost characters (LEFT$ function)
# Arguments:
#   rcx = source string pointer
#   rdx = source string length
#   r8  = number of characters to extract
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
.globl _rt_left
_rt_left:
    mov rax, rcx
    cmp r8, rdx
    cmova r8, rdx
    mov rdx, r8
    ret

# _rt_right - Extract rightmost characters (RIGHT$ function)
# Arguments:
#   rcx = source string pointer
#   rdx = source string length
#   r8  = number of characters to extract
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
.globl _rt_right
_rt_right:
    cmp r8, rdx
    cmova r8, rdx
    mov rax, rcx
    add rax, rdx
    sub rax, r8
    mov rdx, r8
    ret

# _rt_mid - Extract substring (MID$ function)
# Arguments:
#   rcx = source string pointer
#   rdx = source string length
#   r8  = start position (1-based)
#   r9  = count (-1 means "rest of string")
#
# Returns:
#   rax = pointer to start of result
#   rdx = result length
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

# _rt_instr - Find substring position (INSTR function)
# Arguments:
#   rcx = haystack pointer
#   rdx = haystack length
#   r8  = needle pointer
#   r9  = needle length
#   [rsp+40] = start position (1-based)
#
# Returns:
#   rax = position (1-based) or 0 if not found
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

# _rt_strcat - Concatenate two strings (+ operator)
# Arguments:
#   rcx = left string pointer
#   rdx = left string length
#   r8  = right string pointer
#   r9  = right string length
#
# Returns:
#   rax = pointer to new string
#   rdx = total length
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


# _rt_strcmp - Compare two BASIC strings
# BASIC strings are (ptr, len) pairs and are not null-terminated, so CRT strcmp
# cannot be used. Compares lexicographically by unsigned byte value, then by
# length when one string is a prefix of the other -- so "ab" < "abc".
#
# Arguments (Win64):
#   rcx = left pointer,  rdx = left length
#   r8  = right pointer, r9  = right length
#
# Returns:
#   eax < 0 if left < right, 0 if equal, > 0 if left > right (like memcmp)
#
# A length of 0 is valid and means the empty string; the pointer is then never
# dereferenced, so an unassigned (NULL, 0) string compares correctly.
.globl _rt_strcmp
_rt_strcmp:
    push rbp
    mov rbp, rsp

    # r10 = number of bytes to compare = min(left_len, right_len)
    mov r10, rdx
    cmp r10, r9
    jbe .Lstrcmp_have_min
    mov r10, r9
.Lstrcmp_have_min:

    xor r11, r11            # r11 = byte index
    jmp .Lstrcmp_check
.Lstrcmp_loop:
    movzx eax, BYTE PTR [rcx + r11]
    cmp al, BYTE PTR [r8 + r11]
    jne .Lstrcmp_differ
    inc r11
.Lstrcmp_check:
    cmp r11, r10
    jb .Lstrcmp_loop

    # Common prefix is equal: the shorter string sorts first.
    xor eax, eax
    cmp rdx, r9
    je .Lstrcmp_done        # same length -> equal
    jb .Lstrcmp_shorter
    mov eax, 1              # left is longer -> greater
    jmp .Lstrcmp_done
.Lstrcmp_shorter:
    mov eax, -1
    jmp .Lstrcmp_done

.Lstrcmp_differ:
    movzx eax, BYTE PTR [rcx + r11]
    movzx r10d, BYTE PTR [r8 + r11]
    sub eax, r10d

.Lstrcmp_done:
    leave
    ret

# Additional string functions
#
# These were all reachable only through the old "unknown $-suffixed call must
# be an array" heuristic, so every one of them aborted the compiler.
#
# Functions that shorten a string (LTRIM$, RTRIM$) return an interior pointer
# into the original rather than allocating, which is sound because BASIC
# strings are (ptr, len) pairs and are never mutated in place.

# _rt_space - SPACE$(n): a string of n spaces
# Arguments: rcx = count       Returns: rax = pointer, rdx = length
.globl _rt_space
_rt_space:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40

    mov rbx, rcx
    cmp rbx, 0
    jge .Lspace_ok
    xor rbx, rbx            # a negative count yields the empty string
.Lspace_ok:
    lea rcx, [rbx + 1]
    call malloc

    # memset returns its destination, so the pointer needs no saving -- and
    # saving it with a bare push would leave rsp misaligned for the call.
    mov rcx, rax            # dest
    mov rdx, ' '            # fill byte
    mov r8, rbx             # count
    sub rsp, 32
    call memset
    add rsp, 32
    mov rdx, rbx

    add rsp, 40
    pop rbx
    leave
    ret

# _rt_string_n - STRING$(n, ch): a string of n copies of one character
# Arguments: rcx = count, rdx = character code
# Returns:   rax = pointer, rdx = length
.globl _rt_string_n
_rt_string_n:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 32

    mov rbx, rcx
    mov r12, rdx
    cmp rbx, 0
    jge .Lstringn_ok
    xor rbx, rbx
.Lstringn_ok:
    lea rcx, [rbx + 1]
    call malloc

    # memset returns its destination; see _rt_space.
    mov rcx, rax
    mov rdx, r12
    mov r8, rbx
    sub rsp, 32
    call memset
    add rsp, 32
    mov rdx, rbx

    add rsp, 32
    pop r12
    pop rbx
    leave
    ret

# _rt_ltrim - LTRIM$(s): drop leading spaces
# Arguments: rcx = pointer, rdx = length
# Returns:   rax = pointer, rdx = length (an interior view, not a copy)
.globl _rt_ltrim
_rt_ltrim:
    xor r8, r8
.Lltrim_loop:
    cmp r8, rdx
    jae .Lltrim_done
    cmp BYTE PTR [rcx + r8], ' '
    jne .Lltrim_done
    inc r8
    jmp .Lltrim_loop
.Lltrim_done:
    lea rax, [rcx + r8]
    sub rdx, r8
    ret

# _rt_rtrim - RTRIM$(s): drop trailing spaces
# Arguments: rcx = pointer, rdx = length
# Returns:   rax = pointer, rdx = length
.globl _rt_rtrim
_rt_rtrim:
.Lrtrim_loop:
    test rdx, rdx
    jz .Lrtrim_done
    cmp BYTE PTR [rcx + rdx - 1], ' '
    jne .Lrtrim_done
    dec rdx
    jmp .Lrtrim_loop
.Lrtrim_done:
    mov rax, rcx
    ret

# _rt_ucase / _rt_lcase - UCASE$(s) / LCASE$(s)
# These must copy: the source may be a .data literal shared with other uses.
#
# Arguments: rcx = pointer, rdx = length
# Returns:   rax = pointer, rdx = length
.globl _rt_ucase
_rt_ucase:
    mov r9b, 1              # r9b != 0 selects upper-casing
    jmp _rt_case_convert

.globl _rt_lcase
_rt_lcase:
    xor r9b, r9b
    # An explicit jump, not a fall-through: see the System V tree.
    jmp _rt_case_convert

_rt_case_convert:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    sub rsp, 40

    mov rbx, rcx            # source
    mov r12, rdx            # length
    mov r13d, r9d           # direction

    lea rcx, [r12 + 1]
    call malloc

    xor rcx, rcx
.Lcase_loop:
    cmp rcx, r12
    jae .Lcase_done
    mov dl, BYTE PTR [rbx + rcx]
    test r13b, r13b
    jz .Lcase_lower
    cmp dl, 'a'
    jb .Lcase_store
    cmp dl, 'z'
    ja .Lcase_store
    sub dl, 32
    jmp .Lcase_store
.Lcase_lower:
    cmp dl, 'A'
    jb .Lcase_store
    cmp dl, 'Z'
    ja .Lcase_store
    add dl, 32
.Lcase_store:
    mov BYTE PTR [rax + rcx], dl
    inc rcx
    jmp .Lcase_loop
.Lcase_done:
    mov rdx, r12

    add rsp, 40
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_hex / _rt_oct - HEX$(n) / OCT$(n)
# Arguments: rcx = value (already truncated to an integer)
# Returns:   rax = pointer to a fresh heap copy, rdx = length
.globl _rt_hex
_rt_hex:
    push rbp
    mov rbp, rsp
    sub rsp, 48    # shadow space; 48 realigns after push rbp
    mov r8, rcx
    lea rcx, [rip + _str_buf]
    lea rdx, [rip + _fmt_hex]
    call sprintf
    lea rcx, [rip + _str_buf]
    mov rdx, rax
    add rsp, 48
    leave
    jmp _rt_strdup

.globl _rt_oct
_rt_oct:
    push rbp
    mov rbp, rsp
    sub rsp, 48    # shadow space; 48 realigns after push rbp
    mov r8, rcx
    lea rcx, [rip + _str_buf]
    lea rdx, [rip + _fmt_oct]
    call sprintf
    lea rcx, [rip + _str_buf]
    mov rdx, rax
    add rsp, 48
    leave
    jmp _rt_strdup

# _rt_strdup - Copy a string onto the heap
# String assignment copies, so that mutating one variable cannot be seen
# through another -- or, worse, through the shared .data literal a string
# constant points at.
#
# Arguments: rcx = pointer, rdx = length
# Returns:   rax = pointer, rdx = length
.globl _rt_strdup
_rt_strdup:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 32

    mov rbx, rcx
    mov r12, rdx

    lea rcx, [r12 + 1]
    call malloc

    # memcpy returns its destination; see _rt_space.
    mov rcx, rax
    sub rsp, 32
    mov rdx, rbx
    mov r8, r12
    call memcpy
    add rsp, 32
    mov rdx, r12

    add rsp, 32
    pop r12
    pop rbx
    leave
    ret

# _rt_mid_assign - MID$(s, start [, len]) = value
# Overwrites characters of the target in place. The target's length never
# changes. Positions are 1-based.
#
# Arguments (Win64):
#   rcx = target pointer, rdx = target length
#   r8  = start (1-based), r9 = maximum characters to replace
#   [rsp+40] = source pointer, [rsp+48] = source length
#
# Returns: nothing
.globl _rt_mid_assign
_rt_mid_assign:
    mov r10, QWORD PTR [rsp + 40]   # source pointer
    mov r11, QWORD PTR [rsp + 48]   # source length

    cmp r9, r11
    jbe .Lmid_have_count
    mov r9, r11
.Lmid_have_count:

    cmp r8, 1
    jl .Lmid_done
    dec r8

    push rbx
    xor rbx, rbx                    # characters copied
.Lmid_loop:
    cmp rbx, r9
    jae .Lmid_pop_done
    lea rax, [r8 + rbx]
    cmp rax, rdx
    jae .Lmid_pop_done
    mov r11b, BYTE PTR [r10 + rbx]
    mov BYTE PTR [rcx + rax], r11b
    inc rbx
    jmp .Lmid_loop
.Lmid_pop_done:
    pop rbx
.Lmid_done:
    ret
