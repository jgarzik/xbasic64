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
# Mode strings for fopen()
_mode_read:   .asciz "r"        # FOR INPUT
_mode_write:  .asciz "w"        # FOR OUTPUT
_mode_append: .asciz "a"        # FOR APPEND

# Format strings for fprintf/fscanf (same as console I/O)
_file_fmt_str:     .asciz "%.*s"    # String with precision (ptr, len)
_file_fmt_int:     .asciz "%ld"     # Long integer
_file_fmt_float:   .asciz "%g"      # Floating point (compact)
_file_fmt_char:    .asciz "%c"      # Single character
_file_fmt_newline: .asciz "\n"      # Newline
_file_fmt_input:   .asciz "%lf"     # Read double

# Longest INPUT # field kept, leaving room for the NUL in _file_input_buf.
.equ MAX_FIELD_LEN, 1023

# Zero-filled scratch, so .bss rather than .data -- see data_defs.s. The
# .text below restores the section for the code that follows; a .bss left
# open at the end of this file would swallow the next runtime part.
.bss
# The handle table is indexed as quadwords (`[rcx + rbx*8]`), so it needs
# stated alignment: in .data it inherited whatever offset the preceding
# strings happened to leave, which is not something to rely on.
.p2align 3
# File handle table: FILE* pointers indexed by BASIC file number (1-15)
# Index 0 unused, indices 1-15 for BASIC files #1-#15
_file_handles: .skip 128        # 16 * 8 bytes = 16 FILE* pointers

# Temp buffer for null-terminated filename (BASIC strings aren't null-terminated)
_file_name_buf: .skip 1024

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
    call memcpy
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
    call fopen        # returns FILE* in rax (or NULL on error)

    # Store FILE* in handle table: _file_handles[file_number] = rax
    lea rcx, [rip + _file_handles]
    mov [rcx + rbx*8], rax

    # A new file starts at column 1, whatever the last one on this number
    # left behind.
    lea rcx, [rip + _file_col]
    mov QWORD PTR [rcx + rbx*8], 0

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
    call fflush

    # Close file
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # rdi = FILE*
    call fclose

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
    test rdi, rdi
    jz .Lfile_not_open      # a number nobody OPENed is NULL here

    # fprintf(file, "%.*s", len, ptr)
    lea rsi, [rip + _file_fmt_str]  # format → 2nd arg
    mov rdx, r8             # len (precision for %.*s) → 3rd arg
    # rcx already has ptr    → 4th arg
    xor eax, eax            # no vector args
    call fprintf

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
    test rdi, rdi
    jz .Lfile_not_open      # a number nobody OPENed is NULL here
    lea rsi, [rip + _file_fmt_char]
    mov rdx, r12            # char
    xor eax, eax
    call fprintf

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
    test rsi, rsi
    jz .Lfile_not_open      # a number nobody OPENed is NULL here
    mov edi, 10             # '\n' → edi (1st arg)
    call fputc

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
    call fgetc
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
    call strtod

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
    test rdx, rdx
    jz .Lfile_not_open                  # a number nobody OPENed is NULL here
    call fgets

    # Check for EOF/error (fgets returns NULL)
    test rax, rax
    jz .Lfile_input_string_empty

    # Calculate length using strlen
    lea rdi, [rip + _file_input_buf]
    call strlen
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
    call fflush
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    call fclose
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

    call fgetc
    cmp eax, -1
    je .Leof_true

    # Push the character back so the next read still sees it.
    mov edi, eax
    lea rax, [rip + _file_handles]
    mov rsi, [rax + rbx*8]
    call ungetc
    xor eax, eax            # 0 = false
    jmp .Leof_done

.Leof_true:
    mov eax, -1             # BASIC true

.Leof_done:
    add rsp, 8
    pop rbx
    leave
    ret

