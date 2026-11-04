# Your first resource

A **resource** is a folder of scripts and files that the server loads as one unit. This guide creates a small resource with a client script, a server script and a command, and explains how the pieces connect. It works the same on FiveM and RedM (only the manifest's `game` line changes).

## 1. Create the folder

Inside your server's `resources` directory, create a folder. Its name is the resource name, so keep it short, lowercase, without spaces:

```
resources/
  [local]/
    hello/
      fxmanifest.lua
      client.lua
      server.lua
```

Folders whose name is in square brackets, like `[local]`, are only categories: they are not resources themselves.

## 2. Write the manifest

`fxmanifest.lua` tells the server what the resource contains:

```lua
fx_version 'cerulean'
game 'gta5'            -- RedM: game 'rdr3' plus the rdr3_warning line, see the manifest guide

author 'You'
description 'My first resource'
version '1.0.0'

client_script 'client.lua'
server_script 'server.lua'
```

OpenIDEA-CfxLUA reads this file to know that `client.lua` is client code and `server.lua` is server code, and offers the right natives in each. See `cfxdoc:fxmanifest` for every directive.

## 3. Client script

```lua
-- client.lua
RegisterCommand('hello', function(_, args)
    local name = args[1] or 'world'
    local pos = GetEntityCoords(PlayerPedId())
    print(('Hello, %s! You are at %.1f, %.1f, %.1f'):format(name, pos.x, pos.y, pos.z))
    TriggerServerEvent('hello:greeted', name)
end, false)

RegisterNetEvent('hello:reply', function(message)
    print(('Server says: %s'):format(message))
end)
```

## 4. Server script

```lua
-- server.lua
local greetings = 0

RegisterNetEvent('hello:greeted', function(name)
    local src = source
    if type(name) ~= 'string' or #name > 32 then return end

    greetings += 1
    print(('%s greeted %s (%d greetings so far)'):format(GetPlayerName(src), name, greetings))
    TriggerClientEvent('hello:reply', src, ('you are greeting number %d'):format(greetings))
end)

AddEventHandler('onResourceStart', function(resourceName)
    if resourceName == GetCurrentResourceName() then
        print('hello resource started')
    end
end)
```

What happens when a player types `/hello Alice` in chat (or `hello Alice` in the F8 console):

1. The client command handler runs on that player's machine, prints the position, and sends `hello:greeted` to the server.
2. The server handler receives it. `source` is the player's server id. It validates the argument, counts, and answers with `hello:reply` to that player only.
3. The client handler for `hello:reply` prints the answer.

Both net events are registered with `RegisterNetEvent`; without it, the receiving side would drop them. See `cfxdoc:events`.

## 5. Start it

Add the resource to `server.cfg`:

```
ensure hello
```

While the server is running, you can also use its console:

| Command | Effect |
|---|---|
| `refresh` | Rescan the resources folder (needed after creating a new resource or editing a manifest) |
| `ensure hello` | Start the resource, or restart it if it is running |
| `restart hello` | Restart a running resource |
| `stop hello` | Stop it |

Client output appears in the game console (F8). Server output appears in the server console.

## 6. Iterate

- Edit a script, then `restart hello` (or `ensure hello`): no need to restart the server or reconnect.
- Errors are printed with the resource, file and line, followed by a stack trace.
- Use `print` generously while learning; remove noisy prints afterwards.

## 7. Share configuration between client and server

Values needed on both sides go in a shared script, loaded before the client and server scripts:

```lua
-- fxmanifest.lua (excerpt)
shared_script 'config.lua'
client_script 'client.lua'
server_script 'server.lua'
```

```lua
-- config.lua
Config = {
    maxNameLength = 32,
    greetingCooldownMs = 2000,
}
```

Every script of the resource on the same side shares one global environment, so `Config` is visible in `client.lua` and `server.lua`. Keep everything else `local` to avoid accidental clashes between files.

## 8. Add a background loop

A client loop that shows how far you are from a point, checking twice per second:

```lua
-- client.lua
local TARGET <const> = vector3(-268.4, -957.3, 31.2)

CreateThread(function()
    while true do
        local distance = #(GetEntityCoords(PlayerPedId()) - TARGET)
        if distance < 5.0 then
            print(('almost there: %.1f m'):format(distance))
        end
        Wait(500)
    end
end)
```

Every loop must call `Wait`, otherwise it freezes the game. See `cfxdoc:threads-and-timing`.

## Next steps

| Topic | Guide |
|---|---|
| The CfxLua dialect (`+=`, `?.`, backtick hashes...) | `cfxdoc:cfxlua-syntax` |
| Calling natives, return values, out-parameters | `cfxdoc:natives` |
| Threads, `Wait`, timers, promises | `cfxdoc:threads-and-timing` |
| Events between client and server | `cfxdoc:events` |
| Sharing functions between resources | `cfxdoc:exports` |
| Replicated state | `cfxdoc:state-bags` |
| HTML user interfaces | `cfxdoc:nui` |
| Keeping your server safe | `cfxdoc:security` |
| Using the IDE features | `cfxdoc:plugin-guide` |

## Sources

- Introduction to resources: https://docs.fivem.net/docs/scripting-manual/introduction/introduction-to-resources/
- Creating your first script in Lua: https://docs.fivem.net/docs/scripting-manual/introduction/creating-your-first-script/
- Server commands: https://docs.fivem.net/docs/server-manual/server-commands/
- Resource manifest: https://docs.fivem.net/docs/scripting-reference/resource-manifest/resource-manifest/
