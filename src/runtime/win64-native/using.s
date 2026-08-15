# BASIC Runtime: PRINT USING field output (Win64 Native)
#
# The format string itself is parsed at compile time, so the runtime only has
# to render one field at a time. That keeps the format-parsing logic in Rust,
# where it is unit-tested, and leaves just these two helpers to be written per
# platform.
#
# Flag bits (must match src/using.rs):
#   1  COMMA          group the integer part in thousands
#   2  DOLLAR         prefix a currency sign, floated against the digits
#   4  STAR           fill with '*' rather than spaces
#   8  TRAILING_SIGN  place the sign after the number
#   16 FORCE_SIGN     always show a sign, '+' for positives
#   32 EXPONENTIAL    render in exponential form

.equ USING_COMMA,      1
.equ USING_DOLLAR,     2
.equ USING_STAR,       4
.equ USING_TRAIL_SIGN, 8
.equ USING_FORCE_SIGN, 16
.equ USING_EXP,        32

.data
_using_fmt_f:   .asciz "%.*f"
_using_fmt_e:   .asciz "%.*E"
# Buffer sizes are proved, not guessed. A double's widest %f rendering is a
# sign, 309 integer digits, a point and USING_MAX_DEC fractional digits -- 351
# characters -- so 512 bytes cannot be overrun at any clamped precision.
# Comma grouping adds at most one separator per three integer digits, and the
# decorations add a sign, a currency symbol and an overflow marker.
.equ USING_MAX_WIDTH, 255       # must match using::MAX_WIDTH
.equ USING_MAX_DEC,   40        # must match using::MAX_DECIMALS

_using_raw:     .skip 512       # sprintf target
_using_work:    .skip 1024      # after comma insertion
_using_out:     .skip 2048      # after sign/currency, padded to the width

.text

# _rt_print_using_num - Render one numeric field
# Arguments (Win64):
#   xmm0 = value
#   rcx  = field width in characters
#   rdx  = digits after the decimal point
#   r8   = flags
#
# A value too wide for its field is printed in full, preceded by '%', which is
# what GW-BASIC does rather than truncating.
.globl _rt_print_using_num
_rt_print_using_num:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    push rdi
    push r12
    push r13
    push r14
    push r15
    sub rsp, 56                 # shadow space + locals, keeps rsp aligned

    mov r12, rcx                # width
    mov r13, rdx                # decimals
    mov r14, r8                 # flags
    movsd QWORD PTR [rbp - 72], xmm0

    # Clamp both to what the buffers below are sized for. The compiler already
    # clamps them, so this only guards against a hand-written or future caller;
    # without it a value such as 1D300 in a "##.##" field ran sprintf past the
    # end of _using_raw and destroyed everything after it in .data.
    cmp r13, USING_MAX_DEC
    jbe .Luse_num_dec_ok
    mov r13, USING_MAX_DEC
.Luse_num_dec_ok:
    cmp r12, USING_MAX_WIDTH
    jbe .Luse_num_width_ok
    mov r12, USING_MAX_WIDTH
.Luse_num_width_ok:

    xor r15d, r15d              # sign character, 0 = none
    mov rax, r14
    and rax, USING_TRAIL_SIGN | USING_FORCE_SIGN
    test rax, rax
    jz .Luse_num_format

    xorpd xmm1, xmm1
    movsd xmm0, QWORD PTR [rbp - 72]
    ucomisd xmm0, xmm1
    jae .Luse_num_positive
    mov r15d, '-'
    mov rax, 0x8000000000000000
    movq xmm1, rax
    xorpd xmm0, xmm1
    movsd QWORD PTR [rbp - 72], xmm0
    jmp .Luse_num_format
.Luse_num_positive:
    test r14, USING_FORCE_SIGN
    jz .Luse_num_format
    mov r15d, '+'

.Luse_num_format:
    # sprintf(_using_raw, fmt, decimals, value)
    lea rcx, [rip + _using_raw]
    lea rdx, [rip + _using_fmt_f]
    test r14, USING_EXP
    jz .Luse_num_have_fmt
    lea rdx, [rip + _using_fmt_e]
