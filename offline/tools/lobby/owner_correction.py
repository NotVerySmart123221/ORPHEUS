"""Corrections of the owner's own body: the server's mover state after command frame F, sent to the player's own
client as a ch3 record stamped F. From LizRollin's ow174/game/correction.py @ 9f8f3bc (MIT, see
vendor/README.md), which has the client addresses; here in our image (theirs minus 0xB7CF0000).

- The client compares the record with the state it predicted for F field by field (0x7FF6D1E83E00, mode 2): flags,
  frames, throttles, gravity, spring, angles, position, velocity and +832. Equal: nothing happens. Different: it
  puts its body in the server's state and runs the later commands again (research/lizrollin-runtime.md section 2).
- So the server can move the player (a teleport or a push on `player.mover`) and the client follows, but any
  movement the server's mover does not follow is pulled back by every record.
- Every hero's kits body gets records but those in OFF: on a kits body the client runs no ability's movement (the
  ability graphs it gets are HUD cooldowns), and each hero's recorded ability course replays onto the client's feet
  (test_player_mover; research/lizrollin-runtime.md section 21). A body the VM runs (body_vm) gets the graphs'
  own states, movement included, so only body_vm.HEROES get records there.
- One record per packet frame: the newest state of a command frame not after the packet frame that was not sent yet.
  While records go, the command ack stays at most ACK_LEAD past the newest one, so the client still has the commands
  a correction runs again (it keeps them from ack - 1 on).
- Off while OW_CLIENT_SYNC moves the server's feet to the client's: the two would fight.
"""
import os

import body_vm
import client_sync
from kits import DVA
from ow174.game.world import NO_INPUT, EntityUpdate, Movement

ENABLED = os.environ.get('OW_CORRECTIONS', '1') != '0'
OFF = {DVA}  # Boosters run on her client and ours are fitted: up to 0.85 m off (section 8)
ACK_LEAD = 2


def movement(snapshot) -> Movement:
    """The record's state: the mover's snapshot (mover.Snapshot) at its own frame."""
    if snapshot.input_frame is None:
        input_frame = NO_INPUT
    elif snapshot.input_frame == snapshot.frame:
        input_frame = None  # the state's own frame: +34 and +35 are then +32 and +33
    else:
        input_frame = snapshot.input_frame
    return Movement(snapshot.position, snapshot.yaw, snapshot.pitch, snapshot.velocity, snapshot.flags,
                    throttles=snapshot.throttles, input_frame=input_frame, input_throttles=snapshot.input_throttles,
                    air_ticks=snapshot.air_ticks, gravity=snapshot.gravity, frame=snapshot.frame,
                    crouch_frame=snapshot.crouch_end, spring=snapshot.spring, spring_speed=snapshot.spring_speed,
                    fall=snapshot.fall)


def correcting(player) -> bool:
    """Whether the player's own body gets records: a living hero not in OFF (on a VM body: in body_vm.HEROES) whose
    body's create the client has (a dead client sets the dead flag on its own state, which the records do not
    carry)."""
    create = player.body_create
    return (ENABLED and not client_sync.active() and player.has_body and player.health > 0
            and player.hero.guid not in OFF and (player.body_vm is None or player.hero.guid in body_vm.HEROES)
            and create is not None and create.entity == player.body and create.delivered)


def own_record(player, frame: int) -> EntityUpdate | None:
    """Packet frame `frame`'s record of the player's own body, if it gets one."""
    if not correcting(player):
        return None
    mover = player.mover
    for snapshot in reversed(mover.history):
        if snapshot.frame <= frame:
            if mover.corrected is not None and snapshot.frame <= mover.corrected:
                return None
            mover.corrected = snapshot.frame
            return EntityUpdate(player.body, movement=movement(snapshot))
    return None


def command_ack(player, frame: int) -> int:
    """Packet frame `frame`'s command ack: the newest command frame the server has (0 before any), held to
    ACK_LEAD past the newest record while the owner gets them."""
    newest = player.commands.last_frame or 0
    sent = player.mover.corrected
    if sent is None or not correcting(player):
        return newest
    return min(newest, sent + ACK_LEAD)