# ---------------------------------------------------------------------------
# Random-access files
#
# A file opened FOR RANDOM carries three things beyond its handle: a record
# length, a record buffer of that length, and a cursor saying how much of the
# buffer FIELD has handed out so far.
#
# FIELD does not copy anything. It hands each variable the (pointer, length)
# pair naming its slice of that buffer, and a BASIC string is exactly such a
# pair, so the variable *is* a window onto the buffer. GET overwrites the
# buffer and every field variable sees the new record at once; LSET and RSET
# write through the window, so PUT emits what they wrote. This is why LSET
# exists at all: plain assignment would allocate a fresh string and the
# variable would quietly stop tracking the record.
#
# Strings are never freed in this runtime, so a variable outliving its file is
# a dangling read at worst, never a double free.
.data
_mode_update: .asciz "r+b"      # open an existing file for reading and writing
_mode_create: .asciz "w+b"      # ... or create it when it does not exist

# Largest record GW-BASIC allows.
.equ MAX_RECLEN, 32767

.bss
# All four tables are indexed as quadwords, and each is a multiple of 8, so
# aligning the first aligns them all.
.p2align 3
_file_recbuf:   .skip 128       # record buffer per file number (malloc'd)
_file_reclen:   .skip 128       # record length per file number
_file_recnum:   .skip 128       # last record read or written, 1-based; 0 = none
_file_fieldoff: .skip 128       # bytes of the buffer FIELD has assigned so far

# MKI$/MKL$/MKS$/MKD$ assemble their bytes here, then return a heap copy; the
# buffer itself is never handed out. Written a quadword at a time, so it is
# aligned deliberately rather than by accident of what precedes it.
.p2align 3
_mk_buf: .skip 16

.text

# _rt_file_open_random - OPEN ... FOR RANDOM AS #n LEN = r
#
# Four arguments, not five: there is no mode byte, because reaching this entry
# point *is* the mode. Win64 passes only four arguments in registers, and the
# Win64 twin of this function has to match, so the mode was the one to drop.
#
# Arguments:
#   rdi = filename pointer, rsi = filename length
#   rdx = file number, rcx = record length
#
# Returns: nothing
.globl _rt_file_open_random
_rt_file_open_random:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14

    mov r12, rdi            # filename ptr
    mov r13, rsi            # filename len
    mov ebx, edx            # file number
    mov r14, rcx            # record length

    # Null-terminate the filename in the shared buffer.
    lea rdi, [rip + _file_name_buf]
    mov rsi, r12
    mov rdx, r13
    call memcpy
    lea rax, [rip + _file_name_buf]
    mov BYTE PTR [rax + r13], 0

    # A random file is read *and* written, so it is opened for update. "r+b"
    # fails when the file does not exist yet, in which case it is created.
    lea rdi, [rip + _file_name_buf]
    lea rsi, [rip + _mode_update]
    call fopen
    test rax, rax
    jnz .Lrandom_have_handle
    lea rdi, [rip + _file_name_buf]
    lea rsi, [rip + _mode_create]
    call fopen

.Lrandom_have_handle:
    lea rcx, [rip + _file_handles]
    mov [rcx + rbx*8], rax

    lea rcx, [rip + _file_col]
    mov QWORD PTR [rcx + rbx*8], 0

    # Clamp the record length into 1..MAX_RECLEN.
    cmp r14, 1
    jge .Lrandom_len_ok
    mov r14, 128
.Lrandom_len_ok:
    cmp r14, MAX_RECLEN
    jle .Lrandom_len_set
    mov r14, MAX_RECLEN
.Lrandom_len_set:
    lea rcx, [rip + _file_reclen]
    mov [rcx + rbx*8], r14

    # Release the buffer a previous OPEN on this number left behind.
    lea rax, [rip + _file_recbuf]
    mov rdi, [rax + rbx*8]
    test rdi, rdi
    jz .Lrandom_alloc
    call free

