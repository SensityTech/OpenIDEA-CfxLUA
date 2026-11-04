# Threads, waits and timing

Cfx.re scripts are single-threaded. What the API calls "threads" are Lua coroutines run by a cooperative **scheduler**: each one runs until it calls `Wait`, then the scheduler moves on to the next one. Nothing runs in parallel, and a thread that never waits freezes the whole resource (and on the client, the game).

## The scheduler API

| Function | Alias | Purpose |
|---|---|---|
| `Citizen.CreateThread(fn)` | `CreateThread(fn)` | Start `fn` in a new coroutine on the next scheduler tick. `fn` receives no arguments. |
| `Citizen.CreateThreadNow(fn [, name])` | none | Start `fn` immediately; it runs until its first `Wait`, then continues like any other thread. |
| `Citizen.Wait(ms)` | `Wait(ms)` | Suspend the current thread for at least `ms` milliseconds. `Wait()` is `Wait(0)`. |
| `Citizen.SetTimeout(ms, fn)` | `SetTimeout(ms, fn)` | Run `fn` once, in a new thread, after `ms` milliseconds. Returns an id. |
| `Citizen.ClearTimeout(id)` | `ClearTimeout(id)` | Cancel a timeout that has not fired yet. |
| `Citizen.Await(p)` | none | Suspend the current thread until promise `p` settles; return its value or raise its error. |

The global aliases `Wait`, `CreateThread`, `SetTimeout` and `ClearTimeout` are exactly the `Citizen` functions. There is no global `Await`.

```lua
CreateThread(function()
    print('started on the next tick')
    Wait(1000)
    print('one second later')
end)

local timeoutId = SetTimeout(5000, function()
    print('five seconds later')
end)
ClearTimeout(timeoutId) -- changed my mind
```

## Where can I call `Wait`?

`Wait` yields the current coroutine back to the scheduler, so it only works in code the scheduler is running:

- inside `CreateThread`, `Citizen.CreateThreadNow` and `SetTimeout` callbacks;
- inside event handlers (`AddEventHandler`, `RegisterNetEvent`): each handler call runs in its own coroutine;
- inside command handlers, NUI callbacks and export functions called from another resource.

It does **not** work at the top level of a script file (the file body is not a coroutine), and it must not be called from inside a coroutine you created yourself with `coroutine.create` / `coroutine.wrap`: the yield would go to your coroutine instead of the scheduler. Use `CreateThread` instead of raw coroutines.

An event handler runs synchronously until its first `Wait`. The code that triggered the event (for example `TriggerEvent`) continues only after that point:

```lua
AddEventHandler('demo:ping', function()
    print('A: handler starts')
    Wait(0)
    print('C: handler resumes on a later tick')
end)

CreateThread(function()
    TriggerEvent('demo:ping')
    print('B: TriggerEvent returned')
end)
```

## `Wait(0)` versus `Wait(n)`

- `Wait(0)` resumes on the next tick. On the client that means **every frame**. Use it only for code that must run each frame: drawing (markers, text, `DrawRect`), disabling controls, reading `IsControlJustPressed`.
- `Wait(n)` with `n > 0` sleeps at least `n` ms. Use it for everything else: checking distances, polling state, periodic updates.

A loop that runs every frame for no reason is the most common cause of a high resource time in the client resource monitor (`resmon`).

### The adaptive loop

Sleep long when nothing is nearby, and switch to per-frame only when needed:

```lua
local SHOP <const> = vector3(25.7, -1347.3, 29.5)

CreateThread(function()
    while true do
        local sleep = 1000
        local pos = GetEntityCoords(PlayerPedId())
        local distance = #(pos - SHOP)

        if distance < 20.0 then
            sleep = 0
            DrawMarker(1, SHOP.x, SHOP.y, SHOP.z - 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                1.0, 1.0, 0.5, 0, 150, 255, 120, false, false, 2, false, nil, nil, false)
            if distance < 1.5 and IsControlJustPressed(0, 38) then -- 38 = E
                TriggerEvent('shop:open')
            end
        end

        Wait(sleep)
    end
end)
```

### Measuring time

`GetGameTimer()` returns the number of milliseconds since the game (or server) started. Use it for cooldowns instead of counting loop iterations:

```lua
local COOLDOWN_MS <const> = 3000
local lastUse = 0

local function canUse()
    local now = GetGameTimer()
    if now - lastUse < COOLDOWN_MS then
        return false
    end
    lastUse = now
    return true
end
```

`Wait(n)` guarantees *at least* `n` ms, never exactly `n` ms. Do not accumulate `n` to measure elapsed time.

## Stopping a loop

Threads have no handle you can kill. Use a flag the loop checks:

```lua
local running = false

local function startTracking()
    if running then return end
    running = true
    CreateThread(function()
        while running do
            -- work
            Wait(250)
        end
    end)
end

local function stopTracking()
    running = false
end
```

