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
# GW-BASIC convention: a whole number is written without a decimal point. For
# fractional values, write the *shortest* decimal that reads back as the same
# value, trying each format in the given table until one round-trips. A plain
# %g gives only 6 significant digits, which for a dialect whose default type is
# Double discards most of the value.
#
# This is a private helper, so it uses its own argument convention rather than
# the positional one: the value stays in xmm0 and the other two arguments take
# the first integer registers.
#
# Arguments:
#   xmm0 = value (a SINGLE arrives already widened to double)
#   rcx  = pointer to a NULL-terminated table of format-string pointers
#   rdx  = nonzero to compare at SINGLE precision
#
# Returns:
#   rax = length of the text in _num_buf
.globl _rt_fmt_double
_rt_fmt_double:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 64             # shadow space + locals; 64 realigns after 3 pushes

    movsd QWORD PTR [rbp - 32], xmm0    # original value
    mov rbx, rcx            # table cursor
    mov rsi, rdx            # precision flag

    # Whole number? Format as an integer.
    cvttsd2si rax, xmm0
    cvtsi2sd xmm1, rax
    ucomisd xmm0, xmm1
    jne .Lfd_fractional
    jp .Lfd_fractional
    lea rcx, [rip + _num_buf]
    lea rdx, [rip + _fmt_int]
    mov r8, rax
    call sprintf
    jmp .Lfd_done

.Lfd_fractional:
    mov rdx, QWORD PTR [rbx]
    test rdx, rdx
    jz .Lfd_len             # table exhausted: keep the last attempt
    lea rcx, [rip + _num_buf]
    movsd xmm2, QWORD PTR [rbp - 32]
    movq r8, xmm2
    call sprintf

    # Does it read back as the same value?
    lea rcx, [rip + _num_buf]
    xor edx, edx
    call strtod
    movsd xmm1, QWORD PTR [rbp - 32]
    test rsi, rsi
    jz .Lfd_cmp_double
    cvtsd2ss xmm0, xmm0
    cvtsd2ss xmm1, xmm1
    ucomiss xmm0, xmm1
    jmp .Lfd_cmp_done
.Lfd_cmp_double:
    ucomisd xmm0, xmm1
.Lfd_cmp_done:
    jp .Lfd_next            # unordered: keep trying
    je .Lfd_len
.Lfd_next:
    add rbx, 8
    jmp .Lfd_fractional

.Lfd_len:
    lea rcx, [rip + _num_buf]
    call lstrlenA

.Lfd_done:
    # sprintf and lstrlenA both leave the length in rax.
    #
    # Reshape C's rendering into GW-BASIC's, the same way the System V tree
    # does. Only registers volatile in both ABIs are touched -- rsi and rdi are
    # callee-saved here, and rsi is holding the precision flag besides.

    # A value below one drops its leading zero: 0.5 -> .5, -0.5 -> -.5
    lea rcx, [rip + _num_buf]
    xor edx, edx
    cmp BYTE PTR [rcx], 45           # '-'
    jne .Lfd_zero
    mov edx, 1
.Lfd_zero:
    cmp BYTE PTR [rcx + rdx], 48     # '0'
    jne .Lfd_exp
    cmp BYTE PTR [rcx + rdx + 1], 46 # '.'
    jne .Lfd_exp
    # Shift the rest down over the zero, the NUL at [rax] included.
.Lfd_shift:
    mov r9b, BYTE PTR [rcx + rdx + 1]
    mov BYTE PTR [rcx + rdx], r9b
    inc rdx
    cmp rdx, rax
    jl .Lfd_shift
    dec rax

    # The exponent is spelled D for a double and E for a single, never C's
    # lowercase e.
.Lfd_exp:
    xor edx, edx
.Lfd_escan:
    cmp rdx, rax
    jge .Lfd_ret
    cmp BYTE PTR [rcx + rdx], 101    # 'e'
    je .Lfd_efound
    inc rdx
    jmp .Lfd_escan
.Lfd_efound:
    mov r9b, 68                      # 'D', a double
    test esi, esi
    jz .Lfd_eput
    mov r9b, 69                      # 'E', a single
.Lfd_eput:
    mov BYTE PTR [rcx + rdx], r9b

.Lfd_ret:
    add rsp, 64
    pop rsi
    pop rbx
    leave
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
