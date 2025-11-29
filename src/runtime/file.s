# ==============================================================================
# BASIC Runtime: File I/O Functions
# ==============================================================================
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
# ==============================================================================

# ------------------------------------------------------------------------------
# Data Section: File handle table and buffers
# ------------------------------------------------------------------------------
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

# Buffer for string input from files
_file_input_buf: .skip 1024

.text

# ------------------------------------------------------------------------------
# _rt_file_open - Open a file (OPEN statement)
# ------------------------------------------------------------------------------
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
# ------------------------------------------------------------------------------
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

# ------------------------------------------------------------------------------
# _rt_file_close - Close a file (CLOSE statement)
# ------------------------------------------------------------------------------
# Closes the file associated with a file number and clears its handle.
#
# Arguments:
#   rdi = file number (1-15)
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_close
_rt_file_close:
    push rbp
    mov rbp, rsp

    # Get FILE* from handle table
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rdi*8]  # rdi = FILE*
    test rdi, rdi           # Check for NULL (already closed or never opened)
    jz .Lclose_done
    call {libc}fclose
.Lclose_done:
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_string - Write string to file (PRINT# with string)
# ------------------------------------------------------------------------------
# Arguments:
#   rdi = file number
#   rsi = string pointer
#   rdx = string length
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_string
_rt_file_print_string:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8              # Align stack to 16 bytes

    mov ebx, edi            # save file number
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

    add rsp, 8
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_float - Write number to file (PRINT# with number)
# ------------------------------------------------------------------------------
# Like _rt_print_float, prints whole numbers as integers for clean output.
#
# Arguments:
#   rdi = file number
#   xmm0 = value to write (double)
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_float
_rt_file_print_float:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi            # save file number

    # Check if value is a whole number
    cvttsd2si rax, xmm0     # truncate to integer
    cvtsi2sd xmm1, rax      # convert back
    ucomisd xmm0, xmm1      # compare
    jne .Lfile_print_as_float

    # Print as integer (cleaner output)
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # FILE*
    lea rsi, [rip + _file_fmt_int]
    cvttsd2si rdx, xmm0     # integer value
    xor eax, eax
    call {libc}fprintf
    jmp .Lfile_print_float_done

.Lfile_print_as_float:
    # Print as floating point
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # FILE*
    lea rsi, [rip + _file_fmt_float]
    mov eax, 1              # 1 vector register arg
    call {libc}fprintf

.Lfile_print_float_done:
    add rsp, 8
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_char - Write single character to file
# ------------------------------------------------------------------------------
# Arguments:
#   rdi = file number
#   rsi = character code
#
# Returns: nothing
# ------------------------------------------------------------------------------
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

    pop r12
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_print_newline - Write newline to file
# ------------------------------------------------------------------------------
# Called at end of PRINT# statement unless suppressed with ; or ,
#
# Arguments:
#   rdi = file number
#
# Returns: nothing
# ------------------------------------------------------------------------------
.globl _rt_file_print_newline
_rt_file_print_newline:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8              # align stack

    mov ebx, edi            # save file number

    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # FILE*
    lea rsi, [rip + _file_fmt_newline]
    xor eax, eax
    call {libc}fprintf

    add rsp, 8
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_input_number - Read number from file (INPUT# with number)
# ------------------------------------------------------------------------------
# Arguments:
#   rdi = file number
#
# Returns:
#   xmm0 = value read (double)
# ------------------------------------------------------------------------------
.globl _rt_file_input_number
_rt_file_input_number:
    push rbp
    mov rbp, rsp
    push rbx
    sub rsp, 8

    mov ebx, edi            # save file number

    # fscanf(file, "%lf", &result)
    lea rax, [rip + _file_handles]
    mov rdi, [rax + rbx*8]  # FILE*
    lea rsi, [rip + _file_fmt_input]  # format "%lf"
    lea rdx, [rbp - 16]     # pointer to local variable for result
    xor eax, eax
    call {libc}fscanf

    # Load result into xmm0
    movsd xmm0, QWORD PTR [rbp - 16]
    add rsp, 8
    pop rbx
    leave
    ret

# ------------------------------------------------------------------------------
# _rt_file_input_string - Read string from file (INPUT# with string, or LINE INPUT#)
# ------------------------------------------------------------------------------
# Reads a line from file, stripping trailing newline.
#
# Arguments:
#   rdi = file number
#
# Returns:
#   rax = pointer to string data (_file_input_buf)
#   rdx = string length
#
# Note: Uses static buffer - result only valid until next file string read.
# ------------------------------------------------------------------------------
.globl _rt_file_input_string
_rt_file_input_string:
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

    # Strip trailing newline if present
    test rdx, rdx
    jz .Lfile_input_string_done
    lea rax, [rip + _file_input_buf]
    mov cl, BYTE PTR [rax + rdx - 1]    # last character
    cmp cl, 10              # newline?
    jne .Lfile_input_string_done
    dec rdx                 # reduce length
    mov BYTE PTR [rax + rdx], 0         # remove newline

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
