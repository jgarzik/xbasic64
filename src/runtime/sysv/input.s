# BASIC Runtime: Console Input (INPUT, LINE INPUT)
#
# INPUT reads a whole line and splits it on commas, as GW-BASIC and QuickBASIC
# do: a field may be quoted, which lets it hold a comma; an unquoted one has
# its surrounding blanks dropped. Every field is checked against the variable
# it is for before any is stored: a number must be one and nothing else (read
# by _rt_scan_number, as VAL reads one) and must fit the variable's type; a
# quoted field is a string only. A line that fails any of that, or has too few
# fields, prints "?Redo from start" and asks again -- prompt and all. More
# fields than variables print "?Extra ignored". At the end of the input every
# variable reads as empty, rather than the statement waiting forever.
#
# This replaced a reader per variable: scanf("%lf") for a number, which left
# what followed the number for the next INPUT, and the whole raw line for a
# string, so `INPUT A$, B$` read two lines and a comma meant nothing.

# String Return Convention:
#   Strings are returned as (pointer, length) pairs:
#   - rax = pointer to string data
#   - rdx = length in bytes

.equ MAX_INPUT_FIELDS, 64       # sema refuses an INPUT with more variables

.data
_in_redo:  .asciz "?Redo from start"
_in_extra: .asciz "?Extra ignored"

.bss
.p2align 3
_input_fptr: .skip 8 * MAX_INPUT_FIELDS     # where each field starts
_input_flen: .skip 8 * MAX_INPUT_FIELDS     # and its length
_input_vals: .skip 8 * MAX_INPUT_FIELDS     # a numeric field's value

.text

# _rt_con_read_line - Read one line from stdin into _input_buf
#
# The newline is consumed and not kept, as is a CR before it. A line longer
# than the buffer keeps its first 1023 characters. The console's column is 0
# afterwards: the newline the user typed put the cursor there.
#
# Arguments: none
# Returns:
#   rax = length (the text is also NUL-terminated)
#   edx = 1 when the input had ended and nothing was read, else 0
.globl _rt_con_read_line
_rt_con_read_line:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    xor ebx, ebx            # length
    xor r12d, r12d          # 1 once anything, even a newline, was read
.Lrl_loop:
    call getchar
    cmp eax, -1
    je .Lrl_end_input
    mov r12d, 1
    cmp eax, 10
    je .Lrl_done
    cmp eax, 13
    je .Lrl_loop            # a CR before the newline is not text
    cmp rbx, 1023
    jae .Lrl_loop           # past the buffer: dropped
    lea rcx, [rip + _input_buf]
    mov BYTE PTR [rcx + rbx], al
    inc rbx
    jmp .Lrl_loop
.Lrl_end_input:
    xor edx, edx
    test r12d, r12d
    jnz .Lrl_finish         # a last line without a newline still counts
    mov edx, 1
    jmp .Lrl_finish
.Lrl_done:
    xor edx, edx
.Lrl_finish:
    lea rcx, [rip + _input_buf]
    mov BYTE PTR [rcx + rbx], 0
    lea rcx, [rip + _file_col]
    mov QWORD PTR [rcx], 0
    mov rax, rbx
    pop r12
    pop rbx
    leave
    ret

# _rt_input_string - LINE INPUT: one whole line, as typed
#
# Arguments: none
# Returns:
#   rax = pointer to the line (in _input_buf, valid until the next read)
#   rdx = its length; 0 at the end of the input
.globl _rt_input_string
_rt_input_string:
    push rbp
    mov rbp, rsp
    call _rt_con_read_line
    mov rdx, rax
    lea rax, [rip + _input_buf]
    leave
    ret

# _rt_input_line - INPUT: read and check one line for a whole statement
#
# Afterwards _rt_input_field_num and _rt_input_field_str hand out the values.
#
# Arguments:
#   rdi = prompt pointer, rsi = prompt length (0 for none), already including
#         the "? " when the statement asks for one
#   rdx = the variables' types, one character each: '$' string, 'I' INTEGER,
#         'L' LONG, 'S' SINGLE, 'D' DOUBLE
#   rcx = how many variables there are
# Returns: nothing
.globl _rt_input_line
_rt_input_line:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 40             # [rbp-48] prompt, [rbp-56] its length,
                            # [rbp-64] line length, [rbp-72] field quoted,
                            # [rbp-80] field length
    mov QWORD PTR [rbp - 48], rdi
    mov QWORD PTR [rbp - 56], rsi
    mov r12, rdx            # types
    mov r13, rcx            # how many

