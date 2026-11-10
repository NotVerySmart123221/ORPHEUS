# Weapon damage: our tables against the 1.74 data

Theirs: every damaging WeaponVolley in the hero's weapon graph (`graph.state`), evaluated by LizRollin's VM. Damage `near -> far` over `start-end` m (far = amount x floor); `crit` the critical multiplier; `blast` an explosion's amount and its rings (radius: scale).

## Ana
- ours: 70 -> 70 over none, 1.25 shots/s, pellets 1, speed 125
- theirs 0B97.0 (other); blast 50 (4: 1); 1 shots/s, pellets 1, speed 30, reach 300
- theirs 0BDD.6 (other); 5 -> -, crit 1; - shots/s, pellets -, speed 60, reach 0
- theirs 0B94.5 (weapon); 70 -> -, crit 1; 999 shots/s, pellets 1, speed 125, reach 100
- theirs 0B94.44 (weapon); 70 -> -, crit 1; 999 shots/s, pellets 1, speed -, reach 100

## Ashe
- ours: 40 -> 12 over 20-40 m, 3.906 shots/s, pellets 1, speed -
- theirs 14FD.12 (other); 75 -> -, crit -; 1 shots/s, pellets 1, speed 25, reach 1000
- theirs 1513.16 (other); 6 -> -, crit 1; - shots/s, pellets 15, speed -, reach 200
- theirs 14FC.7 (weapon); 75 -> 22.5 over 30-50 m, crit -; 999 shots/s, pellets 1, speed -, reach 400
- theirs 14FC.122 (weapon); 40 -> 12 over 20-40 m, crit -; 999 shots/s, pellets 1, speed -, reach 400

## Baptiste
- ours: 24 -> 7.2 over 25-45 m, 1.724 shots/s, pellets 3, speed -
- theirs 1932.6 (weapon); 24 -> 7.2 over 25-45 m, crit -; 10 shots/s, pellets 1, speed -, reach 100
- theirs 1932.26 (weapon); 20 -> -, crit 1; 1 shots/s, pellets 1, speed 60, reach 500

## Bastion
- ours: 20 -> 6 over 25-45 m, 8 shots/s, pellets 1, speed -
- theirs 013A.11 (other); 15 -> 4.5 over 30-50 m, crit 1; 30 shots/s, pellets 1, speed -, reach 100
- theirs 024F.15 (other); 65 -> -, crit 1; blast 140 (1: 1, 4: 0.3); 1 shots/s, pellets 1, speed 60, reach 100
- theirs 012E.14 (weapon); 20 -> 6 over 25-45 m, crit -; 8 shots/s, pellets 1, speed -, reach 100

## Brigitte
- ours: 35 -> 35 over none, 1.667 shots/s, pellets 1, speed -

## Cassidy
- ours: 70 -> 21 over 20-40 m, 2.016 shots/s, pellets 1, speed -
- theirs 01E9.1 (other); blast 25 (3: 1); 1 shots/s, pellets 1, speed 30, reach 6.5
- theirs 01D2.11 (weapon); 70 -> 21 over 20-40 m, crit -; 1 shots/s, pellets 1, speed -, reach 400
- theirs 01D2.15 (weapon); 50 -> 15 over 20-40 m, crit 1; 8 shots/s, pellets 1, speed -, reach 100

## D.Va
- ours: 2 -> 0.6 over 10-20 m, 6.944 shots/s, pellets 11, speed -
- theirs 02B8.5 (other); 14 -> -, crit -; 7 shots/s, pellets 1, speed 50, reach 100

## D.Va pilot
- ours: 14 -> 14 over none, 7 shots/s, pellets 1, speed 50

## Doomfist
- ours: 6 -> 1.8 over 15-30 m, 3.125 shots/s, pellets 11, speed 80
- theirs 0D0A.2 (weapon); 6 -> 1.8 over 15-30 m, crit -; 3 shots/s, pellets 11, speed 80, reach 500

## Echo
- ours: 17 -> 17 over none, 3.333 shots/s, pellets 3, speed 75
- theirs 1C36.0 (other); 5 -> -, crit 1; blast 25 (2: 1); 15 shots/s, pellets 1, speed 50, reach 0
- theirs 19E5.3 (weapon); 17 -> -, crit -; 100 shots/s, pellets 3, speed 75, reach 100

