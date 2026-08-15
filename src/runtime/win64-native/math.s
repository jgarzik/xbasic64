# BASIC Runtime: Math and Utility Functions (Win64 Native - Pure Win32 API)
#
# Miscellaneous functions. Uses Win32 API instead of libc.
#
# Note: Most math functions (SIN, COS, SQR, etc.) are implemented inline in
# codegen.rs using x87 FPU or SSE instructions.
#
# Win64 ABI:
#   - Args: rcx, rdx, r8, r9
#   - 32-byte shadow space required before calls

# Win32 API Constants
.equ STD_OUTPUT_HANDLE, -11

.data
_rng_state: .quad 0x12345678DEADBEEF
_cls_seq: .ascii "\033[2J\033[H"
.equ _cls_seq_len, . - _cls_seq
_cls_bytes_written: .quad 0

.text

# _rt_rnd - Generate random number (RND function)
# Returns a pseudo-random number in the range [0, 1).
#
# Arguments:
#   xmm0 = seed parameter (currently ignored)
#
# Returns:
#   xmm0 = random double in [0, 1)
#
# Algorithm: Xorshift64
.globl _rt_rnd
_rt_rnd:
    push rbp
    mov rbp, rsp

    # Load current state
    mov rax, QWORD PTR [rip + _rng_state]

    # Xorshift64 algorithm
    mov rcx, rax
    shl rcx, 13
    xor rax, rcx            # state ^= state << 13

    mov rcx, rax
    shr rcx, 7
    xor rax, rcx            # state ^= state >> 7

    mov rcx, rax
    shl rcx, 17
    xor rax, rcx            # state ^= state << 17

    # Save new state
    mov QWORD PTR [rip + _rng_state], rax

    # Convert to double in [0, 1)
    shr rax, 12             # Keep top 52 bits
    mov rcx, 0x3FF0000000000000  # IEEE 754: exponent=1023 (value=1.0)
    or rax, rcx             # Combine: value in [1,2)
    movq xmm0, rax

    # Subtract 1.0 to get [0, 1)
    mov rcx, 0x3FF0000000000000
    movq xmm1, rcx
    subsd xmm0, xmm1

    leave
    ret

# _rt_timer - TIMER: seconds since midnight, UTC
# This used to return GetTickCount64() / 1000, which is seconds since the
# machine booted -- not what LANGREF documents, not what the System V runtime
# returns, and not what any BASIC program expects. A CI runner three minutes
# old reported 208.437 where the time of day was wanted.
#
# GetSystemTime gives UTC, matching the System V twin, with millisecond
# resolution. SYSTEMTIME is eight WORDs: wYear, wMonth, wDayOfWeek, wDay,
# wHour, wMinute, wSecond, wMilliseconds.
#
# Arguments: none
#
# Returns:
#   xmm0 = seconds since midnight (double, 0 <= t < 86400)
.equ ST_HOUR,   8
.equ ST_MINUTE, 10
.equ ST_SECOND, 12
.equ ST_MSEC,   14

.globl _rt_timer
_rt_timer:
    push rbp
    mov rbp, rsp
    sub rsp, 48             # shadow space, then a 16-byte SYSTEMTIME above it

    lea rcx, [rsp + 32]
    call GetSystemTime

    movzx eax, WORD PTR [rsp + 32 + ST_HOUR]
    imul eax, eax, 3600
    movzx ecx, WORD PTR [rsp + 32 + ST_MINUTE]
    imul ecx, ecx, 60
    add eax, ecx
    movzx ecx, WORD PTR [rsp + 32 + ST_SECOND]
    add eax, ecx
    cvtsi2sd xmm0, eax

    movzx ecx, WORD PTR [rsp + 32 + ST_MSEC]
    cvtsi2sd xmm1, ecx
    mov rax, 0x408F400000000000     # 1000.0
    movq xmm2, rax
    divsd xmm1, xmm2
    addsd xmm0, xmm1

    leave
    ret

# _rt_cls - Clear screen (CLS statement)
# Uses ANSI escape sequences via console output.
#
# Arguments: none
# Returns: nothing
.globl _rt_cls
_rt_cls:
    push rbp
    mov rbp, rsp
    sub rsp, 48             # Shadow space + stack arg

    # Get stdout handle
    mov ecx, STD_OUTPUT_HANDLE
    call GetStdHandle

    # WriteFile(handle, cls_seq, cls_seq_len, &bytesWritten, NULL)
    mov rcx, rax            # handle
    lea rdx, [rip + _cls_seq]
    mov r8, _cls_seq_len
    lea r9, [rip + _cls_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    leave
    ret

