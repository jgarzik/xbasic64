# Runtime data section definitions (Win64 Native)
#
# Shared data definitions used across runtime modules.
# Format strings are for UCRT sprintf, not console output.
#
# Note: Console I/O uses Win32 WriteFile/ReadFile directly,
# but we still use sprintf for number-to-string formatting.

.data

# Format strings for sprintf (number formatting)
_fmt_int: .asciz "%lld"
_fmt_float: .asciz "%g"
# _rt_fmt_double's request: P significant digits in exponent form.
_fmt_sci: .asciz "%.*e"
_redo_msg: .ascii "?Redo from start\r\n"
# Bracketed rather than measured by the assembler; see _cls_seq in math.s.
_redo_msg_end:

# Error messages
# Lengths are computed by the assembler (. - label), never hand-counted: a
# hand-counted value here was wrong by one, making WriteFile emit a stray byte.

# Scratch buffers. These start as zeros, so they belong in .bss: in .data
# every one of these bytes is a zero stored in the executable itself, which
# came to 8K of the image across the whole runtime.
#
# Safe to leave .bss active at the end of this file, and only here: runtime.rs
# concatenates data_defs.s and then emits .text unconditionally before the
# first code-bearing part. Every other runtime file must restore .text itself.
.bss
.p2align 3
_num_buf: .skip 64
_fmt_scratch: .skip 64          # _rt_fmt_double's printf output, before reshaping
_fmt_digits: .skip 32           # ... and its significant digits
_console_mode: .skip 4          # GetConsoleMode's output, for the VT enable

