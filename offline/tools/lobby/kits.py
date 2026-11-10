"""Weapon kits of the playable heroes other than Soldier (soldier.py): each hero's weapon graph and the owner states
that show the weapon, its ammo HUD and its fire button. The states come from tools/assets/weapon_kit.py over the
pinned graph catalog, which maps them the way Cassidy's were mapped from Soldier's (research/non-soldier-hero-scope.md)."""
from dataclasses import dataclass, replace
from math import ceil

from ow174.game.bits import BitWriter
from ow174.game.statescript import (Asset, Bool, Entity, Float, Graph, Instance, Int, ScriptRef, StateBound, Vec3,
                                    message_state, owner_full_frame, subscript)
from soldier import (MELEE_BUTTON, MELEE_COOLDOWN, ULT_CHARGE, ULT_COST, ULT_COST_VAR, RifleUpdate, Volley,
                     ability_payload, anim_payload, chase_payload, chase_since, cooldown_left, health_variables, melee_instance,
                     muzzle_payload, ult_meter, volley_payload)
from ow174.game.commands import CROUCH, SECONDARY
from ow174.game.script import graph as catalog
from ow174.game.content import PILOT_BODY, SPAWN_BODY
from target import ALT_KIT, INFINITE, PLAYER_KIT


@dataclass(frozen=True)
class Kit:
    graph: Graph
    states: dict  # owner state -> 1 / 0 (BooleanSwitch), 'button', 'unlinked' (StateRef), 'targets', or None (just on)
    model: int  # the CosmeticEntity state the weapon entity (var49) is bound to; None: the graph has none
    volley: int  # None: no Volley in the graph (melee, beams); shots do not show
    fire: dict  # states on only while a shot shows: 'stack'
    bits: dict  # instance sync var -> presence bit
    hud_texture: int  # the body's graph75 override of var1807: graph 463's portrait
    capacity: int  # target.PLAYER_KIT rounds
    state_count: int  # all the graph's states: var49's state index takes this many bits, not the owner states' count
    # the reload animation (weapon_kit.reload): ({owner state: how} the reload branch, replacing the normal states
    # while the magazine refills, the reload StateAnim); None: no animation
    reload: tuple = None
    # the alternate fire (weapon_kit.alt): ({owner state: how} pulsed with a shot, its Volley); its button and the
    # switches above it are in `states`. None: no weapon volley on the right mouse
    alt: tuple = None
    # ADS: (progress switch owner state, camera angle in degrees, camera angle variable if used when scoped).
    scope: tuple = None
    instance: int = 1
    alt_windup: float = 0.0
    alt_most: int = 0  # > 0: the alt loads a round every alt_windup while held, up to this many, fired on release
    # the reload's phases (soldier.Rifle reload_phase): {phase: (Wait state, Anim state)}, sent instead of reload's Anim
    phases: dict = None


CASSIDY, ROADHOG, TORBJORN = 0x02E0000000000042, 0x02E0000000000040, 0x02E0000000000006
SOMBRA, MEI, BASTION, LUCIO = 0x02E000000000012E, 0x02E00000000000DD, 0x02E0000000000015, 0x02E0000000000079
REAPER, ANA, ZENYATTA = 0x02E0000000000002, 0x02E000000000013B, 0x02E0000000000020
TRACER = 0x02E0000000000003
PHARAH = 0x02E0000000000008
WIDOWMAKER = 0x02E000000000000A
JUNKRAT = 0x02E0000000000065
ORISA = 0x02E000000000013E
WRECKING_BALL = 0x02E00000000001CA
ASHE = 0x02E0000000000200
ECHO = 0x02E0000000000206
BAPTISTE = 0x02E0000000000221
MERCY = 0x02E0000000000004
HANZO, DVA, SIGMA, SYMMETRA = 0x02E0000000000005, 0x02E000000000007A, 0x02E000000000023B, 0x02E0000000000016
MOIRA, GENJI, DOOMFIST, REINHARDT = 0x02E00000000001A2, 0x02E0000000000029, 0x02E000000000012F, 0x02E0000000000007
WINSTON, BRIGITTE, ZARYA = 0x02E0000000000009, 0x02E0000000000195, 0x02E0000000000068
KITS = {
    CASSIDY: Kit(Graph(0x01D2, 63, 18, 38),
                 {0: 1, 1: 0, 2: 0, 8: None, 10: 1, 12: 'button', 27: None, 28: None, 29: None, 30: None, 34: 1,
                  44: 0, 50: None}, 27, 11, {25: 'stack'},
                 {28: 2, 53: 6, 581: 9, 198: 10, 229: 12, 230: 13, 1769: 16, 1770: 17}, 0x0C00000000007D2E, 6, 63, reload=({1: 1, 24: 'stack'}, 9), alt=({25: 'stack', 4: 'button'}, 11)),
    ROADHOG: Kit(Graph(0x01C6, 56, 14, 37),
                 {0: 1, 1: 0, 2: 0, 5: None, 8: 'button', 13: 0, 16: None, 18: None, 35: 1, 38: 0, 41: None, 11: 'button'},
                 16, 9, {25: 'stack'},
                 {581: 0, 198: 1, 476: 2, 28: 6, 230: 8, 1769: 9, 1770: 10, 49: 12, 53: 13}, 0x0C00000000007CD3, 5, 56, reload=({1: 1, 24: 'stack'}, 6), alt=({26: 'stack', 39: 1}, 12)),
    TORBJORN: Kit(Graph(0x1683, 49, 17, 27),
                  {0: 1, 1: 0, 2: 0, 4: 'button', 7: None, 15: None, 26: None, 28: 0, 32: None, 41: 1, 11: 'button', 29: 1},
                  26, 10, {19: 'stack'},
                  {581: 1, 198: 2, 28: 5, 1769: 9, 1770: 10, 49: 14, 53: 16}, 0x0C00000000007D6C, 18, 49, reload=({1: 1, 18: 'stack'}, 8), alt=({20: 'stack'}, 13)),
    SOMBRA: Kit(Graph(0x0B1B, 92, 42, 61),
                {0: None, 1: 1, 2: 0, 3: 0, 5: 'button', 14: 1, 15: None, 22: None, 32: None, 46: None, 48: None,
                 53: None, 58: None, 65: 0, 66: 1, 67: 'stack'}, 0, 6, {19: 'stack'},
                {28: 4, 53: 8, 581: 11, 198: 28, 476: 32, 229: 35, 230: 36, 8909: 29}, 0x0C000000000094A6, 60, 112, reload=({2: 1, 18: 'stack'}, 17)),
    MEI: Kit(Graph(0x05E6, 65, 20, 45),
             {0: None, 1: 1, 2: 0, 3: 0, 5: 'button', 15: None, 20: None, 21: None, 25: None, 26: None, 27: None,
              28: None, 29: None, 46: 1}, 0, 13, {39: 'stack'},
             {28: 2, 53: 7, 581: 13, 198: 14, 1769: 17, 1770: 18}, 0x0C00000000007D53, 120, 68,
             reload=({2: 1, 38: 'stack', 7: None}, 6), alt=({33: 'stack', 49: 1, 61: 1}, 16), alt_windup=.4),
    BASTION: Kit(Graph(0x012E, 46, 16, 28),
                 {0: 1, 1: 0, 2: 0, 5: None, 7: None, 9: 'button', 12: None, 13: 1, 15: 1, 22: 1, 29: 0, 31: None,
                  33: None}, None, 14, {17: 'stack'},
                 {28: 2, 53: 5, 581: 6, 198: 7, 476: 8, 229: 10, 230: 11, 1769: 13}, 0x0C00000000007D60, 35, 49, reload=({1: 1, 16: 'stack'}, 8)),
    LUCIO: Kit(Graph(0x0776, 61, 19, 39),
               {0: None, 1: 1, 2: 0, 3: 0, 5: 'button', 7: 1, 8: None, 13: None, 14: None, 45: 0, 47: None},
               0, 12, {6: 'stack'}, {28: 5, 53: 9, 581: 11, 198: 12, 1769: 18}, 0x0C00000000007D44, 20, 65, reload=({2: 1, 20: 'stack'}, 9)),
    REAPER: Kit(Graph(0x0019, 44, 11, 29), {0: None, 1: None, 2: 1, 8: None, 9: None, 10: None, 29: 0, 31: None},
                0, 6, {19: 'stack'}, {581: 0, 198: 1, 28: 3, 53: 9}, 0x0C00000000007D9B, 8, 49, reload=({3: 1, 18: 'stack'}, 13)),
    ANA: Kit(Graph(0x0B94, 84, 22, 51), {0: 1, 7: None, 23: 'unlinked', 33: None, 50: 'button', 51: 0,
             59: None, 61: 0, 64: 0, 66: None, 71: None}, 59, 5, {11: 'stack'},
             {28: 5, 53: 10, 169: 7, 198: 12, 230: 20, 581: 11, 2137: 15}, 0x0C0000000000962C, 12, 91,
             reload=({1: 1, 10: 'stack'}, 8), scope=(36, 30, 168)),
    ZENYATTA: Kit(Graph(0x0164, 104, 22, 73),
                  {0: 1, 1: None, 6: None, 7: 0, 10: 0, 11: 'button', 15: 0, 16: 'button', 24: 1, 65: 1, 26: None, 28: None, 29: None, 30: None,
                   31: None, 38: None, 72: None, 73: None, 74: 0, 75: 0, 76: None, 77: None, 78: None, 79: None,
                   80: None, 81: None, 82: None, 83: None, 84: None, 87: 0, 90: None, 103: None}, 72, 14, {56: 'stack'},
                  {28: 3, 53: 7, 581: 9, 198: 10, 229: 17, 230: 18, 1392: 21, 1945: 1}, 0x0C00000000007C77, 20, 104, reload=({7: 1, 54: 'stack'}, 8),
                  # Orb Volley: button 16 (under switch 24) only sets var1592 while held; on release switch 22 (under 65)
                  # goes false and its action chain starts Volley 18. shortcut: the charging states (22 true) not sent.
                  alt=({22: 0}, 18), alt_windup=.524, alt_most=5),
    TRACER: Kit(Graph(0x0016, 42, 13, 28), {0: None, 1: None, 2: 1, 3: 0, 4: 0, 7: 'button', 10: None, 12: None,
                13: None, 27: 1, 29: 0, 31: None}, 0, 8, {19: 'stack'}, {581: 0, 198: 1, 28: 4, 229: 6, 230: 7,
                49: 10, 53: 12}, 0x0C00000000007DF7, 40, 42, reload=({3: 1, 18: 'stack'}, 9)),
    PHARAH: Kit(Graph(0x0023, 36, 9, 21), {0: None, 1: 1, 2: 0, 3: 0, 5: 'button', 8: None, 17: 1, 18: None, 23: 0,
                25: None}, 0, 6, {12: 'stack'}, {581: 0, 198: 1, 28: 3, 49: 7, 53: 8}, 0x0C00000000007E2F, 6, 36, reload=({2: 1, 11: 'stack'}, 9)),
    WIDOWMAKER: Kit(Graph(0x001B, 104, 25, 62), {0: None, 1: 1, 2: 0, 3: 0, 5: 'button', 7: 'unlinked', 14: None,
                    16: 1, 28: None, 38: None, 43: None, 69: 0, 70: 'button', 79: 0, 81: 0, 83: None, 91: None},
                    0, 6, {53: 'stack'}, {28: 2, 49: 8, 53: 9, 169: 4, 198: 13, 229: 21, 230: 22, 581: 11, 2137: 16},
                    0x0C00000000007E24, 35, 116, reload=({2: 1, 50: 'stack'}, 33), scope=(21, 30, 168)),
    JUNKRAT: Kit(Graph(0x022B, 39, 11, 23), {0: 1, 1: None, 2: 1, 3: 0, 7: 0, 8: 'button', 11: None, 15: None,
                 17: None, 20: 1, 23: 'stack', 24: 'stack', 25: 0, 27: None}, 1, 12, {13: 'stack'}, {581: 0, 198: 1,
                 28: 7, 53: 10}, 0x0C00000000007D39, 5, 39, reload=({3: 1, 14: 'stack'}, 5)),
    ORISA: Kit(Graph(0x0BD4, 58, 16, 37), {0: 1, 1: 0, 2: 0, 4: 'button', 6: None, 9: 1, 10: None, 14: None, 15: None,
               29: None, 30: None, 33: None, 40: 0, 45: None, 51: 1}, 30, 24, {20: 'stack'}, {28: 1, 53: 3, 581: 5,
               198: 6, 476: 8, 229: 11, 230: 12, 1769: 15}, 0x0C0000000000D6A3, 150, 58, reload=({1: 1, 19: 'stack', 7: None}, 5)),
    WRECKING_BALL: Kit(Graph(0x1514, 62, 17, 39), {0: 0, 1: 0, 3: 'button', 10: None, 11: 0, 22: 1, 24: 0, 26: None,
                       47: 0}, None, 9, {15: 'stack'}, {581: 0, 198: 1, 476: 3, 28: 6, 229: 8, 230: 9, 53: 13},
                       0x0C00000000017F24, 80, 65, reload=({0: 1, 48: 1, 14: 'stack'}, 8)),
    # Ashe's CosmeticEntity reads var16922 (1.176, set by the graph's Entry chain): without it no rifle showed.
    # Switch 96 (the fire button's child, reads the entity's action-priority var33) must be on its true branch, or
    # the client predicts the shots but never sends the fire bit (weapon_kit.kit() misses it: it is under the
    # button's event plug, not on the chain to the Volley's first parent).
    ASHE: Kit(Graph(0x14FC, 127, 32, 70), {0: None, 1: 1, 2: 0, 5: 'button', 6: 'unlinked', 11: None, 13: 1,
              23: None, 30: None, 31: None, 47: 0, 48: 'button', 57: 0, 58: 0, 72: None, 83: 'unlinked', 96: 1,
              102: None, 103: None, 105: 0}, 0, 7, {}, {28: 7, 49: 14, 53: 16, 169: 11, 198: 19, 229: 27, 230: 28,
              1769: 31, 2137: 22, 16922: 5}, 0x0C0000000001BD74, 12, 136, reload=({2: 1, 35: 'stack'}, 69),
              scope=(18, 40, None), phases={'open': (68, 69), 'insert': (71, 70), 'close': (73, 74)}),
    ECHO: Kit(Graph(0x19E5, 47, 16, 30), {0: 1, 1: 0, 2: 'button', 6: None, 8: None, 11: None, 13: 0, 18: 0, 20: None,
              21: None, 32: None, 40: 1}, None, 3, {10: 'stack'}, {581: 0, 198: 1, 28: 3, 229: 7, 230: 8, 53: 15},
              0x0C0000000002AA6D, 12, 47, reload=({18: 1, 16: 'stack', 17: None}, 24)),
    BAPTISTE: Kit(Graph(0x1932, 80, 32, 46), {0: None, 1: 1, 2: 0, 3: 0, 5: 'button', 14: 0, 15: None, 21: None,
                  34: None, 35: 1, 36: None, 37: None, 41: None, 42: None, 27: 'button', 51: 1, 31: 1}, 0, 6, {19: 'stack'}, {28: 5, 53: 11,
                  581: 13, 198: 14, 476: 16, 229: 22, 230: 23, 1769: 25, 1770: 27}, 0x0C0000000001E59E, 45, 96, reload=({2: 1, 18: 'stack'}, 17), alt=({28: 'stack'}, 26)),
    # Mercy spawns with her Caduceus Staff, her body's first weapon (graph 0x006B); the blaster (0x006D) is the second.
    # The staff's beams are not Volleys and are not served (like Moira's).
    # Owner states 51 / 54 (BD02E168 74 / 77, in state 1's true subgraph): 74's DataFlowMappings drive the staff's head.
    MERCY: Kit(Graph(0x006B, 56, 13, 32), {0: None, 1: 1, 51: None, 54: None}, 0, None, {}, {28: 5, 465: 0, 468: 1, 689: 8, 690: 9}, 0x0C00000000007D15,
               INFINITE, 79),
    # shortcut: Hanzo's draw is not sent (every arrow is full charge, target.PLAYER_KIT).
    HANZO: Kit(Graph(0x00B9, 55, 14, 29), {0: None, 1: 1, 2: 0, 3: 'button', 6: None, 27: None, 33: 0}, 0, 4, {},
               {28: 4, 476: 11}, 0x0C00000000007D23, INFINITE, 80),
    # D.Va's mech (content.SPAWN_BODY): Fusion Cannons, graph 0x050A (not in the pinned catalog; hand-mapped with
    # weapon_kit's rules from graph_layout/graph_edges, research/dva-mech.md). No CosmeticEntity: the cannons are
    # the mech's model. Switch 0 true, button 24 (state 26), switch 1 false, switch 25 (state 27) true, Stack 10, Volley 3.
    DVA: Kit(Graph(0x050A, 33, 10, 25), {0: 1, 1: 0, 4: None, 6: None, 7: None, 14: None, 15: None, 21: None,
             24: 'button', 25: 1, 28: None}, None, 3, {10: 'stack'},
             {261: 0, 198: 1, 28: 2, 350: 3, 351: 4, 233: 5, 50: 6, 243: 7, 53: 8, 58: 9}, 0x0C00000000007D85,
             INFINITE, 35),
    SIGMA: Kit(Graph(0x187F, 44, 16, 28), {0: 1, 1: None, 3: 0, 6: 0, 7: 'button', 9: None, 12: None, 14: None, 16: 1,
               25: None, 26: None}, 1, 20, {10: 'stack'}, {581: 1, 28: 7}, 0x0C00000000020CD7, INFINITE, 45, reload=({3: 1, 11: 'stack'}, 5)),
    # Her beam has no Volley; Volley 34 is the orb, the right mouse's (button 41 under StateAbility 40).
    SYMMETRA: Kit(Graph(0x157C, 96, 24, 58), {0: None, 1: 1, 2: 0, 8: 1, 10: 0, 12: None, 13: None, 33: 1, 40: 'ability',
                  41: 'button', 58: 0, 60: 1, 63: None, 83: 1}, 0, None, {}, {28: 2, 53: 7, 581: 11, 198: 12, 476: 15,
                  1770: 19, 1769: 20, 1648: 21}, 0x0C00000000007DE5, 70, 124, reload=({10: 1, 31: 'stack'}, 9), alt=({}, 34),
                  alt_windup=1.0),
    MOIRA: Kit(Graph(0x0F76, 115, 40, 67), {0: 1, 1: 1, 2: None, 3: None, 4: None, 13: 'button', 28: 'button', 69: 1,
               35: None, 39: None,
               54: None, 57: None, 58: None, 71: 0, 87: None, 110: 'stack'}, None, 6, {12: 'stack'}, {28: 11, 1769: 35, 1770: 36, 375: 38, 38: 17, 2318: 8, 14517: 24},
               0x0C0000000000F785, INFINITE, 130),
    # Genji, Doomfist: their button-20 LogicalButton is not on the Volley's parent chain (weapon_kit.kit() misses it):
    # hand-mapped from the button's own chain (Genji: switch 12 true -> button 38; Doomfist: switches 1, 13 -> button 6;
    # his second button 24 under the root also opens a CANCEL prompt, left off).
    GENJI: Kit(Graph(0x0304, 92, 18, 52), {0: 1, 6: None, 30: None, 32: None, 41: 1, 65: 0, 75: None, 12: 1, 38: 'button'}, 30, 4, {},
               {28: 2, 53: 5, 581: 9, 198: 10, 476: 12, 1769: 14, 1770: 15}, 0x0C00000000007DC5, 30, 96, reload=({1: 1, 12: 0, 33: 'stack'}, 7)),
    DOOMFIST: Kit(Graph(0x0D0A, 53, 14, 35), {0: 1, 4: None, 5: None, 22: None, 33: 0, 40: None, 1: 1, 13: 1, 6: 'button'}, None, 2, {9: 'stack'},
                  {198: 1, 28: 5, 53: 13}, 0x0C0000000000F4D5, 4, 57,
                  reload=({17: 1, 54: 'stack'}, 20), phases={'recharge': (19, 20)}),  # switch 17: the passive refill
    # Reinhardt, Winston, Brigitte, Zarya: the weapon graph has no Volley for the primary fire (Zarya's is her
    # alternate fire), so only the weapon and its HUD show.
    REINHARDT: Kit(Graph(0x0048, 43, 5, 21), {0: None}, 0, None, {}, {28: 4}, 0x0C00000000007D76, INFINITE, 43),
    WINSTON: Kit(Graph(0x00FE, 83, 13, 44), {0: None, 1: 1, 7: None, 12: None, 59: None, 61: 0, 66: None}, 0, None, {},
                 {581: 1, 198: 2, 28: 5, 53: 9}, 0x0C00000000007DB9, 100, 99, reload=({2: 1, 9: 0, 41: 'stack', 39: None}, 8)),
    BRIGITTE: Kit(Graph(0x1248, 103, 28, 67), {0: 1, 1: None, 8: None, 24: None, 29: 1, 45: None, 50: None, 52: None,
                  53: None}, 24, None, {}, {28: 6, 1769: 22}, 0x0C00000000015859, INFINITE, 117),
    ZARYA: Kit(Graph(0x0241, 69, 20, 42), {0: None, 1: 1, 10: None, 21: None, 22: None, 26: None, 27: None, 46: None,
               49: 0, 53: None, 13: 'button', 51: 1, 31: 1, 3: 0, 2: 0, 18: 1}, 0, None, {}, {28: 1, 53: 3, 581: 6, 198: 7, 476: 11, 1770: 17, 1769: 18, 1648: 19},
               0x0C00000000007D90, 100, 87, reload=({2: 1, 35: 'stack', 11: None}, 6), alt=({15: 'stack', 37: 'stack'}, 16)),
}
SCOPE_HEROES = frozenset(hero for hero, kit in KITS.items() if kit.scope is not None)
SCOPE_POSE = {0x14FC: (80, 24), 0x0B94: (28, 26), 0x001B: (44, 46)}
MERCY_BLASTER = Kit(Graph(0x006D, 42, 15, 24), {0: None, 1: 1, 2: 0, 5: 'button',
                    14: 1, 15: None, 26: 0, 30: None}, 0, 6, {19: 'stack'},
                   {581: 0, 198: 1, 476: 2, 28: 6, 229: 8, 230: 9, 53: 14},
                   KITS[MERCY].hud_texture, 20, 42, reload=({2: 1, 18: 'stack'}, 16), instance=2)
