10 REM Guess the number - a listing in the period style
20 DEFINT A-Z
30 RANDOMIZE TIMER
40 CLS
50 COLOR 14, 1
60 LOCATE 2, 5
70 PRINT "GUESS THE NUMBER"
80 COLOR 7, 0
90 SECRET = INT(RND * 100) + 1
100 TRIES = 0
110 LOCATE 4, 5
120 PRINT "I am thinking of a number from 1 to 100."
130 REM The listing plays itself, so the example needs no input
140 LOW = 1 : HIGH = 100
150 GUESS = INT((LOW + HIGH) / 2)
160 TRIES = TRIES + 1
170 LOCATE 6 + TRIES, 5
180 PRINT "Guess"; TRIES; "is"; GUESS;
190 IF GUESS = SECRET THEN GOTO 250
200 IF GUESS < SECRET THEN PRINT "- too low" : LOW = GUESS + 1 : GOTO 150
210 PRINT "- too high"
220 HIGH = GUESS - 1
230 GOTO 150
250 PRINT "- correct!"
260 LOCATE 8 + TRIES, 5
270 PRINT "Found"; SECRET; "in"; TRIES; "guesses."
280 END
