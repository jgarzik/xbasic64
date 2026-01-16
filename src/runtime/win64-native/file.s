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

    # Check if value is a whole number
    cvttsd2si rax, xmm0     # truncate to integer
    cvtsi2sd xmm1, rax      # convert back
    ucomisd xmm0, xmm1      # compare
    jne .Lfile_print_as_float

    # Format as integer using sprintf
    lea rcx, [rip + _file_output_buf]
    lea rdx, [rip + _file_fmt_int]
    mov r8, rax             # integer value
    call sprintf
    jmp .Lfile_print_formatted

.Lfile_print_as_float:
    # Format as float using sprintf
    lea rcx, [rip + _file_output_buf]
    lea rdx, [rip + _file_fmt_float]
    movsd xmm2, xmm0        # value in xmm2
    movq r8, xmm0           # also in r8 for varargs
    call sprintf

.Lfile_print_formatted:
    mov r12, rax            # save length from sprintf

    # Get HANDLE from table
    lea rax, [rip + _file_handles]
    mov rcx, [rax + rbx*8]  # hFile

    # WriteFile(hFile, buffer, length, &bytesWritten, NULL)
    lea rdx, [rip + _file_output_buf]
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
# _rt_file_input_number - Read number from file
# ------------------------------------------------------------------------------
# Reads one line (up to newline) and parses as number.
#
# Arguments:
#   rcx = file number
#
# Returns:
#   xmm0 = value read (double)
# ------------------------------------------------------------------------------
.globl _rt_file_input_number
_rt_file_input_number:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    sub rsp, 48             # Shadow space + stack arg (must be 0 mod 16)

    mov ebx, ecx            # save file number
    xor r12d, r12d          # r12 = position in buffer

.Lfile_input_num_loop:
    # Check buffer overflow
    cmp r12d, MAX_NUM_INPUT_LEN
    jge .Lfile_input_num_parse

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
    jz .Lfile_input_num_parse   # EOF

    # Check if it's a newline
    lea rax, [rip + _file_input_buf]
    mov cl, BYTE PTR [rax + r12]
    cmp cl, CHAR_LF
    je .Lfile_input_num_parse
    cmp cl, CHAR_CR         # CR - skip it
    je .Lfile_input_num_loop

    inc r12d                # next position
    jmp .Lfile_input_num_loop

.Lfile_input_num_parse:
    # Null-terminate
    lea rax, [rip + _file_input_buf]
    mov BYTE PTR [rax + r12], 0

    # Parse number using strtod(buffer, NULL)
    lea rcx, [rip + _file_input_buf]
    xor rdx, rdx            # endptr = NULL
    call strtod

    # Result in xmm0
    add rsp, 48
    pop r12
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_input_string - Read string from file (line)
# ------------------------------------------------------------------------------
# Arguments:
#   rcx = file number
#
# Returns:
#   rax = pointer to string data (_file_input_buf)
#   rdx = string length
# ------------------------------------------------------------------------------
.globl _rt_file_input_string
_rt_file_input_string:
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

