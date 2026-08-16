# BASIC Runtime: Math and Utility Functions
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

# _rt_rnd - Generate random number (RND function)
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
.globl _rt_rnd
_rt_rnd:
    push rbp
    mov rbp, rsp
    # GW-BASIC's argument selects the behaviour:
    #   RND(<0) reseeds from that value and returns the next number
    #   RND(0)  returns the previous number again
    #   RND(>0), and a bare RND, return the next number
    # The argument used to be ignored entirely, so RND(0) advanced like any
    # other call and RND(-1) -- the idiom for a repeatable run -- did nothing.
    xorpd xmm1, xmm1
    ucomisd xmm0, xmm1
    jp .Lrnd_next               # NaN: treat as "next"
    je .Lrnd_repeat
    jb .Lrnd_reseed
    jmp .Lrnd_next

.Lrnd_reseed:
    call _rt_randomize          # consumes xmm0, leaves the state seeded
    jmp .Lrnd_next

.Lrnd_repeat:
    movsd xmm0, QWORD PTR [rip + _rng_last]
    leave
    ret

.Lrnd_next:
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
    # Remember it, so that RND(0) can hand back the same number.
    movsd QWORD PTR [rip + _rng_last], xmm0
    leave
    ret

# _rt_randomize - RANDOMIZE: set the generator's seed
#
# The state was a fixed constant with no way to change it, so every run of every
# program produced the same numbers -- a dice game rolled the same dice every
# time it was played.
#
# The seed's bit pattern is passed through a splitmix64 avalanche rather than
# used directly: BASIC seeds are small integers, and xorshift64 started from a
# small state produces a visibly poor first few values. Zero is replaced,
# because it is xorshift's fixed point and would return 0 forever.
#
# Arguments: xmm0 = seed
# Returns: nothing
.globl _rt_randomize
_rt_randomize:
    push rbp
    mov rbp, rsp
    movq rax, xmm0
    mov rcx, 0x9E3779B97F4A7C15
    add rax, rcx
    mov rcx, rax
    shr rcx, 30
    xor rax, rcx
    mov rcx, 0xBF58476D1CE4E5B9
    imul rax, rcx
    mov rcx, rax
    shr rcx, 27
    xor rax, rcx
    mov rcx, 0x94D049BB133111EB
    imul rax, rcx
    mov rcx, rax
    shr rcx, 31
    xor rax, rcx
    test rax, rax
    jnz .Lrandomize_store
    mov rax, 0x12345678DEADBEEF     # xorshift64 must never hold zero
.Lrandomize_store:
    mov QWORD PTR [rip + _rng_state], rax
    leave
    ret

# _rt_timer - TIMER: seconds since midnight, UTC
# GW-BASIC's TIMER counts fractional seconds since midnight, so this uses
# gettimeofday rather than time(): whole seconds alone made a program that
# times a short loop always read zero.
#
# The basis is UTC, which both runtimes share; see the Win64 twin.
#
# Arguments: none
#
# Returns:
#   xmm0 = seconds since midnight (double, 0 <= t < 86400)
.globl _rt_timer
_rt_timer:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # struct timeval, and keeps rsp 16-byte aligned

    mov rdi, rsp            # &tv
    xor esi, esi            # timezone = NULL
    call gettimeofday

    # tv_sec mod 86400
    mov rax, QWORD PTR [rsp]
    xor edx, edx
    mov rcx, 86400
    div rcx                 # rdx = seconds since midnight
    cvtsi2sd xmm0, rdx

    # ... plus tv_usec / 1000000
    cvtsi2sd xmm1, QWORD PTR [rsp + 8]
    mov rax, 0x412E848000000000     # 1000000.0
    movq xmm2, rax
    divsd xmm1, xmm2
    addsd xmm0, xmm1

    leave
    ret

# _rt_cls - Clear screen (CLS statement)
# Clears the terminal screen and moves cursor to home position.
# Uses ANSI escape sequences, which work on most modern terminals.
#
# Arguments: none
# Returns: nothing
#
# Escape sequence: ESC[2J ESC[H
#   ESC[2J = clear entire screen
#   ESC[H  = move cursor to home (top-left)
.globl _rt_cls
_rt_cls:
    push rbp
    mov rbp, rsp
    lea rdi, [rip + _cls_seq]   # ANSI escape sequence
    xor eax, eax                # no vector args
    call printf
    leave
    ret
