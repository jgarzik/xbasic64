' 2D array example - multiplication table

DIM Table(5, 5)

' Fill the table
FOR Row = 1 TO 5
    FOR Col = 1 TO 5
        Table(Row, Col) = Row * Col
    NEXT Col
NEXT Row

' Print the table
PRINT "Multiplication Table:"
FOR Row = 1 TO 5
    FOR Col = 1 TO 5
        PRINT Table(Row, Col);
    NEXT Col
    PRINT
NEXT Row