.Lrandom_alloc:
    mov rdi, r14
    call malloc
    test rax, rax
    jz .Lrandom_nomem
    lea rcx, [rip + _file_recbuf]
    mov [rcx + rbx*8], rax

    # A fresh buffer reads as blanks, so a field never written still holds
    # spaces rather than whatever the allocator handed back.
    mov rdi, rax
    mov esi, ' '
    mov rdx, r14
    call memset

    lea rax, [rip + _file_recnum]
    mov QWORD PTR [rax + rbx*8], 0
    lea rax, [rip + _file_fieldoff]
    mov QWORD PTR [rax + rbx*8], 0

    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

.Lrandom_nomem:
    lea rdi, [rip + _err_memory]
    xor esi, esi
    call _rt_error

# _rt_field_reset - Start a fresh FIELD statement
# Arguments: rdi = file number
.globl _rt_field_reset
_rt_field_reset:
    lea rax, [rip + _file_fieldoff]
    mov QWORD PTR [rax + rdi*8], 0
    ret

# _rt_field_next - Carve the next slice out of the record buffer
# Arguments:
#   rdi = file number, rsi = width in bytes
#
# Returns: rax = pointer into the record buffer, rdx = width
.globl _rt_field_next
_rt_field_next:
    push rbp
    mov rbp, rsp

    # FIELD on a file that was not opened FOR RANDOM has no buffer to carve.
    lea rax, [rip + _file_recbuf]
    mov r8, [rax + rdi*8]
    test r8, r8
    jz .Lfield_badmode

    # A negative width is nonsense; treat it as empty rather than walking back.
    test rsi, rsi
    jns .Lfield_width_ok
    xor esi, esi
.Lfield_width_ok:

    lea rax, [rip + _file_fieldoff]
    mov r9, [rax + rdi*8]           # bytes already assigned
    lea rax, [rip + _file_reclen]
    mov r10, [rax + rdi*8]

    # The fields of one record must fit inside it.
    mov rcx, r9
    add rcx, rsi
    cmp rcx, r10
    jg .Lfield_overflow

    lea rax, [rip + _file_fieldoff]
    mov [rax + rdi*8], rcx

    mov rax, r8
    add rax, r9                     # buffer + offset
    mov rdx, rsi
    leave
    ret

.Lfield_badmode:
    lea rdi, [rip + _err_badmode]
    xor esi, esi
    call _rt_error

.Lfield_overflow:
    lea rdi, [rip + _err_fieldovf]
    xor esi, esi
    call _rt_error

# _rt_lset - LSET: copy left-justified into a fixed-width field
# The destination keeps its address and width; only the bytes change. A short
# source is padded with spaces, a long one is truncated on the right.
#
# Arguments:
#   rdi = destination pointer, rsi = destination length
#   rdx = source pointer,      rcx = source length
.globl _rt_lset
_rt_lset:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    sub rsp, 8

    mov rbx, rdi            # dest ptr
    mov r12, rsi            # dest len
    mov r13, rcx            # source len

    test r12, r12
    jle .Llset_done

    # n = min(source length, destination width)
    cmp r13, r12
    cmova r13, r12
    test r13, r13
    jle .Llset_pad

    mov rdi, rbx
    mov rsi, rdx
    mov rdx, r13
    call memcpy

.Llset_pad:
    # Blank out whatever the source did not fill.
    mov rdi, rbx
    add rdi, r13
    mov esi, ' '
    mov rdx, r12
    sub rdx, r13
    test rdx, rdx
    jle .Llset_done
    call memset

.Llset_done:
    add rsp, 8
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_rset - RSET: the same, right-justified
# Arguments: as _rt_lset
.globl _rt_rset
_rt_rset:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14

    mov rbx, rdi            # dest ptr
    mov r12, rsi            # dest len
    mov r14, rdx            # source ptr
    mov r13, rcx            # source len

    test r12, r12
    jle .Lrset_done

    cmp r13, r12
    cmova r13, r12
    test r13, r13
    jl .Lrset_blank

    # Pad on the left, then place the text against the right edge.
    mov rdi, rbx
    mov esi, ' '
    mov rdx, r12
    sub rdx, r13
    test rdx, rdx
    jle .Lrset_copy
    call memset

