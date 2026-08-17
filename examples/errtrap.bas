10 REM Error trapping: the three ways out of a handler
20 DEFINT A-Z
30 REM ---- RESUME <line>: give up on the file and carry on past it
40 ON ERROR GOTO 900
50 Found = 0
60 OPEN "no-such-data.txt" FOR INPUT AS #1
70 Found = 1
80 CLOSE #1
90 PRINT "found ="; Found
100 REM ---- RESUME: fix the cause, then run the same statement again
110 ON ERROR GOTO 930
120 Divisor = 0
130 Tries = Tries + 1 : Rate = 1000 / Divisor
140 PRINT "rate"; Rate; "after"; Tries; "try"
150 REM ---- RESUME NEXT: step over the statement that failed
160 ON ERROR GOTO 960
170 FOR I = 1 TO 5
180   Total = Total + I * 100 / (I - 3)
190 NEXT I
200 PRINT "total"; Total; "skipping"; Skipped; "term"
210 END
900 PRINT "error"; ERR; "at line"; ERL; "- no file, carrying on"
910 RESUME 90
930 Divisor = 8
940 RESUME
960 Skipped = Skipped + 1
970 RESUME NEXT
