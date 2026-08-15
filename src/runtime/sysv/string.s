# BASIC Runtime: String Functions
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

# _rt_val - Convert string to number (VAL function)
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
.globl _rt_val
_rt_val:
    push rbp
    mov rbp, rsp
    xor rsi, rsi            # endptr = NULL (2nd arg)
    # rdi already has string ptr (1st arg)
    call strtod       # returns double in xmm0
    leave
    ret

# _rt_str - Convert number to string (STR$ function)
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
.globl _rt_str
_rt_str:
    push rbp
    mov rbp, rsp
    sub rsp, 16                     # Stack alignment
    # sprintf(buffer, "%g", value)
    lea rdi, [rip + _str_buf]       # destination buffer (1st arg)
    lea rsi, [rip + _fmt_float]     # format string (2nd arg)
    mov eax, 1                      # 1 vector register arg (xmm0)
    call sprintf
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

# _rt_chr - Convert ASCII code to single character (CHR$ function)
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

# _rt_left - Extract leftmost characters (LEFT$ function)
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
.globl _rt_left
_rt_left:
    mov rax, rdi            # result ptr = source ptr
    cmp rdx, rsi            # if count > length
    cmova rdx, rsi          #   count = length
    ret                     # return (ptr, count)

# _rt_right - Extract rightmost characters (RIGHT$ function)
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
.globl _rt_right
_rt_right:
    cmp rdx, rsi            # if count > length
    cmova rdx, rsi          #   count = length
    mov rax, rdi            # start with source ptr
    add rax, rsi            # point to end
    sub rax, rdx            # back up by count
    ret                     # rdx already has the count

# _rt_mid - Extract substring (MID$ function)
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

# _rt_instr - Find substring position (INSTR function)
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
    call memcmp
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

# _rt_strcat - Concatenate two strings (+ operator)
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
    call malloc       # returns ptr in rax

    # Copy left string: memcpy(result, left, left_len)
    mov QWORD PTR [rsp], rax    # save result ptr (aligned)
    mov rdi, rax            # dest = malloc result
    mov rsi, r12            # src = left ptr
    mov rdx, r13            # len = left len
    call memcpy

    # Copy right string: memcpy(result + left_len, right, right_len)
    mov rdi, QWORD PTR [rsp]    # restore result ptr
    add rdi, r13            # dest = result + left_len
    mov rsi, r14            # src = right ptr
    mov rdx, r15            # len = right len
    call memcpy

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

# _rt_strcmp - Compare two BASIC strings
# BASIC strings are (ptr, len) pairs and are not null-terminated, so libc strcmp
# cannot be used. Compares lexicographically by unsigned byte value, then by
# length when one string is a prefix of the other -- so "ab" < "abc".
#
# Arguments:
#   rdi = left pointer,  rsi = left length
#   rdx = right pointer, rcx = right length
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

    # r8 = number of bytes to compare = min(left_len, right_len)
    mov r8, rsi
    cmp r8, rcx
    jbe .Lstrcmp_have_min
    mov r8, rcx
.Lstrcmp_have_min:

    xor r9, r9              # r9 = byte index
    jmp .Lstrcmp_check
.Lstrcmp_loop:
    movzx eax, BYTE PTR [rdi + r9]
    movzx r10d, BYTE PTR [rdx + r9]
    cmp eax, r10d
    jne .Lstrcmp_differ
    inc r9
.Lstrcmp_check:
    cmp r9, r8
    jb .Lstrcmp_loop

    # Common prefix is equal: the shorter string sorts first.
    xor eax, eax
    cmp rsi, rcx
    je .Lstrcmp_done        # same length -> equal
    jb .Lstrcmp_shorter
    mov eax, 1              # left is longer -> greater
    jmp .Lstrcmp_done
.Lstrcmp_shorter:
    mov eax, -1
    jmp .Lstrcmp_done

.Lstrcmp_differ:
    # eax and r10d hold the differing bytes; return their difference.
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
# Arguments: rdi = count
# Returns:   rax = pointer, rdx = length
.globl _rt_space
_rt_space:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov rbx, rdi
    cmp rbx, 0
    jge .Lspace_ok
    xor rbx, rbx            # a negative count yields the empty string
.Lspace_ok:
    lea rdi, [rbx + 1]
    call malloc

    # memset returns its destination, so the pointer needs no saving -- and
    # saving it with a bare push would leave rsp misaligned for the call.
    mov rdi, rax            # dest
    mov esi, ' '            # fill byte
    mov rdx, rbx            # count
    call memset
    mov rdx, rbx

    add rsp, 8
    pop rbx
    leave
    ret

# _rt_string_n - STRING$(n, ch): a string of n copies of one character
# Arguments: rdi = count, rsi = character code
# Returns:   rax = pointer, rdx = length
.globl _rt_string_n
_rt_string_n:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    mov rbx, rdi
    mov r12, rsi
    cmp rbx, 0
    jge .Lstringn_ok
    xor rbx, rbx
