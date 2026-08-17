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
_rng_last:  .quad 0            # last value RND returned, for RND(0)
_cls_seq: .ascii "\033[2J\033[H"
_locate_fmt: .asciz "\033[%d;%dH"
_color_fmt: .asciz "\033[%d;%dm"
_esc_buf: .skip 32
.equ _cls_seq_len, . - _cls_seq

# Zero-filled scratch, so .bss rather than .data -- see data_defs.s. The
# .text below restores the section for the code that follows.
.bss
.p2align 3
_cls_bytes_written: .skip 8

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
    sub rsp, 32                 # shadow space for the call
    call _rt_randomize          # consumes xmm0, leaves the state seeded
    add rsp, 32
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
    shr rax, 12             # Keep top 52 bits
    mov rcx, 0x3FF0000000000000  # IEEE 754: exponent=1023 (value=1.0)
    or rax, rcx             # Combine: value in [1,2)
    movq xmm0, rax

    # Subtract 1.0 to get [0, 1)
    mov rcx, 0x3FF0000000000000
    movq xmm1, rcx
    subsd xmm0, xmm1

    # Remember it, so that RND(0) can hand back the same number.
    movsd QWORD PTR [rip + _rng_last], xmm0

    leave
    ret

# _rt_pos - POS(n): the column the next character will be written to
#
# The tracker counts characters already on the line, and BASIC columns start at
# one, so this is that count plus one.
#
# Arguments: none
# Returns: eax = column (1-based)
.globl _rt_pos
_rt_pos:
    push rbp
    mov rbp, rsp
    lea rax, [rip + _file_col]
    mov rax, QWORD PTR [rax]
    inc rax
    leave
    ret

# _rt_locate - LOCATE row, col: move the cursor
#
# Written as an ANSI escape, the same way _rt_cls clears the screen. The column
# tracker is updated to match, so that a following TAB or PRINT zone counts from
# where the cursor actually is.
#
# Arguments: rcx = row (1-based), rdx = column (1-based)
# Returns: nothing
.globl _rt_locate
_rt_locate:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 48             # shadow space + the 5th WriteFile argument

    mov r12, rdx            # column, kept for the tracker

    # sprintf(_esc_buf, _locate_fmt, row, col)
    mov r8, rcx             # row
    mov r9, rdx             # col
    lea rcx, [rip + _esc_buf]
    lea rdx, [rip + _locate_fmt]
    call sprintf
    mov rbx, rax            # length written

    mov ecx, STD_OUTPUT_HANDLE
    call GetStdHandle
    mov rcx, rax
    lea rdx, [rip + _esc_buf]
    mov r8, rbx
    lea r9, [rip + _cls_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    # The tracker counts characters before the cursor, so column 1 is 0.
    dec r12
    lea rax, [rip + _file_col]
    mov QWORD PTR [rax], r12

    add rsp, 48
    pop r12
    pop rbx
    leave
    ret

# _rt_color - COLOR foreground, background
#
# GW-BASIC numbers 0-15 with 8-15 as the bright half; ANSI splits that into two
# ranges, 30-37 and 90-97 for the foreground and 40-47 and 100-107 for the
# background. Values outside 0-15 are left to the terminal.
#
# Arguments: rcx = foreground, rdx = background
# Returns: nothing
.globl _rt_color
_rt_color:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 48             # shadow space + the 5th WriteFile argument

    # Foreground: 0-7 -> 30-37, 8-15 -> 90-97
    mov rax, rcx
    cmp rax, 8
    jl .Lwcolor_fg_normal
    sub rax, 8
    add rax, 90
    jmp .Lwcolor_fg_done
.Lwcolor_fg_normal:
    add rax, 30
.Lwcolor_fg_done:
    mov r12, rax

    # Background: 0-7 -> 40-47, 8-15 -> 100-107
    mov rax, rdx
    cmp rax, 8
    jl .Lwcolor_bg_normal
    sub rax, 8
    add rax, 100
    jmp .Lwcolor_bg_done
.Lwcolor_bg_normal:
    add rax, 40
.Lwcolor_bg_done:

    # sprintf(_esc_buf, _color_fmt, fg, bg)
    mov r9, rax             # background
    mov r8, r12             # foreground
    lea rcx, [rip + _esc_buf]
    lea rdx, [rip + _color_fmt]
    call sprintf
    mov rbx, rax

    mov ecx, STD_OUTPUT_HANDLE
    call GetStdHandle
    mov rcx, rax
    lea rdx, [rip + _esc_buf]
    mov r8, rbx
    lea r9, [rip + _cls_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    add rsp, 48
    pop r12
    pop rbx
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

    # Home the column tracker too. The cursor is at column 1 after this, and a
    # later TAB that believed the old column emitted a newline to reach a
    # column it had already passed.
    lea rax, [rip + _file_col]
    mov QWORD PTR [rax], 0

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

