# H2 run 20260920T110157Z

model claude-haiku-4-5-20251001, k=3, 14 defects in the truth set

| arm | recall mean | recall per repeat | false positives mean | decoy hits | input tok | output tok | cost USD |
|---|---|---|---|---|---|---|---|
| A | 0.59 | sd 0.215 over 12 attempts | 0.33 | 0 | 110575 | 36833 | 0.2979 |
| B | 0.59 | sd 0.215 over 12 attempts | 0.17 | 1 | 284367 | 145664 | 0.9693 |

## Per defect, how many attempts found it

| defect | rule | difficulty | arm A | arm B |
|---|---|---|---|---|
| a1-d1 | t1 | moderate | 3/3 | 2/3 |
| a1-d2 | t3 | obvious | 1/3 | 2/3 |
| a1-d3 | t2 | subtle | 0/3 | 0/3 |
| a2-d1 | t5 | subtle | 3/3 | 1/3 |
| a2-d2 | t2 | subtle | 3/3 | 3/3 |
| a2-d3 | t4 | subtle | 3/3 | 3/3 |
| a2-d4 | t3 | moderate | 1/3 | 3/3 |
| a3-d1 | t4 | subtle | 2/3 | 2/3 |
| a3-d2 | t2 | moderate | 0/3 | 0/3 |
| a3-d3 | t1 | moderate | 3/3 | 3/3 |
| a3-d4 | t2 | subtle | 0/3 | 0/3 |
| a4-d1 | t2 | moderate | 0/3 | 1/3 |
| a4-d2 | t4 | obvious | 3/3 | 3/3 |
| a4-d3 | t5 | moderate | 3/3 | 2/3 |
