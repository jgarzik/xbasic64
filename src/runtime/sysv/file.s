# BASIC Runtime: File I/O Functions
#
# File input/output functions implementing BASIC's OPEN, CLOSE, PRINT#, INPUT#
# statements. Uses libc file operations (fopen, fclose, fprintf, fscanf, fgets).
#
# BASIC File I/O Model:
#   Files are referenced by number (1-15). The OPEN statement associates a
#   filename with a file number, and subsequent I/O uses that number:
#
#     OPEN "data.txt" FOR OUTPUT AS #1
#     PRINT #1, "Hello"
#     CLOSE #1
#
# File Handle Table:
#   _file_handles is an array of 16 FILE* pointers (128 bytes).
#   Index 0 is unused (BASIC file numbers start at 1).
#   Handles 1-15 are available for user files.
#
# File Modes:
#   0 = INPUT  - read existing file (fopen "r")
#   1 = OUTPUT - create/truncate file (fopen "w")
#   2 = APPEND - append to file (fopen "a")
#
# String Handling:
#   BASIC strings are (ptr, len) pairs but libc expects null-terminated strings.
#   For filenames, we copy to _file_name_buf and null-terminate.
#   For string output, we use fprintf with "%.*s" (precision = length).
#   For string input, fgets reads into _file_input_buf.
#
# Error Handling:
#   Currently minimal - fopen failure results in NULL handle, which will
#   cause subsequent operations to fail silently or crash.

# Data Section: File handle table and buffers
.data
# File handle table: FILE* pointers indexed by BASIC file number (1-15)
# Index 0 unused, indices 1-15 for BASIC files #1-#15
_file_handles: .skip 128        # 16 * 8 bytes = 16 FILE* pointers

# Mode strings for fopen()
_mode_read:   .asciz "r"        # FOR INPUT
_mode_write:  .asciz "w"        # FOR OUTPUT
_mode_append: .asciz "a"        # FOR APPEND

# Temp buffer for null-terminated filename (BASIC strings aren't null-terminated)
_file_name_buf: .skip 1024

# Format strings for fprintf/fscanf (same as console I/O)
_file_fmt_str:     .asciz "%.*s"    # String with precision (ptr, len)
_file_fmt_int:     .asciz "%ld"     # Long integer
_file_fmt_float:   .asciz "%g"      # Floating point (compact)
_file_fmt_char:    .asciz "%c"      # Single character
_file_fmt_newline: .asciz "\n"      # Newline
_file_fmt_input:   .asciz "%lf"     # Read double

# Longest INPUT # field kept, leaving room for the NUL in _file_input_buf.
.equ MAX_FIELD_LEN, 1023

# Buffer for string input from files
_file_input_buf: .skip 1024

.text

# _rt_file_open - Open a file (OPEN statement)
# Associates a filename with a file number for subsequent I/O.
#
# Arguments:
#   rdi = filename pointer (BASIC string, not null-terminated)
#   rsi = filename length
#   rdx = mode: 0=INPUT, 1=OUTPUT, 2=APPEND
#   rcx = file number (1-15)
#
# Returns: nothing (FILE* stored in _file_handles[file_number])
#
# Implementation:
#   1. Copy filename to _file_name_buf and null-terminate
#   2. Select mode string based on mode argument
#   3. Call fopen(filename, mode)
#   4. Store resulting FILE* in handle table
.globl _rt_file_open
_rt_file_open:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14

    # Save arguments in callee-saved registers
    mov r12, rdi            # filename ptr
    mov r13, rsi            # filename len
    mov r14d, edx           # mode (0/1/2)
    mov ebx, ecx            # file number

    # Copy filename to buffer and null-terminate
    # memcpy(_file_name_buf, filename_ptr, filename_len)
    lea rdi, [rip + _file_name_buf]
    mov rsi, r12
    mov rdx, r13
    call {libc}memcpy
    # Null-terminate
    lea rax, [rip + _file_name_buf]
    mov BYTE PTR [rax + r13], 0

    # Select mode string based on mode argument
    cmp r14d, 0
    je .Lmode_read
    cmp r14d, 1
    je .Lmode_write
    # else: append
    lea rsi, [rip + _mode_append]
    jmp .Ldo_fopen
