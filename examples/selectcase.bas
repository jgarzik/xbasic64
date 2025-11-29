' SELECT CASE branching example

FOR DayNum = 1 TO 7
    SELECT CASE DayNum
        CASE 1
            PRINT "Monday"
        CASE 2
            PRINT "Tuesday"
        CASE 3
            PRINT "Wednesday"
        CASE 4
            PRINT "Thursday"
        CASE 5
            PRINT "Friday"
        CASE ELSE
            PRINT "Weekend!"
    END SELECT
NEXT DayNum
