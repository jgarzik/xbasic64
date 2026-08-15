' User-defined record types (TYPE), including nesting and arrays of records
TYPE Point
    X AS INTEGER
    Y AS INTEGER
END TYPE

TYPE Employee
    Name AS STRING * 30
    Desk AS Point
    Salary AS DOUBLE
END TYPE

SUB Describe(E AS Employee)
    PRINT E.Name; " sits at ("; E.Desk.X; ","; E.Desk.Y; ")"
END SUB

DIM Staff(2) AS Employee

Staff(0).Name = "Alice"
Staff(0).Desk.X = 1
Staff(0).Desk.Y = 4
Staff(0).Salary = 55000

Staff(1).Name = "Bob"
Staff(1).Desk.X = 2
Staff(1).Desk.Y = 7
Staff(1).Salary = 48000

PRINT "Staff:"
FOR I = 0 TO 1
    PRINT " "; Staff(I).Name; " earns"; Staff(I).Salary
NEXT I

' A record is passed to a procedure by value
DIM Lead AS Employee
Lead = Staff(0)
Describe Lead
