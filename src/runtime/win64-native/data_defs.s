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
# Shortest-round-trip formats, tried in order until the printed text reads
# back as the identical double. %g alone gives only 6 significant digits,
# which discards most of a Double's precision.
_fmt_g15: .asciz "%.15g"
_fmt_g16: .asciz "%.16g"
_fmt_g17: .asciz "%.17g"
.p2align 3
_fmt_g_table: .quad _fmt_g15, _fmt_g16, _fmt_g17, 0
# SINGLE carries ~7 significant digits, so its search starts far lower: at
# 15 digits even a float's value round-trips, printing 3.14159! as
# 3.1415901184082.
_fmt_g6: .asciz "%.6g"
_fmt_g7: .asciz "%.7g"
_fmt_g8: .asciz "%.8g"
_fmt_g9: .asciz "%.9g"
.p2align 3
_fmt_g_single_table: .quad _fmt_g6, _fmt_g7, _fmt_g8, _fmt_g9, 0
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
_console_mode: .skip 4          # GetConsoleMode's output, for the VT enable

