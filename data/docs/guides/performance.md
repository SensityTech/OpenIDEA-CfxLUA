# Performance

Client scripts share the game's frame time: every millisecond a resource spends per frame is a millisecond the game cannot use for rendering. Server scripts share one main thread with every other resource. Most performance problems in FiveM and RedM resources come from a handful of patterns, listed here with their fixes.

## Measure first

- **Client:** open the console (F8) and run `resmon 1` to show the resource monitor: CPU time per frame and memory per resource. Some servers or update channels restrict this command.
- **Profiler:** `profiler record 500` records about 500 frames, `profiler view` opens the result in a Chrome-based viewer. It works on the client and on the server (`profiler saveJSON file.json` on the server).
- **Timing code by hand:**

```lua
local function timed(label, fn, ...)
    local start = GetGameTimer()
    local results = table.pack(fn(...))
    print(('%s took %d ms'):format(label, GetGameTimer() - start))
    return table.unpack(results, 1, results.n)
end
```

`GetGameTimer` has millisecond resolution; for very short operations, time a loop of many calls.

## 1. Loops that run every frame for nothing

```lua
-- BAD: checks a distance 60+ times per second, forever
CreateThread(function()
    while true do
        Wait(0)
        local pos = GetEntityCoords(PlayerPedId())
        if #(pos - vector3(441.8, -982.0, 30.7)) < 2.0 then
            -- show a prompt
        end
    end
end)
```

```lua
-- GOOD: sleep while far away, run per frame only when close
CreateThread(function()
    local target = vector3(441.8, -982.0, 30.7)
    while true do
        local sleep = 1000
        local distance = #(GetEntityCoords(PlayerPedId()) - target)
        if distance < 10.0 then
            sleep = 0
            if distance < 2.0 then
                -- show a prompt, check a key
            end
        end
        Wait(sleep)
    end
end)
```

Rules of thumb:

- `Wait(0)` only while something must be drawn or checked every frame.
- One loop handling many points beats many loops handling one point each.
- For many points of interest, group them by area and only check the nearby group.

## 2. Polling instead of reacting

Prefer events and state bag change handlers over loops that compare values:

```lua
-- instead of checking a player's job every second, react when it changes
AddStateBagChangeHandler('job', ('player:%d'):format(GetPlayerServerId(PlayerId())), function(_, _, value)
    print(('my job is now %s'):format(tostring(value)))
end)
```

Use key mappings (`RegisterKeyMapping`, FiveM) instead of `IsControlJustPressed` loops for actions bound to keys.

## 3. Repeated native calls

Each native call crosses from Lua into the game. Cache values inside an iteration:

```lua
-- BAD
if IsPedInAnyVehicle(PlayerPedId(), false) then
    local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
    print(GetEntitySpeed(GetVehiclePedIsIn(PlayerPedId(), false)))
end

-- GOOD
local ped = PlayerPedId()
local vehicle = GetVehiclePedIsIn(ped, false)
if vehicle ~= 0 then
    print(GetEntitySpeed(vehicle))
end
```

Do not cache handles **across** long periods without checking them: the player's ped changes on respawn or model change, and entities can be deleted. Re-read them each iteration, or check `DoesEntityExist`.

## 4. Distances and math

- `#(a - b)` is the fastest distance between two vectors. Compare squared values only if you write the math yourself; `#` is already cheap.
- For a horizontal distance, use `#(a.xy - b.xy)`.
- Use backtick hashes (`` `adder` ``) instead of `GetHashKey('adder')` or string hash parameters in loops: they are computed at compile time.

## 5. Garbage

Creating tables, closures and strings in a per-frame loop produces garbage the collector must clean, which shows up as periodic spikes.

```lua
-- BAD: a new table and a new closure every frame
CreateThread(function()
    while true do
        local options = { scale = 0.35, color = { 255, 255, 255 } }
        local draw = function() end
        draw(options)
        Wait(0)
    end
end)

-- GOOD: build once, reuse
local OPTIONS <const> = { scale = 0.35, color = { 255, 255, 255 } }
local function draw(options) end

CreateThread(function()
    while true do
        draw(OPTIONS)
        Wait(0)
    end
end)
```

- Build strings with `string.format` or `table.concat` instead of `..` in loops.
- Reuse tables with `table.wipe(t)` when a fresh table is needed each tick (see `cfxdoc:cfxlua-std-extensions`).

## 6. Locals versus globals

Globals are looked up in a table on every access; locals live in registers. In hot code:

```lua
local GetEntityCoords = GetEntityCoords
local PlayerPedId = PlayerPedId
local vector3 = vector3
```

Also keep your own functions and module state `local`. Besides being faster, it avoids clashes: all files of a resource on the same side share one global environment.

## 7. Network traffic

- Do not trigger network events every frame. Send changes, not states, and throttle continuous data (positions) to a few updates per second at most.
- Use latent events (`TriggerLatentClientEvent`, `TriggerLatentServerEvent`) for payloads of several kilobytes.
- Send ids, not whole objects. Never send entity handles (send network IDs).
- Keep state bag values small and avoid rewriting large tables often: every write is sent to every client that sees the bag.
- `TriggerClientEvent(name, -1, ...)` sends to everybody. Target the players who need it.

## 8. Server-side

- Long synchronous work (big loops over all players, heavy JSON encoding) blocks every resource on the server. Split it with `Wait(0)` between chunks.
- Use asynchronous APIs for I/O: `PerformHttpRequest`, and your database library's async functions.
- Cache data that does not change (configuration, item definitions) at resource start instead of reloading it on each request.

```lua
local function forEachChunked(list, chunkSize, fn)
    for i, item in ipairs(list) do
        fn(item)
        if i % chunkSize == 0 then
            Wait(0) -- let other resources run
        end
    end
end
```

## 9. Exports and cross-resource calls

Export calls serialize their arguments and results. Avoid calling another resource's export every frame; cache the result and refresh it on an event or a timer.

## Checklist

- [ ] No `while true` loop without `Wait`.
- [ ] `Wait(0)` only while needed; otherwise 250 ms to several seconds.
- [ ] Native results cached within an iteration.
- [ ] No tables or closures created per frame.
- [ ] Network events throttled, large payloads latent.
- [ ] Hot functions and state are `local`.
- [ ] Measured with `resmon` / the profiler before and after.

## Sources

- Using the profiler: https://docs.fivem.net/docs/scripting-manual/debugging/using-profiler/
- Client console commands: https://docs.fivem.net/docs/client-manual/console-commands/
- Triggering events (latent events): https://docs.fivem.net/docs/scripting-manual/working-with-events/triggering-events/
- Lua performance tips (Roberto Ierusalimschy, Lua Programming Gems): https://www.lua.org/gems/sample.pdf