.Lmode_read:
    lea rsi, [rip + _mode_read]
    jmp .Ldo_fopen
.Lmode_write:
    lea rsi, [rip + _mode_write]

.Ldo_fopen:
    # fopen(filename, mode)
    lea rdi, [rip + _file_name_buf]
    call {libc}fopen        # returns FILE* in rax (or NULL on error)

    # Store FILE* in handle table: _file_handles[file_number] = rax
    lea rcx, [rip + _file_handles]
    mov [rcx + rbx*8], rax

    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_file_close - Close a file (CLOSE statement)
# Closes the file associated with a file number and clears its handle.
#
# Arguments:
#   rdi = file number (1-15)
#
# Returns: nothing
.globl _rt_file_close
_rt_file_close:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8              # Alignment

    mov ebx, edi            # save file number

    # Get FILE* from handle table
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # rdi = FILE*
    test rdi, rdi           # Check for NULL (already closed or never opened)
    jz .Lclose_done

    # Flush before close
    call {libc}fflush

    # Close file
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # rdi = FILE*
    call {libc}fclose

    # Clear handle from table
    lea rax, [rip + _file_handles]
    mov QWORD PTR [rax + rbx*8], 0

.Lclose_done:
    add rsp, 8
    pop rbx
    leave
    ret

# _rt_file_print_string - Write string to file (PRINT# with string)
# Arguments:
#   rdi = file number
#   rsi = string pointer
#   rdx = string length
#
# Returns: nothing
.globl _rt_file_print_string
_rt_file_print_string:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    # An unassigned string is (NULL, 0); writing nothing is correct and avoids
    # passing NULL to fprintf.
    test rdx, rdx
    jz .Lfile_print_str_done

    mov ebx, edi            # save file number
    mov r12, rdx            # remember the length for the column tracker
    mov rcx, rsi            # string ptr → 4th arg (for %.*s format)
    mov r8, rdx             # string len → will become 3rd arg

    # Get FILE* from handle table
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # FILE* → 1st arg

    # fprintf(file, "%.*s", len, ptr)
    lea rsi, [rip + _file_fmt_str]  # format → 2nd arg
    mov rdx, r8             # len (precision for %.*s) → 3rd arg
    # rcx already has ptr    → 4th arg
    xor eax, eax            # no vector args
    call {libc}fprintf

    lea rax, [rip + _file_col]
    add QWORD PTR [rax + rbx*8], r12

.Lfile_print_str_done:
    pop r12
    pop rbx
    leave
    ret

# _rt_file_print_float - Write number to file (PRINT# with number)
# Prints whole numbers as integers for clean output.
#
# Arguments:
#   rdi = file number
#   xmm0 = value to write (double)
#
# Returns: nothing
.globl _rt_file_print_float
_rt_file_print_float:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi            # save file number

    # Format through the shared helper, so every sink renders a number the
    # same way.
    lea rdi, [rip + _fmt_g_table]
    xor esi, esi
    call _rt_fmt_double
    jmp .Lfile_num_emit

# _rt_file_print_single - Write a SINGLE
# A SINGLE carries only ~7 significant digits, so it uses a table starting at a
# shorter format and compares at 32-bit precision. Otherwise 3.14159! would
# print as 3.1415901184082: at 15 digits even a float's value round-trips.
#
# Shares the emit tail below with _rt_file_print_float, whose prologue is
# identical.
#
# Arguments:
#   rdi = file number
#   xmm0 = value, already widened to double
#
# Returns: nothing
.globl _rt_file_print_single
_rt_file_print_single:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi            # save file number

    lea rdi, [rip + _fmt_g_single_table]
    mov esi, 1
    call _rt_fmt_double

.Lfile_num_emit:
    # rax = length of the text _rt_fmt_double left in _num_buf.
    mov rdx, rax
    lea rsi, [rip + _num_buf]
    mov edi, ebx
    call _rt_file_print_string

    add rsp, 8
    pop rbx
    leave
    ret

