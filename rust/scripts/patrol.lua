-- Example ORPHEUS Lua debug override on the bounded AI think() ABI (v2).
-- Route slots are zero-based indexes into the map's authored hint sequence.
return {
    abi = 2,
    think = function(snapshot, events)
        local patrol = snapshot.actor.patrol
        local commands = {}
        for i = 1, #events do
            local event = events[i]
            if event.kind == "spawn" then
                commands[#commands + 1] = { kind = "wait", seconds = 3.0 }
            elseif event.kind == "think" then
                commands[#commands + 1] = {
                    kind = "patrol_slot",
                    slot = (patrol.route_index + 1) % patrol.route_count,
                }
            elseif event.kind == "release" then
                commands[#commands + 1] = { kind = "wait", seconds = 1.2 }
            end
        end
        return commands
    end,
}
