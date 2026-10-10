# BASIC Runtime: Print Functions (Win64 Native - Pure Win32 API)
#
# Output functions using Win32 API (WriteFile) instead of libc printf.
# Uses UCRT sprintf for number formatting.
#
# Win64 ABI:
#   - Integer args: rcx, rdx, r8, r9 (then stack)
#   - 32-byte shadow space required before every call
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#

# Win32 API Constants
.equ STD_OUTPUT_HANDLE, -11
.equ ENABLE_VIRTUAL_TERMINAL_PROCESSING, 4

# I/O size constants
.equ SINGLE_BYTE, 1
.equ CRLF_LEN, 2


.data
_newline_str: .ascii "\r\n"      # Windows uses CRLF

# Zero-filled scratch, so .bss rather than .data -- see data_defs.s. The
# .text below restores the section for the code that follows; a .bss left
# open at the end of this file would swallow the next runtime part.
.bss
.p2align 3
_print_buffer: .skip 64          # Buffer for number formatting
_bytes_written: .skip 8          # For WriteFile output parameter

.text

# _rt_platform_init - Acquire the console handles (call once at startup)
# The console is file handle 0, so every print helper reaches it the same way
# it reaches an OPENed file. Windows has to ask the OS for the handle first;
# System V finds its standard output stream already open.
#
# Arguments: none      Returns: nothing
.globl _rt_platform_init
_rt_platform_init:
    push rbp
    mov rbp, rsp
    sub rsp, 32

    # GetStdHandle(STD_OUTPUT_HANDLE) -> _file_handles[0]
    mov ecx, STD_OUTPUT_HANDLE
    call GetStdHandle
    lea rcx, [rip + _file_handles]
    mov [rcx], rax
    lea rcx, [rip + _file_mode]
    mov QWORD PTR [rcx], 1          # the console is written to

    # Turn on VT processing, so the escape sequences CLS, LOCATE and COLOR
    # write are acted on rather than printed literally. CLS has emitted them
    # since it was written and simply assumed this was already set; on a
    # console where it is not, it printed "^[[2J^[[H" and cleared nothing.
    #
    # Best effort: a failure here means the handle is not a console -- output
    # redirected to a file, say -- and the escapes are then just bytes in the
    # file, which is what any terminal program does.
    mov rcx, [rcx]                      # the handle just stored
    lea rdx, [rip + _console_mode]
    call GetConsoleMode
    test eax, eax
    jz .Lplatform_no_console
    lea rax, [rip + _console_mode]
    mov ecx, DWORD PTR [rax]
    or ecx, ENABLE_VIRTUAL_TERMINAL_PROCESSING
    mov edx, ecx
    lea rcx, [rip + _file_handles]
    mov rcx, [rcx]
    call SetConsoleMode
.Lplatform_no_console:

    call _rt_init_input

    leave
    ret


# _rt_fmt_double - Format a number into _num_buf
# Shared by console and file output so both render numbers identically.
#
# GW-BASIC's rule, which QuickBASIC documents: a value is rounded to the
# type's significant digits -- 7 for a SINGLE, 16 for a DOUBLE -- and written
# in fixed point when that shows it in at most that many digits, otherwise
# with an exponent. So a SINGLE's 1.1E-6 prints .0000011 and 1.1E-7 prints
# 1.1E-07; 1234567 prints in full and 12345678 as 1.234568E+07. Trailing zeros
# are dropped, a value below one has no leading zero, and the exponent is
# spelled E for a SINGLE and D for a DOUBLE, with at least two digits.
#
# This used to print the shortest decimal that read back unchanged, so a
# DOUBLE showed 17 digits (.1 + .2 printed .30000000000000004), a SINGLE up to
# 9, and any whole number below 2^63 in full whatever its type.
#
# Arguments:
#   xmm0 = value (a SINGLE arrives already widened to double)
#   ecx  = nonzero for a SINGLE
#
# Returns:
#   rax = length of the text in _num_buf
.globl _rt_fmt_double
_rt_fmt_double:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    sub rsp, 32             # shadow space; rsp is 16-byte aligned
    mov r12d, ecx           # nonzero for a SINGLE
    mov r13d, 16            # significant digits, P
    test r12d, r12d
    jz .Lfd_digits
    mov r13d, 7
