# BASIC Runtime: PRINT USING field output (Win64 Native)
#
# _rt_print_using_num and _rt_print_using_str render one field each; the
# format itself is read at run time by _rt_using_next, below.
#
# Flag bits, as _rt_using_next sets them for _rt_print_using_num:
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

# 3.5K of zeros, which is why these are .bss -- see data_defs.s.
.bss
.p2align 3
_using_raw:     .skip 512       # sprintf target
_using_work:    .skip 1024      # after comma insertion
_using_out:     .skip 2048      # after sign/currency, padded to the width

.text

# _rt_print_using_num - Render one numeric field
# Arguments (Win64), the value last so that both ABIs can name it:
#   rcx  = field width in characters
#   rdx  = digits after the decimal point
#   r8   = flags
#   xmm3 = value, the fourth argument by position
#
# A value too wide for its field is printed in full, preceded by '%', which is
# what GW-BASIC does rather than truncating.
.globl _rt_print_using_num
_rt_print_using_num:
    movaps xmm0, xmm3           # the body works on the value in xmm0
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

    # A trailing sign is a column of its own, and a positive number leaves it
    # blank, as GW-BASIC and QuickBASIC do: "###-" prints 5 as "  5 " beside
    # -5's "  5-", so the digits line up. Writing nothing there padded the
    # positive on the left instead, one column out of line.
    test r14, USING_TRAIL_SIGN
    jz .Luse_num_no_trail
    mov al, r15b
    test al, al
    jnz .Luse_num_trail_char
    mov al, ' '
.Luse_num_trail_char:
    mov BYTE PTR [rdi + r9], al
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
    call .Luse_out

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

    # r8, not rdi: rdi is callee-saved on Win64 and volatile on System V, so
    # scratching it here is invisible on the tree this is not compiled into.
    lea r8, [rip + _using_out]
    xor rcx, rcx
.Luse_str_copy:
    cmp rcx, r13
    jae .Luse_str_copied
    cmp rcx, r12
    jae .Luse_str_pad
    mov al, BYTE PTR [rbx + rcx]
    mov BYTE PTR [r8 + rcx], al
    inc rcx
    jmp .Luse_str_copy
.Luse_str_pad:
    mov BYTE PTR [r8 + rcx], ' '
    inc rcx
    jmp .Luse_str_copy
.Luse_str_copied:
    lea rcx, [rip + _using_out]
    mov rdx, r13
    call .Luse_out
    jmp .Luse_str_done

.Luse_str_whole:
    mov rcx, rbx
    mov rdx, r12
    call .Luse_out

.Luse_str_done:
    add rsp, 40
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

# ---------------------------------------------------------------------------
# PRINT USING, interpreted at run time
#
# The format is read here as the statement runs, so it may be any string
# expression and the output may go to a file. It used to be parsed when
# compiling, which allowed only a literal and only the console; one reading
# here, for every format, also means there is no second parser to disagree
# with this one.
#
# _rt_using_start names the file and the format; then each value goes through
# _rt_using_num or _rt_using_str, which print the literal text up to the next
# field and the value in it; _rt_using_end prints the text after the last
# field used, up to the next one. A format runs out before the values does
# what GW-BASIC does: it starts again from the beginning. A format with no
# field at all is Illegal function call, and a value of the wrong kind for its
# field is Type mismatch.
# ---------------------------------------------------------------------------

.bss
.p2align 3
_using_file:  .skip 8           # where the output goes
_using_fmt:   .skip 8           # the format
_using_len:   .skip 8
_using_pos:   .skip 8           # how far through it
_using_found: .skip 8           # a field was taken since the format began
_using_fw:    .skip 8           # the field just found: width,
_using_fd:    .skip 8           # digits after the point,
_using_ff:    .skip 8           # and flags, as _rt_print_using_num takes them

.text

# Write what _rt_print_using_num and _rt_print_using_str laid out to the file
# the statement names. Reached by `call`; returns from _rt_file_print_string.
#   rcx = pointer, rdx = length
.Luse_out:
    mov r8, rdx
    mov rdx, rcx
    mov rcx, QWORD PTR [rip + _using_file]
    jmp _rt_file_print_string

