# BASIC Runtime: Input Functions (Win64 Native - Pure Win32 API)
#
# Keyboard input functions using Win32 API (ReadFile) instead of libc scanf.
# Uses UCRT strtod for number parsing.
#
# Win64 ABI:
#   - Integer args: rcx, rdx, r8, r9 (then stack)
#   - 32-byte shadow space required before every call
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#

# Win32 API Constants
.equ STD_INPUT_HANDLE, -10

# ASCII character codes
.equ CHAR_LF, 10
.equ CHAR_CR, 13

# Buffer size constants
.equ INPUT_BUF_SIZE, 1024
.equ MAX_INPUT_LEN, 1023            # INPUT_BUF_SIZE - 1 (for null terminator)

.data
_stdin_handle: .quad 0
_input_buf: .skip 1024           # Buffer for string input
_bytes_read: .quad 0             # For ReadFile output parameter

.text

# _rt_init_input - Initialize stdin handle (call once at startup)
# Internal: _rt_platform_init calls this; it is not a codegen entry point.
_rt_init_input:
    push rbp
    mov rbp, rsp
    sub rsp, 32

    # GetStdHandle(STD_INPUT_HANDLE)
    mov ecx, STD_INPUT_HANDLE
    call GetStdHandle
    lea rcx, [rip + _stdin_handle]
    mov [rcx], rax

    leave
    ret

# _rt_input_string - Read a line of text from stdin
# Reads characters until newline (which is not included in result).
# Uses a static buffer, so the returned pointer is only valid until the next
# call to _rt_input_string.
#
# Arguments: none
#
# Returns:
#   rax = pointer to string data (in _input_buf)
#   rdx = length of string
.globl _rt_input_string
_rt_input_string:
    push rbp
    mov rbp, rsp
    sub rsp, 48             # Shadow space + stack args

    # Clear buffer
    lea rax, [rip + _input_buf]
    mov BYTE PTR [rax], 0

    # Get stdin handle
    lea rax, [rip + _stdin_handle]
    mov rcx, [rax]          # handle → rcx (1st arg)

    # ReadFile(handle, buffer, maxlen, &bytesRead, NULL)
    lea rdx, [rip + _input_buf]     # buffer → rdx (2nd arg)
    mov r8, MAX_INPUT_LEN                     # max bytes → r8 (3rd arg)
    lea r9, [rip + _bytes_read]     # &bytesRead → r9 (4th arg)
    mov QWORD PTR [rsp + 32], 0     # NULL → 5th arg (stack)
    call ReadFile

    # Get number of bytes read
    lea rax, [rip + _bytes_read]
    mov rdx, [rax]          # rdx = bytes read

    # Strip trailing CR/LF
    lea rax, [rip + _input_buf]
    test rdx, rdx
    jz .Linput_str_done

    # Check for trailing LF
    mov cl, BYTE PTR [rax + rdx - 1]
    cmp cl, CHAR_LF         # LF?
    jne .Lcheck_cr
    dec rdx
    mov BYTE PTR [rax + rdx], 0
    test rdx, rdx
    jz .Linput_str_done

.Lcheck_cr:
    # Check for trailing CR
    mov cl, BYTE PTR [rax + rdx - 1]
    cmp cl, CHAR_CR         # CR?
    jne .Linput_str_done
    dec rdx
    mov BYTE PTR [rax + rdx], 0

.Linput_str_done:
    # Null-terminate
    lea rax, [rip + _input_buf]
    mov BYTE PTR [rax + rdx], 0

    # Return: rax = pointer, rdx = length
    leave
    ret

# _rt_input_number - Read a numeric value from stdin
# Reads a double-precision floating point number. Uses strtod from UCRT.
#
# Arguments: none
#
# Returns:
#   xmm0 = the number read (double)
.globl _rt_input_number
_rt_input_number:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 48             # Shadow space + stack args

.Linput_num_try:
    # Clear buffer
    lea rax, [rip + _input_buf]
    mov BYTE PTR [rax], 0

    # Get stdin handle
    lea rax, [rip + _stdin_handle]
    mov rcx, [rax]          # handle

    # ReadFile(handle, buffer, maxlen, &bytesRead, NULL)
    lea rdx, [rip + _input_buf]
    mov r8, MAX_INPUT_LEN
    lea r9, [rip + _bytes_read]
    mov QWORD PTR [rsp + 32], 0
    call ReadFile

    # Null-terminate the input
    lea rax, [rip + _bytes_read]
    mov rcx, [rax]          # bytes read
    test rcx, rcx
    jz .Linput_num_eof      # no input left: yield 0
    lea rax, [rip + _input_buf]
    mov BYTE PTR [rax + rcx], 0

    # Parse number using strtod(buffer, &end)
    lea rcx, [rip + _input_buf]
    lea rdx, [rbp - 32]     # endptr
    call strtod

    # strtod leaves endptr at the start when nothing parsed. Reject that and
    # ask again, as GW-BASIC does; without this, text silently yielded 0.
    mov rax, QWORD PTR [rbp - 32]
    lea rcx, [rip + _input_buf]
    cmp rax, rcx
    je .Linput_num_redo

    add rsp, 48
    pop rsi
    pop rbx
    leave
    ret

.Linput_num_redo:
    lea rcx, [rip + _redo_msg]
    mov rdx, _redo_msg_len
    call _rt_con_string
    jmp .Linput_num_try

.Linput_num_eof:
    xorpd xmm0, xmm0
    add rsp, 48
    pop rsi
    pop rbx
    leave
    ret