.Lrset_copy:
    test r13, r13
    jle .Lrset_done
    mov rdi, rbx
    add rdi, r12
    sub rdi, r13            # dest + width - n
    mov rsi, r14
    mov rdx, r13
    call memcpy
    jmp .Lrset_done

.Lrset_blank:
    mov rdi, rbx
    mov esi, ' '
    mov rdx, r12
    call memset

.Lrset_done:
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_random_prepare - Shared GET/PUT setup: validate and seek
# Arguments:
#   rdi = file number, rsi = record number (0 = the one after the last)
#   rdx = BASIC line number
#
# Returns: rax = FILE*, rbx = file number, r12 = record length, r13 = buffer
# Clobbers the callee-saved registers it returns in; both callers save them.
#
# Reached by `call`, so it keeps its own frame: without one, rsp would still be
# 8 (mod 16) at the fseek below, which is exactly the misalignment that faults
# a `movaps` spill inside the CRT.
.globl _rt_random_prepare
_rt_random_prepare:
    push rbp
    mov rbp, rsp

    mov ebx, edi
    mov r14, rdx            # line number, for the error paths

    lea rax, [rip + _file_handles]
    mov r15, [rax + rbx*8]
    test r15, r15
    jz .Lrandom_badfile

    lea rax, [rip + _file_recbuf]
    mov r13, [rax + rbx*8]
    test r13, r13
    jz .Lrandom_badmode

    lea rax, [rip + _file_reclen]
    mov r12, [rax + rbx*8]

    # No record number means the one after the last one touched.
    test rsi, rsi
    jg .Lrandom_have_rec
    lea rax, [rip + _file_recnum]
    mov rsi, [rax + rbx*8]
    inc rsi
.Lrandom_have_rec:

    lea rax, [rip + _file_recnum]
    mov [rax + rbx*8], rsi

    # Records are 1-based, so record n starts at (n-1) * reclen.
    dec rsi
    mov rax, rsi
    imul rax, r12
    mov rdi, r15
    mov rsi, rax
    xor edx, edx            # SEEK_SET
    call fseek

    mov rax, r15
    leave
    ret

.Lrandom_badfile:
    lea rdi, [rip + _err_badfile]
    mov rsi, r14
    call _rt_error

.Lrandom_badmode:
    lea rdi, [rip + _err_badmode]
    mov rsi, r14
    call _rt_error

# _rt_file_get - GET #n[, rec]: read one record into the buffer
# A record reaching past the end of the file is padded with NULs, so a field
# read from beyond the data is empty rather than holding the previous record.
#
# Arguments:
#   rdi = file number, rsi = record number (0 = next), rdx = BASIC line
.globl _rt_file_get
_rt_file_get:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 8

    call _rt_random_prepare

    # fread(buffer, 1, reclen, file)
    mov rdi, r13
    mov rsi, 1
    mov rdx, r12
    mov rcx, r15
    call fread

    # Zero whatever the file was too short to supply.
    mov rdx, r12
    sub rdx, rax
    jle .Lget_done
    mov rdi, r13
    add rdi, rax
    xor esi, esi
    call memset

.Lget_done:
    add rsp, 8
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_file_put - PUT #n[, rec]: write the buffer as one record
# Arguments: as _rt_file_get
.globl _rt_file_put
_rt_file_put:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 8

    call _rt_random_prepare

    # fwrite(buffer, 1, reclen, file)
    mov rdi, r13
    mov rsi, 1
    mov rdx, r12
    mov rcx, r15
    call fwrite

    # Flush so that LOF and a reader on another handle see the record now.
    mov rdi, r15
    call fflush

    add rsp, 8
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# _rt_file_loc - LOC(n): where the file is positioned
# For a random file this is the last record read or written. For a sequential
# one GW-BASIC counts 128-byte blocks, which is what this reports.
#
# Arguments: rdi = file number
# Returns:   xmm0 = the position
.globl _rt_file_loc
_rt_file_loc:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi

    lea rax, [rip + _file_recbuf]
    mov rax, [rax + rbx*8]
    test rax, rax
    jz .Lloc_sequential

    lea rax, [rip + _file_recnum]
    mov rax, [rax + rbx*8]
    cvtsi2sd xmm0, rax
    jmp .Lloc_done