BASTION_SENTRY = Kit(Graph(0x013A, 46, 16, 28), {0: 0, 1: 0, 3: 'button', 8: None, 12: None,
                     13: 1, 14: 0, 16: None, 27: 1, 31: 0, 33: None, 35: None}, None, 11, {19: 'stack'},
                    {581: 0, 198: 1, 476: 3, 28: 6, 229: 9, 230: 10, 53: 14},
                    KITS[BASTION].hud_texture, 300, 46, reload=({18: 'stack', 0: 1, 14: 0, 13: 1}, 10), instance=314)
BASTION_REPAIR_BLEND = .25  # Graph 339 entry, vars 636 and 1322.


@dataclass(frozen=True)
class Ability:
    """A hero ability's HUD: its graph's StateAbility, the command bit it waits on, the UX presenters drawing its
    icon, and the cooldown (var472 = seconds, var61 = seconds left, bound to the StateAbility)."""
    graph: Graph
    state: int  # None: a passive, whose graph has no StateAbility (Angelic Descent, Glide)
    button: int  # command-record bit: 0x08 Shift, 0x10 E, 0x02 right mouse, 0x40 jump
    ux: tuple
    cooldown_bit: int
    left_bit: int
    seconds: float
    state_bits: int  # all the graph's states, as for var49
    bound: int = None  # var61's state: the full state index, when client-only states come before the StateAbility
    # more (presence bit, value) pairs, typed as the graph writes them: var11 (charges left) and var181 (most charges);
    # var4993 (available) where the icon's red unavailable mark reads it (UX 0x0786; var2575 is *unavailable*).
    # A charge ability's first pair is var11: Charges keeps it.
    extra: tuple = ()
    charges: int = 0  # most charges (var181); 0: a plain cooldown
    # (path owner states, Volley owner state) from the StateAbility down to its projectile (ability_kit.cast):
    # pulsed on a press like Biotic Field's throw. None: no projectile, or an undecoded state on the way.
    cast: tuple = None
    # (path owner states, StateAnim owner state) from the StateAbility to its cast animation (ability_kit.anim):
    # pulsed with the cast, ANIM payload (research/statescript-payloads.md)
    anim: tuple = None
    # ((pick command bit, (path, Volley)), ...): the press only arms it and the pick fires that cast (Choice);
    # replaces `cast`
    choice: tuple = None
    # ((owner state, how), ...) sent while it runs (Held / Timed below): how as Kit.states, or 'effect' (its
    # activation count), 'anim', 'chase' (the clock's ChaseVar value: meter left or seconds run). A Timed ability's
    # entry may add (on, off): the seconds after the press it is on from and until (None: its end), as
    # tools/research/ability_states.py --kit reads them off the graph. The server's mover takes the mods of every
    # MovementMod state among them (player_mover.frame_mods), and nothing hurts the player while PHASED is on.
    running: tuple = ()
    # held on its button while a meter lasts (Held): (meter presence bit, most, drain / s, refill / s)
    meter: tuple = None
    # run by a press for `seconds`, cancelled by another press after `cancel` (Timed): (seconds, cancel, flight m/s)
    timed: tuple = None
    cancel_on: int = 0  # more command bits whose press ends a Timed ability after `cancel` (Wraith Form: primary fire)
    hides: float = None  # seconds after the press from which no bot sees or shoots the hero while it runs (Stealth)
    lands: float = None  # seconds after the press from which touching the ground ends a Timed ability (Jump Pack)
    # ((owner state, how), ...) sent while an Aimed ability's aim is open, how as `always`
    aiming: tuple = ()
    # (reach in metres, seconds after the click the body is moved there, presence bit of the aim-open flag, presence
    # bit of the destination): the press opens an aim, primary fire runs it (Shadow Step; Aimed below)
    steps: tuple = None
    # (presence bit of the charge, its step, seconds between steps, seconds it lasts once he stands up): a charge
    # that builds while crouch is held and the next jump spends (Exo Boots; Crouched below)
    crouch: tuple = None
    # seconds of her own path a Timed ability takes the hero back along while it runs, to the most health she had on
    # it and a full magazine (Recall; Player.rewind)
    rewinds: float = None
    reloads: bool = False  # its start fills the magazine (Combat Roll)
    delay: float = 0.0  # seconds a Held ability's button is held before it comes on (Hover Jets)
    refills: int = None  # the ChaseVar owner state that fills a Held ability's meter again, sent while it does
    lifts: tuple = None  # (m/s2 up, the rise in m/s at which it pushes no more) while a Held ability is on (PlayerMover.lift)
    # a Held ability comes on only while the hero falls with at least this many metres of air under the feet (Glide)
    falling: float = None
    # (PlayerMover method, its numbers) a press plays on the server's body: 'dash' (Blink), 'jet' (Jump Jet), 'leap'
    # (Jump Pack), 'roll' (Combat Roll), 'strike' (Swift Strike), 'fly' (Flight), 'kick' (Coach Gun), 'uppercut' (Rising Uppercut). The client follows
    # by the owner's records.
    moves: tuple = None
    heals: tuple = None  # (health a second, seconds after the press it stops) a Timed ability gives its hero (Cryo-Freeze)
    # ((owner state, how), ...) sent always, how as `running` ('message': a message state with no fields) or a
    # payload callable
    always: tuple = ()
    # (m/s, reach in metres, metres short of the ally, metres over his feet): a Timed ability that only starts at
    # an ally bot under the crosshair, flies the hero to him and ends there (Guardian Angel; Match.guard)
    follows: tuple = None
    clock: int = None  # presence bit of the instance var a Timed ability's ChaseVar runs (seconds left), sent while on
    effects: tuple = ()  # Effect owner states pulsed with the cast (Begin bit, the cast's serial as the counter)