# _rt_using_start - Begin a PRINT USING statement
#
# Arguments: rcx = file number (0 = the console), rdx = format pointer,
#            r8 = format length
# Returns: nothing
.globl _rt_using_start
_rt_using_start:
    mov QWORD PTR [rip + _using_file], rcx
    mov QWORD PTR [rip + _using_fmt], rdx
    mov QWORD PTR [rip + _using_len], r8
    mov QWORD PTR [rip + _using_pos], 0
    mov QWORD PTR [rip + _using_found], 0
    ret

# _rt_using_next - Print literal text up to the next field, and take the field
#
# The fields are GW-BASIC's, as LANGREF lists them: `!`, `&` and `\  \` for a
# string; for a number, digit positions `#` with an optional `.`, `,` among
# the digits, a leading `+`, `**`, `$$` or `**$`, a trailing `^^^^`, and a
# trailing `+` or `-`. `_` makes the next character literal, and anything that
# is not a field is printed as it is.
#
# Arguments: ecx = 1 at the end of the statement: stop before a field instead
#            of taking it, and at the end of the format instead of starting it
#            again
# Returns: rax = 1 a numeric field, 2 a string field, 0 neither; the field's
#          width, digits and flags are in _using_fw, _using_fd, _using_ff
.globl _rt_using_next
_rt_using_next:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 40
    mov r14d, ecx                 # stop mode
    mov rbx, QWORD PTR [rip + _using_fmt]
    mov r12, QWORD PTR [rip + _using_len]
    mov r13, QWORD PTR [rip + _using_pos]

.Lun_loop:
    cmp r13, r12
    jb .Lun_char
    test r14d, r14d
    jnz .Lun_none               # the statement's end: done
    cmp QWORD PTR [rip + _using_found], 0
    je .Lun_no_field            # a whole pass found nothing to fill
    mov QWORD PTR [rip + _using_found], 0
    xor r13d, r13d              # use the format again from its start
    jmp .Lun_loop

.Lun_char:
    movzx eax, BYTE PTR [rbx + r13]
    cmp al, 95                  # '_' makes the next character literal
    jne .Lun_bang
    lea rcx, [r13 + 1]
    cmp rcx, r12
    jae .Lun_literal
    movzx eax, BYTE PTR [rbx + r13 + 1]
    add r13, 2
    jmp .Lun_emit
.Lun_bang:
    cmp al, 33                  # '!': one character
    jne .Lun_amp
    mov r9d, 1
    lea r15, [r13 + 1]
    jmp .Lun_string
.Lun_amp:
    cmp al, 38                  # '&': the whole string
    jne .Lun_backslash
    xor r9d, r9d
    lea r15, [r13 + 1]
    jmp .Lun_string
.Lun_backslash:
    cmp al, 92                  # '\': spaces, then '\'
    jne .Lun_numeric_start
    lea r15, [r13 + 1]
.Lun_bs_scan:
    cmp r15, r12
    jae .Lun_literal
    movzx ecx, BYTE PTR [rbx + r15]
    cmp ecx, 32
    jne .Lun_bs_end
    inc r15
    jmp .Lun_bs_scan
.Lun_bs_end:
    cmp ecx, 92
    jne .Lun_literal
    mov r9, r15
    sub r9, r13
    inc r9                      # the two backslashes and the gap
    inc r15
    cmp r9, USING_MAX_WIDTH
    jbe .Lun_string
    mov r9d, USING_MAX_WIDTH
    jmp .Lun_string

.Lun_numeric_start:
    cmp al, 35                  # '#'
    je .Lun_numeric
    cmp al, 43                  # '+'
    je .Lun_numeric
    cmp al, 36                  # '$'
    je .Lun_numeric
    cmp al, 42                  # '*'
    jne .Lun_literal
