' Factorial using a recursive FUNCTION
FUNCTION Factorial(N)
    IF N <= 1 THEN
        Factorial = 1
    ELSE
        Factorial = N * Factorial(N - 1)
    END IF
END FUNCTION

PRINT "5! ="; Factorial(5)
PRINT "7! ="; Factorial(7)
PRINT "10! ="; Factorial(10)