PHASED = 'STU_9028E035'  # the state Wraith Form, Fade and Cryo-Freeze run under while nothing can hurt the hero


def phased(instances) -> bool:
    """Whether a body frame's instances have a PHASED state on: the server then deals the player no damage and
    fires none of his shots, as the client shows it."""
    for instance in instances:
        graph = catalog.graph(instance.graph.index)
        states = graph.owner_states() if graph else ()
        if any(slot < len(states) and states[slot].cls == PHASED for slot in instance.active):
            return True
    return False


class Held(Volley):
    """An ability held on its button while its meter lasts (D.Va's Defense Matrix): it drains while held and
    refills once the cooldown after the release has run. `active` while it runs."""

    def __init__(self, quanta, button, seconds, most, drain, refill, delay=0.0, refills=False):
        super().__init__(quanta, button, seconds)
        self.most = self.meter = most
        self.drain, self.refill = drain * quanta / 1e6, refill * quanta / 1e6  # per command frame
        self.active, self.count = False, 0
        self.delay = round(delay * 1e6 / quanta)  # frames the button is held before it comes on (Hover Jets)
        self.pressed = None  # the frame the button went down
        self.refills = refills  # `command` also tells when the meter starts to fill again and when it is full

    @property
    def refilling(self):
        return not self.active and self.frame >= self.ready_at and self.meter < self.most

    def ability(self):
        return ability_payload(1) if self.active else super().ability()

    def command(self, command):
        filling = self.refills and self.refilling
        last, self.frame = self.frame or command.frame, command.frame
        if self.active:
            self.meter = max(0.0, self.meter - self.drain * (command.frame - last))
        elif command.frame >= self.ready_at:  # refills from the end of the cooldown on
            self.meter = min(self.most, self.meter + self.refill * (command.frame - max(last, self.ready_at)))
        held = bool(command.buttons & self.button)
        self.pressed = (command.frame if self.pressed is None else self.pressed) if held else None
        on = held and self.meter > 0 and (self.active or command.frame >= self.ready_at
                                          and command.frame - self.pressed >= self.delay)
        if on == self.active:
            return filling != (self.refills and self.refilling)
        self.active, self.count = on, self.count + on
        if not on:
            self.ready_at = command.frame + self.cooldown
        return True


class Crouched(Volley):
    """A charge that builds while crouch is held (Baptiste's Exo Boots, graph 1983): `step` at once and again every
    `every` seconds up to 1, lost `keep` seconds after he stands up, spent by a jump (`spend`, from Match). `meter`
    is the charge; `command` tells when it changed, so that a frame goes out with it.
    shortcut: the crouch button, not the crouched stance the graph watches."""
    active = False

    def __init__(self, quanta, step, every, keep):
        super().__init__(quanta, CROUCH, 0.0)
        self.step, self.every, self.keep = step, round(every * 1e6 / quanta), round(keep * 1e6 / quanta)
        self.meter, self.due, self.stood = 0.0, None, None  # the charge, its next step's frame, the frame he stood up

    def command(self, command):
        before, self.frame = self.meter, command.frame
        if command.buttons & CROUCH:
            self.due, self.stood = command.frame if self.due is None else self.due, None
            while self.due <= command.frame:
                self.meter, self.due = min(1.0, self.meter + self.step), self.due + self.every
        else:
            self.due = None
            self.stood = command.frame if self.stood is None else self.stood
            if command.frame - self.stood >= self.keep:
                self.meter = 0.0
        return self.meter != before

    def spend(self):
        self.meter, self.due = 0.0, None


class Timed(Volley):
    """An ability a press runs for a while (D.Va's Boosters): another press after `cancel` seconds ends it early; the
    cooldown starts when it ends. `active` while it runs; `flight`: the speed the mover flies along the aim.
    ability: its Ability, for the buttons that also end it (cancel_on), the times its running states change (due)
    and whether a frame has to go every tick (live: a clock the client does not run itself)."""

    def __init__(self, quanta, button, seconds, length, cancel, flight, ability=None):
        super().__init__(quanta, button, seconds)
        self.length, self.cancel = ceil(length * 1e6 / quanta), ceil(cancel * 1e6 / quanta)
        self.flight, self.started, self.until, self.held, self.count = flight, 0, 0, False, 0
        self.spec = ability  # (`ability` is Volley's payload method)
        self.also = ability.cancel_on if ability else 0
        self.heal_at = 0.0  # match time of its next heal pulse (Match.update)
        self.live = bool(ability and ability.clock is not None)
        self.edges = sorted({time for _, _, *window in (ability.running if ability else ()) for time in window if time})
        self.passed = 0

    @property
    def active(self):
        return bool(self.until)

    @property
    def meter(self):  # seconds left, for its ChaseVar: Boosters' ring shrinks and its digits count down with it
        return (self.until - self.frame) * self.quanta / 1e6 if self.until else 0.0

    @property
    def elapsed(self):  # seconds it has run: which of Ability.running's timed states are on
        return (self.frame - self.started) * self.quanta / 1e6 if self.until else 0.0

    @property
    def hidden(self):
        return bool(self.until and self.spec and self.spec.hides is not None and self.elapsed >= self.spec.hides)

    @property
    def landing(self):  # touching the ground ends it now
        return bool(self.until and self.spec and self.spec.lands is not None and self.elapsed >= self.spec.lands)

    @property
    def healing(self):  # health a second it gives now
        heals = self.spec.heals if self.until and self.spec else None
        return heals[0] if heals and self.elapsed < heals[1] else 0.0

    def stop(self):
        """End it now from outside (Charge into a wall): the cooldown starts."""
        if self.until:
            self.until, self.ready_at = 0, self.frame + self.cooldown

    def due(self):
        """Whether a timed state came on or went off since the last look: the body's instances are then built anew."""
        passed = sum(edge <= self.elapsed for edge in self.edges) if self.until else 0
        changed, self.passed = passed != self.passed, passed
        return changed

    def ability(self):
        return ability_payload(1) if self.until else super().ability()

    def command(self, command):
        self.frame = command.frame
        pressed = bool(command.buttons & (self.button | self.also if self.until else self.button))
        press, self.held = pressed and not self.held, pressed
        if self.until and (command.frame >= self.until or press and command.frame >= self.started + self.cancel):
            self.until, self.ready_at = 0, command.frame + self.cooldown
            return True
        if press and not self.until and command.frame >= self.ready_at:
            self.started, self.until = command.frame, command.frame + self.length
            self.count += 1
            return True
        return False


