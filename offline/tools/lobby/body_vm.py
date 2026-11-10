"""Hero bodies whose statescript LizRollin's VM runs (vendor/ow174/game/script/driver.py), in place of kits.py's
hand-written frames (research/lizrollin-runtime.md section 15).

The VM runs the body's own graphs from the player's commands, as the client predicts them, and sends the owner frames
that keep that prediction (full frame, then one owner delta per command frame). Our server keeps the gameplay: each
shot the VM's Volleys fire goes to our damage code (SHOTS), quick melee's ModifyHealth to MELEE, the ultimate's start
to Tactical Visor's aim, and the movement mods the VM has on (Sprint) to the player's mover. Health and ultimate charge
go the other way, from the match into the VM's variables.

Opt-in: OW_BODY_VM=1 runs it for HEROES (Soldier: 76, the one hero LizRollin verified live), OW_BODY_VM=all for every
hero: their Volleys reach the damage code by the button that fires each (VOLLEYS), and what has no Volley (beams,
melee weapons, most ultimates, the kits' hero-specific code) is not played. Off, or when the VM fails, the body gets
kits.py's frames as before.
"""
import logging
import os

from ow174.game.content import SOLDIER
from ow174.game.mover import statescript_mods
from ow174.game.script import expr, nodes, runtime
from ow174.game.script.driver import BodyScript

from soldier import VISOR_CAST, VISOR_SECONDS
from target import ABILITY_HITS, ALT_KIT, MELEE, AbilityHit

log = logging.getLogger('ow174.game')

HEROES = frozenset({SOLDIER})
ULT_COST = 338  # the ultimate's cost, from the Entries (driver.ULT_COST); #342 is the charge
FIRE, SECONDARY = 0x01, 0x02  # command buttons
SHOTS = {(0x0254, 16): 'helix', (0x0257, 1): 'field'}  # Soldier: 76's Helix Rockets and Biotic Field's throw
# Each hero's Volleys (graph, owner state) and the button that fires each: tools/research/vm_heroes.py --volleys.
VOLLEYS = {
    (0x0b94, 5): 0x1, (0x0b97, 0): 0x10, (0x0bdd, 6): 0x8,  # Ana
    (0x14fc, 122): 0x1, (0x14fd, 12): 0x10, (0x1513, 16): 0x8,  # Ashe
    (0x1761, 12): 0x10, (0x1932, 6): 0x1, (0x1932, 26): 0x2,  # Baptiste
    (0x012e, 14): 0x1,  # Bastion
    (0x01d2, 11): 0x1, (0x01d2, 15): 0x2, (0x01e9, 1): 0x10,  # Cassidy
    (0x0d0a, 2): 0x1,  # Doomfist
    (0x19e5, 3): 0x1, (0x1c36, 0): 0x2,  # Echo
    (0x0304, 4): 0x1, (0x0304, 11): 0x2,  # Genji
    (0x022b, 12): 0x1, (0x096a, 8): 0x8, (0x09ba, 1): 0x10,  # Junkrat
    (0x0776, 14): 0x1,  # Lúcio
    (0x05e6, 13): 0x1, (0x05e6, 16): 0x2,  # Mei
    (0x0f76, 6): 0x1,  # Moira
    (0x0bd4, 24): 0x1, (0x0bd7, 0): 0x10, (0x0ccb, 4): 0x2,  # Orisa
    (0x0023, 6): 0x1, (0x002b, 1): 0x20, (0x007b, 0): 0x10,  # Pharah
    (0x0019, 6): 0x1,  # Reaper
    (0x01c6, 9): 0x1, (0x01c6, 12): 0x2, (0x01c6, 14): 0x20,  # Roadhog
    (0x187f, 20): 0x1, (0x187f, 28): 0x1,  # Sigma
    (0x0254, 10): 0x1, (0x0254, 16): 0x2, (0x0257, 1): 0x10,  # Soldier: 76
    (0x0b1b, 6): 0x1, (0x0c19, 3): 0x10,  # Sombra
    (0x157e, 12): 0x8,  # Symmetra
    (0x1683, 10): 0x1, (0x1683, 13): 0x2, (0x16c5, 2): 0x8,  # Torbjörn
    (0x0016, 8): 0x1,  # Tracer
    (0x001b, 6): 0x1,  # Widowmaker
    (0x1514, 9): 0x1, (0x1517, 3): 0x20, (0x1517, 8): 0x20, (0x1517, 18): 0x20,  # Wrecking Ball
    (0x0241, 19): 0x2, (0x025f, 0): 0x20,  # Zarya
    (0x0164, 14): 0x1, (0x0164, 18): 0x2,  # Zenyatta
}


