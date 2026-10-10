//! Array tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_arrays_1d_2d() {
    // Test 1D and 2D arrays with various access patterns
    let output = compile_and_run(
        r#"
DIM A(5)
A(1) = 10
A(3) = 30
PRINT A(1)
PRINT A(3)
DIM B(2, 3)
B(0, 0) = 1
B(1, 1) = 5
B(2, 2) = 9
PRINT B(0, 0) + B(1, 1) + B(2, 2)
DIM Grid(1, 2)
FOR I = 0 TO 1
    FOR J = 0 TO 2
        Grid(I, J) = I * 10 + J
    NEXT J
NEXT I
PRINT Grid(0, 0), Grid(0, 1), Grid(0, 2), Grid(1, 0), Grid(1, 1), Grid(1, 2)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "10", "1d a(1)");
    assert_eq!(lines[1], "30", "1d a(3)");
    assert_eq!(lines[2], "15", "2d diagonal sum");
    let values: Vec<&str> = lines[3].split_whitespace().collect();
    assert_eq!(values, vec!["0", "1", "2", "10", "11", "12"], "2d loop");
}

#[test]
fn test_3d_array() {
    let output = compile_and_run(
        r#"
DIM Cube(1, 1, 1)
Cube(0, 0, 0) = 1
Cube(0, 0, 1) = 2
Cube(0, 1, 0) = 3
Cube(0, 1, 1) = 4
Cube(1, 0, 0) = 5
Cube(1, 0, 1) = 6
Cube(1, 1, 0) = 7
Cube(1, 1, 1) = 8
PRINT Cube(0, 0, 0) + Cube(1, 1, 1)
"#,
    )
    .unwrap();
    // 1 + 8 = 9
    assert_eq!(output.trim(), "9");
}

/// LANGREF documents `DIM Values%(50)`, but typed numeric arrays returned
/// garbage: every element was stored as an f64 while an array access is typed
/// by its name's suffix, so the bit pattern was reinterpreted.
#[test]
fn test_typed_numeric_arrays() {
    let output = compile_and_run(
        r#"
DIM A%(3)
DIM B&(3)
DIM C!(3)
DIM D#(3)
A%(0) = 3
B&(0) = 100000
C!(0) = 3.5
D#(0) = 3.5
PRINT A%(0)
PRINT B&(0)
PRINT C!(0)
PRINT D#(0)
A%(1) = 3.7
PRINT A%(1)
A%(2) = 2
PRINT A%(0) + A%(2)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["3", "100000", "3.5", "3.5", "4", "5"],
        "typed elements keep their declared type; INTEGER rounds"
    );
}

/// Multi-dimensional typed arrays index correctly at the narrower element size.
#[test]
fn test_typed_multidim_array() {
    let output = compile_and_run(
        r#"
DIM M%(2,2)
M%(0,1) = 5
M%(1,1) = 7
M%(2,0) = 9
PRINT M%(0,1); M%(1,1); M%(2,0)
DIM V%(4)
FOR I = 0 TO 4
V%(I) = I * I
NEXT I
FOR I = 0 TO 4
PRINT V%(I);
NEXT I
PRINT ""
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "5  7  9");
    assert_eq!(lines[1], "0  1  4  9  16");
}

/// A fresh array reads as 0 / "": allocation is zeroed, which plain malloc
/// does not guarantee.
#[test]
fn test_arrays_start_zeroed() {
    let output =
        compile_and_run("DIM A(5)\nDIM S$(2)\nPRINT A(0); A(3); A(5)\nPRINT LEN(S$(1))\n").unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["0  0  0", "0"]);
}