## Genji
- ours: 29 -> 29 over none, 3.409 shots/s, pellets 1, speed 60
- theirs 0304.4 (weapon); 29 -> -, crit -; 9 shots/s, pellets 1, speed 60, reach 100
- theirs 0304.11 (weapon); 29 -> -, crit -; 999 shots/s, pellets 3, speed 60, reach 100

## Hanzo
- ours: 125 -> 125 over none, 2 shots/s, pellets 1, speed 110
- theirs 0101.21 (other); 125 -> -, crit -; 2 shots/s, pellets 1, speed 20, reach 10

## Junkrat
- ours: 120 -> 120 over none, 1.562 shots/s, pellets 1, speed 25
- theirs 022A.0 (other); 1 -> -, crit 1; blast 50 (0: 1); 50 shots/s, pellets 1, speed -, reach 100
- theirs 022A.1 (other); 1 -> -, crit 1; blast 50 (0: 1); 50 shots/s, pellets 1, speed -, reach 100
- theirs 022A.2 (other); 1 -> -, crit 1; blast 50 (0: 1); 50 shots/s, pellets 1, speed -, reach 100
- theirs 022A.3 (other); 1 -> -, crit 1; blast 50 (0: 1); 50 shots/s, pellets 1, speed -, reach 100
- theirs 022A.4 (other); 1 -> -, crit 1; blast 50 (0: 1); 50 shots/s, pellets 1, speed -, reach 100
- theirs 022A.5 (other); 1 -> -, crit 1; blast 50 (0: 1); 50 shots/s, pellets 1, speed -, reach 100
- theirs 096A.8 (other); blast 120 (3: 1); 1 shots/s, pellets 1, speed 25, reach 100
- theirs 022B.12 (weapon); 40 -> -, crit 1; blast 80 (0.5: 1, 2: 0.125); 1 shots/s, pellets 1, speed 25, reach 100

## Lúcio
- ours: 20 -> 20 over none, 4.348 shots/s, pellets 1, speed 50
- theirs 0776.14 (weapon); 20 -> -, crit -; 8 shots/s, pellets 1, speed 50, reach 100

## Mei
- ours: 2.75 -> 2.75 over none, 20 shots/s, pellets 1, speed 40
- theirs 05E6.13 (weapon); 2.75 -> -, crit 1; 20 shots/s, pellets 1, speed 40, reach 10
- theirs 05E6.16 (weapon); 75 -> -, crit -; 1 shots/s, pellets 1, speed 115, reach 100

## Mercy
- ours: 0 -> 0 over none, 10 shots/s, pellets 1, speed -
- theirs 006D.6 (other); 20 -> -, crit -; 5 shots/s, pellets 1, speed 50, reach 100

## Moira
- ours: 0 -> 0 over none, 5.208 shots/s, pellets 1, speed -
- theirs 10E4.11 (other); 100 -> -, crit 1; 999 shots/s, pellets 1, speed 20, reach 1000
- theirs 0F76.6 (weapon); 3.5 -> -, crit 1; 20 shots/s, pellets 1, speed 40, reach 15

## Orisa
- ours: 11 -> 11 over none, 12 shots/s, pellets 1, speed 120
- theirs 0BD4.24 (weapon); 11 -> -, crit -; 12 shots/s, pellets 1, speed 120, reach 0

## Pharah
- ours: 120 -> 120 over none, 1.179 shots/s, pellets 1, speed 35
- theirs 002B.1 (other); 10 -> -, crit 1; blast 30 (0.5: 1, 1.5: 0.3); 30 shots/s, pellets 1, speed 45, reach 100
- theirs 0023.6 (weapon); 40 -> -, crit 1; blast 80 (0.5: 1, 2.5: 0.308); 2 shots/s, pellets 1, speed 35, reach 100

## Reaper
- ours: 6 -> 1.8 over 10-20 m, 2.016 shots/s, pellets 20, speed -
- theirs 0019.6 (weapon); 6 -> 1.8 over 10-20 m, crit -; 999 shots/s, pellets 20, speed -, reach 100

## Reinhardt
- ours: 85 -> 85 over none, 1.042 shots/s, pellets 1, speed -
- theirs 00E2.0 (other); 100 -> -, crit 1; 1 shots/s, pellets 1, speed 25, reach 100

