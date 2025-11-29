' GOSUB/RETURN example - subroutine calls

10 PRINT "Main program start"
20 X = 5
30 GOSUB 100
40 X = 10
50 GOSUB 100
60 PRINT "Main program end"
70 END

100 REM Square subroutine
110 PRINT X; "squared ="; X * X
120 RETURN
