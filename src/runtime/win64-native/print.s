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

    leave
    ret

# ------------------------------------------------------------------------------
# _rt_print_float - Print a numeric value
# ------------------------------------------------------------------------------
# Arguments:
#   xmm0 = value to print (double)
# ------------------------------------------------------------------------------
.globl _rt_print_float
_rt_print_float:
    push rbp
    mov rbp, rsp
    sub rsp, 64             # Shadow space + locals

    # Check if value is a whole number
    cvttsd2si rax, xmm0     # truncate to integer
    cvtsi2sd xmm1, rax      # convert back to double
    ucomisd xmm0, xmm1      # compare
    jne .Lprint_as_float

    # Format as integer using sprintf
    # sprintf(buffer, "%lld", value)
    lea rcx, [rip + _print_buffer]
    lea rdx, [rip + _fmt_int]
    mov r8, rax             # integer value
    call sprintf
    jmp .Lprint_formatted

.Lprint_as_float:
    # Format as float using sprintf
    # sprintf(buffer, "%g", value)
    lea rcx, [rip + _print_buffer]
    lea rdx, [rip + _fmt_float]
    movsd xmm2, xmm0        # value in xmm2
    movq r8, xmm0           # also in r8 for varargs
    call sprintf

.Lprint_formatted:
    # rax = number of chars written by sprintf

    # Get stdout handle
    lea rcx, [rip + _stdout_handle]
    mov rcx, [rcx]

    # WriteFile(handle, buffer, strlen, &bytesWritten, NULL)
    lea rdx, [rip + _print_buffer]
    mov r8, rax             # length from sprintf return
    lea r9, [rip + _bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

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