.Lin_ask:
    mov rdx, QWORD PTR [rbp - 56]
    test rdx, rdx
    jz .Lin_read
    xor edi, edi            # the console
    mov rsi, QWORD PTR [rbp - 48]
    call _rt_file_print_string
.Lin_read:
    call _rt_con_read_line
    mov QWORD PTR [rbp - 64], rax
    test edx, edx
    jnz .Lin_end_input
    xor r14d, r14d          # which field
    xor r15d, r15d          # where in the line
    lea rbx, [rip + _input_buf]

.Lin_field:
    # Leading blanks are not part of a field.
    cmp r15, QWORD PTR [rbp - 64]
    jae .Lin_unquoted
    movzx eax, BYTE PTR [rbx + r15]
    cmp al, 32
    je .Lin_skip
    cmp al, 9
    je .Lin_skip
    cmp al, 34              # '"'
    jne .Lin_unquoted

    # A quoted field runs to the closing quote; only blanks may follow it.
    inc r15
    lea rax, [rbx + r15]
    lea rcx, [rip + _input_fptr]
    mov QWORD PTR [rcx + r14*8], rax
    mov rcx, r15            # where it starts
.Lin_quoted:
    cmp r15, QWORD PTR [rbp - 64]
    jae .Lin_quoted_end     # no closing quote: the rest of the line
    cmp BYTE PTR [rbx + r15], 34
    je .Lin_quoted_end
    inc r15
    jmp .Lin_quoted
.Lin_quoted_end:
    mov rax, r15
    sub rax, rcx
    lea rcx, [rip + _input_flen]
    mov QWORD PTR [rcx + r14*8], rax
    cmp r15, QWORD PTR [rbp - 64]
    jae .Lin_quoted_flag
    inc r15                 # past the closing quote
.Lin_quoted_after:
    cmp r15, QWORD PTR [rbp - 64]
    jae .Lin_quoted_flag
    movzx eax, BYTE PTR [rbx + r15]
    cmp al, 44              # ','
    je .Lin_quoted_flag
    cmp al, 32
    je .Lin_quoted_blank
    cmp al, 9
    jne .Lin_redo           # text after a quoted field
.Lin_quoted_blank:
    inc r15
    jmp .Lin_quoted_after
.Lin_quoted_flag:
    mov QWORD PTR [rbp - 72], 1
    jmp .Lin_check
.Lin_skip:
    inc r15
    jmp .Lin_field

.Lin_unquoted:
    # An unquoted field runs to the next comma, less its trailing blanks.
    mov QWORD PTR [rbp - 72], 0
    lea rax, [rbx + r15]
    lea rcx, [rip + _input_fptr]
    mov QWORD PTR [rcx + r14*8], rax
    mov rcx, r15
.Lin_unq_scan:
    cmp r15, QWORD PTR [rbp - 64]
    jae .Lin_unq_end
    cmp BYTE PTR [rbx + r15], 44
    je .Lin_unq_end
    inc r15
    jmp .Lin_unq_scan
.Lin_unq_end:
    mov rdx, r15
.Lin_trim:
    cmp rdx, rcx
    jbe .Lin_trimmed
    movzx eax, BYTE PTR [rbx + rdx - 1]
    cmp al, 32
    je .Lin_trim_one
    cmp al, 9
    jne .Lin_trimmed
.Lin_trim_one:
    dec rdx
    jmp .Lin_trim
.Lin_trimmed:
    sub rdx, rcx
    lea rax, [rip + _input_flen]
    mov QWORD PTR [rax + r14*8], rdx