# _rt_con_string - Write a string to the console
# The console is file handle 0. This exists for the runtime's own messages and
# for PRINT USING, which has no file form; generated code passes a handle like
# any other caller.
#
# Arguments:
#   rdi = string pointer
#   rsi = string length
#
# Returns: nothing
.globl _rt_con_string
_rt_con_string:
    mov rdx, rsi            # length  -> 3rd arg
    mov rsi, rdi            # pointer -> 2nd arg
    xor edi, edi            # console -> 1st arg
    jmp _rt_file_print_string

# _rt_file_print_spc - SPC(n): write n spaces
# Arguments:
#   rdi = file number
#   rsi = count
#
# Returns: nothing
.globl _rt_file_print_spc
_rt_file_print_spc:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    mov ebx, edi            # file number
    mov r12, rsi            # count remaining
.Lfile_spc_loop:
    cmp r12, 0
    jle .Lfile_spc_done
    mov edi, ebx
    mov esi, ' '
    call _rt_file_print_char
    dec r12
    jmp .Lfile_spc_loop
.Lfile_spc_done:
    pop r12
    pop rbx
    leave
    ret

# _rt_file_print_tab - TAB(n): advance to column n
# Columns are 1-based, as in GW-BASIC. If the sink is already at or past the
# requested column, a newline is written first and the tab applies to the new
# line.
#
# Arguments:
#   rdi = file number
#   rsi = target column
#
# Returns: nothing
.globl _rt_file_print_tab
_rt_file_print_tab:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    mov ebx, edi            # file number
    mov r12, rsi            # target column
    cmp r12, 1
    jge .Lfile_tab_have_target
    mov r12, 1
.Lfile_tab_have_target:
    dec r12                 # 1-based column -> count of characters before it

    lea rax, [rip + _file_col]
    cmp r12, QWORD PTR [rax + rbx*8]
    jge .Lfile_tab_pad
    mov edi, ebx
    call _rt_file_print_newline

.Lfile_tab_pad:
    lea rax, [rip + _file_col]
    mov rsi, r12
    sub rsi, QWORD PTR [rax + rbx*8]
    mov edi, ebx
    call _rt_file_print_spc

    pop r12
    pop rbx
    leave
    ret

# _rt_file_print_char - Write single character to file
# Arguments:
#   rdi = file number
#   rsi = character code
#
# Returns: nothing
.globl _rt_file_print_char
_rt_file_print_char:
    push rbp
    mov rbp, rsp
    push rbx
    push r12

    mov ebx, edi            # save file number
    mov r12d, esi           # save char

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # FILE*
    lea rsi, [rip + _file_fmt_char]
    mov rdx, r12            # char
    xor eax, eax
    call {libc}fprintf

    lea rax, [rip + _file_col]
    inc QWORD PTR [rax + rbx*8]

    pop r12
    pop rbx
    leave
    ret

# _rt_file_print_newline - Write newline to file
# Called at end of PRINT# statement unless suppressed with ; or ,
#
# Arguments:
#   rdi = file number
#
# Returns: nothing
.globl _rt_file_print_newline
_rt_file_print_newline:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8              # align stack

    mov ebx, edi            # save file number

    # Use fputc('\n', file) - simpler than fprintf
    lea rax, [rip + _file_handles]
    mov rsi, [rax + rbx*8]  # FILE* → rsi (2nd arg)
    mov edi, 10             # '\n' → edi (1st arg)
    call {libc}fputc

    lea rax, [rip + _file_col]
    mov QWORD PTR [rax + rbx*8], 0

    add rsp, 8
    pop rbx
    leave
    ret


# _rt_file_read_field - Read one INPUT # field
# INPUT # reads comma-delimited fields, not whole lines. Reading a line per
# variable made `INPUT #1, A, B` on "10,20" yield 10 twice, and reading through
# fscanf("%lf") was worse: it stopped at the comma without consuming it, so
# every later read failed on the same character.
#
# Leading blanks and line breaks are skipped. A field either is quoted, in
# which case it runs to the closing quote and everything up to the next
# delimiter is discarded, or runs to the next comma, newline or end of file
# with trailing blanks trimmed. The delimiter is consumed.
#
# Arguments:
#   rdi = file number
#
# Returns:
#   rax = pointer to the field (in _file_input_buf), rdx = its length
#
# Note: uses a static buffer, so the result is valid only until the next read.
# _rt_file_getc - Read one byte
# Arguments: rdi = file number
# Returns:   eax = the byte, or -1 at end of file or on a file that is not open
.globl _rt_file_getc
_rt_file_getc:
    push rbp
    mov rbp, rsp

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rdi*8]
    test rdi, rdi
    jz .Lfile_getc_eof
    call {libc}fgetc
    leave
    ret
