# Unattended plan: 12 hours (2026-11-03)

This continues Practice vs AI (`tasks/practice-vs-ai.md`) after the 2026-11-03 run (`research/run-summary-2026-11-03.md`).

## Rules for the run

- Each item has a time box. When the box runs out, I write down what I found, revert the TEMP probes and move on.
- I make one local commit per step that was verified in game. Research-only items commit their notes. I never push.
- I keep every standing project rule:
  - no game-file patches, injection or process writes;
  - gameplay stays in the server;
  - foreground input goes only through `gameinput.py` fg-*;
  - I never click Allow on firewall prompts;
  - I kill Overwatch.exe after each run;
  - files stay LF.
- Agents: at most one at a time, with Sonnet or Haiku subagents, for wiki look-ups and reading documents.
- I don't retry hardware breakpoints, because round 3 of `research/numeric-health.md` showed the client crashes.
- When something blocks, I take the next item, or a fallback, and record the blocker in the summary.

## Pre-flight (15 min)

1. Keep the machine awake. The screen must stay unlocked, because every in-game check needs the game window in the foreground.
2. Confirm the tree has no tracked changes, the 115 lobby tests pass, and Overwatch.exe is not running.
3. Check IDA. The idalib MCP server failed to connect at the start of this session; if it is still down, use headless `idat.exe`.

## Core items (about 10 h)

| # | Item | Box | Done when |
|---|---|---|---|
| 1 | **Paris collision** | 1.5 h | A unit test covers the cause, Paris's mesh extracts, Hanamura's mesh is unchanged byte for byte, and an Assault screenshot on Paris shows bots that walked to the point. |
| 2 | **Zarya's beam visible** | 1.5 h | A screenshot shows her bots' beam. The approach is the ch2 shot-event path from `research/bot-fire.md`, adapted to a continuous beam. |
| 3 | **Draw signature abilities and heals** | 2.5 h | Each ability whose client effect can be triggered from the server shows in a per-hero screenshot. The rest are listed with the reason they don't show. |
| 4 | **Round-end screen: score, time, victory/defeat** | 3 h | After a forced capture of A and B, the round-complete screen shows real values, and a victory or defeat screen shows if one can be found. If not, the vars and slots I tried are written up. |
| 5 | **Play menu: Practice vs. AI and difficulty** | 2.5 h | On the Play menu the two locked items are hidden, if the menu data allows it. The game's own Practice vs. AI option is shown and leads to its difficulty choices. The lobby log shows the difficulty picked there by an fg-* click, and the bot damage scale follows it. The `--difficulty` flag stays as an override. |

## Fallbacks: research, pick up when blocked (about 3.5 h)

| # | Item | Box | Output |
|---|---|---|---|
| 6 | **Capture-progress HUD** | 2 h | Decode the presenter `m_expression` bytes in graph 1824 to find which entity each widget reads. Test those vars in game. Update `research/assault-mode-graph.md`. |
| 7 | **Scope a playable hero other than Soldier** | 1 h | A doc only: what a second player hero needs (weapon graph, Volley, HUD, abilities), the cheapest candidate, and a cost estimate. |
| 8 | **Numeric health `0/200`** | 1.5 h | Option 2 of `research/numeric-health.md`: deobfuscate UI `019C`'s STUv2 ids to see what `Health` maps to. This is the least likely item to pay off, so it comes last. |

## Close-out (30 min)

- Run the full test suite, make sure no TEMP probes are left in the tree, and kill Overwatch.exe.
- Write `research/run-summary-<date>.md` with the same layout as last time:
  - what was done, with its commit and evidence;
  - what was blocked and why;
  - what is left.
- Update `tasks/practice-vs-ai.md` and the tool CHANGELOGs, then commit.

## Order and why

1. Paris comes first because it's a small, sure result and it warms up the run scripts.
2. Items 2 and 3 come next because they reuse the shot-event work from the last run while it is still fresh.
3. Item 4 is the biggest open gameplay gap. It sits in the middle, so a hard block still leaves time for the fallbacks.
4. Item 5 relies on menu clicks, the most fragile part of an unattended run, so it comes last among the core items.
5. Items 6–8 fill any gaps, in that order.

## Risks

- **The screen locks or sleeps:** every in-game check fails. Pure-server work and research can continue.
- **A firewall prompt appears:** I leave it unanswered and record it.
- **Items 2–4 may have no client hook the server can drive:** each one's time box covers the research. The output is then a write-up instead of a commit.

## After items 1–8

If time is left, I write the next steps here (picked from `tasks/practice-vs-ai.md` and the run's findings) and work on them with the same rules.

Picked after items 1-8 (2026-11-03):

| # | Step | Status |
|---|---|---|
| A | Engaged hero bots keep facing their target while they strafe (the Helix/Ana-dart lead in `research/bot-fire.md`) | done, unit-tested; live check inconclusive |
| B | Tournament Match card as the vs-AI entry | done: the card's click (group `0x692C511B`) now gets the game handoff; Practice Range card = the Range |
| C | Round-complete round number and time: try the untested 0C35 vars | done: capture progress (2198, 8018) sent, "FINAL ATTEMPT!" found (8027); round number and time still not found |
| D | Zarya beam: read-only memory check that the remote Zarya instance holds the sent states | done: no 0241 instance found in any variant; next lead in `research/bot-fire.md` |