.Lfd_digits:

    # Zero, of either sign, is 0.
    xorpd xmm1, xmm1
    ucomisd xmm0, xmm1
    jp .Lfd_nonzero
    jne .Lfd_nonzero
    lea rcx, [rip + _num_buf]
    mov WORD PTR [rcx], 48              # "0", NUL
    mov eax, 1
    jmp .Lfd_ret
.Lfd_nonzero:
    # sprintf(_fmt_scratch, "%.*e", P - 1, value): a variadic double goes in
    # its integer register as well as its vector one.
    lea rcx, [rip + _fmt_scratch]
    lea rdx, [rip + _fmt_sci]
    lea r8d, [r13 - 1]
    movq r9, xmm0
    movapd xmm3, xmm0
    call sprintf
    lea rbx, [rip + _fmt_scratch]       # read cursor

    # The text is "[-]d.ddd...e[+-]xx": P significant digits, already rounded
    # by printf. From here on no call is made, and only registers volatile in
    # both ABIs are used (plus rbx, r12-r14, saved above), so this body is
    # identical in both trees.
    lea r14, [rip + _num_buf]           # write cursor
    cmp BYTE PTR [rbx], 45              # '-'
    jne .Lfd_unsigned
    mov BYTE PTR [r14], 45
    inc rbx
    inc r14
.Lfd_unsigned:
    movzx eax, BYTE PTR [rbx]
    sub eax, 48
    cmp eax, 9
    ja .Lfd_raw                         # inf or nan: as the C library wrote it

    # The P digits, without the point.
    lea r8, [rip + _fmt_digits]
    mov al, BYTE PTR [rbx]
    mov BYTE PTR [r8], al
    mov ecx, 1
    lea r9, [rbx + 2]                   # past the leading digit and the point
.Lfd_gather:
    cmp ecx, r13d
    jge .Lfd_gathered
    mov al, BYTE PTR [r9]
    mov BYTE PTR [r8 + rcx], al
    inc r9
    inc ecx
    jmp .Lfd_gather
.Lfd_gathered:
    # The decimal exponent X, so the value is d.ddd * 10^X.
    inc r9                              # past 'e'
    mov r10b, BYTE PTR [r9]             # its sign
    inc r9
    xor edx, edx
.Lfd_expdig:
    movzx eax, BYTE PTR [r9]
    test eax, eax
    jz .Lfd_expdone
    sub eax, 48
    imul edx, edx, 10
    add edx, eax
    inc r9
    jmp .Lfd_expdig
.Lfd_expdone:
    cmp r10b, 45                        # '-'
    jne .Lfd_exppos
    neg edx
.Lfd_exppos:

    # n = how many of them are significant: trailing zeros are not.
    mov ecx, r13d
.Lfd_trim:
    cmp ecx, 1
    jle .Lfd_trimmed
    cmp BYTE PTR [r8 + rcx - 1], 48
    jne .Lfd_trimmed
    dec ecx
    jmp .Lfd_trim
.Lfd_trimmed:
    # Fixed point when it shows the value in at most P digits, as GW-BASIC
    # and QuickBASIC do: X+1 integer digits for X >= 0, or for X < 0 the
    # -X-1 zeros after the point and then the n digits.
    test edx, edx
    js .Lfd_small
    cmp edx, r13d
    jge .Lfd_sci

    xor r11d, r11d                      # X >= 0: the integer part ...
.Lfd_int:
    cmp r11d, edx
    jg .Lfd_intdone
    mov al, 48                          # a digit past n is a zero
    cmp r11d, ecx
    jge .Lfd_intput
    mov al, BYTE PTR [r8 + r11]
.Lfd_intput:
    mov BYTE PTR [r14], al
    inc r14
    inc r11d
    jmp .Lfd_int
.Lfd_intdone:
    cmp r11d, ecx                       # ... and a fraction, if any is left
    jge .Lfd_end
    mov BYTE PTR [r14], 46              # '.'
    inc r14
.Lfd_frac:
    cmp r11d, ecx
    jge .Lfd_end
    mov al, BYTE PTR [r8 + r11]
    mov BYTE PTR [r14], al
    inc r14
    inc r11d
    jmp .Lfd_frac

.Lfd_small:                             # X < 0: no leading zero, as .5
    mov eax, edx
    neg eax
    dec eax                             # zeros between the point and n
    lea r10d, [rax + rcx]
    cmp r10d, r13d
    jg .Lfd_sci
    mov BYTE PTR [r14], 46              # '.'
    inc r14