.Lfile_getc_eof:
    mov eax, -1
    leave
    ret

.globl _rt_file_read_field
_rt_file_read_field:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    sub rsp, 8

    mov ebx, edi                # file number, reloaded before every read
    xor r12d, r12d              # length written so far

    # Skip leading blanks, tabs and line breaks.
.Lfield_skip:
    mov edi, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_done
    cmp eax, ' '
    je .Lfield_skip
    cmp eax, 9                  # tab
    je .Lfield_skip
    cmp eax, 13                 # CR
    je .Lfield_skip
    cmp eax, 10                 # LF
    je .Lfield_skip

    cmp eax, '"'
    je .Lfield_quoted

    # Unquoted: this character and everything up to the delimiter.
    mov r13d, eax
.Lfield_plain_store:
    cmp r12d, MAX_FIELD_LEN
    jge .Lfield_plain_next
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], r13b
    inc r12d
.Lfield_plain_next:
    mov edi, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_trim
    cmp eax, ','
    je .Lfield_trim
    cmp eax, 10
    je .Lfield_trim
    cmp eax, 13
    je .Lfield_plain_next
    mov r13d, eax
    jmp .Lfield_plain_store

.Lfield_trim:
    # Drop trailing blanks, which are separators rather than data.
    test r12d, r12d
    jz .Lfield_done
    lea rax, [rip + _file_input_buf]
    movzx ecx, BYTE PTR [rax + r12 - 1]
    cmp ecx, ' '
    je .Lfield_trim_one
    cmp ecx, 9
    jne .Lfield_done
.Lfield_trim_one:
    dec r12d
    jmp .Lfield_trim

.Lfield_quoted:
    mov edi, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_done
    cmp eax, '"'
    je .Lfield_after_quote
    cmp r12d, MAX_FIELD_LEN
    jge .Lfield_quoted
    mov r13d, eax
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], r13b
    inc r12d
    jmp .Lfield_quoted

.Lfield_after_quote:
    # Discard whatever separates the closing quote from the delimiter.
    mov edi, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_done
    cmp eax, ','
    je .Lfield_done
    cmp eax, 10
    je .Lfield_done
    jmp .Lfield_after_quote

.Lfield_done:
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], 0
    mov rdx, r12

    add rsp, 8
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_file_input_number - INPUT #: read one numeric field
# Arguments:
#   rdi = file number
#
# Returns:
#   xmm0 = value read, or 0.0 for a field that is not a number
.globl _rt_file_input_number
_rt_file_input_number:
    push rbp
    mov rbp, rsp
    sub rsp, 16

    call _rt_file_read_field
    mov rdi, rax                # NUL-terminated by _rt_file_read_field
    xor esi, esi                # endptr = NULL
    call {libc}strtod

    add rsp, 16
    leave
    ret

# _rt_file_input_string - INPUT #: read one string field
# Arguments: rdi = file number
# Returns:   rax = pointer, rdx = length
.globl _rt_file_input_string
_rt_file_input_string:
    jmp _rt_file_read_field

