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