.Lun_numeric:
    mov r15, r13                # i
    xor r8d, r8d                # flags
    xor r9d, r9d                # width
    cmp BYTE PTR [rbx + r15], 43
    jne .Lun_n_fill
    or r8d, USING_FORCE_SIGN    # a leading '+'
    inc r9
    inc r15
.Lun_n_fill:
    lea rcx, [r15 + 1]
    cmp rcx, r12
    jae .Lun_n_digits
    cmp BYTE PTR [rbx + r15], 42
    jne .Lun_n_dollar
    cmp BYTE PTR [rbx + r15 + 1], 42
    jne .Lun_n_dollar
    or r8d, USING_STAR          # '**', and '**$'
    add r9, 2
    add r15, 2
    cmp r15, r12
    jae .Lun_n_digits
    cmp BYTE PTR [rbx + r15], 36
    jne .Lun_n_digits
    or r8d, USING_DOLLAR
    inc r9
    inc r15
    jmp .Lun_n_digits
.Lun_n_dollar:
    cmp BYTE PTR [rbx + r15], 36
    jne .Lun_n_digits
    cmp BYTE PTR [rbx + r15 + 1], 36
    jne .Lun_n_digits
    or r8d, USING_DOLLAR        # '$$': a sign and a digit
    add r9, 2
    add r15, 2
.Lun_n_digits:
    xor r10d, r10d              # digit positions before the point
    xor edx, edx                # 1 when there is a ','
.Lun_n_digit:
    cmp r15, r12
    jae .Lun_n_digits_end
    movzx eax, BYTE PTR [rbx + r15]
    cmp al, 35
    jne .Lun_n_comma
    inc r10
    inc r9
    inc r15
    jmp .Lun_n_digit
.Lun_n_comma:
    cmp al, 44
    jne .Lun_n_digits_end
    or r8d, USING_COMMA
    mov edx, 1
    inc r15
    jmp .Lun_n_digit
.Lun_n_digits_end:
    # A comma takes no position itself; the field grows by the separators
    # its digits will need.
    test edx, edx
    jz .Lun_n_grouped
    mov rax, r10
    test rax, rax
    jz .Lun_n_grouped
    dec rax
    xor edx, edx
    mov ecx, 3
    div rcx
    add r9, rax
.Lun_n_grouped:
    test r10, r10
    jnz .Lun_n_point
    test r9, r9
    jz .Lun_n_fail              # nothing here formed a field
.Lun_n_point:
    xor r11d, r11d              # digits after the point
    cmp r15, r12
    jae .Lun_n_exp
    cmp BYTE PTR [rbx + r15], 46
    jne .Lun_n_exp
    lea rcx, [r15 + 1]
    xor eax, eax
.Lun_n_frac:
    cmp rcx, r12
    jae .Lun_n_frac_end
    cmp BYTE PTR [rbx + rcx], 35
    jne .Lun_n_frac_end
    inc eax
    inc rcx
    jmp .Lun_n_frac
.Lun_n_frac_end:
    test eax, eax
    jz .Lun_n_exp               # a point with no digits is not part of it
    mov r11d, eax
    cmp r11, USING_MAX_DEC
    jbe .Lun_n_frac_ok
    mov r11d, USING_MAX_DEC
.Lun_n_frac_ok:
    lea r9, [r9 + r11 + 1]      # the point and its digits
    mov r15, rcx
.Lun_n_exp:
    test r10, r10
    jnz .Lun_n_exp_test
    test r11, r11
    jz .Lun_n_fail              # a field needs a digit somewhere
.Lun_n_exp_test:
    lea rcx, [r15 + 3]
    cmp rcx, r12
    jae .Lun_n_sign
    cmp DWORD PTR [rbx + r15], 0x5E5E5E5E   # "^^^^"
    jne .Lun_n_sign
    or r8d, USING_EXP
    add r9, 4
    add r15, 4
.Lun_n_sign:
    cmp r15, r12
    jae .Lun_n_done
    movzx eax, BYTE PTR [rbx + r15]
    cmp al, 45                  # a trailing '-'
    je .Lun_n_trail
    cmp al, 43                  # or '+'
    jne .Lun_n_done
    or r8d, USING_FORCE_SIGN
