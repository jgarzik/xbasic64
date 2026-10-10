# Runtime data section definitions
.data
_fmt_str: .asciz "%.*s"
_fmt_int: .asciz "%ld"
_fmt_float: .asciz "%g"
# _rt_fmt_double's request: P significant digits in exponent form.
_fmt_sci: .asciz "%.*e"
_fmt_s: .asciz "%s"
_fmt_char: .asciz "%c"
_fmt_newline: .asciz "\n"
_fmt_input: .asciz "%lf"
_fmt_input_str: .asciz "%1023[^\n]"
_fmt_hex: .asciz "%llX"
_fmt_oct: .asciz "%llo"
_rng_state: .quad 0x12345678DEADBEEF
_rng_last:  .quad 0            # last value RND returned, for RND(0)
_cls_seq: .asciz "\033[2J\033[H"
_locate_fmt: .asciz "\033[%d;%dH"
_locate_row_fmt: .asciz "\033[%dd"
_locate_col_fmt: .asciz "\033[%dG"
_date_fmt: .asciz "%m-%d-%Y"
_time_fmt: .asciz "%H:%M:%S"
_color_fmt: .asciz "\033[%d;%d;%dm"
_color_fg_fmt: .asciz "\033[%d;%dm"
_color_bg_fmt: .asciz "\033[%dm"
# GW-BASIC colour 0-7 -> ANSI colour: black blue green cyan red magenta brown white
_gw_to_ansi: .byte 0, 4, 2, 6, 1, 5, 3, 7
_redo_msg: .asciz "?Redo from start\n"

# Scratch buffers. These start as zeros, so they belong in .bss: in .data
# every one of these bytes is a zero stored in the executable itself, which
# came to 7.5K of the image across the whole runtime.
#
# Safe to leave .bss active at the end of this file, and only here: runtime.rs
# concatenates data_defs.s and then emits .text unconditionally before the
# first code-bearing part. Every other runtime file must restore .text itself.
.bss
.p2align 3
_num_buf: .skip 64
_fmt_scratch: .skip 64          # _rt_fmt_double's printf output, before reshaping
_fmt_digits: .skip 32           # ... and its significant digits
# Current output column per file number, so TAB(n) knows how far to advance.
# Updated by the print helpers and reset by a newline. Slot 0 is the console,
# which is file handle 0. Indexed as quadwords, hence the alignment above.
_file_col: .skip 128
_input_buf: .skip 1024
_chr_buf: .skip 2
_str_buf: .skip 64