class Aimed(Timed):
    """An ability whose press opens an aim and whose click runs it (Reaper's Shadow Step): `aiming` from the press
    until primary fire, which starts it as a Timed ability if Match found it a place to go (`target`, the feet's
    destination); the button again or secondary fire puts the aim away. Only its time ends it. `holstered`: no shot
    leaves the weapon from the press until the click is let go and it is over. `moved`: the body is there."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.aiming, self.target, self.moved, self.last, self.holstered = False, None, False, 0, False

    def command(self, command):
        pressed, self.last = command.buttons & ~self.last, command.buttons
        if self.until:
            changed = super().command(replace(command, buttons=command.buttons & ~self.button))
            self.holstered = bool(self.until) or bool(command.buttons & FIRE)
            return changed
        self.frame = command.frame
        if not self.aiming:
            self.holstered = self.holstered and bool(command.buttons & FIRE)  # the click, until it is let go
            if pressed & self.button and command.frame >= self.ready_at:
                self.aiming = self.holstered = True
            return self.aiming
        if pressed & FIRE and self.target is not None:
            self.aiming, self.held, self.moved = False, False, False
            return super().command(replace(command, buttons=command.buttons | self.button))
        if pressed & (self.button | SECONDARY):
            self.aiming = self.holstered = False
            return True
        return False


class Choice(Volley):
    """An ability that waits for a pick after its press (Moira's Biotic Orb: E, then left or right mouse, graph
    4324's int switch 17 on var59): the press arms it, a pick bit fires it as a press would, and `pick` keeps which.
    `choosing` stays on until the pick's button is let go: that click is no weapon shot.
    shortcut: armed until a pick (no cancel on a second press)."""

    def __init__(self, quanta, button, seconds, picks):
        super().__init__(quanta, button, seconds)
        self.picks, self.held = picks, None

    def command(self, command):
        if self.held:
            self.frame = command.frame
            if not command.buttons & self.held:
                self.held, self.choosing = None, False
            return False
        pick = next((bit for bit in self.picks if command.buttons & bit), None) if self.choosing else None
        if pick is None:
            self.frame = command.frame
            if command.buttons & self.button and command.frame >= self.ready_at:
                self.choosing = True
            return False
        self.pick = self.held = pick
        return super().command(replace(command, buttons=command.buttons | self.button))


# What each of these abilities' own graph turns on over time, as Ability.running takes it: (owner state, how, on, off)
# in seconds after the press. From tools/research/ability_states.py HERO BUTTON --kit, which runs the graph offline;
# tools/research/test_ability_states.py keeps the tables equal to its output. Each has its MovementMod state, so the
# hero moves on the client as the graph says and the server's mover takes the same mods (player_mover.frame_mods).
# Left out: states the kits cannot write yet (ChaseVar, TrackTargets) and the server's own (Pet, ModifyHealth).
# shortcut: beside the movement and PHASED the server plays Charge's bump and wall stop (no pin), Cryo-Freeze's heal
# (no ice block) and Stealth's hiding from bots (she is still drawn); add the rest when each hero is worked on.
WRAITH_FORM = (  # Reaper: 3 s at SPEED +0.5 without character collision (4) under PHASED (0), then 0.4 s of recovery
    (0, 'unlinked', 0.00, 3.0), (2, 'anim', 0.00, 3.0), (3, None, 0.00, 3.0), (4, None, 0.00, 3.0), (5, 'stack', 0.00, 3.0),
    (7, 'stack', 0.00, 3.0), (8, 'stack', 0.00, 3.4), (10, None, 0.00, 3.4), (13, 'stack', 0.00, 3.4), (16, 'stack', 0.00, 3.0),
    (20, 'stack', 0.00, 3.0), (22, None, 0.00, 3.4), (23, None, 0.00, 3.0), (26, None, 0.00, 0.4), (27, 0, 0.00, 3.0),
    (28, None, 0.00, 2.5), (31, 'stack', 0.00, 3.4), (33, None, 0.00, 3.0), (24, 'button', 0.40, 3.0), (25, 'button', 0.40, 3.0),
    (32, None, 0.40, 3.0), (29, None, 2.50, 3.5), (30, None, 2.50, 3.5), (6, None, 3.00, 3.4))
FADE = (  # Moira: 0.75 s at SPEED +2.5 and jump speed +6 without character collision (19) under PHASED (0)
    (0, 'unlinked', 0.00, 0.75), (2, 'anim', 0.00, 0.94), (3, None, 0.00, 0.75), (4, 'stack', 0.00, 0.75), (5, 'effect', 0.00, 0.75),
    (7, 'stack', 0.00, 0.75), (8, 'stack', 0.00, 0.94), (10, None, 0.00, 0.94), (11, 'stack', 0.00, 0.94), (17, 'stack', 0.00, 0.75),
    (18, 'unlinked', 0.00, 0.75), (19, None, 0.00, 0.75), (20, 'stack', 0.00, 0.75), (22, 'anim', 0.00, 0.2), (26, 'stack', 0.00, 0.75),
    (27, None, 0.00, 0.1), (28, None, 0.00, 0.1), (30, 'stack', 0.00, 0.94), (6, None, 0.75, 0.94), (21, 'anim', 0.75, None),
    (29, None, 0.75, 0.85), (31, None, 0.75, 0.85))
CRYO_FREEZE = (  # Mei: 3.99 s without moving, jumping or crouching (3) under PHASED (48), then 0.25 s of recovery
    (2, 'stack', 0.00, 3.99), (3, None, 0.00, 3.99), (4, None, 0.00, 3.99), (5, 'stack', 0.00, 4.24), (9, None, 0.00, 0.2),
    (10, 'stack', 0.00, 3.99), (12, 'stack', 0.00, 3.99), (14, 'stack', 0.00, 4.24), (16, 'effect', 0.00, 3.99), (18, 'anim', 0.00, 3.99),
    (20, 'stack', 0.00, 3.99), (24, 'stack', 0.00, 3.99), (33, 'stack', 0.00, 3.99), (34, 'stack', 0.00, 3.99), (35, 'stack', 0.00, 3.99),
    (39, 'stack', 0.00, 3.99), (42, 'stack', 0.00, 4.24), (43, None, 0.00, 3.99), (48, 'unlinked', 0.00, 3.99), (49, None, 0.00, 3.99),
    (7, 'button', 0.20, 4.24), (8, 'button', 0.20, 4.24), (22, 0, 0.20, 4.24), (19, None, 3.99, 4.24), (47, None, 3.99, 4.24),
    (45, None, 4.24, 4.75), (46, None, 4.24, 4.75))
CHARGE = (  # Reinhardt: 0.59 s rooted (35), then 3 s forced forward at SPEED +2 without character collision (36)
    (0, 'stack', 0.00, 3.59), (31, None, 0.00, 0.15), (35, None, 0.00, 0.59), (38, 'anim', 0.00, 3.59), (39, 'stack', 0.00, 3.59),
    (40, None, 0.00, 3.59), (44, 'stack', 0.00, 3.59), (45, 'stack', 0.00, 3.59), (50, 'stack', 0.00, 0.59), (51, 1, 0.00, 3.59),
    (64, 'stack', 0.00, 3.59), (68, 'stack', 0.00, 3.59), (69, 'stack', 0.00, 3.59), (70, 0, 0.00, 3.59), (72, None, 0.00, 0.59),
    (78, 'stack', 0.00, 3.59), (80, 'stack', 0.00, 3.59), (89, 'stack', 0.00, 3.59), (90, 'stack', 0.00, 3.59), (91, 'stack', 0.00, 3.59),
    (8, 0, 0.59, 3.59), (18, 'stack', 0.59, 3.59), (21, 'effect', 0.59, 3.59), (26, 0, 0.59, 3.59), (36, None, 0.59, 3.59),
    (57, 0, 0.59, 3.59), (77, None, 0.59, 3.59), (17, None, 3.59, 3.89), (23, 'anim', 3.59, None), (42, None, 3.59, 3.79))
STEALTH = (  # Sombra: 0.75 s to fade out, then SPEED +0.65 (2; var7779, sent with it: STEALTH_SPEED) until it is ended
    (7, 'stack', 0.00, None), (15, 'anim', 0.00, 0.99), (18, 'stack', 0.00, 0.75), (29, 'stack', 0.00, None), (32, 'stack', 0.00, None),
    (39, 'stack', 0.00, None), (0, 'stack', 0.75, None), (1, 'effect', 0.75, None), (2, None, 0.75, None), (3, 'stack', 0.75, None),
    (8, 'button', 0.75, None), (9, None, 0.75, None), (20, 'button', 0.75, None), (21, 'button', 0.75, None), (22, 'button', 0.75, None),
    (24, 'stack', 0.75, None), (25, 'button', 0.75, None), (26, 'unlinked', 0.75, None), (36, 0, 0.75, None), (38, 'stack', 0.75, None),
    (43, None, 0.75, None))
COMBAT_ROLL = (  # Cassidy: 0.4 s without input or ground deceleration (2, 5); the roll's speed is PlayerMover.roll
    (1, None, 0.00, 0.4), (2, None, 0.00, 0.4), (4, 'anim', 0.00, None), (5, None, 0.00, 0.4), (10, 'stack', 0.00, 0.4),
    (11, 'stack', 0.00, 0.4), (15, None, 0.00, 0.4), (16, 'anim', 0.00, 0.1), (22, 'stack', 0.00, 0.4), (25, None, 0.00, 0.4))
SWIFT_STRIKE = (  # Genji: 0.4 s without character collision (32); the MoveTo (1) is PlayerMover.strike
    (2, 'anim', 0.00, 0.5), (4, 'stack', 0.00, 0.4), (5, 'stack', 0.00, 0.4), (9, 'stack', 0.00, 0.4), (10, 'anim', 0.00, 0.1),
    (16, None, 0.00, 0.4), (17, 'effect', 0.00, 0.4), (19, None, 0.00, 0.4), (20, 1, 0.00, 0.4), (23, 'stack', 0.00, 0.4),
    (25, 1, 0.00, 0.4), (26, 'stack', 0.00, 0.4), (32, None, 0.00, 0.45), (30, None, 0.40, 0.45))
# Mercy's Guardian Angel (graph 006E), read off the graph's tree (the VM does not run it without a target): the
# flight's pose (2) under the StateMoveTo (0), which is the server's own (Ability.follows).
GUARDIAN_ANGEL = ((2, 'anim', 0.00, None),)
JUMP_PACK = (  # Winston: until he lands, gravity x 1.5 and half the air's deceleration, without character collision (2)
    (2, None, 0.00, None), (5, 'stack', 0.00, None), (9, 'anim', 0.00, None), (22, 'stack', 0.00, 0.24), (26, None, 0.00, 0.24),
    (30, None, 0.00, 0.2), (31, None, 0.00, 0.2), (32, 'stack', 0.00, 0.2), (33, 'stack', 0.00, None))
HOVER_JETS = (  # Pharah, while the jets push: the thrust (6), SPEED +0.2 (37), the fuel's ChaseVar (4)
    (3, 1), (4, 'chase'), (6, None), (8, 'stack'), (11, 'effect'), (12, 0), (24, 'stack'), (29, 0), (33, None), (34, 'stack'),
    (37, None))
# 0, 14 and 18 are classes the tool leaves out (the camera, a switch, a pose), all without data: sent as they are
# Reaper's Shadow Step (graph 00F2), read off the graph's tree (the VM does not run it: research/lizrollin-runtime.md
# section 35). While the aim is open: the targeting state (0, STU_4C96470A, a byte of data) under the primary fire
# button's state (2), with its marker (4).
SHADOW_STEP_AIM = ((6, 'stack'), (37, 'stack'), (59, 1), (2, 'button'), (0, lambda: _bits((0, 8))), (4, None), (65, None),
                   (15, 'stack'), (64, None))
# From the click, 2 s: no input (8) and the cast's camera (50); nothing can hurt him from 1 s (33), the action
# STU_CFCE55DC moves him at var1722 = 1.5 s (Ability.steps: the server's own), then 0.5 s more of STU_9028E035 (47).
# Left out: the entity the graph creates at the destination (42), its linked-target state (49), its messages (62).
SHADOW_STEP = (
    (18, 'stack', 0.00, None), (38, 'stack', 0.00, None), (58, 1, 0.00, None), (9, None, 0.00, None), (31, None, 0.00, 1.5),
    (32, None, 0.00, 1.0), (46, None, 0.00, 1.5), (33, 'unlinked', 1.00, 1.5), (55, 'stack', 1.00, 1.5), (57, 'stack', 1.00, 1.5),
    (21, 'stack', 0.00, None), (12, 'stack', 0.00, None), (19, 'stack', 0.00, None), (13, 'effect', 0.00, None),
    (22, 'stack', 0.00, None), (60, 'stack', 0.00, None), (40, 'stack', 0.00, None), (51, None, 0.00, None), (50, None, 0.00, None),
    (8, None, 0.00, None), (53, None, 0.00, None), (11, 'anim', 0.00, None), (20, 'stack', 0.00, None), (7, None, 1.50, None),
    (47, 'unlinked', 1.50, None), (48, 'stack', 1.50, None), (54, 'stack', 1.50, None), (56, 'stack', 1.50, None),
    (52, 'unlinked', 1.50, None), (29, None, 1.50, 1.6), (30, 'stack', 1.50, 1.6))
# Tracer's Recall, 1.25 s: nothing can hurt her (2). The graph's rewind is a state of its own (0, STU_41B3FC2B)
# whose data is not decoded: it is not sent, the server takes its copy back (Ability.rewinds) and the client follows
# by the owner's records. shortcut: so she is drawn sliding back, without the rewind's own look.
RECALL = (
    (1, 'unlinked', 0.00, None), (2, 'unlinked', 0.00, None), (5, None, 0.00, None), (6, 'anim', 0.00, 0.9), (7, 'stack', 0.00, None),
    (12, 'stack', 0.00, None), (14, 'stack', 0.00, None), (15, 'stack', 0.00, None), (17, None, 0.00, None), (18, None, 0.00, 0.71),
    (27, 'stack', 0.00, None), (28, 'stack', 0.00, None), (31, None, 0.00, 0.1), (33, 'stack', 0.00, None), (34, 'stack', 0.00, None),
    (35, 'stack', 0.00, None))
ROLL = (  # Wrecking Ball: the ball's own mover values and no crouch (1) until Shift is pressed again
    (0, None, 0.00, None), (14, None, 0.00, None), (18, None, 0.00, None), (1, None, 0.00, None), (7, 0, 0.00, None), (9, None, 0.00, 0.4), (12, 0, 0.00, None), (15, 0, 0.00, None), (25, 'anim', 0.00, None),
    (27, 1, 0.00, None), (31, 'stack', 0.00, None), (42, 'stack', 0.00, None), (44, 0, 0.00, None), (45, 'stack', 0.00, None),
    (50, 'stack', 0.00, None), (52, 0, 0.00, None), (55, 0, 0.00, None), (73, 'stack', 0.00, None), (74, 0, 0.00, None),
    (75, 'stack', 0.00, None), (77, 0, 0.00, None), (78, None, 0.00, 0.99), (79, 'stack', 0.00, None), (81, None, 0.00, None),
    (10, 'button', 0.40, None), (11, 1, 0.40, None), (21, 'button', 0.40, None), (23, 0, 0.40, None))
# Doomfist: no input, jump or character collision while it lasts (4); from 0.24 s he falls at 1 m/s for 0.75 s (15)
RISING_UPPERCUT = (
    (3, 'stack', 0.00, 0.59), (4, None, 0.00, 0.59), (5, None, 0.00, 0.15), (6, None, 0.00, 0.59), (7, 'anim', 0.00, 0.45),
    (8, 'stack', 0.00, 0.59), (12, None, 0.00, 0.29), (19, None, 0.00, 0.1), (27, None, 0.00, 0.59), (29, 'stack', 0.00, 0.59),
    (11, None, 0.15, 0.24), (14, None, 0.24, 0.99), (15, None, 0.24, 0.99), (21, None, 0.24, 0.59))
COACH_GUN = (  # Ashe: from the shot (0.165 s) the ground hardly slows or steers her (14); the kick is PlayerMover.kick
    (0, 'stack', 0.00, 0.55), (1, 'stack', 0.00, 0.55), (3, None, 0.00, 0.55), (4, None, 0.00, 0.16), (10, 'anim', 0.00, 0.55),
    (12, None, 0.00, 0.45), (18, 'stack', 0.00, 0.55), (14, None, 0.16, 0.55), (15, None, 0.16, 0.55))
# Echo's Flight: 3.15 s along the aim on the state's own mover values with no gravity (6), no input for the first
# 0.15 s (23). State 24 limits the speed (mod 32) to var28952, which the graph runs from 30 down to 8 in the 0.1 s
# from 0.15 s and cuts off at 0.24 s: FLIGHT_LIMIT is where that line is when it bites, sent for the state's end.
# shortcut: the falling limit is one value, a frame or two later than the graph's; send the ChaseVar (25) to be exact.
FLIGHT = (
    (5, 1, 0.00, 3.14), (6, None, 0.00, 3.14), (7, None, 0.00, 0.15), (8, 'stack', 0.00, 3.14), (14, 'effect', 0.00, 3.14),
    (17, 0, 0.00, 3.14), (18, 'stack', 0.00, 3.14), (19, 'anim', 0.00, 3.14), (20, 'anim', 0.00, 3.14),
    (22, 'effect', 0.00, 3.14), (23, None, 0.00, 0.15), (26, None, 0.00, 3.14), (34, None, 0.00, 0.2), (37, 0, 0.00, 3.14),
    (24, None, 0.23, 0.35), (28, 0, 0.15, 3.14), (33, 0, 0.20, 3.14), (35, 'button', 0.20, 3.14))
FLIGHT_LIMIT = (5, Float(12.42))  # 1BB6's presence bit of var28952
UNTIL_ENDED = 86400.0  # a Timed ability's length when only a press ends it (Stealth)
# A press of jump in the air (Genji's second jump, graph 0157; Hanzo's Lunge, graph 010C): hero -> (PlayerMover
# method and its numbers, cooldown seconds). Once each time in the air, from AIR_JUMP_AFTER seconds off the ground
# (both graphs' Wait). The server's own launch: the client follows by the owner's records.
# shortcut: no instance is sent for either, so neither has its animation or sound and the Lunge's icon does not
# count down; climbing a wall does not give Genji his jump back.
AIR_JUMPS = {GENJI: (('hop', 1.5), 0.0), HANZO: (('lunge', 15.0, 0.3), 5.0)}
AIR_JUMP_AFTER = 0.1
# 0C9B's presence bit of var7779 and the 0.65 its Entry sets: a synced variable, so the client has only what the
# frame sends (unsent, its MovementMod added 0 and every record pulled the client along).
STEALTH_SPEED = (14, Float(0.65))
FIRE = 0x01  # the command's primary fire bit: what also ends Wraith Form, Cryo-Freeze and Stealth
# Cryo-Freeze's heal while the ice holds. shortcut: the wiki's 37.5 health a second (150 in all); the graph's
# ModifyHealth reads its amount from an entity variable (6578) that no graph of the catalog sets.
CRYO_HEAL = (37.5, 3.99)

# From tools/assets/ability_kit.py: per body graph, its first StateAbility on Shift, E or right mouse that has a
# cooldown. Soldier's Biotic Field comes out as soldier.field_instance has it.
# shortcut: a press only runs the icon's cooldown sweep; the ability itself (movement, projectiles, effects) is not
# served. Abilities without a var472 cooldown (Torbjorn, Zenyatta, Hanzo, Bastion's configuration) and the
# ultimates are left out.
# UI 0x0D800000000060F7 is a targeting prompt (DEPLOY, BUILD, TELEPORT / CANCEL): sent always on it stuck on screen
# from spawn, so no ability sends its presenter (Symmetra's Teleporter 51, Mei's Ice Wall 55, Reaper's Shadow
# Step 61, Doomfist's Rocket Punch 68). shortcut: no prompt while aiming either; send it during the targeting.
ABILITIES = {
    # Combat Roll: 15 m/s for its 0.4 s, then no more than 5.5 (graph 01F0: var1648 and the STU_FF3471E9 actions).
    CASSIDY: (Ability(Graph(496, 27, 8, 23), 0, 0x08, (21,), 1, 7, 6, 5, timed=(0.4, 0.4, 0.0), running=COMBAT_ROLL,
                      moves=('roll', 15.0, 0.4, 5.5), reloads=True),
              Ability(Graph(489, 16, 8, 11), 0, 0x10, (13,), 3, 6, 10, 4, cast=((), 1), anim=((), 2))),
    ROADHOG: (Ability(Graph(456, 80, 29, 69), 7, 0x08, (58,), 19, 15, 8, 8), Ability(Graph(576, 21, 7, 18), 0, 0x10, (16,), 1, 6, 8, 5)),
    TORBJORN: (),
    SOMBRA: (Ability(Graph(3097, 64, 26, 42), 2, 0x10, (41, 46), 20, 7, 6, 7),
             Ability(Graph(3227, 45, 15, 34), 6, 0x08, (34, 35), 10, 7, 6, 7, 13, timed=(UNTIL_ENDED, 0.75, 0.0), cancel_on=FIRE,
                     running=STEALTH, extra=(STEALTH_SPEED,), hides=0.75)),
    MEI: (Ability(Graph(1534, 51, 10, 36), 0, 0x08, (36, 37), 3, 9, 12, 6, timed=(4.24, 0.2, 0.0), cancel_on=FIRE,
                  running=CRYO_FREEZE, heals=CRYO_HEAL),
          Ability(Graph(1570, 69, 25, 45), 24, 0x10, (50,), 20, 13, 16, 7, 33)),
    BASTION: (Ability(Graph(339, 62, 18, 41), 59, 0x02, (57,), 8, 17, 1, 6, 61),),
    LUCIO: (Ability(Graph(631, 19, 5, 15), 0, 0x10, (15,), 3, 4, 12, 5),),
    # shortcut: a Timed ability ends on its own button or cancel_on's; the client's other cancels are not followed.
    REAPER: (Ability(Graph(32, 34, 6, 20), 9, 0x08, (21,), 1, 5, 8, 6, 10, timed=(3.4, 0.4, 0.0), cancel_on=FIRE,
                     running=WRAITH_FORM),
             Ability(Graph(242, 67, 16, 52), 25, 0x10, (41,), 4, 15, 10, 7, 26, extra=((13, Float(1.5)),),
                     timed=(2.0, 2.0, 0.0), running=SHADOW_STEP, aiming=SHADOW_STEP_AIM, steps=(35.0, 1.5, 1, 5))),
    ANA: (Ability(Graph(2967, 14, 9, 13), 4, 0x10, (12,), 3, 8, 10, 4, cast=((5,), 0), anim=((5, 1), 6)), Ability(Graph(3037, 18, 8, 14), 2, 0x08, (16,), 4, 6, 12, 5, cast=((5, 7), 6), anim=((5, 7), 10))),
    ZENYATTA: (),
    # Blink: graph 0x0024's action STU_A807207A carries her 7.5 m in 0.1 s (m_6A79BD98, m_162A28FA).
    # shortcut: the server moves her and the client follows by the owner's records; the blink's own states (its
    # trail and sound, the field-of-view kick of STU_75D1C308) are not sent yet.
    TRACER: (Ability(Graph(26, 39, 8, 30), 16, 0x10, (32,), 3, 7, 12, 6, timed=(1.25, 1.25, 0.0), running=RECALL, rewinds=3.0),
             Ability(Graph(36, 38, 10, 32), 36, 0x08, (32,), 2, 9, 3, 6, 38, ((0, Float(3.0)), (8, Float(3.0))), charges=3,
                     moves=('dash', 7.5, 0.1))),
    # Jump Jet: 20 m/s up and 10 m/s ahead (graph 0027's STU_FF3471E9). shortcut: its own states (the jets' effect
    # and animation) are not sent.
    # Hover Jets (graph 0025, on the jump button): held 0.2 s, the thrust state STU_A9C4FF83 pushes her up while the
    # fuel (var87: 100, 50 a second) lasts; it fills again at 43 a second from 0.2 s after the release.
    # shortcut: the thrust is 50 m/s2 fading to nothing at a rise of 6.875 m/s (2.5 x the state's 2.75), fitted to
    # the client's own hover within 4 cm over 2 s, not read from its code; secondary fire does not hover.
    PHARAH: (Ability(Graph(39, 14, 4, 12), 1, 0x08, (10,), 2, 3, 10, 5, 4, moves=('jet', 10.0, 20.0)),
             Ability(Graph(37, 39, 6, 18), 35, 0x40, (), None, None, 0.2, 6, 37, extra=((2, Float(100.0)),),
                     running=HOVER_JETS, meter=(4, 100.0, 50.0, 43.0), delay=0.2, refills=2, lifts=(50.0, 6.875)),
             Ability(Graph(123, 14, 4, 8), 5, 0x10, (12,), 2, 3, 9, 4, cast=((6, 1), 0), anim=((6,), 2))),
    WIDOWMAKER: (Ability(Graph(86, 16, 8, 13), 6, 0x10, (12,), 5, 7, 15, 5, cast=((7, 2), 0), anim=((7,), 1)), Ability(Graph(90, 56, 26, 42), 36, 0x08, (45,), 17, 10, 12, 6)),
    JUNKRAT: (Ability(Graph(2410, 38, 17, 28), 0, 0x08, (32,), 4, 15, 8, 6, extra=((0, Int(2)), (12, Int(2))), charges=2, cast=((10, 13), 8), anim=((10,), 2)), Ability(Graph(2490, 15, 8, 12), 0, 0x10, (11,), 4, 7, 10, 4, cast=((3,), 1), anim=((3,), 6))),
    ORISA: (Ability(Graph(3275, 35, 13, 20), 2, 0x02, (32,), 3, 12, 6, 6, cast=((6, 8), 4)), Ability(Graph(3316, 28, 8, 23), 0, 0x08, (22, 23), 2, 7, 10, 5)),
    WRECKING_BALL: (Ability(Graph(5348, 57, 32, 40), 25, 0x02, (24,), 22, 15, 5, 7), Ability(Graph(5478, 19, 13, 21), 0, 0x10, (4, 5), 6, 12, 15, 5),
                    # Roll (graph 1203): a toggle with no cooldown
                    Ability(Graph(4611, 83, 8, 44), 2, 0x08, (3,), None, 7, 0.0, 7, timed=(UNTIL_ENDED, 0.4, 0.0), running=ROLL)),
    ASHE: (Ability(Graph(5373, 19, 6, 13), 4, 0x10, (10,), 3, 5, 12, 5, cast=((5, 0), 12), anim=((5,), 11)), Ability(Graph(5395, 19, 8, 15), 2, 0x08, (9,), 3, 5, 10, 5, cast=((3, 4), 16), anim=((3,), 10), timed=(0.55, 0.55, 0.0),
                   running=COACH_GUN, moves=('kick', 18.0, 0.165))),
    # Glide (graph 1DFC): jump held while she falls with no ground within 2.5 m of a point 0.25 m above her feet
    # (the graph's RayCast): terminal velocity 4 m/s and SPEED +0.5 (MovementMod 8).
    # shortcut: of the states the graph has on beside it, only the wings' effect and animation are sent.
    ECHO: (Ability(Graph(7676, 23, 2, 15), None, 0x40, (), None, None, 0.0, 5,
                   running=((9, 'button'), (8, None), (5, 'anim'), (7, 'effect')), meter=(None, 1.0, 0.0, 0.0), falling=2.25),
           # Flight (graph 1BB6). shortcut: a second press does not end it, and its end does not cut the speed to 8.
           Ability(Graph(7094, 38, 13, 22), 0, 0x08, (4, 21), 3, 12, 6, 6, extra=(FLIGHT_LIMIT,), timed=(3.15, 3.15, 0.0),
                   running=FLIGHT, moves=('fly', 28.0)),
           Ability(Graph(8125, 38, 15, 26), 0, 0x10, (4,), 2, 14, 8, 6), Ability(Graph(7222, 17, 16, 12), 5, 0x02, (1,), 7, 15, 6, 5, cast=((10,), 0), anim=((), 8))),
    # Exo Boots (graph 1983): the charge var154 (bit 2) builds 0.1428 at once and every 0.1 s of crouch, lasts 1 s
    # after it, and the MovementMod state 5 (always on, under switch 19) multiplies the jump speed by
    # 1 + var154 x (var176 - 1), var176 = 3 (bit 0): 18 m/s at full charge. shortcut: none of its effects or sounds.
    BAPTISTE: (Ability(Graph(5985, 19, 9, 14), 3, 0x10, (11,), 2, 8, 25, 5, cast=((4,), 12), anim=((4, 0), 5)), Ability(Graph(6565, 12, 6, 8), 3, 0x08, (5,), 3, 5, 13, 4),
               Ability(Graph(6531, 27, 5, 12), None, CROUCH, (), None, None, 0.0, 5, extra=((0, Float(3.0)),),
                       always=((19, 1), (5, None)), crouch=(2, 0.1428, 0.1, 1.0))),
    # Angelic Descent (her passives' graph 0071): while jump is held a MovementMod state (1) sets the fall's terminal
    # velocity to 2 m/s. A Held ability with a meter that never drains.
    # Guardian Angel: 17 m/s (var1002) to a point by an ally within 30 m (var1648), ended by the button again after
    # 0.15 s; its 1.5 s run from its end. shortcut: the point is 2.5 m short of the ally where the graph takes 2.5
    # to 3.7 m by the two bodies; a jump does not sling her on (launches 73, 77, 100); 2 s at most, where the graph's
    # MoveTo has its own limits.
    MERCY: (Ability(Graph(110, 60, 20, 34), 16, 0x08, (25,), 11, 9, 1.5, 7, 22, timed=(2.0, 0.15, 0.0),
                    running=GUARDIAN_ANGEL, follows=(17.0, 30.0, 2.5, 0.5)),
            Ability(Graph(113, 24, 2, 10), None, 0x40, (), None, None, 0.0, 5, running=((0, 'button'), (1, None)),
                    meter=(None, 1.0, 0.0, 0.0))),
    HANZO: (),
    # Her mech's body graphs (content.SPAWN_BODY; not in the pinned catalog, hand-mapped with ability_kit's rules,
    # research/dva-mech.md): Boosters 0x047E, Micro Missiles 0x1128 (Wait 7 -> Volley 5), Defense Matrix 0x02CC.
    # Their icons' unavailable mark (UX 0x0D80...0786) is var2575 || var2575(entity) || !var4993 (Boosters': no
    # var4993 term), so they send var4993 = True.
    # shortcut: Self-Destruct (0x0B2F, button 37) is an ultimate: left out like the others.
    # Boosters run 2 s (ChaseVar 4: var215 up to var567 = 2.0, sent), a second press after Wait 8's 0.4 s ends them;
    # running: the movement modifier 316CFEF2 2, effect 3, Stacks 21/10/40/11/37/30, Anims 1/13, switch 42 false ->
    # C1BA7545 36. Defense Matrix runs while held and var87 (its meter, drained by ChaseVar 32 at var12097 = 50/s,
    # refilled at var12096 = 16/s after its 1 s cooldown) lasts: switch 8 (v87 > 0) true -> effect 4, state 5, Anims
    # 25/24, Stack 1 -> effects 29/28, DeflectProjectiles 3 -> BD02E168 36 (its client-only visuals); switch 19 ->
    # ChaseVar 32. DeflectProjectiles, PhysicsExplosion and C1BA7545 read no bits (empty DSR, config_var_eval.resolve).
    # Boosters' collision damage and knockback are the server's (Match.boost_collide); TrackTargets 6 and
    # PhysicsExplosion 25 are not sent. shortcut: Defense Matrix's native transform/scale remain unverified.
    DVA: (Ability(Graph(0x047E, 43, 10, 27, frozenset({10})), 0, 0x08, (31,), 2, 9, 4, 6, extra=((6, Float(2.0)),),
                  running=((4, 'chase'), (2, None), (3, 'effect'), (21, 'stack'), (1, 'anim'), (13, 'anim'),
                           (10, 'stack'), (40, 'stack'), (11, 'stack'), (37, 'stack'), (30, 'stack'), (42, 0), (36, None),
                           # UX 41: the CANCEL prompt (under Wait 8). UX 32: the centre ring (var215 / var567) and
                           # seconds; both parent to var11815 (sent with the body, kits.initialize_practice_kit).
                           (41, None), (32, None)),
                  timed=(2.0, 0.4, 11.99), clock=1),  # var215 (presence bit 1): what UX 32's ring reads
          Ability(Graph(0x1128, 16, 10, 12), 0, 0x10, (12,), 6, 9, 8, 4, cast=((7,), 5), anim=((), 11),
                  extra=((0, Bool(True)),),
                  # Wait 7's effect 8 (0x0300000000001F99) and Volley 5's effect 6 (0x030000000000028A): the
                  # launch effects; left out, the missiles fired silently
                  effects=(8, 6)),
          Ability(Graph(0x02CC, 37, 19, 24, frozenset({16, 5})), 0, 0x02, (23,), 8, 18, 1, 6,
                  extra=((1, Bool(True)), (0, Float(16.0)), (2, Float(50.0)), (4, Float(100.0))),
                  running=((8, 1), (4, 'effect'), (5, None), (25, 'anim'), (24, 'anim'), (1, 'stack'), (29, 'effect'),
                           (28, 'effect'), (3, None), (36, None), (19, 1), (32, 'chase')),
                  meter=(7, 100.0, 50.0, 16.0),
                  # SubScript 46 runs the meter graph 214E (its child instance, kits.initialize_practice_kit); message
                  # state 47 posts into that child. Posted with no child running, the client asserted.
                  always=((33, lambda: subscript(0x214E)), (34, 'message')))),
    SIGMA: (Ability(Graph(7426, 28, 14, 22), 0, 0x08, (13,), 4, 13, 12, 6), Ability(Graph(7099, 23, 11, 21), 0, 0x10, (6,), 2, 10, 10, 5, cast=((2,), 3), anim=((2,), 9))),
    SYMMETRA: (Ability(Graph(5502, 21, 10, 16), 5, 0x08, (10, 11), 4, 9, 10, 5, extra=((1, Int(3)), (8, Int(3))), charges=3, cast=((14,), 12), anim=((), 17)), Ability(Graph(5489, 56, 35, 42), 23, 0x10, (36,), 25, 19, 10, 6)),
    MOIRA: (Ability(Graph(3964, 33, 7, 29), 9, 0x08, (25,), 2, 6, 6, 6, timed=(0.94, 0.94, 0.0), running=FADE),
            Ability(Graph(4324, 38, 14, 27), 0, 0x10, (16,), 4, 12, 8, 6, anim=((17, 18), 13),
                          choice=((0x01, ((17,), 10)), (0x02, ((17,), 11))))),
    # Swift Strike: 15 m along the aim in 0.4 s (graph 01DB's StateMoveTo: the client's own takes 57 frames to its
    # target however far that is, at an even speed). shortcut: straight to the first wall, where the graph casts a
    # ray for its end point; its cooldown does not reset on an elimination.
    GENJI: (Ability(Graph(475, 34, 11, 23), 0, 0x08, (24,), 2, 10, 8, 6, timed=(0.5, 0.5, 0.0), running=SWIFT_STRIKE,
                    moves=('strike', 15.0, 37.5)),
            Ability(Graph(931, 30, 10, 24), 0, 0x10, (20,), 3, 9, 8, 7)),
    DOOMFIST: (Ability(Graph(3334, 74, 27, 45), 0, 0x02, (60, 61), 16, 11, 4, 7),
               # Rising Uppercut (graph 0DD8). shortcut: it hits and lifts nobody, and its cooldown starts when the
               # slow fall ends (0.99 s), not with the ability (0.59 s).
               Ability(Graph(3544, 32, 7, 18), 0, 0x08, (26,), 3, 5, 6, 5, timed=(0.99, 0.99, 0.0), running=RISING_UPPERCUT,
                       moves=('uppercut', 10.0, 40.0))),
    REINHARDT: (Ability(Graph(125, 97, 22, 52), 46, 0x08, (76,), 16, 10, 10, 7, timed=(3.89, 3.89, 0.0), running=CHARGE),
                Ability(Graph(149, 55, 15, 32), 50, 0x02, (38,), 4, 14, 5, 6, 59), Ability(Graph(226, 19, 6, 13), 9, 0x10, (17,), 3, 5, 6, 5, cast=((10, 4, 5, 16, 1), 0), anim=((10, 4), 3))),
    # Jump Pack (graph 010A; its cooldown is 6 s, 2 in Primal Rage: two writers, so ability_kit.py prints none):
    # 30 m/s along the lifted aim, its states on until he lands. shortcut: no damage where he lands, and the
    # cooldown runs from the landing.
    WINSTON: (Ability(Graph(250, 16, 7, 12), 2, 0x10, (9,), 2, 6, 13, 5),
              Ability(Graph(266, 36, 6, 24), 19, 0x08, (29,), 1, 5, 6, 6, timed=(UNTIL_ENDED, UNTIL_ENDED, 0.0),
                      running=JUMP_PACK, moves=('leap', 30.0), lands=0.2)),
    BRIGITTE: (Ability(Graph(4697, 73, 28, 52), 0, 0x08, (1,), 17, 15, 4, 7), Ability(Graph(3805, 33, 19, 27), 3, 0x10, (12, 29), 6, 17, 6, 6, 6, ((3, Int(3)), (13, Int(3))), charges=3)),
    ZARYA: (Ability(Graph(580, 21, 7, 15), 0, 0x08, (18,), 2, 6, 10, 5), Ability(Graph(604, 27, 13, 18), 6, 0x10, (23,), 6, 12, 8, 5, 9)),
}


class AltFire(Volley):
    """A weapon's alternate fire (right mouse) from the primary's magazine: its own cadence and damage (target.ALT_KIT),
    `cost` rounds a shot; its Volley is pulsed like Helix's (alt_states)."""

    def __init__(self, quanta, rifle, weapon, cost=1, *, windup=0.0):
        super().__init__(quanta, SECONDARY, weapon.shot_seconds)
        self.rifle, self.weapon, self.cost = rifle, weapon, cost
        self.windup = ceil(windup * 1_000_000 / quanta)
        self.windup_at = None

    def command(self, command):
        if self.rifle.refill_at is not None or not self.rifle.ammo or not command.buttons & self.button:
            self.windup_at = None
            return False
        if self.windup:
            if command.frame + self.windup < self.ready_at:
                return False
            if self.windup_at is None:
                self.windup_at = command.frame + self.windup
            if command.frame < self.windup_at:
                return False
        if not self.rifle.can_queue_shot() or not super().command(command):
            return False
        self.windup_at = None
        self.rifle.spend(command.frame, self.cost)
        return True


class ChargedAlt(AltFire):
    """An alternate fire loaded while held and fired on release (Zenyatta's Orb Volley): a round every `windup`, up
    to `most` and the magazine; `weapon` then shoots that many as pellets.
    shortcut: the orbs land at once (as other alts), not one per 0.144 s; a release under one `windup` fires nothing."""

    def __init__(self, quanta, rifle, weapon, cost=1, *, windup, most):
        super().__init__(quanta, rifle, weapon, cost, windup=windup)
        self.base, self.most, self.held_at = weapon, most, None

    def command(self, command):
        self.frame = command.frame
        if self.rifle.refill_at is not None:  # a reload drops the charge
            self.held_at = None
            return False
        if command.buttons & self.button:
            if self.held_at is None and command.frame >= self.ready_at:
                self.held_at = command.frame
            return False
        if self.held_at is None:
            return False
        loaded = min(self.most, (command.frame - self.held_at) // self.windup, self.rifle.ammo)
        self.held_at = None
        if not loaded or not self.rifle.can_queue_shot():
            return False
        Volley.command(self, replace(command, buttons=command.buttons | self.button))
        self.weapon = replace(self.base, pellets=loaded)
        self.rifle.spend(command.frame, loaded * self.cost)
        return True


def alt_states(kit, update):
    """The alternate fire's pulsed states with the Volley's start or stop (kits.Kit.alt)."""
    pulsed, volley = kit.alt
    return {**{state: PAYLOADS[how]() for state, how in pulsed.items()}, volley: volley_payload(update)}


class Charges(Volley):
    """A charge ability's clock: a press spends a charge while any is left, and one comes back every `seconds`
    (the sweep runs while any is missing). A held button spends one charge, not one per frame."""

    def __init__(self, quanta, button, seconds, most):
        super().__init__(quanta, button, seconds)
        self.most = self.charges = most
        self.held = False

    def refill(self, frame):
        while self.charges < self.most and 0 < self.ready_at <= frame:
            self.charges += 1
            if self.charges < self.most:
                self.ready_at += self.cooldown
            # full: ready_at stays at the last return, so showing_cooldown() resends var11 once more

    def ability(self):
        self.refill(self.frame)
        return super().ability()

    def command(self, command):
        self.frame = command.frame
        self.refill(command.frame)
        pressed = bool(command.buttons & self.button)
        pressed, self.held = pressed and not self.held, pressed
        if not pressed or not self.charges or self.serial == 65535:
            return False
        if self.charges == self.most:
            self.ready_at = command.frame + self.cooldown
        self.charges -= 1
        self.serial += 1
        self.updates.append(RifleUpdate(0, self.serial, True, True))
        self.updates.append(RifleUpdate(0, self.serial, False, True))
        return True


def ability_instance(ability, cooldown, update=None):
    """The ability graph's instance: its icon, and its StateAbility with the cooldown's timing while it runs; with a
    press's volley update, also its cast path and Volley (as Biotic Field's throw)."""
    graph = ability.graph
    active = {state: None for state in ability.ux}
    active.update({state: how() if callable(how) else PAYLOADS[how]() for state, how in ability.always})
    if getattr(cooldown, 'aiming', False):
        active.update({state: how() if callable(how) else PAYLOADS[how]() for state, how in ability.aiming})
    if getattr(cooldown, 'active', False):
        special = {'effect': lambda: muzzle_payload(True, cooldown.count), 'anim': anim_payload,
                   'chase': lambda: chase_payload(cooldown.meter)}
        elapsed = getattr(cooldown, 'elapsed', 0.0)
        active.update({state: special.get(how, PAYLOADS.get(how))() for state, how, *window in ability.running
                       if not window or window[0] <= elapsed and (window[1] is None or elapsed < window[1])})
    elif getattr(cooldown, 'count', 0):  # an effect left out keeps playing: stop it with its Begin bit cleared
        active.update({state: muzzle_payload(False, cooldown.count) for state, how, *_ in ability.running if how == 'effect'})
    if ability.refills is not None and cooldown.refilling:
        active[ability.refills] = chase_payload(cooldown.meter)
    if ability.state is not None:
        active[ability.state] = cooldown.ability  # built at send time: each resend carries the elapsed time
    cast = dict(ability.choice).get(cooldown.pick) if ability.choice else ability.cast
    if update is not None and cast is not None:
        path, volley = cast
        active.update({state: None for state in path})
        active[volley] = volley_payload(update)
        if ability.anim is not None:
            path, anim = ability.anim
            active.update({state: None for state in path})
            active[anim] = anim_payload()
        active.update({state: muzzle_payload(True, update.serial) for state in ability.effects})
    presence = dict(ability.extra)
    if ability.cooldown_bit is not None:  # Hover Jets have no cooldown
        presence[ability.cooldown_bit] = Float(ability.seconds)
        presence[ability.left_bit] = cooldown_left(graph.index, ability.state if ability.bound is None else ability.bound,
                                                   ability.state_bits)
    if ability.meter and ability.meter[0] is not None:
        presence[ability.meter[0]] = Float(cooldown.meter)
    if ability.crouch:
        presence[ability.crouch[0]] = Float(cooldown.meter)
    if ability.steps:
        presence[ability.steps[2]] = Bool(bool(getattr(cooldown, 'aiming', False)))
        if getattr(cooldown, 'target', None) is not None:
            presence[ability.steps[3]] = Vec3(*cooldown.target)
    if ability.clock is not None and cooldown.active:  # read at send time: a value fixed at the press stays 0
        presence[ability.clock] = lambda: Float(cooldown.meter)
    if ability.charges:
        cooldown.refill(cooldown.frame)  # before the payload callable runs: var11 is read now
        bit, most = ability.extra[0]
        presence[bit] = type(most)(cooldown.charges)
    return Instance(graph.index, graph, presence=presence, active=active)


def _bits(*fields):
    out = BitWriter()
    for value, width in fields:
        out.bits(value, width)
    return out


PAYLOADS = {1: lambda: _bits((1, 1)), 0: lambda: _bits((0, 1)), 'button': lambda: _bits((1, 1), (0, 4)),
            'stack': lambda: _bits((1, 2)), 'unlinked': lambda: _bits((0, 1)), None: lambda: None,
            'targets': lambda: _bits((0, 1)),  # TrackTargets with no targets (sombra_hack_states sends one)
            'ability': ability_payload, 'message': message_state}  # StateAbility, no cooldown running
BEAM_RANGE = {0x0241: 15.0, 0x157C: 12.0}  # Zarya, Symmetra: metres, their Entry Stack's constant (var1648)
# What each weapon graph's own SetVar actions set, as constants, and weapon_instance's defaults must not override:
# var229 / var230, the Volley's spread range (its Stack lerps var243 between them), and var476. Read from the
# vendored catalog (research/lizrollin-runtime.md section 10; test_kits checks the table against it). Sent as 0 / 20
# before, every hero's shots had no spread on the client.
AUTHORED = {
    0x0016: {229: Float(0), 230: Float(3.5)},  # Tracer
    0x001B: {229: Float(0), 230: Float(3)},  # Widowmaker
    0x012E: {229: Int(0), 230: Float(1.2)},  # Bastion
    0x0164: {229: Float(0), 230: Float(0)},  # Zenyatta
    0x01C6: {230: Float(8), 476: Float(6.6)},  # Roadhog
    0x01D2: {229: Int(0), 230: Int(6)},  # Cassidy
    0x0304: {476: Int(29)},  # Genji
    0x0B1B: {229: Float(0.5), 230: Float(2.4)},  # Sombra
    0x0BD4: {229: Float(0.25), 230: Float(1.2), 476: Int(11)},  # Orisa
    0x14FC: {229: Float(0), 230: Float(2)},  # Ashe
    0x1514: {229: Int(0), 230: Float(2)},  # Wrecking Ball
    0x1932: {229: Float(0), 230: Float(0)},  # Baptiste
}
# var476, the Volley's damage, where the graph's SetVar is an expression that gives one number on our bodies (the
# other branch asks for a mode flag no offline mode sets). 20 for the rest: Bastion's own, and the three whose script
# only sets it when a shot is charged (Hanzo, Symmetra, Zarya). test_kits checks it against the body VM.
VOLLEY_DAMAGE = {0x0B1B: 8, 0x1514: 5, 0x1932: 24}  # Sombra, Wrecking Ball, Baptiste
# D.Va's Fusion Cannons (graph 0x050A, owner indices; heroes/dva.md): Volley 3's muzzle is var58, which the graph's
# server-side SetVars 112/200 flip between the cannons' hardpoints before each shot (first 0x05, then 0x07); unsent,
# shots left the mech's origin. Shot n (from 1) uses DVA_MUZZLES[n % 2].
DVA_MUZZLES = (0x0DC0000000000007, 0x0DC0000000000005)
# While fire is held, the button's m_80AF45FB plug runs switch 16 (var33 priority) true: Stack 13 (var1273, the
# firing anim category) with Anims 17/18, ChaseVars 19/20 (var261, DFM 21), Stacks 27/29 (var233, var689) and
# BD02E168 31 (empty DSR; its client-only Waits flash the muzzle effects 0x151B/0x151C at var58's cannon).
DVA_HUD = Graph(0x0276, 58, 10, 36, frozenset({3, 7, 8, 9, 10, 27}))  # the mech's HUD graph (graph_layout.py)
# Her pilot's Light Gun (content.pilot), graph 0x02B8: the kit she played as before the mech was found (git 0887900).
DVA_PILOT = Kit(Graph(0x02B8, 41, 13, 25), {0: 1, 1: 0, 3: 'button', 11: None, 13: 1, 14: None, 23: 0, 25: None,
                27: None, 37: 0}, 27, 5,
                # a shot starts switch 29 (owner 26) under Stack 17 and Stacks 8/31 (owner 28) under Volley 5; left to
                # the client they never ended and the firing animation kept playing: the server turns them off
                {17: 'stack', 26: 1, 8: 'stack', 28: 'stack'}, {581: 0, 198: 1, 28: 3, 350: 4, 229: 7, 230: 8, 233: 10, 53: 12},
                0x0C00000000007D85, 20, 46, reload=({4: 'stack', 1: 1, 0: 1}, 15))  # weapon_kit.py: switch 1 true
# Her ultimates by body: (the HUD instance, the wiki's points to charge it). Both presenters parent to the icon row
# (var10964) and bind var342 against var338: Self-Destruct graph 0x0B2F's presenter 26 (owner 25) in the mech, the
# pilot graph 0x026A's switch 85 (owner 82) false -> presenter 78 (owner 75) out of it (heroes/dva.md).
DVA_ULTS = {SPAWN_BODY[DVA]: (Instance(0x0B2F, Graph(0x0B2F, 37, 7, 25, frozenset({3, 8, 15})), active={25: None}),
                              1540),
            PILOT_BODY[DVA]: (Instance(0x026A, Graph(0x026A, 108, 24, 59, frozenset({16, 24, 25, 33, 46})),
                                       active={82: _bits((0, 1)), 75: None}), 319.2)}


def dva_call_mech():
    """The pilot's graph 0x026A casting Call Mech (owner = state index below 16): StateAbility 5, switch 15 (owner 14)
    true, Wait 6 (0.2 s) with Anim 22 (owner 21, alias 0x0C80000000000433); the client runs Wait 6's onFinished on
    to STU_54FEB21C 32 and Anim 36 (alias 0x0C800000000000E5). Sent while she waits for the mech."""
    instance = DVA_ULTS[PILOT_BODY[DVA]][0]
    return Instance(0x026A, instance.graph, active={**instance.active, 5: ability_payload(1), 14: _bits((1, 1)),
                                                    6: None, 21: anim_payload()})


def dva_eject():
    """The pilot's graph 0x026A just after Eject! or Self-Destruct: Wait 28 (owner 27) with Anim 27 (owner 26,
    alias 0x0C80000000000439), at the end of the branch an event starts (state 10 -> 11 -> 7 -> 45 -> Wait 28).
    shortcut: the states before Wait 28 (undecoded classes CA861E44, 5AFB02C7, 72DDE005) are not sent."""
    instance = DVA_ULTS[PILOT_BODY[DVA]][0]
    return Instance(0x026A, instance.graph, active={**instance.active, 27: None, 26: anim_payload()})


def dva_self_destruct(fuse):
    """The mech's Self-Destruct graph 0x0B2F with its countdown on (owner = state index below 18): Ability 0, base
    state 9 with Stack 1, effects 5 (0x0300000000001702, the glow) and 10, Anim 13, and the fuse: Wait 6 times out
    on var215 (instance bit 2), then its onFinished runs the explosion effect action (0x0300000000001703) and
    TrackTargets 3. Sent as the mech's last owner frame before D.Va leaves it, so the client runs the rest itself.
    shortcut: STU_D07EBF01 2, STU_F08E1453 21, STU_B9898052 36 and the BD02E168s under them are left out (payloads
    not decoded)."""
    graph = DVA_ULTS[SPAWN_BODY[DVA]][0].graph
    return Instance(0x0B2F, graph, presence={2: Float(fuse)},
                    active={25: None, 0: ability_payload(1), 9: None, 1: _bits((1, 2)), 5: muzzle_payload(True, 1),
                            10: muzzle_payload(True, 1), 13: anim_payload(), 6: None, 12: chase_payload(0.0)})
DVA_HELD = {16: PAYLOADS[1], 13: PAYLOADS['stack'], 17: anim_payload, 18: anim_payload,
            19: lambda: chase_payload(0.0), 20: lambda: chase_payload(0.0), 27: PAYLOADS['stack'],
            29: PAYLOADS['stack'], 31: PAYLOADS[None]}
# Sombra's hack (graph 0x0B1B owner indices): Stack 21, TrackTargets 25, Anim 33, button 60. The channel Wait 46
# (timeout var670) runs the tether: STU_9932D728 49..52 (effects 0x1AC7, 0x1B30..0x1B32 to var8909's hardpoint 0x01;
# empty DSR), which the client starts only when the server lists them; Match sends var8909 as the target.
SOMBRA_HACK_STATES = (21, 25, 33, 60, 49, 50, 51, 52)


def _targets(entity):
    """TrackTargets with one target (research/statescript-payloads.md)."""
    targets = BitWriter()
    targets.bit(1)
    targets.w_u16(1)
    targets.bits(entity, 32)
    targets.w_var(0)
    return targets


def sombra_hack_states(entity, frame):
    return {21: PAYLOADS['stack'](), 25: _targets(entity), 33: anim_payload(frame), 60: PAYLOADS['button'](),
            49: None, 50: None, 51: None, 52: None}


# Moira's Biotic Grasp beam (graph 0x0F76 owner indices, under button 28): Stack 27 -> state 25 -> TrackTargets 96
# (switch 85 on its target), switch 91 -> TrackRay 21, switch 22 true -> effect 75 (0x201B on the target), switch 92
# (var375 && filter) -> switch 81 false -> STU_9932D728 23, the beam itself (effect 0x1F31 from hand hardpoint 0x0A to
# var375's hardpoint 0x01; empty DSR). 22's false branch is STU_9932D728 30, a free beam to TrackRay 29's hit. The
# client evaluates switch 22 itself (var38 && var2318 && var375), so Match sends those vars (research/asi-tracer.md).
# shortcut: switch 81 (true: beam 82, effect 0x2061, an undecoded entity test) always false; ChaseVar 59, rumble 93 not sent.
MOIRA_GRASP_STATES = (27, 25, 96, 85, 91, 21, 22, 75, 92, 81, 23)


# Mercy's Caduceus Staff (graph 0x006B owner indices): Stack 46 (Entry) -> switch 27 (var465 || var7650 && var468) ->
# Stack 12 -> Stack 17 (var699) and switch 26 (var465 == heal identifier). Heal: Stacks 18 / 48 (var689), switch 20
# (var468 valid) -> STU_431F3FCC 21 (the status), Wait 28, BD02E168 52; effect 25 (the staff glow). Boost: Stacks
# 19 / 49 (var690), switch 22 -> 23, Wait 29, BD02E168 53; effect 24. Match sends var465 (bit 0) and var468 (bit 1).
MERCY_HEAL, MERCY_BOOST = 0x0D80000000001DBA, 0x0D80000000001DBB  # var465: switch 26 compares it to the heal one
MERCY_STAFF_STATES = (46, 27, 12, 17, 26, 18, 48, 20, 21, 28, 52, 25, 19, 49, 22, 23, 29, 53, 24)


def mercy_staff_states(boost):
    effect, stack = _bits((1, 1), (1, 2)), PAYLOADS['stack']
    states = {46: stack(), 27: PAYLOADS[1](), 12: stack(), 17: stack(), 26: PAYLOADS[int(not boost)]()}
    if boost:
        states.update({19: stack(), 49: stack(), 22: PAYLOADS[1](), 23: None, 29: None, 53: None, 24: effect})
    else:
        states.update({18: stack(), 48: stack(), 20: PAYLOADS[1](), 21: None, 28: None, 52: None, 25: effect})
    return states


def moira_grasp_states(entity):
    return {27: PAYLOADS['stack'](), 25: None, 96: _targets(entity), 85: PAYLOADS[1](), 91: PAYLOADS[1](), 21: None,
            22: PAYLOADS[1](), 75: _bits((1, 1), (1, 2)), 92: PAYLOADS[1](), 81: PAYLOADS[0](), 23: None}


def weapon_instance(kit, ammo, update=None, extra_active=None, reload_seconds=1.5, *, scoped=False, fire_elapsed=None):
    """var581 is the reload's length: the reload StateAnim stretches the shared alias over it and the graph's
    Waits split it at var6884 (soldier.Rifle). Send the hero's own (target.PLAYER_KIT), or the anim ends early."""
    if type(ammo) is not int or not 0 <= ammo <= kit.capacity:
        raise ValueError('invalid ammo')
    active = {state: PAYLOADS[how]() for state, how in kit.states.items()}
    extra = {}
    if update is not None:
        active.update({state: PAYLOADS[how]() for state, how in kit.fire.items()})
        if kit.volley is not None:
            active[kit.volley] = volley_payload(update)
        if kit is KITS[WINSTON]:
            # Omitting the effect states leaves the arcs running. Send their stop bit on release or reload.
            effect = _bits((int(update.active), 1), (1, 2))
            active.update({10: effect, 43: effect})
            if update.active:
                active.update({6: PAYLOADS['button'](), 40: PAYLOADS['stack'](), 42: PAYLOADS['stack']()})
        elif kit is KITS[MEI] or kit is KITS[MOIRA]:
            first, second = (11, 12) if kit is KITS[MEI] else (7, 8)
            effect = _bits((int(update.active), 1), (1, 2))
            active.update({first: effect, second: effect})
        elif (kit is KITS[ZARYA] or kit is KITS[SYMMETRA]) and update.active:
            # STU_BD02E168 (empty DSR): the client runs its beam (client-only TrackRay -> STU_9932D728) under it
            active[67 if kit is KITS[ZARYA] else 94] = None
        elif kit is KITS[DVA] and update.serial:
            # the shot's cannon kicks: Volley 3's end runs ChaseVar 11 (var351, DFM 14) after a shot from the
            # first hardpoint, else ChaseVar 12 (var350, DFM 15); each restarts from 0 when listed again
            active[11 if update.serial % 2 else 12] = chase_payload(0.0)
            if update.active:
                active.update({state: how() for state, how in DVA_HELD.items()})
        if kit.reload is not None:  # as Soldier's rifle: var51 (reload requested) and the Anim while it refills
            path, anim = kit.reload
            extra[51] = Bool(update.reloading and update.reload_phase != 'recharge')
            if update.reloading:
                active.update({state: PAYLOADS[how]() for state, how in path.items()})
                if update.reload_phase is not None:
                    wait, anim = kit.phases[update.reload_phase]
                    active[wait] = None
                    if update.reload_phase == 'insert':
                        extra[8743] = Int(1)  # Native graph picks 0..2. Select category 0x119 for the insert animation.
                    active[anim] = anim_payload(update.ammo)
                else:
                    active[anim] = anim_payload(update.serial)
    if extra_active:
        active.update(extra_active)
    values = {28: Int(1), 53: Int(ammo), 198: Int(kit.capacity), 581: Float(reload_seconds), 229: Int(0), 230: Float(0),
              1769: Int(100), 1770: Int(100), 476: Float(VOLLEY_DAMAGE.get(kit.graph.index, 20)), 16922: Float(1.176),
              **AUTHORED.get(kit.graph.index, {})}
    if kit.scope is not None:
        switch, angle, angle_var = kit.scope
        scoped = scoped and not (update is not None and update.reloading)
        values[169], values[2137] = Float(float(scoped)), Bool(scoped)
        active[switch] = PAYLOADS[int(scoped)]()
        if scoped:
            stack, pose = SCOPE_POSE[kit.graph.index]
            active.update({stack: PAYLOADS['stack'](), pose: PAYLOADS[1]()})
        extra[172] = Int(angle)  # Native entry parameter for STU_75D1C308.
        if angle_var is not None:
            extra[angle_var] = Int(angle if scoped else 0)
    if kit is KITS[DVA] and update is not None and update.serial:
        values[58] = Asset(DVA_MUZZLES[update.serial % 2])
    if kit is KITS[DVA] or kit is DVA_PILOT:
        # var233 (firing) and var350 / var351, the time since a shot (100: long ago), feed the gun's animation through
        # DataFlowMappings; unsent, they stayed 0 and the gun moved as if just fired. The mech's cannon kicks are
        # ChaseVars 12 / 11 (begun with each shot), the pilot's release timer ChaseVar 10 (begun when a burst ends),
        # bound per frame as Soldier's Chase19 (heroes/dva.md 15, 17)
        firing, bits = update is not None and update.active, (kit.state_count - 1).bit_length()
        values[233] = Bool(firing)
        if kit is DVA_PILOT:
            values[350] = Float(100)
            if fire_elapsed is not None and not firing:
                active[10] = chase_since(fire_elapsed)
                values[350] = StateBound(Float(100), kit.instance, 10, bits, per_frame=True)
        else:
            values[350], values[351] = (StateBound(Float(100), kit.instance, chase, bits, per_frame=True)
                                        for chase in (12, 11))
    if 1648 in kit.bits:
        # the beam TrackRay's length; the Entry chain's Stack (Zarya 66, Symmetra 91) computes 0 for our player
        values[1648] = Float(BEAM_RANGE[kit.graph.index])
    presence = {bit: values[var] for var, bit in kit.bits.items() if var in values}
    if 49 in kit.bits and kit.model is not None:
        presence[kit.bits[49]] = model_entity(kit)
    return Instance(kit.instance, kit.graph, presence=presence, extra=extra, active=active)


def model_entity(kit):
    """var49, bound to the weapon model's state."""
    return StateBound(Entity(0), kit.instance, kit.model, (kit.state_count - 1).bit_length())


def notify_animation_categories(instance):
    # The common graph's ForCodeValue state tells the animator to reread its category variables.
    return replace(instance, active={**instance.active, 23: None}) if instance.graph.index == 75 else instance


def frame_builder(kit, reload_seconds=1.5):
    """A body frame builder with soldier.body_frame's signature for this kit."""
    def body_frame(cmfd, update=None, extra_active=None, weapon_entity=None, *, extra_instances=(),
                   extra_variables=None, serialize=None, scoped=False, scope_charge=1.0, weapon_values=None,
                   fire_elapsed=None):
        ammo = update.ammo if update is not None else kit.capacity
        variables = {31: Int(1), 34: Bool(False), 478: Bool(False), 7184: Int(ammo), 7185: Int(kit.capacity)}
        if 49 not in kit.bits and kit.model is not None:  # entity-scoped in this graph
            variables[49] = model_entity(kit)
        if extra_variables:
            variables.update(extra_variables)
        mercy = kit is KITS[MERCY] or kit is MERCY_BLASTER
        if mercy:
            variables[1226] = Asset(0x028000000000007D if kit is MERCY_BLASTER else 0x028000000000007F)
        if kit.scope is not None:
            scoped = scoped and not (update is not None and update.reloading)
            variables[1273] = Asset(0x0280000000000081 if scoped else 0)
        if mercy or kit.scope is not None:
            extra_instances = [notify_animation_categories(item) for item in extra_instances]
        weapon = weapon_instance(kit, ammo, update, extra_active, reload_seconds, scoped=scoped, fire_elapsed=fire_elapsed)
        for var, value in (weapon_values or {}).items():  # instance-scope vars that change with play (kit.bits)
            weapon.presence[kit.bits[var]] = value
        if kit is KITS[WIDOWMAKER]:
            # The weapon presenter attaches to graph 118's HUD_Widowmaker parent at state 6.
            variables.update({11821: ScriptRef(118, 6), 1032: Bool(scoped)})
            weapon.active[103] = None
            weapon.presence[7] = Float(scope_charge if scoped else 1.0)
        return (serialize or owner_full_frame)(cmfd,
            [weapon, *extra_instances], variables)
    return body_frame


def bastion_body(phase, repair, instances, variables, weapon_states):
    """Bastion's shared animation categories and authored Configuration state machine."""
    variables = {**variables, 1216: Int(phase)}
    active = {0: None, (1, 3, 2, 4)[phase]: None}
    stack = PAYLOADS['stack']()
    if phase:  # var2291's switch: no moving, jumping or crouching (state 5), as match.py roots him, transforms included
        active.update({5: None, 34: _bits((1, 1)), 35: stack})
    if phase == 0:
        variables.update({1226: Asset(0x028000000000007F), 1273: Asset(0),
                          2291: Bool(False), 2293: Bool(False), 15882: Bool(False)})
    elif phase == 1:
        active.update({7: anim_payload(), 8: None, 114: anim_payload()})
        variables.update({1273: Asset(0x028000000000008E), 2291: Bool(True)})
    elif phase == 2:
        active.update({18: stack, 20: stack, 21: stack, 129: stack, 130: None})
        variables.update({1214: Bool(True), 1226: Asset(0x028000000000007D), 1273: Asset(0x028000000000008E),
                          2291: Bool(True), 2293: Bool(True), 15882: Bool(True)})
        recon = Instance(1, KITS[BASTION].graph, active={13: _bits((0, 1))})
        instances = [*instances, recon]
        weapon_states = {**weapon_states, 17: stack}
    else:
        active.update({25: anim_payload(), 26: None})
        variables.update({2291: Bool(True), 2293: Bool(False), 15882: Bool(False)})
    repair_states = {5: _bits((int(repair == 2), 1), (1, 2))}  # Graph 339's sustained repair effect.
    if repair:
        path, anim = {1: ((11, 40), 23), 2: ((12, 41), 24), 3: ((7,), 25)}[repair]
        repair_states.update({**dict.fromkeys(path, stack), anim: anim_payload()})
    configured = []
    for item in instances:
        if phase in (0, 2):
            item = notify_animation_categories(item)
        if item.graph.index == 339:
            item = replace(item, active={**item.active, **repair_states},
                           extra={**item.extra, 636: Float(BASTION_REPAIR_BLEND), 1322: Float(BASTION_REPAIR_BLEND),
                                  1735: Int(repair)})
        configured.append(item)
    body = Instance(303, Graph(0x012F, 139, 15, 99), extra={670: Float(1), 636: Float(.5)}, active=active)
    return [*configured, body], variables, weapon_states


def initialize_practice_kit(player):
    """The hero's weapon, its abilities' HUD (ABILITIES) and soldier.initialize_practice_soldier's body hosts with
    the hero's HUD texture. shortcut: no ult meter yet but D.Va's (non-soldier-hero-scope.md)."""
    kit = DVA_PILOT if player.hero.body == PILOT_BODY.get(player.hero.guid) else KITS[player.hero.guid]
    hidden, stack = _bits((0, 1)), _bits((1, 2))
    player.weapon_extra_states = {}
    player.weapon_entity = None
    player.kit_frame = frame_builder(kit, PLAYER_KIT[player.hero.name][1].reload_seconds)
    player.alt = (None if not kit.alt
                  else ChargedAlt(player.quanta, player.rifle, *ALT_KIT[player.hero.name], windup=kit.alt_windup,
                                  most=kit.alt_most) if kit.alt_most
                  else AltFire(player.quanta, player.rifle, *ALT_KIT[player.hero.name], windup=kit.alt_windup))
    player.sprint = player.helix = player.field = None  # Soldier's, from a previous pick
    abilities = () if kit is DVA_PILOT else ABILITIES.get(player.hero.guid, ())
    player.cooldowns = [Choice(player.quanta, a.button, a.seconds, tuple(bit for bit, _ in a.choice)) if a.choice
                        else Crouched(player.quanta, *a.crouch[1:]) if a.crouch
                        else Aimed(player.quanta, a.button, a.seconds, *a.timed, ability=a) if a.steps
                        else Held(player.quanta, a.button, a.seconds, *a.meter[1:], delay=a.delay,
                                  refills=a.refills is not None) if a.meter
                        else Timed(player.quanta, a.button, a.seconds, *a.timed, ability=a) if a.timed
                        else Charges(player.quanta, a.button, a.seconds, a.charges) if a.charges
                        else Volley(player.quanta, a.button, a.seconds) for a in abilities]
    for ability, cooldown in zip(abilities, player.cooldowns):
        cooldown.spec = ability  # what Match plays on its press beside the cooldown (moves)
    player.melee = Volley(player.quanta, MELEE_BUTTON, MELEE_COOLDOWN)
    player.cooldowns.append(player.melee)  # after the abilities: zip() below pairs only those
    base = [
        Instance(21, Graph(21, 35, 1), active={11: hidden, 23: stack, 29: None, 32: None, 33: None}),
        Instance(75, Graph(75, 39, 4), presence={2: Int(1)}, active={19: None}),
        Instance(4429, Graph(4429, 1, 0), active={0: None}),
        Instance(463, Graph(463, 41, 22), presence={0: Asset(kit.hud_texture)}, active={39: None, 40: None}),
    ]
    if player.hero.guid == WIDOWMAKER:
        base.append(Instance(118, Graph(118, 2, 0, 1), active={1: None}))
    tail = []  # after the abilities: SubScript children follow their parent
    if kit is KITS[DVA]:
        # Defense Matrix's meter: Matrix's Entry SubScript 46 starts graph 214E, whose client-only presenter waits for
        # message 0x104E (posted by Matrix's state 47, owner 34, always sent) and parents to var11821 = the mech HUD
        # graph 0276's client-only presenter 55 (heroes/dva.md).
        base.append(Instance(0x0276, DVA_HUD))
        tail.append(Instance(0x214E, Graph(0x214E, 1, 0, 1), parent=(0x02CC, 46)))
    # D.Va's ultimate graph, swapped by Match for Self-Destruct, Call Mech and the eject animation
    player.ult_instance = DVA_ULTS[player.hero.body][0] if player.hero.body in DVA_ULTS else None
    if player.hero.body == PILOT_BODY[DVA] and player.match.now < player.invulnerable_until:
        player.ult_instance = dva_eject()  # she has just bailed out
    # pulse: (the pressed ability's cooldown, its volley update) while a cast goes out
    player.refresh_abilities = lambda pulse=None: setattr(player, 'body_extra_instances',
        [*base, *([player.ult_instance] if player.ult_instance else []),
         melee_instance(player.melee, pulse[1] if pulse and pulse[0] is player.melee else None),
         *(ability_instance(a, c, pulse[1] if pulse and pulse[0] is c else None)
           for a, c in zip(abilities, player.cooldowns)), *tail])
    player.refresh_abilities()
    player.body_extra_variables = {982: Int(1), 3772: Bool(True), **health_variables(player.health, player.max_health)}
    if kit is KITS[DVA]:  # kept in every frame, as Widowmaker's ScriptRef(118, 6)
        # 11815: Boosters' ring parent = the player's HUD 20E4 (instance 4) presenter 10, as natively (0xA00040004)
        # shortcut: instance 4 is the practice controller's HUD (heroselect.controller_frame), recheck for other modes
        player.body_extra_variables.update({11821: ScriptRef(0x0276, 55), 11815: ScriptRef(4, 10)})
    if player.hero.body in DVA_ULTS:
        player.body_extra_variables.update({ULT_COST_VAR: Int(ULT_COST),
                                            ULT_CHARGE: ult_meter(player.ult_charge, DVA_ULTS[player.hero.body][1])})
