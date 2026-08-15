' String comparison, searching and the case/trim functions
DIM Names$(4)
Names$(0) = "pear"
Names$(1) = "Apple"
Names$(2) = "fig"
Names$(3) = "cherry"
Names$(4) = "banana"

PRINT "Unsorted:"
FOR I = 0 TO 4
    PRINT " "; Names$(I)
NEXT I

' Sort them, which needs working string comparison
FOR I = 0 TO 3
    FOR J = 0 TO 3 - I
        IF UCASE$(Names$(J)) > UCASE$(Names$(J + 1)) THEN
            SWAP Names$(J), Names$(J + 1)
        END IF
    NEXT J
NEXT I

PRINT "Sorted (case-insensitive):"
FOR I = 0 TO 4
    PRINT " "; Names$(I)
NEXT I

' In-place edit: capitalize the first letter
Word$ = "banana"
MID$(Word$, 1, 1) = "B"
PRINT "Capitalized: "; Word$

Padded$ = "   spaced out   "
PRINT "Trimmed: ["; LTRIM$(RTRIM$(Padded$)); "]"
PRINT "Hex of 255: "; HEX$(255)
