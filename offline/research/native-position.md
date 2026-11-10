# Retail native body-position trace

Build 1.74.0.0.104319; analysis image base `0x7FF6D1220000`, SHA-256 `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`.

## Current evidence

- Parent's cached graph-state trace reports `CosmeticState + 0x78 → ScriptInstance`, then `ScriptInstance + 0x10 →` an entity-local statescript context with vtable VA `0x7FF6D384C010` (RVA `0x262C010`). I could not independently confirm that live pointer chain: the single hash-guarded, read-only follow-up read at `state + 0x78` did not yield a canonical pointer for the next read. No further process reads were attempted.
- The static constructor at RVA `0x977BD0` installs vtable RVA `0x262C010`, initializes state storage at `this + 0x5C0`, and accesses other context fields near `+0x5A8` and `+0x5B0`. Its deleting-destructor path uses allocation size `0xEF8` (RVA `0x977D20`). This is a large per-component context, not a global world object.
- Context methods at RVAs `0x97A170`, `0x97A1E0`, `0x97A310`, `0x97A4B0`, `0x97A510`, `0x97A580`, `0x97A5D0`, `0x97A7B0`, and `0x97A980` pass `this + 0x5C0` to state/network helpers. This corroborates the per-instance netstate role of that field; it is not a world-position field.
- Separate field `this + 0x528` is read by context methods `0x97A020`, `0x978580`, `0x97A8B0`, `0x97AC20`, `0xD0E7C0`, and `0xD0E890`. `0x97A020` calls the pointed object's virtual `+0x68` and returns its boolean result. `0x97AC20` conditionally sets byte `[object + 0xB8]`. These uses show an associated object, but do not identify it as the owning entity or transform.
- No reviewed context vtable method exposed a position vector or eye-height field. The names `ConfigVarEntityPosition`, `OwEntity`, and transform were not present in the checked local source/report files. There is no evidence here for a coordinate scale or an authoritative native position getter.

## Handoff

Treat `context + 0x528` as an untyped associated-object candidate, not an `OwEntity*`. The verified body movement/collision screenshots prove client-side movement and one wall collision; they do not give an authoritative position field. Server mover coordinates should not be reconciled with the client camera until a transform getter and coordinate convention are identified.

Static method addresses above come from a read-only section snapshot of the hash-verified binary; no IDB edits or process writes were made. The attempted process read reused the hash-checked module guard and 8-byte-capped read helper in [debug_assert.py](../tools/research/debug_assert.py#L222) and [debug_assert.py](../tools/research/debug_assert.py#L271). The follow-on pointer failed its address-range check, so the live chain remains unresolved.
