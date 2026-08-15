# ==============================================================================
# BASIC Runtime: File I/O Functions (Win64 Native - Pure Win32 API)
# ==============================================================================
#
# File input/output functions using Win32 API instead of libc stdio.
# Uses CreateFileA, CloseHandle, WriteFile, ReadFile.
#
# File Handle Table:
#   _file_handles is an array of 16 HANDLE values (128 bytes).
#   Index 0 is unused (BASIC file numbers start at 1).
#   Handles 1-15 are available for user files.
#
# Win64 ABI:
#   - Args: rcx, rdx, r8, r9 (then stack)
#   - Callee-saved: rbx, rbp, rdi, rsi, r12-r15
#   - 32-byte shadow space required before calls
# ==============================================================================

# Win32 API Constants
.equ GENERIC_READ,          0x80000000
.equ GENERIC_WRITE,         0x40000000
.equ FILE_SHARE_READ,       1
.equ CREATE_ALWAYS,         2
.equ OPEN_EXISTING,         3
.equ OPEN_ALWAYS,           4
.equ FILE_ATTRIBUTE_NORMAL, 0x80
.equ INVALID_HANDLE_VALUE,  -1
.equ FILE_END,              2

# ASCII character codes
.equ CHAR_LF,               10
.equ CHAR_CR,               13

# BASIC file modes (from codegen)
.equ MODE_INPUT,            0
.equ MODE_OUTPUT,           1
.equ MODE_APPEND,           2

# Buffer size constants
.equ INPUT_BUF_SIZE,        1024
.equ MAX_NUM_INPUT_LEN,     254     # INPUT_BUF_SIZE - 2 (null + safety)
.equ MAX_STR_INPUT_LEN,     1022    # INPUT_BUF_SIZE - 2 (null + safety)

# I/O size constants
.equ SINGLE_BYTE,           1
.equ CRLF_LEN,              2

.data
_file_handles: .skip 128        # 16 * 8 bytes = 16 HANDLEs
_file_name_buf: .skip 1024      # Buffer for null-terminated filename
_file_output_buf: .skip 256     # Buffer for formatted output
_file_bytes_written: .quad 0    # For WriteFile output
_file_bytes_read: .quad 0       # For ReadFile output
_file_input_buf: .skip 1024     # Buffer for file input
_file_getc_buf:  .skip 8        # One byte at a time, for _rt_file_getc
_file_fmt_int:     .asciz "%lld"
_file_fmt_float:   .asciz "%g"
_file_newline:     .ascii "\r\n"

.text

# ------------------------------------------------------------------------------
# _rt_file_open - Open a file (OPEN statement)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = filename pointer (BASIC string, not null-terminated)
#   rdx = filename length
#   r8  = mode: 0=INPUT, 1=OUTPUT, 2=APPEND
#   r9  = file number (1-15)
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_open
_rt_file_open:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push rdi
    push rsi
    sub rsp, 80             # Shadow space + stack args (must be 0 mod 16)

    # Save arguments
    mov rdi, rcx            # filename ptr
    mov rsi, rdx            # filename len
    mov r14d, r8d           # mode (0/1/2)
    mov ebx, r9d            # file number

    # Copy filename and null-terminate
    lea rcx, [rip + _file_name_buf]
    mov rdx, rdi            # src
    mov r8, rsi             # len
    call memcpy
    lea rax, [rip + _file_name_buf]
    mov BYTE PTR [rax + rsi], 0

    # Determine access and creation mode based on mode argument
    # r12 = dwDesiredAccess, r13 = dwCreationDisposition
    cmp r14d, MODE_INPUT
    je .Lfile_mode_read
    cmp r14d, MODE_OUTPUT
    je .Lfile_mode_write
    # else: append
    mov r12d, GENERIC_WRITE
    mov r13d, OPEN_ALWAYS
    jmp .Ldo_create_file

.Lfile_mode_read:
    mov r12d, GENERIC_READ
    mov r13d, OPEN_EXISTING
    jmp .Ldo_create_file

.Lfile_mode_write:
    mov r12d, GENERIC_WRITE
    mov r13d, CREATE_ALWAYS

