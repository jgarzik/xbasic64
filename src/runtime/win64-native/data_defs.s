# ==============================================================================
# Runtime data section definitions (Win64 Native)
# ==============================================================================
#
# Shared data definitions used across runtime modules.
# Format strings are for UCRT sprintf, not console output.
#
# Note: Console I/O uses Win32 WriteFile/ReadFile directly,
# but we still use sprintf for number-to-string formatting.
# ==============================================================================

.data

# Format strings for sprintf (number formatting)
_fmt_int: .asciz "%lld"
_fmt_float: .asciz "%g"

# Error messages
# Lengths are computed by the assembler (. - label), never hand-counted: a
# hand-counted value here was wrong by one, making WriteFile emit a stray byte.
_gosub_overflow_msg: .ascii "Error: GOSUB stack overflow\r\n"
.equ _gosub_overflow_msg_len, . - _gosub_overflow_msg

