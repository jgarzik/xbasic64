# Runtime data section definitions
.data
_fmt_str: .asciz "%.*s"
_fmt_int: .asciz "%ld"
_fmt_float: .asciz "%g"
_fmt_char: .asciz "%c"
_fmt_newline: .asciz "\n"
_fmt_input: .asciz "%lf"
_fmt_input_str: .asciz "%1023[^\n]"
_input_buf: .skip 1024
_chr_buf: .skip 2
_str_buf: .skip 64
_rng_state: .quad 0x12345678DEADBEEF
_cls_seq: .asciz "\033[2J\033[H"