.Ldo_create_file:
    # CreateFileA(lpFileName, dwDesiredAccess, dwShareMode,
    #             lpSecurityAttributes, dwCreationDisposition,
    #             dwFlagsAndAttributes, hTemplateFile)
    lea rcx, [rip + _file_name_buf]     # lpFileName
    mov edx, r12d                        # dwDesiredAccess
    mov r8d, FILE_SHARE_READ             # dwShareMode
    xor r9d, r9d                         # lpSecurityAttributes = NULL
    mov DWORD PTR [rsp + 32], r13d       # dwCreationDisposition
    mov DWORD PTR [rsp + 40], FILE_ATTRIBUTE_NORMAL
    mov QWORD PTR [rsp + 48], 0          # hTemplateFile = NULL
    call CreateFileA

    # Store HANDLE in handle table
    lea rcx, [rip + _file_handles]
    mov [rcx + rbx*8], rax

    # If APPEND mode, seek to end
    cmp r14d, MODE_APPEND
    jne .Lfile_open_done

    # SetFilePointer(hFile, 0, NULL, FILE_END)
    mov rcx, rax            # hFile
    xor edx, edx            # lDistanceToMove = 0
    xor r8d, r8d            # lpDistanceToMoveHigh = NULL
    mov r9d, FILE_END       # dwMoveMethod
    call SetFilePointer

.Lfile_open_done:
    add rsp, 80
    pop rsi
    pop rdi
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_close - Close a file (CLOSE statement)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number (1-15)
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_close
_rt_file_close:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40             # Shadow space + alignment

    mov ebx, ecx            # save file number

    # Get HANDLE from table
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]

    # Check for NULL/INVALID
    test rcx, rcx
    jz .Lfile_close_done
    cmp rcx, INVALID_HANDLE_VALUE
    je .Lfile_close_done

    # CloseHandle(hFile)
    call CloseHandle

    # Clear handle from table
    lea rax, [rip + _file_handles]
    mov QWORD PTR [rax + rbx*8], 0

.Lfile_close_done:
    add rsp, 40
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_string - Write string to file
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#   rdx = string pointer
#   r8  = string length
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_string
_rt_file_print_string:
    push rbp
    mov rbp, rsp
    push rbx
    push rdi
    push rsi
    sub rsp, 40             # Shadow space + stack arg

    # An unassigned string is (NULL, 0); writing nothing is correct and avoids
    # handing WriteFile a NULL buffer.
    test r8, r8
    jz .Lfile_print_str_done

    mov ebx, ecx            # save file number
    mov rdi, rdx            # save string ptr
    mov rsi, r8             # save string len

    # Get HANDLE from table
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]  # hFile

    # WriteFile(hFile, lpBuffer, nNumberOfBytesToWrite, lpNumberOfBytesWritten, lpOverlapped)
    mov rdx, rdi            # lpBuffer = string ptr
    mov r8, rsi             # nNumberOfBytesToWrite = length
    lea r9, [rip + _file_bytes_written]
    mov QWORD PTR [rsp + 32], 0  # lpOverlapped = NULL
    call WriteFile

.Lfile_print_str_done:
    add rsp, 40
    pop rsi
    pop rdi
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_float - Write number to file
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#   xmm0 = value to write (double)
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_float
_rt_file_print_float:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 48             # Shadow space + alignment

    mov ebx, ecx            # save file number

    # Format through the shared helper, so file output matches console output
    # digit for digit.
    lea rcx, [rip + _fmt_g_table]
    xor edx, edx
    call _rt_fmt_double
    mov r12, rax            # length

    # Get HANDLE from table
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]  # hFile

    # WriteFile(hFile, buffer, length, &bytesWritten, NULL)
    lea rdx, [rip + _num_buf]
    mov r8, r12             # length
    lea r9, [rip + _file_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    add rsp, 48
    pop r12
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_char - Write single character to file
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#   rdx = character code
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_char
_rt_file_print_char:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40             # Shadow space + stack arg

    mov ebx, ecx            # save file number

    # Store char in buffer
    lea rax, [rip + _file_output_buf]
    mov [rax], dl

    # Get HANDLE
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]  # hFile

    # WriteFile(hFile, buffer, 1, &bytesWritten, NULL)
    lea rdx, [rip + _file_output_buf]
    mov r8, SINGLE_BYTE
    lea r9, [rip + _file_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    add rsp, 40
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_newline - Write CRLF newline to file
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_newline
_rt_file_print_newline:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40             # Shadow space + stack arg

    mov ebx, ecx            # save file number

    # Get HANDLE
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]  # hFile

    # WriteFile(hFile, "\r\n", CRLF_LEN, &bytesWritten, NULL)
    lea rdx, [rip + _file_newline]
    mov r8, CRLF_LEN
    lea r9, [rip + _file_bytes_written]
    mov QWORD PTR [rsp + 32], 0
    call WriteFile

    add rsp, 40
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_getc - Read one byte
# ------------------------------------------------------------------------------
# Arguments: rcx = file number
# Returns:   eax = the byte, or -1 at end of file or on a file that is not open
# ------------------------------------------------------------------------------
.globl _rt_file_getc
_rt_file_getc:
    push rbp
    mov rbp, rsp
    sub rsp, 48                 # shadow space + the 5th argument

    lea rax, [rip + _file_handles]
    mov rcx, [rax + rcx*8]
    test rcx, rcx
    jz .Lfile_getc_eof
    lea rdx, [rip + _file_getc_buf]
    mov r8, SINGLE_BYTE
    lea r9, [rip + _file_bytes_read]
    mov QWORD PTR [rsp + 32], 0
    call ReadFile

    lea rax, [rip + _file_bytes_read]
    mov rax, [rax]
    test rax, rax
    jz .Lfile_getc_eof
    lea rax, [rip + _file_getc_buf]
    movzx eax, BYTE PTR [rax]
    jmp .Lfile_getc_done
