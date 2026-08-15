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
# _rt_print_float / _rt_print_single - Print a numeric value
# ------------------------------------------------------------------------------
# GW-BASIC convention: a whole number prints without a decimal point.
#
# For fractional values, print the *shortest* decimal that reads back as the
# same value: try successively longer %g precisions and keep the first whose
# text strtod's back unchanged. A plain %g gives 6 significant digits, which
# for a dialect whose default type is Double discards most of the value.
# _rt_print_single compares at 32-bit precision and starts from a shorter
# format, since a SINGLE carries only ~7 significant digits.
#
# Arguments:
#   xmm0 = value to print (double; a SINGLE arrives already widened)
# ------------------------------------------------------------------------------
.globl _rt_print_float
_rt_print_float:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 56             # shadow space + locals, keeps rsp 16-byte aligned
    movsd QWORD PTR [rbp - 32], xmm0    # original value
    lea rbx, [rip + _fmt_g_table]
    xor esi, esi            # esi = 0: compare at double precision
    jmp .Lpf_common

.globl _rt_print_single
_rt_print_single:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 56
    movsd QWORD PTR [rbp - 32], xmm0
    lea rbx, [rip + _fmt_g_single_table]
    mov esi, 1              # esi = 1: compare at single precision

.Lpf_common:
    # Whole number? Print as an integer.
    movsd xmm0, QWORD PTR [rbp - 32]
    cvttsd2si rax, xmm0
    cvtsi2sd xmm1, rax
    ucomisd xmm0, xmm1
    jne .Lpf_fractional
    jp .Lpf_fractional
    lea rcx, [rip + _num_buf]
    lea rdx, [rip + _fmt_int]
    mov r8, rax
    call sprintf
    jmp .Lpf_write

.Lpf_fractional:
    mov rdx, QWORD PTR [rbx]
    test rdx, rdx
    jz .Lpf_write           # table exhausted: write the last attempt
    lea rcx, [rip + _num_buf]
    movsd xmm2, QWORD PTR [rbp - 32]
    movq r8, xmm2
    call sprintf

    # Does it read back as the same value?
    lea rcx, [rip + _num_buf]
    xor edx, edx
    call strtod
    movsd xmm1, QWORD PTR [rbp - 32]
    test esi, esi
    jz .Lpf_cmp_double
    cvtsd2ss xmm0, xmm0
    cvtsd2ss xmm1, xmm1
    ucomiss xmm0, xmm1
    jmp .Lpf_cmp_done
.Lpf_cmp_double:
    ucomisd xmm0, xmm1
.Lpf_cmp_done:
    jp .Lpf_next
    je .Lpf_write
.Lpf_next:
    add rbx, 8
    jmp .Lpf_fractional

.Lpf_write:
    # WriteFile(stdout, _num_buf, strlen(_num_buf), &bytesWritten, NULL)
    lea rcx, [rip + _num_buf]
    call lstrlenA
    mov r8, rax             # length
    lea rax, [rip + _stdout_handle]
    mov rcx, [rax]
    lea rdx, [rip + _num_buf]
    lea r9, [rip + _bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    lea rsp, [rbp - 16]
    pop rsi
    pop rbx
    pop rbp
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
