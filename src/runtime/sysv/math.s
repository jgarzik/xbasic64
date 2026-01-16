# ==============================================================================
# BASIC Runtime: Math and Utility Functions
# ==============================================================================
#
# Miscellaneous functions that don't fit in other categories.
#
# Note: Most math functions (SIN, COS, SQR, etc.) are implemented inline in
# codegen.rs using x87 FPU or SSE instructions. This file contains only the
# functions that require more complex logic or libc calls.
#
# Global state (from data_defs.s):
#   _rng_state = 8 bytes for random number generator state
#   _cls_seq   = ANSI escape sequence for clear screen
# ==============================================================================

# ------------------------------------------------------------------------------
# _rt_rnd - Generate random number (RND function)
# ------------------------------------------------------------------------------
# Returns a pseudo-random number in the range [0, 1).
#
# Arguments:
#   xmm0 = seed parameter (currently ignored, BASIC convention)
#          In GW-BASIC: RND(0) repeats last, RND(<0) reseeds, RND(>0) next
#          We simplify: always return next random number.
#
# Returns:
#   xmm0 = random double in [0, 1)
#
# Algorithm: Xorshift64
#   A fast, high-quality PRNG with 64-bit state. Period is 2^64 - 1.
#
#   state ^= state << 13
#   state ^= state >> 7
#   state ^= state << 17
#
# Conversion to [0,1):
#   IEEE 754 double has 52-bit mantissa. We take 52 bits of state,
#   set exponent to 1023 (representing 1.xxx in binary), then subtract 1.0.
#   This gives a uniformly distributed value in [0, 1).
#
#   Bit layout: [sign=0][exp=1023][mantissa=random52bits] = 1.xxxxx
#   Subtract 1.0 to get 0.xxxxx
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
    shr rax, 12             # Keep top 52 bits (discard low 12)
    mov rcx, 0x3FF0000000000000  # IEEE 754: exponent=1023, mantissa=0 (value=1.0)
    or rax, rcx             # Combine: exponent=1023, mantissa=random (value in [1,2))
    movq xmm0, rax          # Move to SSE register
    # Subtract 1.0 to get [0, 1)
    mov rcx, 0x3FF0000000000000
    movq xmm1, rcx
    subsd xmm0, xmm1        # result = [1,2) - 1.0 = [0,1)
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_timer - Get seconds since midnight (TIMER function)
# ------------------------------------------------------------------------------
# Returns the number of seconds elapsed since midnight, as a floating-point
# number. This matches GW-BASIC's TIMER function.
#
# Arguments: none
#
# Returns:
#   xmm0 = seconds since midnight (double, 0-86399)
#
# Implementation:
#   1. Call time(NULL) to get Unix timestamp (seconds since 1970)
#   2. Compute timestamp mod 86400 (seconds per day)
#   3. Convert to double
#
# Note: This gives UTC-based "seconds since midnight", not local time.
# For most timing purposes (measuring elapsed time), this doesn't matter.
# ------------------------------------------------------------------------------
.globl _rt_timer
_rt_timer:
    push rbp
    mov rbp, rsp
    sub rsp, 16             # Stack alignment
    # time(NULL) returns seconds since epoch
    xor rdi, rdi            # NULL pointer (1st arg)
    call {libc}time         # returns time_t in rax
    # Compute seconds mod 86400 (seconds per day)
    xor rdx, rdx            # Clear rdx for division
    mov rcx, 86400          # divisor = seconds per day
    div rcx                 # rax = quotient, rdx = remainder
    # Convert remainder to double
    cvtsi2sd xmm0, rdx      # rdx = seconds since midnight
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_cls - Clear screen (CLS statement)
# ------------------------------------------------------------------------------
# Clears the terminal screen and moves cursor to home position.
# Uses ANSI escape sequences, which work on most modern terminals.
#
# Arguments: none
# Returns: nothing
#
# Escape sequence: ESC[2J ESC[H
#   ESC[2J = clear entire screen
#   ESC[H  = move cursor to home (top-left)
# ------------------------------------------------------------------------------
.globl _rt_cls
_rt_cls:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _cls_seq]   # ANSI escape sequence
    xor eax, eax                # no vector args
    call {libc}printf
    leave
    ret