def effect(hero_name: str, key: tuple):
    """What the server plays for a shot of this Volley: 'helix' or 'field' (Soldier: 76's own code), 'primary'
    (the hero's weapon), the target.AbilityHit its button casts (flown by Match.fire_ability), the alt-fire Weapon,
    or None when the server has nothing for it."""
    if key in SHOTS:
        return SHOTS[key]
    button = VOLLEYS.get(key)
    if button == FIRE:
        return 'primary'
    hit = ABILITY_HITS.get((hero_name, button))
    if hit is None and button == SECONDARY and hero_name in ALT_KIT:
        return ALT_KIT[hero_name][0]
    return hit
MELEE_STATES = {0x0043: 4}  # quick melee's ModifyHealth state (their combat.MELEE)
DIED = 0x024000000000003E  # game message 003E.025, the body's "died" (their combat.DIED)
KILLER, CRIT, LOADOUT, ASSISTS = 0x0012, 0x0095, 0x03E6, 0x03E7  # its parameters, by the low 16 bits of their ids


def wanted(hero_guid: int) -> bool:
    mode = os.environ.get('OW_BODY_VM', '').strip().lower()
    return mode == 'all' or mode not in ('', '0') and hero_guid in HEROES


class BodyVM:
    """One body's VM: made with the body's create, which it waits for before its full frame."""

    def __init__(self, match, player, create):
        last = player.commands.last_frame  # before any command, the client's frame clock (server.Client._frame)
        start = last if last is not None else player.frame_at(match.now)
        self.script = BodyScript(player.hero.guid, player.body, start, quanta=player.quanta,
                                 mode=match.game_map.mode_guid, health=player.health, ult_seconds=0)
        self.script.created(create)
        self.melee_seen = {}
        self.ult_running = False
        self.ult_sent = None
        self.sync(player)

    def sync(self, player) -> None:
        """Health and ultimate charge into the VM (the HUD's numbers and the graphs' checks)."""
        self.script.set_health(player.health, player.max_health)
        if player.ult_charge != self.ult_sent:
            self.ult_sent = player.ult_charge
            cost = expr.to_float(self.script.component.entity_var(ULT_COST).value())
            self.script.set_ult_charge(cost * min(1.0, player.ult_charge / player.ult_most) if cost > 0 else 0.0)

    def command(self, match, player, command) -> None:
        """Run one command frame, then play what it did."""
        script = self.script
        script.command(command.frame, command.buttons, command.action, command.forward, command.right)
        player.mover.script_mods = statescript_mods(script.component)  # they act from the next frame (mover.py)
        shots, script.component.shots[:] = list(script.component.shots), []
        if not match.gameplay_enabled or player.health <= 0:
            return
        primary = False
        for state, _, _ in shots:
            key = (state.instance.graph.index, state.index)
            played = effect(player.hero.name, key)
            if played == 'helix':
                match.fire_helix(player, command)
            elif played == 'field':
                match.throw_field(player)
            elif played == 'primary':
                # shortcut: once a command frame. Sigma's two spheres are two Volleys, and his weapon counts both as
                # pellets; if they fire in different frames this doubles them (not checked live).
                if not primary:
                    match.fire_at_target(player, command)
                primary = True
            elif isinstance(played, AbilityHit):
                match.fire_ability(player, command, played)
            elif played is not None:
                match.fire_at_target(player, command, played)
            else:
                expr.warn_once(f'vm-shot{key}', 'the VM fired Volley %#06x state %d, which the server does not play', *key)
        for instance in script.component.instances.values():
            index = MELEE_STATES.get(instance.graph.index)
            state = instance.states.get(index) if index is not None else None
            if state is None:
                continue
            key = (instance.id, index)
            seen, self.melee_seen[key] = self.melee_seen.get(key, 0), state.counter
            if state.counter != seen and state.active:
                match.fire_at_target(player, command, MELEE)
        running = any(state.flags & nodes.Ability.RUNNING for state in script.ultimates)
        if running and not self.ult_running:
            log.info('[game] %s ultimate (VM) frame=%d', player.hero.name, command.frame)
            player.ult_charge = self.ult_sent = 0  # the graph zeroed #342 itself
            if player.hero.guid == SOLDIER:  # Tactical Visor: the server aims the shots (Match.fire_at_target)
                player.visor_cast_until = match.now + VISOR_CAST
                player.visor_until = player.visor_cast_until + VISOR_SECONDS
        self.ult_running = running

    def died(self, killer: int) -> None:
        """The kill feed line: game message 003E.025 to the dead body's script, whose 01CF pulses st108; every
        client's 01CF then hands the line to the mode entity's feed 0DEA under 0CB6 (LizRollin's combat._died).
        shortcut: CRIT always false and no loadout or assists; pass the killing hit's when the match tracks them."""
        runtime.send_message(self.script.component, DIED,
                             {KILLER: expr.Entity(killer), CRIT: False, LOADOUT: None, ASSISTS: ()}, killer)

    def frames(self) -> list:
        return self.script.frames()

    def retire(self) -> None:
        self.script.retire()
