' String function examples

S$ = "Hello, World!"

PRINT "Original:"; S$
PRINT "Length:"; LEN(S$)
PRINT "Left 5:"; LEFT$(S$, 5)
PRINT "Right 6:"; RIGHT$(S$, 6)
PRINT "Mid 8,5:"; MID$(S$, 8, 5)

' Character/ASCII conversion
PRINT "CHR$(65) ="; CHR$(65)
PRINT "ASC(A) ="; ASC("A")

' String/number conversion
X = VAL("123")
PRINT "VAL(123) + 1 ="; X + 1
PRINT "STR$(456) ="; STR$(456)

' Find substring
PRINT "Position of 'World':"; INSTR(S$, "World")