.Lin_check:
    movzx eax, BYTE PTR [r12 + r14]
    cmp al, 36              # '$': any text will do
    je .Lin_next
    cmp QWORD PTR [rbp - 72], 0
    jne .Lin_redo           # a quoted field is not a number
    lea rax, [rip + _input_flen]
    mov rsi, QWORD PTR [rax + r14*8]
    test rsi, rsi
    jz .Lin_zero            # an empty field is 0
    mov QWORD PTR [rbp - 80], rsi
    lea rax, [rip + _input_fptr]
    mov rdi, QWORD PTR [rax + r14*8]
    call _rt_scan_number
    test edx, edx
    jnz .Lin_redo           # too large for a DOUBLE
    cmp rax, QWORD PTR [rbp - 80]
    jne .Lin_redo           # not a number, or not only one
    # It must fit the variable, rounded as an assignment rounds.
    movzx eax, BYTE PTR [r12 + r14]
    cmp al, 73              # 'I'
    je .Lin_fit_integer
    cmp al, 76              # 'L'
    je .Lin_fit_long
    cmp al, 83              # 'S'
    je .Lin_fit_single
    jmp .Lin_store
.Lin_fit_integer:
    cvtsd2si rax, xmm0
    movsx rcx, ax
    cmp rcx, rax
    jne .Lin_redo
    jmp .Lin_store
.Lin_fit_long:
    cvtsd2si rax, xmm0
    movsxd rcx, eax
    cmp rcx, rax
    jne .Lin_redo
    jmp .Lin_store
.Lin_fit_single:
    cvtsd2ss xmm1, xmm0
    movd eax, xmm1
    shr eax, 23
    and eax, 0xFF
    cmp eax, 0xFF
    je .Lin_redo
    jmp .Lin_store
.Lin_zero:
    xorpd xmm0, xmm0
.Lin_store:
    lea rax, [rip + _input_vals]
    movsd QWORD PTR [rax + r14*8], xmm0

.Lin_next:
    inc r14
    cmp r15, QWORD PTR [rbp - 64]
    jae .Lin_at_end
    inc r15                 # past the comma
    cmp r14, r13
    jb .Lin_field
    # Every variable has its value and the line goes on.
    lea rdi, [rip + _in_extra]
    call .Lin_say
    jmp .Lin_done
.Lin_at_end:
    cmp r14, r13
    jb .Lin_redo            # too few fields
    jmp .Lin_done

.Lin_redo:
    lea rdi, [rip + _in_redo]
    call .Lin_say
    jmp .Lin_ask

.Lin_end_input:
    # Nothing more to read: every variable gets an empty value.
    xor r14d, r14d
.Lin_empty:
    cmp r14, r13
    jae .Lin_done
    lea rax, [rip + _input_flen]
    mov QWORD PTR [rax + r14*8], 0
    lea rax, [rip + _input_vals]
    mov QWORD PTR [rax + r14*8], 0
    inc r14
    jmp .Lin_empty

.Lin_done:
    add rsp, 40
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# Write the message at rdi on a line of its own. Reached by `call` from
# _rt_input_line's aligned body.
.Lin_say:
    push rbx
    mov rbx, rdi
    call strlen
    mov rdx, rax
    mov rsi, rbx
    xor edi, edi
    call _rt_file_print_string
    xor edi, edi
    call _rt_file_print_newline
    pop rbx
    ret

# _rt_input_field_num - The value INPUT read for numeric variable `k`
# Arguments: rdi = k     Returns: xmm0 = the value
.globl _rt_input_field_num
_rt_input_field_num:
    lea rax, [rip + _input_vals]
    movsd xmm0, QWORD PTR [rax + rdi*8]
    ret

# _rt_input_field_str - The text INPUT read for string variable `k`
#
# A copy, since the line buffer is reused by the next read.
#
# Arguments: rdi = k     Returns: rax = pointer, rdx = length
.globl _rt_input_field_str
_rt_input_field_str:
    lea rax, [rip + _input_flen]
    mov rsi, QWORD PTR [rax + rdi*8]
    test rsi, rsi
    jz .Lin_field_empty
    lea rax, [rip + _input_fptr]
    mov rdi, QWORD PTR [rax + rdi*8]
    jmp _rt_strdup
.Lin_field_empty:
    xor eax, eax
    xor edx, edx
    ret
