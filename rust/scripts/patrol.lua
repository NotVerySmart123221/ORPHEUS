-- ORPHEUS patrol policy, translated from the existing 104319 route reconstruction.
-- Route slots are zero-based indexes into the map's authored hint sequence.
return 1, function(event, _frame, index, count)
    if event == "spawn" then
        return 3.0
    elseif event == "think" then
        return (index + 1) % count
    elseif event == "release" then
        return 1.2
    end
end
