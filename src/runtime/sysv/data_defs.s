# Runtime data section definitions
.data
_fmt_str: .asciz "%.*s"
_fmt_int: .asciz "%ld"
_fmt_float: .asciz "%g"
# Shortest-round-trip formats, tried in order until the printed text reads
# back as the identical double. %g alone gives only 6 significant digits,
# which discards most of a Double's precision.
_fmt_g15: .asciz "%.15g"
_fmt_g16: .asciz "%.16g"
_fmt_g17: .asciz "%.17g"
_fmt_s: .asciz "%s"
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
_num_buf: .skip 64
# Current output column per file number, so TAB(n) knows how far to advance.
# Updated by the print helpers and reset by a newline. Slot 0 is the console,
# which is file handle 0.
.p2align 3
_file_col: .skip 128
_fmt_char: .asciz "%c"
_fmt_newline: .asciz "\n"
_fmt_input: .asciz "%lf"
_fmt_input_str: .asciz "%1023[^\n]"
_input_buf: .skip 1024
_chr_buf: .skip 2
_str_buf: .skip 64
_fmt_hex: .asciz "%llX"
_fmt_oct: .asciz "%llo"
_rng_state: .quad 0x12345678DEADBEEF
_cls_seq: .asciz "\033[2J\033[H"
_redo_msg: .asciz "?Redo from start\n"