/// REDIM resizes an existing array, reusing its descriptor, and clears it.
#[test]
fn test_redim() {
    let output = compile_and_run(
        "DIM A(2)\nA(0) = 7\nREDIM A(5)\nPRINT A(0)\nA(5) = 9\nPRINT A(5); UBOUND(A)\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["0", "9  5"], "contents cleared, bound updated");
}

/// REDIM PRESERVE keeps the existing elements and zeroes the new tail.
#[test]
fn test_redim_preserve() {
    let output = compile_and_run(
        "DIM A(2)\nA(0) = 7\nA(1) = 8\nREDIM PRESERVE A(5)\nPRINT A(0); A(1); A(5)\nPRINT UBOUND(A)\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["7  8  0", "5"]);
}

/// Growing an array a step at a time, which is what PRESERVE is for.
#[test]
fn test_redim_preserve_in_loop() {
    let output = compile_and_run(
        "DIM A(1)\nFOR I = 1 TO 3\nREDIM PRESERVE A(I)\nA(I) = I * 10\nNEXT I\nPRINT A(1); A(2); A(3)\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "10  20  30");
}

/// String arrays survive PRESERVE too.
#[test]
fn test_redim_preserve_strings() {
    let output = compile_and_run(
        "DIM S$(1)\nS$(0) = \"keep\"\nREDIM PRESERVE S$(3)\nPRINT S$(0); LEN(S$(0))\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "keep 4");
}

/// LBOUND and UBOUND take an array *name*, so a string array is fine.
#[test]
fn test_bounds_of_string_array() {
    let output = compile_and_run("DIM N$(5)\nPRINT LBOUND(N$); UBOUND(N$)\n").unwrap();
    assert_eq!(output.trim(), "0  5");
}

/// The dimension may be computed, not only written as a literal.
///
/// A non-literal dimension used to be silently treated as dimension 1.
#[test]
fn test_bounds_with_computed_dimension() {
    let output = compile_and_run(
        "DIM A(2,5)\nCONST D = 2\nK = 2\nPRINT UBOUND(A, K); UBOUND(A, D); UBOUND(A, 1)\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "5  5  2");
}

/// An array behaves the same wherever its DIM is written.
///
/// The parser used to decide between array access and function call from the
/// DIM statements it had read so far, so the same source produced a different
/// AST depending on whether the DIM came earlier or later -- and it ignored
/// scope, so a DIM inside a SUB changed how module-level code parsed. Sema now
/// resolves it against the finished symbol table.
#[test]
fn test_array_use_before_its_dim() {
    let output = compile_and_run(
        r#"
SUB Show(N)
SHARED A()
PRINT A(N)
END SUB
DIM A(3)
A(1) = 7
Show 1
PRINT A(1)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, &["7", "7"], "read before and after the DIM agree");
}

/// A DIM inside a procedure must not make module-level code resolve to it.
#[test]
fn test_procedure_local_dim_does_not_escape() {
    let output = compile_and_run(
        r#"
SUB Local
DIM Q(3)
Q(1) = 5
PRINT Q(1)
END SUB
Local
PRINT Q
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "5", "the local array works");
    assert_eq!(lines[1], "0", "Q at module level is an untouched scalar");
}

/// `DIM A(lo TO hi)` gives a dimension its own lower bound, as in QuickBASIC.
/// A subscript below it or above the upper one is out of range, and LBOUND
/// and UBOUND report both, whichever dimension is asked for.
#[test]
fn test_dim_with_lower_bounds() {
    let output = crate::common::compile_and_run(
        r#"
DIM A(5 TO 8), M$(-2 TO 2, 10 TO 11)
FOR I = 5 TO 8: A(I) = I * 10: NEXT I
PRINT A(5); A(8)
M$(-2, 10) = "lo": M$(2, 11) = "hi"
PRINT M$(-2, 10); M$(2, 11)
PRINT LBOUND(A); UBOUND(A); LBOUND(M$, 1); UBOUND(M$, 1); LBOUND(M$, 2); UBOUND(M$, 2)
D = 2: PRINT LBOUND(M$, D); UBOUND(M$, D)
REDIM A(1 TO 3)
PRINT LBOUND(A); UBOUND(A)
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["50  80", "lohi", "5  8 -2  2  10  11", "10  11", "1  3"]
    );
    for source in [
        "DIM A(5 TO 8)\nA(4) = 1\n",
        "DIM A(5 TO 8)\nA(9) = 1\n",
        "DIM M(1 TO 2, 3 TO 4)\nPRINT M(1, 2)\n",
        "DIM A(5 TO 4)\n",
    ] {
        let run = crate::common::compile_and_run_raw(source, "").unwrap();
        assert_eq!(run.exit_code, Some(1), "{source:?}: {}", run.stderr);
        assert!(
            run.stderr.contains("Subscript out of range"),
            "{source:?}: {}",
            run.stderr
        );
    }
}

/// Under OPTION BASE 1 a DIM's lower bound is 1, and so is an array created
/// by its use, which then runs 1 to 10.
#[test]
fn test_option_base_one_bounds_and_created_arrays() {
    let output = crate::common::compile_and_run(
        "OPTION BASE 1\nDIM A(3)\nA(1) = 7: A(3) = 9\nB(10) = 4\nPRINT A(1); A(3); B(10); LBOUND(B); UBOUND(B); LBOUND(A); UBOUND(A)\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "7  9  4  1  10  1  3");
    let run =
        crate::common::compile_and_run_raw("OPTION BASE 1\nDIM A(3)\nA(0) = 1\n", "").unwrap();
    assert!(
        run.stderr.contains("Subscript out of range"),
        "{}",
        run.stderr
    );
}

/// An array too large to address is Out of memory, not a small block.
///
/// The size is the product of the dimensions' counts and the element size,
/// and 65536^4 * 2 bytes is 2^65: computed in 64 bits it wrapped to 0, calloc
/// returned a block of nothing, and every subscript still passed its bounds
/// check -- so the assignment below wrote far outside the heap and the
/// program carried on as if nothing had happened.
#[test]
fn test_array_size_overflow_is_out_of_memory() {
    for src in [
        "DIM A%(65535, 65535, 65535, 65535)\nPRINT \"allocated\"\nA%(65535, 65535, 65535, 65535) = 7\n",
        "DIM A%(1, 1, 1, 1)\nREDIM A%(65535, 65535, 65535, 65535)\nPRINT \"allocated\"\n",
    ] {
        let run = crate::common::compile_and_run_raw(src, "").expect("should compile");
        assert_eq!(run.exit_code, Some(1), "{src}\nstdout {:?}", run.stdout);
        assert!(
            run.stderr.contains("Out of memory"),
            "got: {:?}",
            run.stderr
        );
        assert!(!run.stdout.contains("allocated"), "{src}");
    }
}

/// An array declared `AS` a type holds that type, whatever its name's suffix
/// says: an INTEGER array rounds what it is given, a LONG one holds past
/// 16 bits, and a `STRING * n` one pads to its width -- including through a
/// by-reference argument, INPUT-style stores and REDIM.
#[test]
fn test_arrays_declared_with_a_type() {
    let out = compile_and_run(
        r#"
SUB Bump(N AS INTEGER)
  N = N + 1
END SUB
DIM A(3) AS INTEGER
A(1) = 2.6: PRINT A(1)
Bump A(1): PRINT A(1)
DIM L(2) AS LONG
L(2) = 70000: PRINT L(2)
DIM S(2) AS STRING * 4
S(1) = "x": PRINT "["; S(1); "]"; LEN(S(1))
S(2) = "toolong": PRINT S(2)
READ S(0): PRINT "["; S(0); "]"
DATA ab
REDIM D(2) AS DOUBLE
D(1) = 1 / 3: PRINT D(1)
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&out),
        vec![
            "3",
            "4",
            "70000",
            "[x   ] 4",
            "tool",
            "[ab  ]",
            ".3333333333333333"
        ]
    );
}

/// REDIM PRESERVE of a two-dimensional array keeps every element whose
/// subscripts still exist, growing or shrinking the last dimension, as
/// QuickBASIC does; strings dropped by a shrink are freed. Changing any
/// other dimension is Subscript out of range.
#[test]
fn test_redim_preserve_of_a_2d_array() {
    let out = compile_and_run(
        r#"
DIM G(2, 3), S$(1, 2)
FOR I = 0 TO 2: FOR J = 0 TO 3: G(I, J) = I * 10 + J: NEXT J: NEXT I
REDIM PRESERVE G(2, 5)
PRINT G(0, 3); G(1, 0); G(2, 3); G(2, 5)
REDIM PRESERVE G(2, 1)
PRINT G(0, 1); G(1, 1); G(2, 0); G(2, 1)
S$(0, 2) = "x": S$(1, 0) = "y": S$(1, 1) = "z"
REDIM PRESERVE S$(1, 1)
PRINT S$(1, 0); S$(1, 1); UBOUND(S$, 2)
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&out),
        vec!["3  10  23  0", "1  11  20  21", "yz 1"]
    );

    let run = crate::common::compile_and_run_raw(
        "DIM G(2, 3)\nG(1, 1) = 5\nREDIM PRESERVE G(3, 3)\nPRINT \"no\"\n",
        "",
    )
    .unwrap();
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Subscript out of range"),
        "{:?}",
        run.stderr
    );
}

/// A plain `AS STRING` array holds variable-length strings, suffix or no.
#[test]
fn test_array_declared_as_string() {
    let out = compile_and_run(
        "DIM A(3) AS STRING\nA(1) = \"one\"\nA(2) = A(1) + \"+\"\nPRINT A(2); LEN(A(3))\n",
    )
    .unwrap();
    assert_eq!(out.trim(), "one+ 0");
}

/// ERASE leaves an array with no elements, so REDIM PRESERVE after it keeps
/// nothing: every element starts empty, not as whatever the heap held.
#[test]
fn test_redim_preserve_after_erase() {
    let out = compile_and_run(
        r#"
DIM A$(1000)
FOR I = 0 TO 1000: A$(I) = "x" + STR$(I): NEXT
ERASE A$
DIM B$(1000)
FOR I = 0 TO 1000: B$(I) = "yyyyyyyyyyyyyyyyyyyy" + STR$(I): NEXT
ERASE B$
REDIM PRESERVE A$(1001)
PRINT LEN(A$(5)); "["; A$(5); "]"
DIM C#(5)
C#(1) = 3.5
ERASE C#
REDIM PRESERVE C#(6)
PRINT C#(1)
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&out), vec!["0 []", "0"]);
}

/// An array whose element is passed by reference is locked while the call
/// runs: ERASE or REDIM of it there is error 10, as QuickBASIC's "array is
/// temporarily locked", rather than leaving the parameter in freed memory.
#[test]
fn test_erase_of_an_array_whose_element_is_an_argument() {
    for stmt in ["ERASE Z", "REDIM Z(100000)"] {
        let src = format!(
            "SUB M(X)\n  SHARED Z()\n  {stmt}\n  X = 99\nEND SUB\nDIM SHARED Z(3)\nZ(1) = 4\nM Z(1)\nPRINT \"no\"\n"
        );
        let run = crate::common::compile_and_run_raw(&src, "").unwrap();
        assert_eq!(run.exit_code, Some(1), "{stmt}: {:?}", run.stdout);
        assert!(
            run.stderr.contains("Duplicate definition"),
            "{stmt}: {:?}",
            run.stderr
        );
    }
    // Once the call is over the array is free to change again, and a BYVAL
    // element or an element in an expression locks nothing.
    let out = compile_and_run(
        r#"
SUB M(X)
  X = 7
END SUB
SUB N(BYVAL X)
  SHARED Z()
  REDIM Z(5)
  PRINT X
END SUB
DIM SHARED Z(3)
Z(1) = 4
M Z(1)
PRINT Z(1)
N Z(1)
Z(1) = 2
N Z(1) + 1
ERASE Z
PRINT "ok"
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&out), vec!["7", "7", "3", "ok"]);
}

/// The shape check of a many-dimensional REDIM PRESERVE stays under
/// `--unsafe`: without it the copy would write past the new block.
#[test]
fn test_redim_preserve_shape_is_checked_under_unsafe() {
    let run = crate::common::compile_and_run_flags(
        "DIM A(9, 3)\nA(9, 3) = 1\nREDIM PRESERVE A(1, 3)\nPRINT \"no\"\n",
        "",
        &["--unsafe"],
    )
    .unwrap();
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Subscript out of range"),
        "{:?}",
        run.stderr
    );
}