.Lfile_getc_eof:
    mov eax, -1
.Lfile_getc_done:
    add rsp, 48
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_read_field - Read one INPUT # field
# ------------------------------------------------------------------------------
# INPUT # reads comma-delimited fields, not whole lines. Reading a line per
# variable made `INPUT #1, A, B` on "10,20" yield 10 twice, and left the second
# line unread.
#
# Leading blanks and line breaks are skipped. A field either is quoted, in
# which case it runs to the closing quote and everything up to the next
# delimiter is discarded, or runs to the next comma, newline or end of file
# with trailing blanks trimmed. The delimiter is consumed.
#
# Arguments:
#   rcx = file number
#
# Returns:
#   rax = pointer to the field (in _file_input_buf), rdx = its length
#
# Note: uses a static buffer, so the result is valid only until the next read.
# ------------------------------------------------------------------------------
.globl _rt_file_read_field
_rt_file_read_field:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    push r12
    sub rsp, 40                 # three pushes plus the return address

    mov ebx, ecx                # file number, reloaded before every read
    xor r12d, r12d              # length written so far

.Lfield_skip:
    mov ecx, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_done
    cmp eax, ' '
    je .Lfield_skip
    cmp eax, 9                  # tab
    je .Lfield_skip
    cmp eax, CHAR_CR
    je .Lfield_skip
    cmp eax, CHAR_LF
    je .Lfield_skip

    cmp eax, '"'
    je .Lfield_quoted

    mov esi, eax
.Lfield_plain_store:
    cmp r12d, MAX_STR_INPUT_LEN
    jge .Lfield_plain_next
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], sil
    inc r12d
.Lfield_plain_next:
    mov ecx, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_trim
    cmp eax, ','
    je .Lfield_trim
    cmp eax, CHAR_LF
    je .Lfield_trim
    cmp eax, CHAR_CR
    je .Lfield_plain_next
    mov esi, eax
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
    mov ecx, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_done
    cmp eax, '"'
    je .Lfield_after_quote
    cmp r12d, MAX_STR_INPUT_LEN
    jge .Lfield_quoted
    mov esi, eax
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], sil
    inc r12d
    jmp .Lfield_quoted

.Lfield_after_quote:
    # Discard whatever separates the closing quote from the delimiter.
    mov ecx, ebx
    call _rt_file_getc
    cmp eax, -1
    je .Lfield_done
    cmp eax, ','
    je .Lfield_done
    cmp eax, CHAR_LF
    je .Lfield_done
    jmp .Lfield_after_quote

.Lfield_done:
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], 0
    mov rdx, r12

    add rsp, 40
    pop r12
    pop rsi
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_input_number - INPUT #: read one numeric field
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#
# Returns:
#   xmm0 = value read, or 0.0 for a field that is not a number
# ------------------------------------------------------------------------------
.globl _rt_file_input_number
_rt_file_input_number:
    push rbp
    mov rbp, rsp
    sub rsp, 32                 # shadow space

    call _rt_file_read_field
    mov rcx, rax                # NUL-terminated by _rt_file_read_field
    xor edx, edx                # endptr = NULL
    call strtod

    add rsp, 32
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_input_string - INPUT #: read one string field
# ------------------------------------------------------------------------------
# Arguments: rcx = file number
# Returns:   rax = pointer, rdx = length
# ------------------------------------------------------------------------------
.globl _rt_file_input_string
_rt_file_input_string:
    jmp _rt_file_read_field