.Lfd_lead:
    test eax, eax
    jz .Lfd_leaddone
    mov BYTE PTR [r14], 48
    inc r14
    dec eax
    jmp .Lfd_lead
.Lfd_leaddone:
    xor r11d, r11d
    jmp .Lfd_frac

.Lfd_sci:                               # d[.ddd]E+xx, D for a DOUBLE
    mov al, BYTE PTR [r8]
    mov BYTE PTR [r14], al
    inc r14
    cmp ecx, 1
    jle .Lfd_sciexp
    mov BYTE PTR [r14], 46
    inc r14
    mov r11d, 1
.Lfd_scifrac:
    cmp r11d, ecx
    jge .Lfd_sciexp
    mov al, BYTE PTR [r8 + r11]
    mov BYTE PTR [r14], al
    inc r14
    inc r11d
    jmp .Lfd_scifrac
.Lfd_sciexp:
    mov al, 68                          # 'D'
    test r12d, r12d
    jz .Lfd_letter
    mov al, 69                          # 'E'
.Lfd_letter:
    mov BYTE PTR [r14], al
    inc r14
    mov al, 43                          # '+'
    test edx, edx
    jns .Lfd_expsign
    mov al, 45                          # '-'
    neg edx
.Lfd_expsign:
    mov BYTE PTR [r14], al
    inc r14
    mov r10d, 10
    cmp edx, 100                        # two digits at least, three if needed
    jl .Lfd_twodigits
    mov eax, edx
    xor edx, edx
    mov r10d, 100
    div r10d
    add al, 48
    mov BYTE PTR [r14], al
    inc r14
    mov r10d, 10
.Lfd_twodigits:
    mov eax, edx
    xor edx, edx
    div r10d
    add al, 48
    mov BYTE PTR [r14], al
    add dl, 48
    mov BYTE PTR [r14 + 1], dl
    add r14, 2
    jmp .Lfd_end

.Lfd_raw:
    mov al, BYTE PTR [rbx]
    test al, al
    jz .Lfd_end
    mov BYTE PTR [r14], al
    inc rbx
    inc r14
    jmp .Lfd_raw

.Lfd_end:
    mov BYTE PTR [r14], 0
    lea rax, [rip + _num_buf]
    sub r14, rax
    mov rax, r14                        # the length

.Lfd_ret:
    add rsp, 32
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

# _rt_fmt_basic - Render a number the way BASIC writes it
#
# _rt_fmt_double gives the digits; this adds the blank that stands where a
# minus sign would go. GW-BASIC puts one there for every non-negative number,
# which is why `PRINT 1; 2` reads " 1  2 " and why STR$(5) is " 5" -- the
# `MID$(STR$(N), 2)` idiom exists to strip exactly this blank.
#
# Not folded into _rt_fmt_double, because WRITE and PRINT USING render numbers
# through that and must not gain the blank.
#
# Arguments: the same as _rt_fmt_double
# Returns:   rax = length of the text in _num_buf
.globl _rt_fmt_basic
_rt_fmt_basic:
    push rbp
    mov rbp, rsp
    sub rsp, 32                     # shadow space
    call _rt_fmt_double
    lea rcx, [rip + _num_buf]
    cmp BYTE PTR [rcx], 45          # '-' already occupies the sign position
    je .Lfb_done
    # Shift right by one, the NUL at [rax] included, and blank the vacancy.
    mov rdx, rax
.Lfb_shift:
    mov r9b, BYTE PTR [rcx + rdx]
    mov BYTE PTR [rcx + rdx + 1], r9b
    dec rdx
    jns .Lfb_shift
    mov BYTE PTR [rcx], 32          # ' '
    inc rax
.Lfb_done:
    leave
    ret

# _rt_end - Terminate the program normally (END / STOP)
# Valid from any frame, including inside a SUB or FUNCTION. Emitting a plain
# `leave; ret` for END only terminates when it appears in main; inside a
# procedure it merely returns to the caller and execution continues.
#
# ExitProcess is safe here because this runtime writes console and file output
# with WriteFile rather than through CRT buffering.
#
# Arguments: none
# Returns: never
.globl _rt_end
_rt_end:
    push rbp
    mov rbp, rsp
    sub rsp, 32             # Shadow space
    xor ecx, ecx            # exit code 0
    call ExitProcess