.Lloc_sequential:
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    test rdi, rdi
    jz .Lloc_zero
    call ftell
    # GW-BASIC reports sequential position in 128-byte blocks.
    mov rcx, 128
    xor edx, edx
    test rax, rax
    js .Lloc_zero
    div rcx
    cvtsi2sd xmm0, rax
    jmp .Lloc_done

.Lloc_zero:
    xorpd xmm0, xmm0

.Lloc_done:
    add rsp, 8
    pop rbx
    leave
    ret

# _rt_lock / _rt_unlock - LOCK and UNLOCK: advisory record locking
#
# POSIX advisory locks through fcntl(F_SETLK). "Advisory" is the honest word:
# they hold against other processes that also lock, which is exactly what
# GW-BASIC's LOCK was for, and they do nothing against a process that simply
# writes.
#
# Arguments:
#   rdi = file number, rsi = first record, rdx = last record
#   rcx = BASIC line number
# A first record of 0 means the whole file.
.equ F_SETLK,  6
.equ F_RDLCK,  0
.equ F_WRLCK,  1
.equ F_UNLCK,  2

.globl _rt_lock
_rt_lock:
    mov r8d, F_WRLCK
    jmp .Ldo_lock

.globl _rt_unlock
_rt_unlock:
    mov r8d, F_UNLCK

.Ldo_lock:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    sub rsp, 48                     # struct flock is 32 bytes; 48 keeps rsp
                                    # 16-byte aligned for the calls below

    mov ebx, edi
    mov r13, rcx                    # BASIC line
    mov r14d, r8d                   # lock type

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    test rdi, rdi
    jz .Llock_badfile
    call fileno
    mov r12d, eax                   # file descriptor

    # Build struct flock in the scratch space just reserved. Addressing it from
    # rsp keeps it clear of the saved registers sitting below rbp.
    mov rcx, rsp
    mov WORD PTR [rcx], r14w        # l_type
    mov WORD PTR [rcx + 2], 0       # l_whence = SEEK_SET
    mov DWORD PTR [rcx + 4], 0      # padding
    mov QWORD PTR [rcx + 24], 0     # l_pid

    # A first record of 0 locks the whole file, which fcntl spells as
    # start 0, length 0.
    test rsi, rsi
    jle .Llock_whole

    lea rax, [rip + _file_reclen]
    mov r8, [rax + rbx*8]
    test r8, r8
    jz .Llock_whole                 # sequential: no records to address

    # Byte range [ (first-1)*reclen, (last-first+1)*reclen )
    mov rax, rsi
    dec rax
    imul rax, r8
    mov QWORD PTR [rcx + 8], rax    # l_start

    sub rdx, rsi
    inc rdx
    test rdx, rdx
    jg .Llock_have_count
    mov rdx, 1
.Llock_have_count:
    imul rdx, r8
    mov QWORD PTR [rcx + 16], rdx   # l_len
    jmp .Llock_call

.Llock_whole:
    mov QWORD PTR [rcx + 8], 0
    mov QWORD PTR [rcx + 16], 0

.Llock_call:
    mov edi, r12d
    mov esi, F_SETLK
    mov rdx, rcx
    xor eax, eax
    call fcntl
    test eax, eax
    js .Llock_denied

    add rsp, 48
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

.Llock_denied:
    lea rdi, [rip + _err_permission]
    mov rsi, r13
    call _rt_error

.Llock_badfile:
    lea rdi, [rip + _err_badfile]
    mov rsi, r13
    call _rt_error