.Luse_num_have_fmt:
    mov r8, r13                 # precision
    movsd xmm3, QWORD PTR [rbp - 72]
    movq r9, xmm3               # 4th vararg goes in both xmm3 and r9
    call sprintf
    mov rbx, rax                # length of the raw text

    lea rsi, [rip + _using_raw]
    lea rdi, [rip + _using_work]
    test r14, USING_COMMA
    jnz .Luse_num_commas
    xor rcx, rcx
.Luse_num_copy:
    cmp rcx, rbx
    jae .Luse_num_copied
    mov al, BYTE PTR [rsi + rcx]
    mov BYTE PTR [rdi + rcx], al
    inc rcx
    jmp .Luse_num_copy
.Luse_num_copied:
    mov r8, rbx
    jmp .Luse_num_decorate

.Luse_num_commas:
    # Locate the end of the integer part: the '.' or 'E', else the whole text.
    xor rcx, rcx
.Luse_num_find_dot:
    cmp rcx, rbx
    jae .Luse_num_dot_found
    mov al, BYTE PTR [rsi + rcx]
    cmp al, '.'
    je .Luse_num_dot_found
    cmp al, 'E'
    je .Luse_num_dot_found
    inc rcx
    jmp .Luse_num_find_dot
.Luse_num_dot_found:
    mov r9, rcx                 # r9 = one past the last integer digit

    # Skip any leading sign so it is not counted as a digit.
    xor rcx, rcx
    cmp rbx, 0
    je .Luse_num_no_sign_skip
    mov al, BYTE PTR [rsi]
    cmp al, '-'
    je .Luse_num_skip_one
    cmp al, '+'
    jne .Luse_num_no_sign_skip
.Luse_num_skip_one:
    mov rcx, 1
.Luse_num_no_sign_skip:
    mov r10, rcx                # r10 = index of the first integer digit

    # Single left-to-right pass: a separator goes before digit i whenever the
    # number of digits remaining, (r9 - i), is a positive multiple of 3.
    xor r8, r8                  # output length
    xor rcx, rcx
.Luse_num_ins:
    cmp rcx, rbx
    jae .Luse_num_ins_done
    cmp rcx, r9
    jae .Luse_num_ins_copy         # past the integer part: copy verbatim
    cmp rcx, r10
    jbe .Luse_num_ins_copy         # first digit (or sign) never gets a separator
    mov rdx, r9
    sub rdx, rcx                # digits remaining before the decimal point
    mov rax, rdx
    xor rdx, rdx
    mov r11, 3
    div r11                     # rdx = (r9 - rcx) mod 3
    test rdx, rdx
    jnz .Luse_num_ins_copy
    lea rdx, [rip + _using_work]
    mov BYTE PTR [rdx + r8], ','
    inc r8
.Luse_num_ins_copy:
    lea rdx, [rip + _using_work]
    mov al, BYTE PTR [rsi + rcx]
    mov BYTE PTR [rdx + r8], al
    inc r8
    inc rcx
    jmp .Luse_num_ins
.Luse_num_ins_done:

.Luse_num_decorate:
    # r8 = length in _using_work. Prepend '$' and/or the leading sign.
    lea rdi, [rip + _using_out]
    xor r9, r9                  # output length

    test r15b, r15b
    jz .Luse_num_no_lead_sign
    test r14, USING_TRAIL_SIGN
    jnz .Luse_num_no_lead_sign
    mov BYTE PTR [rdi + r9], r15b
    inc r9
.Luse_num_no_lead_sign:

    test r14, USING_DOLLAR
    jz .Luse_num_no_dollar
    mov BYTE PTR [rdi + r9], '$'
    inc r9
.Luse_num_no_dollar:

    lea rsi, [rip + _using_work]
    xor rcx, rcx
.Luse_num_body:
    cmp rcx, r8
    jae .Luse_num_body_done
    mov al, BYTE PTR [rsi + rcx]
    mov BYTE PTR [rdi + r9], al
    inc r9
    inc rcx
    jmp .Luse_num_body