All threads of a resource stop automatically when the resource stops. Clean up what they created (entities, blips, NUI focus) in an `onResourceStop` handler:

```lua
AddEventHandler('onResourceStop', function(resourceName)
    if resourceName ~= GetCurrentResourceName() then return end
    stopTracking()
end)
```

## Errors in threads

An error inside a thread, a timeout or an event handler prints a script error with a stack trace and ends **that** coroutine only. Other threads keep running. A `while true` loop that raises once is gone for good, so protect long-lived loops whose body can fail:

```lua
CreateThread(function()
    while true do
        local ok, err = pcall(function()
            -- body that may fail
        end)
        if not ok then
            print(('tick failed: %s'):format(err))
        end
        Wait(1000)
    end
end)
```

## Promises and `Citizen.Await`

The global `promise` library (lua-promises) represents a value that will be available later. `Citizen.Await(p)` suspends the current thread until `p` is resolved or rejected.

| API | Description |
|---|---|
| `promise.new()` | Create a pending promise. |
| `promise.new(function(p) ... end)` | Create a promise and run the function with the promise itself as argument. If the function raises, the promise is rejected. |
| `p:resolve(value)` | Fulfil the promise. Ignored if it is already settled. |
| `p:reject(err)` | Reject the promise. Ignored if it is already settled. |
| `p:next(onResolve, onReject)` | Chain callbacks; returns a new promise. |
| `promise.all(list)` | Settles when every promise in `list` has settled; the value is the list of results. Rejected if any of them was rejected. |
| `promise.first(list)` | Settles like the first promise of `list` that settles. |
| `promise.map(list, fn)` | Calls `fn(item)` (which must return a promise) for each item, one after another; resolves with the results. |
| `Citizen.Await(p)` | Wait for `p`; return its value, or raise its error if rejected. |

```lua
local function delay(ms)
    local p = promise.new()
    SetTimeout(ms, function()
        p:resolve(ms)
    end)
    return p
end

CreateThread(function()
    local waited = Citizen.Await(delay(500))
    print(('waited %d ms'):format(waited))

    local results = Citizen.Await(promise.all({ delay(100), delay(200) }))
    print(#results) -- 2
end)
```

`Citizen.Await` only accepts promise objects and must run inside a scheduler thread. To await something callback-based, wrap it:

```lua
-- server: turn a callback API into an awaitable one
local function httpGet(url)
    local p = promise.new()
    PerformHttpRequest(url, function(status, body, headers, errorData)
        p:resolve({ status = status, body = body, err = errorData })
    end, 'GET')
    return Citizen.Await(p)
end
```

On the server, `PerformHttpRequestAwait(url, method, data, headers, options)` already does this and returns `status, body, headers, errorData`.

### Awaiting with a timeout

Because resolving or rejecting an already settled promise is ignored, a timeout is just a race:

```lua
local function awaitWithTimeout(p, ms)
    SetTimeout(ms, function()
        p:reject('timeout')
    end)
    return pcall(Citizen.Await, p)
end

CreateThread(function()
    local p = promise.new()
    local ok, result = awaitWithTimeout(p, 2000)
    if not ok then
        print(('failed: %s'):format(result))
    end
end)
```

## Typical patterns

### Wait until something is ready

```lua
local function loadModel(model)
    local hash = type(model) == 'number' and model or joaat(model)
    if not IsModelInCdimage(hash) then
        return false
    end
    RequestModel(hash)
    local deadline = GetGameTimer() + 5000
    while not HasModelLoaded(hash) do
        if GetGameTimer() > deadline then
            return false
        end
        Wait(0)
    end
    return true
end
```

Always put a deadline on such loops: a model that never loads would otherwise keep the thread spinning forever.

### Debounce

Run an action once things have been quiet for a while:

```lua
local pendingSave = nil

local function scheduleSave(saveFn)
    if pendingSave then
        ClearTimeout(pendingSave)
    end
    pendingSave = SetTimeout(2000, function()
        pendingSave = nil
        saveFn()
    end)
end
```

### Run once after the resource is fully started

```lua
AddEventHandler('onClientResourceStart', function(resourceName)
    if resourceName ~= GetCurrentResourceName() then return end
    print('this resource started on the client')
end)
```

## Performance checklist

- Never write `while true do ... end` without a `Wait` inside: it hangs the game or the server.
- Prefer one loop that handles several things over many small loops.
- Prefer events and state bag change handlers over polling.
- Use `Wait(0)` only while something has to happen every frame; fall back to a longer wait as soon as possible.
- Cache native results inside an iteration (`local ped = PlayerPedId()`).

See `cfxdoc:performance` for more.

## Sources

- Lua runtime functions: https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/
- Citizen.CreateThread: https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.CreateThread/
- Citizen.Await: https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.Await/
- Scheduler source (`scheduler.lua`, `deferred.lua`): https://github.com/citizenfx/fivem/tree/master/data/shared/citizen/scripting/lua
- lua-promises: https://github.com/zserge/lua-promises