// ---- descriptors cached across a loop ----
//
// A loop whose body cannot change an array's descriptor reads the pointer,
// counts and lower bounds once, before its head. These are the programs where
// that would go wrong: the array created, moved or replaced inside the loop,
// or the body entered other than through the head.

/// An array first used inside a loop is created there, and later iterations
/// use the array that now exists -- including when it is created on an
/// iteration after the first.
#[test]
fn test_hoisted_loop_creates_array() {
    let output = compile_and_run(
        "\
FOR I = 0 TO 10: B(I) = I * 2: NEXT
PRINT B(3); B(10); UBOUND(B)
FOR I = 1 TO 3
IF I >= 2 THEN C(I) = I: T = T + C(I - 1)
NEXT
PRINT C(2); C(3); T
I = 0
WHILE I < 4: D%(I) = D%(I) + I: I = I + 1: WEND
DO: E$(I) = STR$(I): I = I - 1: LOOP UNTIL I < 0
PRINT D%(3); E$(4); LBOUND(E$); UBOUND(E$)
",
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["6  20  10", "2  3  2", "3  4 0  10"]
    );
}

/// A loop that never runs creates nothing, so a DIM after it is not a
/// Duplicate definition.
#[test]
fn test_zero_trip_loop_then_dim() {
    let output = compile_and_run(
        "\
FOR I = 1 TO 0: A(I) = 1: NEXT
DIM A(5)
A(5) = 7
N = 0
WHILE N > 0: B%(N) = 1: WEND
DIM B%(2 TO 3)
B%(3) = 8
DO WHILE N > 0: C$(N) = \"x\": LOOP
DIM C$(1)
C$(1) = \"y\"
PRINT A(5); B%(3); C$(1)
",
    )
    .unwrap();
    assert_eq!(output.trim(), "7  8 y");
}