.Lun_n_trail:
    or r8d, USING_TRAIL_SIGN
    inc r9
    inc r15
.Lun_n_done:
    cmp r9, USING_MAX_WIDTH
    jbe .Lun_n_store
    mov r9d, USING_MAX_WIDTH
.Lun_n_store:
    test r14d, r14d
    jnz .Lun_none
    mov QWORD PTR [rip + _using_fw], r9
    mov QWORD PTR [rip + _using_fd], r11
    mov QWORD PTR [rip + _using_ff], r8
    mov r13, r15
    mov QWORD PTR [rip + _using_found], 1
    mov eax, 1
    jmp .Lun_ret
.Lun_n_fail:                    # not a field after all: the character is text
.Lun_literal:
    movzx eax, BYTE PTR [rbx + r13]
    inc r13
.Lun_emit:
    mov rcx, QWORD PTR [rip + _using_file]
    mov edx, eax
    call _rt_file_print_char
    jmp .Lun_loop

.Lun_string:                    # r9 = width (0 = the whole string), r15 = past it
    test r14d, r14d
    jnz .Lun_none
    mov QWORD PTR [rip + _using_fw], r9
    mov r13, r15
    mov QWORD PTR [rip + _using_found], 1
    mov eax, 2
    jmp .Lun_ret

.Lun_no_field:
    lea rcx, [rip + _err_domain]
    xor rdx, rdx
    call _rt_error              # returns only when trapped
.Lun_none:
    xor eax, eax
.Lun_ret:
    mov QWORD PTR [rip + _using_pos], r13
    add rsp, 40
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

# _rt_using_num - PRINT USING: a numeric value
#
# Arguments: xmm0 = the value
# Returns: nothing
.globl _rt_using_num
_rt_using_num:
    push rbp
    mov rbp, rsp
    sub rsp, 48
    movsd QWORD PTR [rbp - 8], xmm0
    xor ecx, ecx
    call _rt_using_next
    cmp eax, 1
    je .Luse_num_render
    test eax, eax
    jz .Luse_num_done           # an error was raised and trapped
    lea rcx, [rip + _err_typemismatch]
    xor rdx, rdx
    call _rt_error              # a string field: returns only when trapped
    jmp .Luse_num_done
.Luse_num_render:
    mov rcx, QWORD PTR [rip + _using_fw]
    mov rdx, QWORD PTR [rip + _using_fd]
    mov r8, QWORD PTR [rip + _using_ff]
    movsd xmm3, QWORD PTR [rbp - 8]     # the 4th argument's register
    call _rt_print_using_num
.Luse_num_done:
    leave
    ret

# _rt_using_str - PRINT USING: a string value
#
# Arguments: rcx = pointer, rdx = length
# Returns: nothing
.globl _rt_using_str
_rt_using_str:
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov QWORD PTR [rbp - 8], rcx
    mov QWORD PTR [rbp - 16], rdx
    xor ecx, ecx
    call _rt_using_next
    cmp eax, 2
    je .Luse_str_render
    test eax, eax
    jz .Luse_str_value_done     # an error was raised and trapped
    lea rcx, [rip + _err_typemismatch]
    xor rdx, rdx
    call _rt_error              # a numeric field: returns only when trapped
    jmp .Luse_str_value_done
.Luse_str_render:
    mov rcx, QWORD PTR [rbp - 8]
    mov rdx, QWORD PTR [rbp - 16]
    mov r8, QWORD PTR [rip + _using_fw]
    call _rt_print_using_str
.Luse_str_value_done:
    leave
    ret

# _rt_using_end - Finish a PRINT USING statement: the text after the last
# field used, up to the next one
#
# Arguments: none
# Returns: nothing
.globl _rt_using_end
_rt_using_end:
    push rbp
    mov rbp, rsp
    sub rsp, 32
    mov ecx, 1
    call _rt_using_next
    leave
    ret
