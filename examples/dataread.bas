' DATA/READ example - inline numeric data

DATA 10, 20, 30, 40, 50

PRINT "Reading 5 values:"
Sum = 0
FOR I = 1 TO 5
    READ X
    PRINT "Value"; I; "="; X
    Sum = Sum + X
NEXT I
PRINT "Sum ="; Sum

' RESTORE resets the data pointer
RESTORE
READ First
PRINT "First value again:"; First