.Lstringn_ok:
    lea rdi, [rbx + 1]
    call malloc

    # memset returns its destination; see _rt_space.
    mov rdi, rax
    mov esi, r12d
    mov rdx, rbx
    call memset
    mov rdx, rbx

    pop r12
    pop rbx
    leave
    ret

# _rt_ltrim - LTRIM$(s): drop leading spaces
# Arguments: rdi = pointer, rsi = length
# Returns:   rax = pointer, rdx = length (an interior view, not a copy)
.globl _rt_ltrim
_rt_ltrim:
    xor rcx, rcx
.Lltrim_loop:
    cmp rcx, rsi
    jae .Lltrim_done
    cmp BYTE PTR [rdi + rcx], ' '
    jne .Lltrim_done
    inc rcx
    jmp .Lltrim_loop
.Lltrim_done:
    lea rax, [rdi + rcx]
    mov rdx, rsi
    sub rdx, rcx
    ret

# _rt_rtrim - RTRIM$(s): drop trailing spaces
# Arguments: rdi = pointer, rsi = length
# Returns:   rax = pointer, rdx = length
.globl _rt_rtrim
_rt_rtrim:
    mov rdx, rsi
.Lrtrim_loop:
    test rdx, rdx
    jz .Lrtrim_done
    cmp BYTE PTR [rdi + rdx - 1], ' '
    jne .Lrtrim_done
    dec rdx
    jmp .Lrtrim_loop
.Lrtrim_done:
    mov rax, rdi
    ret

# _rt_ucase / _rt_lcase - UCASE$(s) / LCASE$(s)
# These must copy: the source may be a .data literal shared with other uses.
#
# Arguments: rdi = pointer, rsi = length
# Returns:   rax = pointer, rdx = length
.globl _rt_ucase
_rt_ucase:
    mov r8b, 1              # r8b != 0 selects upper-casing
    jmp _rt_case_convert

.globl _rt_lcase
_rt_lcase:
    xor r8b, r8b

_rt_case_convert:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    sub rsp, 8

    mov rbx, rdi            # source
    mov r12, rsi            # length
    mov r13d, r8d           # direction

    lea rdi, [r12 + 1]
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

    add rsp, 8
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_hex / _rt_oct - HEX$(n) / OCT$(n)
# Arguments: rdi = value (already truncated to an integer)
# Returns:   rax = pointer, rdx = length
.globl _rt_hex
_rt_hex:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov rdx, rdi
    lea rdi, [rip + _str_buf]
    lea rsi, [rip + _fmt_hex]
    xor eax, eax
    call sprintf
    mov rdx, rax
    lea rax, [rip + _str_buf]
    leave
    ret

.globl _rt_oct
_rt_oct:
    push rbp
    mov rbp, rsp
    sub rsp, 16
    mov rdx, rdi
    lea rdi, [rip + _str_buf]
    lea rsi, [rip + _fmt_oct]
    xor eax, eax
    call sprintf
    mov rdx, rax
    lea rax, [rip + _str_buf]
    leave
    ret

# _rt_strdup - Copy a string onto the heap
# String assignment copies, so that mutating one variable cannot be seen
# through another -- or, worse, through the shared .data literal a string
# constant points at. Without this, MID$(A$,1,1) = "J" after A$ = "HELLO"
# would rewrite the literal for every other use of "HELLO" in the program.
#
# Arguments: rdi = pointer, rsi = length
# Returns:   rax = pointer, rdx = length
.globl _rt_strdup
_rt_strdup:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    mov rbx, rdi
    mov r12, rsi

    lea rdi, [r12 + 1]
    call malloc

    # memcpy returns its destination; see _rt_space.
    mov rdi, rax
    mov rsi, rbx
    mov rdx, r12
    call memcpy
    mov rdx, r12

    pop r12
    pop rbx
    leave
    ret

# _rt_mid_assign - MID$(s, start [, len]) = value
# Overwrites characters of the target in place. The target's length never
# changes: at most `len` characters are replaced, and never past the end.
# Positions are 1-based.
#
# Arguments:
#   rdi = target pointer, rsi = target length
#   rdx = start (1-based), rcx = maximum characters to replace
#   r8  = source pointer,  r9  = source length
#
# Returns: nothing
.globl _rt_mid_assign
_rt_mid_assign:
    # Clamp the count to the source length.
    cmp rcx, r9
    jbe .Lmid_have_count
    mov rcx, r9
.Lmid_have_count:

    # Convert the 1-based start to an index; a start below 1 writes nothing.
    cmp rdx, 1
    jl .Lmid_done
    dec rdx

    xor r10, r10            # characters copied
.Lmid_loop:
    cmp r10, rcx
    jae .Lmid_done
    lea r11, [rdx + r10]
    cmp r11, rsi            # stop at the end of the target
    jae .Lmid_done
    mov al, BYTE PTR [r8 + r10]
    mov BYTE PTR [rdi + r11], al
    inc r10
    jmp .Lmid_loop
.Lmid_done:
    ret
