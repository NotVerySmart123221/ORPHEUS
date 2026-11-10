# Spawn voice line and Biotic Field ring: findings (2026-11-05, open)

Correction (2026-11-07): native lookup searches `m_voiceLineGuids`, not `m_FCF3C39E`; 27501 also has a different tail layout from 27500. See [current primary-source findings](presentation-next-steps-2026-11-07.md) before following the historical experiment proposal below. The ordinary spawn stimulus and remaining sender arguments are still unproven.

## Spawn voice line (every hero): open

- No body graph the server sends plays a hero voice line. Soldier's graphs (21, 51, 67, 75, 593, 596, 597, 599, 601, 2230, 2331) have no voice-stimulus state (`STU_40E05F6B`).
- Graph 75 is common to every hero. Its nested state 1 (`STU_37D754C8`) holds graph 458 (`0x01CA`), the hero voice director. Every node in graph 458 is server-only. Its stimulus states fire on game messages:
  - `0x0EE...0D` on game message `0x0240...01A4`, which the mode graphs 3778 and 3914 send
  - `0x0EE...19` on `0x0240...00BB`, which the ultimate graphs send (graph 601 is one)
  - `0x0EE...39`, `32` and `08` on `0x0240...0093` and four others
- The client plays hero voice lines from protocol ClientInVoice (`0x82A2F991`, first id 27500), messages 27500 and 27501 (IDA, `Overwatch-state-analysis.dump.i64`):
  - The dispatcher is `0x7FF6D19F2400`. It queues each message as a record (`0x7FF6D1D32CD0` / `0x7FF6D1D32D60`) at voice system `+0x138`.
  - The record processor is `0x7FF6D1D34C80`, called from `0x7FF6D1D35E70`. It loads the asset GUID at message `+0x88` (`0x7FF6D295BA30`; it returns early if that asset is missing) and resolves the entity at `+0x78`, the speaker.
  - It then looks up message `+0x90` in a u64 key table of the speaker's voice data (`0x7FF6D1E067F0` → `0x7FF6D1FD5380`; 64-byte entries) and plays through `0x7FF6D1D3AC60`.
  - Message fields: `+0x78`, `+0x7C` and `+0x80` are entity ids; `+0x88` and `+0x90` are u64; `+0x98` is an int; `+0xA0` is a u64. Message 27501 adds `+0xA8` u64, `+0xB0` and `+0xB4` floats, `+0xB8` bool and `+0xB9` u8.
- The lobby never sends 27500 or 27501. That is why no line plays [inference from the trace above; not tested in game].
- Soldier's voice set is `0x07A0000000000056` (body `0x03CF`, field `m_voiceDefinition`). It is not in the root CMF; it is in each `*_speech_EExt.cmf`. The enUS copy (431,776 bytes) has 612 `STUVoiceLine`s over 240 stimuli; `0x0EE...0D` has 32 lines and `0x0EE...08` has 14. `m_FCF3C39E` holds 45 `0x0F30...` u64s, which may be the key table the client searches [unverified].
- Not established:
  - which stimulus is the spawn line (`0x0D`, on the mode graphs' message, is the leading candidate [unverified])
  - what `+0x88` holds: a stimulus or a voice-line asset
  - the roles of `+0x7C`, `+0x80`, `+0x98` and `+0xA0`
- Next step: read the key table at runtime from the speaker's voice data object (`0x7FF6D1E067F0` on the body entity), match it to `m_FCF3C39E`, then send one 27500 from `Match._spawn_steps` with speaker = body, `+0x88` = stimulus `0x0EE...0D`, `+0x90` = one of its keys, and check the audio.

## Biotic Field ring: open (3 runs)

- Volley 4's impact entity `0x03DC` has no model. Its graph 600 (`0x0258`, not in the pinned catalog; raw asset via `tools/assets/stu_graph.py`) holds the canister (CosmeticEntity, owner state 3) and the ring effects (states 2 and 9).
- The CosmeticEntity's `m_1DEF59FA` is var2712, its output (like var49 on the weapon models), and effect state 9 is gated on it.
- Runs 2 and 3 (reverted experiment): the server created `0x03DC` with an H0 graph-600 frame. That frame put effects on remote 0 and 4, the canister on remote 1, and bound var2712 to state 3 in presence bit 0 (the graph has one).
  - The client built the instance: states present 0, 1, 3–10; active (3, 5, 9). Effect state 2, although sent, was not present.
  - Nothing drew, looking straight down at the field: no canister and no ring ([states](field-ring-graph600-states.txt), [capture](field-ring-impact-graph600-none.png)).
- So the frame is applied, but the active CosmeticEntity and effect have nothing to render. Leading hypothesis [unverified]: the entity created by the world-create path lacks what the canister and ring attach to, either the owner's spawn record or a transform/parent the native Volley impact supplies.
- Next step: on a live 60 s field, read CosmeticEntity state 3's output (`+0x1E0`) and effect state 9's resolved effect and hardpoint. A null output points at the entity creation, not the frame.