# _rt_file_line_input - LINE INPUT #: read a whole line
# Reads a line from file, stripping the trailing newline.
#
# Arguments:
#   rdi = file number
#
# Returns:
#   rax = pointer to string data (_file_input_buf)
#   rdx = string length
#
# Note: Uses static buffer - result only valid until next file string read.
.globl _rt_file_line_input
_rt_file_line_input:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8              # align stack

    mov ebx, edi            # save file number

    # fgets(buffer, size, file)
    lea rdi, [rip + _file_input_buf]    # buffer
    mov rsi, 1023                        # max chars (leave room for null)
    lea rax, [rip + _file_handles]
    mov rdx, [rax + rbx*8]              # FILE*
    call {libc}fgets

    # Check for EOF/error (fgets returns NULL)
    test rax, rax
    jz .Lfile_input_string_empty

    # Calculate length using strlen
    lea rdi, [rip + _file_input_buf]
    call {libc}strlen
    mov rdx, rax            # length → rdx

    # Strip the trailing newline, and the CR before it in a file written on a
    # platform that uses CRLF. Dropping only the LF handed every line back with
    # a stray CR, which then showed up in every comparison and every LEN.
    test rdx, rdx
    jz .Lfile_input_string_done
    lea rax, [rip + _file_input_buf]
    mov cl, BYTE PTR [rax + rdx - 1]    # last character
    cmp cl, 10              # newline?
    jne .Lfile_input_string_done
    dec rdx                 # reduce length
    mov BYTE PTR [rax + rdx], 0         # remove newline

    test rdx, rdx
    jz .Lfile_input_string_done
    mov cl, BYTE PTR [rax + rdx - 1]
    cmp cl, 13              # carriage return?
    jne .Lfile_input_string_done
    dec rdx
    mov BYTE PTR [rax + rdx], 0

.Lfile_input_string_done:
    lea rax, [rip + _file_input_buf]
    add rsp, 8
    pop rbx
    leave
    ret

.Lfile_input_string_empty:
    # EOF or error - return empty string
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax], 0   # empty string
    xor edx, edx            # length = 0
    add rsp, 8
    pop rbx
    leave
    ret

# _rt_file_close_all - Close every open file (bare CLOSE)
# Arguments: none
# Returns: nothing
.globl _rt_file_close_all
_rt_file_close_all:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, 1              # BASIC file numbers start at 1
.Lclose_all_loop:
    cmp ebx, 15
    jg .Lclose_all_done
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    test rdi, rdi
    jz .Lclose_all_next
    call {libc}fflush
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    call {libc}fclose
    lea rax, [rip + _file_handles]
    mov QWORD PTR [rax + rbx*8], 0
.Lclose_all_next:
    inc ebx
    jmp .Lclose_all_loop
.Lclose_all_done:
    add rsp, 8
    pop rbx
    leave
    ret

# _rt_file_eof - EOF(n): has the file been read to the end?
# Peeks one character and pushes it back, since feof() only reports end-of-file
# after a read has already failed, which would make EOF() lag by one line.
#
# Arguments:
#   rdi = file number
#
# Returns:
#   eax = -1 at end of file, 0 otherwise (BASIC's true/false, as a LONG)
.globl _rt_file_eof
_rt_file_eof:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    test rdi, rdi
    jz .Leof_true           # never opened: treat as at end

    call {libc}fgetc
    cmp eax, -1
    je .Leof_true

    # Push the character back so the next read still sees it.
    mov edi, eax
    lea rax, [rip + _file_handles]
    mov rsi, [rax + rbx*8]
    call {libc}ungetc
    xor eax, eax            # 0 = false
    jmp .Leof_done

.Leof_true:
    mov eax, -1             # BASIC true

.Leof_done:
    add rsp, 8
    pop rbx
    leave
    ret

# _rt_file_lof - LOF(n): length of the file in bytes
# Arguments:
#   rdi = file number
#
# Returns:
#   xmm0 = length in bytes, or 0.0 when the file is not open
.globl _rt_file_lof
_rt_file_lof:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 16             # one local, and keeps rsp 16-byte aligned
    mov ebx, edi

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    test rdi, rdi
    jz .Llof_zero

    # Remember the position, seek to the end, read it, then restore.
    call {libc}ftell
    mov r12, rax                    # saved position

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    xor esi, esi
    mov edx, 2                      # SEEK_END
    call {libc}fseek

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    call {libc}ftell
    mov QWORD PTR [rbp - 24], rax   # length; pushing it here would misalign
                                    # rsp for the fseek below

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    mov rsi, r12
    xor edx, edx                    # SEEK_SET
    call {libc}fseek

    mov rax, QWORD PTR [rbp - 24]
    cvtsi2sd xmm0, rax
    jmp .Llof_done

.Llof_zero:
    xorpd xmm0, xmm0

.Llof_done:
    add rsp, 16
    pop r12
    pop rbx
    leave
    ret