## Roadhog
- ours: 6.6 -> 1.98 over 15-30 m, 1.179 shots/s, pellets 25, speed 80
- theirs 01C6.9 (weapon); 6.6 -> 1.98 over 15-30 m, crit -; 999 shots/s, pellets 25, speed 80, reach 100
- theirs 01C6.12 (weapon); 50 -> 15 over 15-30 m, crit -; 999 shots/s, pellets 1, speed 60, reach 100
- theirs 01C6.14 (weapon); 7 -> 2.1 over 30-40 m, crit -; 8 shots/s, pellets 16, speed -, reach 100

## Sigma
- ours: 55 -> 55 over none, 0.6757 shots/s, pellets 2, speed 50
- theirs 1BBB.3 (other); 30 -> -, crit 1; blast 40 (1.5: 1, 2.5: 0.3); - shots/s, pellets -, speed 37, reach 200
- theirs 187F.20 (weapon); 25 -> -, crit 1; blast 30 (1: 1, 2.85: 0.3); 11 shots/s, pellets 1, speed 50, reach 200
- theirs 187F.28 (weapon); 25 -> -, crit 1; blast 30 (1: 1, 2.85: 0.3); 11 shots/s, pellets 1, speed 50, reach 200

## Soldier: 76
- ours: 20 -> 6 over 30-50 m, 9 shots/s, pellets 1, speed -
- theirs 0254.10 (other); 20 -> 6 over 30-50 m, crit 2; 9 shots/s, pellets 1, speed -, reach 180
- theirs 0254.16 (other); 40 -> -, crit 1; blast 80 (1: 1, 3: 0.5); 1 shots/s, pellets 1, speed 50, reach 100

## Sombra
- ours: 8 -> 2.4 over 15-35 m, 20 shots/s, pellets 1, speed -
- theirs 0B1B.6 (weapon); 8 -> 2.4 over 15-35 m, crit -; 20 shots/s, pellets 1, speed -, reach 100

## Symmetra
- ours: 6 -> 6 over none, 10 shots/s, pellets 1, speed -

## Torbjörn
- ours: 70 -> 70 over none, 1.645 shots/s, pellets 1, speed 70
- theirs 16C5.2 (other); 50 -> -, crit -; 1 shots/s, pellets 1, speed 17, reach 0
- theirs 16DA.17 (other); 25 -> -, crit 1; 6.5 shots/s, pellets 1, speed 40, reach 100
- theirs 1683.10 (weapon); 70 -> -, crit -; 999 shots/s, pellets 1, speed 70, reach 1000
- theirs 1683.13 (weapon); 12.5 -> 3.75 over 10-20 m, crit -; 999 shots/s, pellets 10, speed 120, reach 1000

## Tracer
- ours: 6 -> 1.8 over 12-20 m, 20 shots/s, pellets 2, speed -
- theirs 0034.0 (other); 5 -> -, crit 1; 1 shots/s, pellets 1, speed 15, reach 100
- theirs 0016.8 (weapon); 6 -> 1.8 over 12-20 m, crit -; 20 shots/s, pellets 2, speed -, reach 100

## Widowmaker
- ours: 13 -> 3.9 over 20-40 m, 10 shots/s, pellets 1, speed -
- theirs 001B.6 (weapon); 13 -> 3.9 over 20-40 m, crit -; 10 shots/s, pellets 1, speed -, reach 100

## Winston
- ours: 5.76 -> 5.76 over none, 10.42 shots/s, pellets 1, speed -

## Wrecking Ball
- ours: 5 -> 1.5 over 15-25 m, 25 shots/s, pellets 1, speed -
- theirs 1514.9 (weapon); 5 -> 1.5 over 15-25 m, crit -; 25 shots/s, pellets 1, speed -, reach 100

## Zarya
- ours: 3.75 -> 3.75 over none, 20 shots/s, pellets 1, speed -
- theirs 025F.0 (other); 5 -> -, crit 0; 1 shots/s, pellets 1, speed 25, reach 100

## Zenyatta
- ours: 48 -> 48 over none, 2.5 shots/s, pellets 1, speed 90
- theirs 0164.14 (weapon); 48 -> -, crit -; 1 shots/s, pellets 1, speed 90, reach 180
- theirs 0164.18 (weapon); 48 -> -, crit -; 9 shots/s, pellets 1, speed 90, reach 180