# ------------------------------------------------------------------------------
# _rt_file_line_input - LINE INPUT #: read a whole line
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#
# Returns:
#   rax = pointer to string data (_file_input_buf)
#   rdx = string length
# ------------------------------------------------------------------------------
.globl _rt_file_line_input
_rt_file_line_input:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 48             # Shadow space + stack arg (must be 0 mod 16)

    mov ebx, ecx            # save file number

    # Clear buffer
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax], 0

    # Read one character at a time until newline or EOF
    xor r12d, r12d          # r12 = position in buffer

.Lfile_input_str_loop:
    # Check buffer overflow
    cmp r12d, MAX_STR_INPUT_LEN
    jge .Lfile_input_str_done

    # ReadFile(hFile, &buffer[pos], 1, &bytesRead, NULL)
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]  # hFile
    lea rdx, [rip + _file_input_buf]
    add rdx, r12            # &buffer[pos]
    mov r8, SINGLE_BYTE
    lea r9, [rip + _file_bytes_read]
    mov QWORD PTR [rsp + 32], 0
    call ReadFile

    # Check if we read anything
    lea rax, [rip + _file_bytes_read]
    mov rax, [rax]
    test rax, rax
    jz .Lfile_input_str_done    # EOF

    # Check if it's a newline
    lea rax, [rip + _file_input_buf]
    mov cl, BYTE PTR [rax + r12]
    cmp cl, CHAR_LF
    je .Lfile_input_str_done
    cmp cl, CHAR_CR         # CR - skip it
    je .Lfile_input_str_loop

    inc r12d                # next position
    jmp .Lfile_input_str_loop

.Lfile_input_str_done:
    # Null-terminate
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], 0
    mov rdx, r12            # length

    add rsp, 48
    pop r12
    pop rbx
    leave
    ret


# ------------------------------------------------------------------------------
# _rt_file_close_all - Close every open file (bare CLOSE)
# ------------------------------------------------------------------------------
# Arguments: none
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_close_all
_rt_file_close_all:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40

    mov ebx, 1              # BASIC file numbers start at 1
.Lclose_all_loop:
    cmp ebx, 15
    jg .Lclose_all_done
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]
    test rcx, rcx
    jz .Lclose_all_next
    call CloseHandle
    lea rax, [rip + _file_handles]
    mov QWORD PTR [rax + rbx*8], 0
.Lclose_all_next:
    inc ebx
    jmp .Lclose_all_loop
.Lclose_all_done:
    add rsp, 40
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_eof - EOF(n): has the file been read to the end?
# ------------------------------------------------------------------------------
# Compares the current position against the file size, which needs no
# pushback and so keeps the next read unaffected.
#
# Arguments:
#   rcx = file number
#
# Returns:
#   eax = -1 at end of file, 0 otherwise (BASIC's true/false, as a LONG)
# ------------------------------------------------------------------------------
.globl _rt_file_eof
_rt_file_eof:
    push rbp
    mov rbp, rsp
    push rbx
    push rsi
    sub rsp, 48    # two pushes plus the return address, so 48 realigns

    mov ebx, ecx
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]
    test rcx, rcx
    jz .Leof_true           # never opened: treat as at end

    # Current position: SetFilePointer(h, 0, NULL, FILE_CURRENT)
    xor edx, edx
    xor r8, r8
    mov r9d, 1              # FILE_CURRENT
    call SetFilePointer
    mov rsi, rax            # position

    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]
    xor edx, edx
    call GetFileSize

    cmp rsi, rax
    jb .Leof_false
.Leof_true:
    mov eax, -1
    jmp .Leof_done
.Leof_false:
    xor eax, eax
.Leof_done:
    add rsp, 48
    pop rsi
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_lof - LOF(n): length of the file in bytes
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#
# Returns:
#   xmm0 = length in bytes, or 0.0 when the file is not open
# ------------------------------------------------------------------------------
.globl _rt_file_lof
_rt_file_lof:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 40

    mov ebx, ecx
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]
    test rcx, rcx
    jz .Llof_zero

    xor edx, edx
    call GetFileSize
    cvtsi2sd xmm0, rax
    jmp .Llof_done

.Llof_zero:
    xorpd xmm0, xmm0

.Llof_done:
    add rsp, 40
    pop rbx
    leave
    ret
