' Formatted output with PRINT USING, and reading inline DATA
DATA "Widget", 12, 4.5
DATA "Gadget", 3, 19.99
DATA "Doohickey", 140, 0.75
DATA "", 0, 0

PRINT "Item          Qty      Each      Total"
PRINT "--------------------------------------"

Total# = 0
DO
    READ Item$, Qty, Each
    IF Item$ = "" THEN EXIT DO
    Line# = Qty * Each
    Total# = Total# + Line#
    PRINT USING "\           \ ####  $$####.##  $$####.##"; Item$; Qty; Each; Line#
LOOP

PRINT "--------------------------------------"
PRINT USING "Grand total:            $$#####.##"; Total#