/// GOTO into a loop's body, from outside it and from inside it.
#[test]
fn test_goto_into_loop_body() {
    let output = compile_and_run(
        "\
DIM A(5)
I = 3
GOTO Inside
WHILE I < 5
I = I + 1
Inside:
A(I) = A(I) + I
WEND
PRINT A(3); A(4); A(5)
DIM B(5)
FOR I = 0 TO 5
IF I = 2 THEN GOTO Skip
B(I) = I
Skip:
B(I) = B(I) + 10
NEXT
PRINT B(1); B(2); B(5)
",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["3  4  5", "11  10  15"]);
}

/// REDIM and ERASE inside a loop: later iterations see the new block.
#[test]
fn test_redim_inside_loop() {
    let output = compile_and_run(
        "\
DIM A(2)
FOR I = 0 TO 5
IF I > UBOUND(A) THEN REDIM PRESERVE A(I)
A(I) = I * I
NEXT
PRINT A(5); UBOUND(A); A(3)
FOR I = 0 TO 2
B(I) = B(I) + 1
IF I = 1 THEN ERASE B
NEXT
PRINT B(0); B(1); B(2)
",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["25  5  9", "0  0  1"]);
}

/// A procedure called in the loop -- a SUB, or a FUNCTION in an expression --
/// that REDIMs the shared array the loop indexes.
#[test]
fn test_loop_calling_procedure_that_redims() {
    let output = compile_and_run(
        "\
DIM SHARED A(2)
SUB Grow (N)
REDIM PRESERVE A(N)
END SUB
FUNCTION Fit (N)
IF N > UBOUND(A) THEN REDIM PRESERVE A(N + 1)
Fit = N
END FUNCTION
FOR I = 0 TO 5
IF I > UBOUND(A) THEN Grow I + 0
A(I) = I + 100
NEXT
PRINT A(0); A(5); UBOUND(A)
FOR I = 6 TO 9
A(Fit(I)) = A(I - 1) + 1
NEXT
PRINT A(9); UBOUND(A)
",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["100  105  5", "109  9"]);
}

/// Nested loops: one array indexed at both levels, one only inside.
#[test]
fn test_nested_hoisted_loops() {
    let output = compile_and_run(
        "\
DIM M(3, 4), R(3)
FOR I = 0 TO 3
FOR J = 0 TO 4
M(I, J) = I * 10 + J
R(I) = R(I) + M(I, J)
NEXT
NEXT
PRINT R(0); R(3); M(3, 4)
I = 0
WHILE I < 3
J = 0
DO WHILE J < 3
Q%(I * 3 + J) = I + J
J = J + 1
LOOP
R(I) = Q%(I * 3 + 2)
I = I + 1
WEND
PRINT R(0); R(2); Q%(8)
",
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["10  160  34", "2  4  4"]
    );
}

/// A SUB's loops over an array parameter, a local array and a STATIC one.
#[test]
fn test_hoisted_loop_in_sub() {
    let output = compile_and_run(
        "\
SUB Fill (A(), N)
FOR I = LBOUND(A) TO UBOUND(A)
A(I) = I * N
NEXT
END SUB
SUB Work
DIM L(4)
STATIC S()
FOR I = 0 TO 4: L(I) = I + 1: NEXT
FOR I = 0 TO 3: S(I) = S(I) + L(I + 1): NEXT
PRINT L(4); S(3)
END SUB
DIM X(1 TO 4)
Fill X(), 3
PRINT X(1); X(4)
Work
Work
",
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["3  12", "5  5", "5  10"]
    );
}

/// Lower bounds from OPTION BASE 1 and from `lo TO hi`, read before the loop.
#[test]
fn test_hoisted_loop_lower_bounds() {
    let output = compile_and_run(
        "\
OPTION BASE 1
DIM A(3), B(5 TO 9), G(2 TO 3, 4 TO 5)
FOR I = 1 TO 3: A(I) = I: NEXT
FOR I = 5 TO 9: B(I) = I * A(1 + (I MOD 3)): NEXT
FOR I = 2 TO 3: FOR J = 4 TO 5: G(I, J) = I * J: NEXT: NEXT
FOR I = 1 TO 10: H(I) = I: NEXT
PRINT B(5); B(9); G(3, 5); G(2, 4); H(10); LBOUND(H)
",
    )
    .unwrap();
    assert_eq!(output.trim(), "15  9  15  8  10  1");
}

/// A subscript out of range in a loop is reported on its own line, whether
/// below a lower bound or past the end, in either dimension.
#[test]
fn test_hoisted_loop_subscript_errors() {
    for (source, line) in [
        ("DIM A(5)\nFOR I = 0 TO 9\nA(I) = I\nNEXT\n", 3),
        (
            "DIM B(5 TO 9)\nFOR I = 9 TO 0 STEP -1\nX = 1\nB(I) = I\nNEXT\n",
            4,
        ),
        (
            "DIM M(2, 3)\nFOR I = 0 TO 2\nFOR J = 0 TO 4\n\nM(I, J) = 1\nNEXT\nNEXT\n",
            5,
        ),
        (
            "OPTION BASE 1\nI = 3\nWHILE I >= 0\nC(I) = 1\nI = I - 1\nWEND\n",
            4,
        ),
    ] {
        let run = crate::common::compile_and_run_raw(source, "").unwrap();
        assert_eq!(run.exit_code, Some(1), "{source:?}: {}", run.stderr);
        assert!(
            run.stderr
                .contains(&format!("Subscript out of range in {line}")),
            "{source:?}: {}",
            run.stderr
        );
    }
}

/// A FUNCTION called on the right of an assignment to an array element may
/// REDIM or ERASE that array; the value lands in the array as it is after
/// the call, not in the block the call freed. Likewise a record field of
/// an element, a string element, SWAP and the MID$ statement.
#[test]
fn test_element_target_outlives_a_redim_in_the_statement() {
    let out = compile_and_run(
        r#"
TYPE R
  N AS INTEGER
END TYPE
DIM SHARED A(3), S$(3), T(3) AS R
FUNCTION G(X)
  REDIM A(100000), S$(100000), T(100000) AS R
  G = X
END FUNCTION
FUNCTION K$(X)
  ERASE S$
  K$ = "k" + STR$(X)
END FUNCTION
FUNCTION P$
  REDIM PRESERVE S$(100000)
  P$ = "ZZ"
END FUNCTION
A(1) = G(7)
PRINT A(1); UBOUND(A)
S$(2) = K$(5)
PRINT S$(2)
T(1).N = G(9)
PRINT T(1).N
B = 4
SWAP B, A(G(2))
PRINT A(2); B
S$(1) = "abcdef"
MID$(S$(1), 2) = P$
PRINT S$(1)
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&out),
        vec!["7  100000", "k 5", "9", "4  0", "aZZdef"]
    );
}

/// The rest of the statements that write into an element: a whole record,
/// SWAP of records and of strings, and LSET, which writes in place -- into
/// the target, not into the copy a load is in a statement that calls a
/// FUNCTION.
#[test]
fn test_element_targets_outlive_a_redim_elsewhere_in_the_statement() {
    let out = compile_and_run(
        r#"
TYPE R
  N AS INTEGER
END TYPE
DIM SHARED T(3) AS R, U(3) AS R, S$(3)
FUNCTION G(X)
  REDIM T(100000) AS R, S$(100000)
  G = X
END FUNCTION
FUNCTION F$
  REDIM PRESERVE S$(200000)
  F$ = "LL"
END FUNCTION
U(2).N = 5
T(1) = U(G(2))
PRINT T(1).N
SWAP U(2), T(G(1))
PRINT U(2).N; T(1).N
S$(1) = "x"
S$(2) = "y"
SWAP S$(2), S$(G(1))
PRINT "[" + S$(1) + S$(2) + "]"
S$(1) = "abcdef"
LSET S$(1) = F$
PRINT S$(1); "|"
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&out),
        vec!["5", "0  5", "[]", "LL    |"]
    );
}
