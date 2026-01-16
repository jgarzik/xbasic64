# ==============================================================================
# BASIC Runtime: Math and Utility Functions (Win64 Native - Pure Win32 API)
# ==============================================================================
#
# Miscellaneous functions. Uses Win32 API instead of libc.
#
# Note: Most math functions (SIN, COS, SQR, etc.) are implemented inline in
# codegen.rs using x87 FPU or SSE instructions.
#
# Win64 ABI:
#   - Args: rcx, rdx, r8, r9
#   - 32-byte shadow space required before calls
# ==============================================================================

# Win32 API Constants
.equ STD_OUTPUT_HANDLE, -11

.data
_rng_state: .quad 0x12345678DEADBEEF
_cls_seq: .ascii "\033[2J\033[H"
_cls_seq_len = 7
_cls_bytes_written: .quad 0

.text

# ------------------------------------------------------------------------------
# _rt_rnd - Generate random number (RND function)
# ------------------------------------------------------------------------------
# Returns a pseudo-random number in the range [0, 1).
#
# Arguments:
#   xmm0 = seed parameter (currently ignored)
#
# Returns:
#   xmm0 = random double in [0, 1)
#
# Algorithm: Xorshift64
# ------------------------------------------------------------------------------
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

# ------------------------------------------------------------------------------
# _rt_timer - Get seconds since midnight (TIMER function)
# ------------------------------------------------------------------------------
# Returns seconds based on GetTickCount64 (milliseconds since system start).
# Note: This differs from classic BASIC which returns seconds since midnight.
# For compatibility, we use GetTickCount64 / 1000.0 which gives elapsed time.
#
# Arguments: none
#
# Returns:
#   xmm0 = seconds as double
# ------------------------------------------------------------------------------
.globl _rt_timer
_rt_timer:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # Shadow space

    # GetTickCount64() returns milliseconds since system start
    call GetTickCount64     # returns uint64 in rax

    # Convert to double and divide by 1000
    cvtsi2sd xmm0, rax      # milliseconds as double
    mov rax, 0x408F400000000000  # 1000.0 in IEEE 754
    movq xmm1, rax
    divsd xmm0, xmm1        # seconds = ms / 1000.0

    leave
    ret

# ------------------------------------------------------------------------------
# _rt_cls - Clear screen (CLS statement)
# ------------------------------------------------------------------------------
# Uses ANSI escape sequences via console output.
#
# Arguments: none
# Returns: nothing
# ------------------------------------------------------------------------------
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