.Luse_num_body_done:

    test r14, USING_TRAIL_SIGN
    jz .Luse_num_no_trail
    test r15b, r15b
    jz .Luse_num_no_trail
    mov BYTE PTR [rdi + r9], r15b
    inc r9
.Luse_num_no_trail:

    # Pad on the left to the field width, or mark an overflow with '%'.
    cmp r9, r12
    jae .Luse_num_overflow

    mov rcx, r12
    sub rcx, r9                 # pad count
    mov al, ' '
    test r14, USING_STAR
    jz .Luse_num_pad_char
    mov al, '*'
.Luse_num_pad_char:
    # Shift the text right by rcx, then fill the gap. x86 addressing has no
    # three-register form, so the shifted destination base is precomputed.
    lea r8, [rdi + rcx]
    mov rdx, r9
.Luse_num_shift:
    cmp rdx, 0
    jle .Luse_num_shift_done
    dec rdx
    mov r11b, BYTE PTR [rdi + rdx]
    mov BYTE PTR [r8 + rdx], r11b
    jmp .Luse_num_shift
.Luse_num_shift_done:
    xor rdx, rdx
.Luse_num_fill:
    cmp rdx, rcx
    jae .Luse_num_fill_done
    mov BYTE PTR [rdi + rdx], al
    inc rdx
    jmp .Luse_num_fill
.Luse_num_fill_done:
    mov r9, r12
    jmp .Luse_num_write

.Luse_num_overflow:
    je .Luse_num_write              # exactly the width: no marker needed
    # Too wide: shift right by one and write '%' in front.
    mov rdx, r9
.Luse_num_ovf_shift:
    cmp rdx, 0
    jle .Luse_num_ovf_done
    dec rdx
    mov r11b, BYTE PTR [rdi + rdx]
    mov BYTE PTR [rdi + rdx + 1], r11b
    jmp .Luse_num_ovf_shift
.Luse_num_ovf_done:
    mov BYTE PTR [rdi], '%'
    inc r9

.Luse_num_write:
    lea rcx, [rip + _using_out]
    mov rdx, r9
    call _rt_con_string

    add rsp, 56
    pop r15
    pop r14
    pop r13
    pop r12
    pop rdi
    pop rsi
    pop rbx
    pop rbp
    ret

# _rt_print_using_str - Render one string field
# Arguments (Win64):
#   rcx = string pointer
#   rdx = string length
#   r8  = field width, or 0 for "the whole string"
#
# A string shorter than the field is padded on the right with spaces; a longer
# one is truncated, as GW-BASIC does.
.globl _rt_print_using_str
_rt_print_using_str:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    sub rsp, 40

    mov rbx, rcx                # ptr
    mov r12, rdx                # len
    mov r13, r8                 # width

    # Clamp to the buffer's capacity, as in _rt_print_using_num.
    cmp r13, USING_MAX_WIDTH
    jbe .Luse_str_width_ok
    mov r13, USING_MAX_WIDTH
.Luse_str_width_ok:

    test r13, r13
    jz .Luse_str_whole

    lea rdi, [rip + _using_out]
    xor rcx, rcx
.Luse_str_copy:
    cmp rcx, r13
    jae .Luse_str_copied
    cmp rcx, r12
    jae .Luse_str_pad
    mov al, BYTE PTR [rbx + rcx]
    mov BYTE PTR [rdi + rcx], al
    inc rcx
    jmp .Luse_str_copy
.Luse_str_pad:
    mov BYTE PTR [rdi + rcx], ' '
    inc rcx
    jmp .Luse_str_copy
.Luse_str_copied:
    lea rcx, [rip + _using_out]
    mov rdx, r13
    call _rt_con_string
    jmp .Luse_str_done

.Luse_str_whole:
    mov rcx, rbx
    mov rdx, r12
    call _rt_con_string

.Luse_str_done:
    add rsp, 40
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
