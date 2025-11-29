' Fibonacci sequence using a FOR loop
A = 0
B = 1
PRINT "Fibonacci sequence:"
FOR I = 1 TO 10
    PRINT A
    C = A + B
    A = B
    B = C
NEXT I