# _rt_mk - MKI$/MKL$/MKS$/MKD$: a number's bytes, as a string
# The value arrives as raw bits so one helper serves all four widths: the
# caller has already coerced it to the type whose width it passes.
#
# Arguments:
#   rdi = the value's bits, rsi = width in bytes (2, 4 or 8)
#
# Returns: rax = pointer to a fresh heap copy of the bytes, rdx = width
# (_mk_buf is scratch and never leaves the runtime, so that two MK*$ calls in
# one expression cannot alias each other.)
.globl _rt_mk
_rt_mk:
    lea rax, [rip + _mk_buf]
    mov QWORD PTR [rax], rdi        # little-endian, so a short width just
                                    # takes the low bytes
    mov rdi, rax
    jmp _rt_strdup                  # rsi already holds the width

# _rt_cvi / _rt_cvl / _rt_cvs / _rt_cvd - read MK*$ bytes back as a number
#
# The string must be at least as wide as the type being read. Zero-padding a
# short one instead would be worse than useless: two characters read as a
# DOUBLE do not give 0, they give a denormal near 1e-319, which is a plausible
# number no program would ever notice was wrong. GW-BASIC calls this an illegal
# function call, and so does this.
#
# Arguments: rdi = string pointer, rsi = string length, rdx = BASIC line
# Returns:   xmm0 = the value
.globl _rt_cvi
_rt_cvi:
    push rbp
    mov rbp, rsp
    cmp rsi, 2
    jb .Lcv_short
    mov edx, 2
    call .Lcv_gather
    movsx rax, ax                   # INTEGER is signed 16-bit
    cvtsi2sd xmm0, rax
    leave
    ret

.globl _rt_cvl
_rt_cvl:
    push rbp
    mov rbp, rsp
    cmp rsi, 4
    jb .Lcv_short
    mov edx, 4
    call .Lcv_gather
    movsxd rax, eax                 # LONG is signed 32-bit
    cvtsi2sd xmm0, rax
    leave
    ret

.globl _rt_cvs
_rt_cvs:
    push rbp
    mov rbp, rsp
    cmp rsi, 4
    jb .Lcv_short
    mov edx, 4
    call .Lcv_gather
    movd xmm0, eax
    cvtss2sd xmm0, xmm0
    leave
    ret

.globl _rt_cvd
_rt_cvd:
    push rbp
    mov rbp, rsp
    cmp rsi, 8
    jb .Lcv_short
    mov edx, 8
    call .Lcv_gather
    movq xmm0, rax
    leave
    ret

# Reached before rdx is reused as the width, so it still holds the line number.
.Lcv_short:
    lea rdi, [rip + _err_domain]
    mov rsi, rdx
    call _rt_error

# Gather rdx bytes of the string at rdi into rax, low byte first.
.Lcv_gather:
    xor eax, eax
    test rdi, rdi
    jz .Lcv_gather_done
    test rdx, rdx
    jle .Lcv_gather_done
    xor ecx, ecx
.Lcv_gather_loop:
    movzx r8d, BYTE PTR [rdi + rcx]
    mov r9d, ecx
    shl r9d, 3                      # bit position = index * 8
    mov r10, r8
    mov r11, rcx
    mov rcx, r9
    shl r10, cl
    mov rcx, r11
    or rax, r10
    inc rcx
    cmp rcx, rdx
    jl .Lcv_gather_loop
.Lcv_gather_done:
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
    call ftell
    mov r12, rax                    # saved position

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    xor esi, esi
    mov edx, 2                      # SEEK_END
    call fseek

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    call ftell
    mov QWORD PTR [rbp - 24], rax   # length; pushing it here would misalign
                                    # rsp for the fseek below

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]
    mov rsi, r12
    xor edx, edx                    # SEEK_SET
    call fseek

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

# Shared tail for a file number that was never OPENed.
#
# PRINT # and LINE INPUT # used to load the NULL handle and hand it straight to
# fprintf/fgets, so `PRINT #3, "x"` on an unopened number died with SIGSEGV --
# no message, exit 139. _rt_file_getc and _rt_file_eof already guarded; these
# did not. The line number is unknown here (these helpers are not given one),
# and _rt_error prints a bare message for 0.
.Lfile_not_open:
    lea rdi, [rip + _err_badfile]
    xor esi, esi
    call _rt_error
