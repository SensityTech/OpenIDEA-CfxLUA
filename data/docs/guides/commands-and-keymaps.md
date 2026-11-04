# Commands, key mappings and permissions

Commands are the standard way to expose actions to players (`/car adder`), to the server console and to other resources. On FiveM, commands are also the basis of rebindable key mappings.

## RegisterCommand

```lua
RegisterCommand(name, function(source, args, rawCommand) end, restricted)
```

| Parameter | Meaning |
|---|---|
| `name` | Command name, without `/`. |
| `source` | Server side: the player's server id, or `0` when the command comes from the server console, RCON or a resource. Client side: not meaningful. |
| `args` | Array of the words after the command name (strings). |
| `rawCommand` | The full text that was typed. |
| `restricted` | Server side: when `true`, the player needs the ACE permission `command.<name>`. Ignored on the client. |

Commands registered on the client run locally (client console, chat with `/`). Commands registered on the server can be run by players (through the client console or chat), by the server console, by RCON, and by resources with `ExecuteCommand`.

### Client command

```lua
RegisterCommand('coords', function(_, args)
    local pos = GetEntityCoords(PlayerPedId())
    local heading = GetEntityHeading(PlayerPedId())
    print(('vector4(%.2f, %.2f, %.2f, %.2f)'):format(pos.x, pos.y, pos.z, heading))
end, false)
```

### Server command with arguments

```lua
RegisterCommand('heal', function(source, args)
    local target = tonumber(args[1]) or source
    if source == 0 and not args[1] then
        print('usage from console: heal <playerId>')
        return
    end
    if not GetPlayerName(target) then
        return
    end
    TriggerClientEvent('medic:heal', target)
end, true) -- restricted: requires the ACE "command.heal"
```

- Always validate `args`: they are free text typed by the player. `tonumber` returns `nil` for non-numbers.
- `GetPlayerName(id)` returns `nil` for an id that is not connected, which makes a quick existence check.

### Running a command from code

```lua
ExecuteCommand('heal 3')
```

## ACE permissions

Permissions on the server are expressed with ACEs (access control entries) in the server configuration (`server.cfg`) or the console:

```
# give the group "admin" the right to use /heal and anything under "mygarage."
add_ace group.admin command.heal allow
add_ace group.admin mygarage allow

# put a player in the admin group (by identifier)
add_principal identifier.license:0123456789abcdef group.admin
```

| Command | Effect |
|---|---|
| `add_ace <principal> <object> allow\|deny` | Allow or deny an object for a principal |
| `remove_ace <principal> <object> allow\|deny` | Remove an entry |
| `add_principal <child> <parent>` | Make a principal inherit another one (for example a player into a group) |
| `remove_principal <child> <parent>` | Remove the inheritance |
| `test_ace <principal> <object>` | Check whether a principal has an object |

Principals are names such as `group.admin` or `identifier.license:...`. Objects are dotted names such as `command.heal`; you can invent your own (`mygarage.manage`). Granting a parent object (`mygarage`) also covers its children (`mygarage.manage`).

Check permissions in server code with `IsPlayerAceAllowed`:

```lua
RegisterNetEvent('mygarage:deleteVehicle', function(netId)
    local src = source
    if not IsPlayerAceAllowed(src, 'mygarage.manage') then
        print(('player %d tried to delete a vehicle without permission'):format(src))
        return
    end
    local vehicle = NetworkGetEntityFromNetworkId(netId)
    if vehicle ~= 0 and DoesEntityExist(vehicle) then
        DeleteEntity(vehicle)
    end
end)
```

Never decide permissions on the client: client code can be modified.

## Key mappings (FiveM)

`RegisterKeyMapping(command, description, defaultMapper, defaultKey)` binds a command to a key. Players see it in **Settings > Key Bindings > FiveM** and can rebind it; their choice is saved on their machine.

```lua
RegisterCommand('+handsup', function()
    -- key pressed
    local ped = PlayerPedId()
    TaskHandsUp(ped, -1, ped, -1, true)
end, false)

RegisterCommand('-handsup', function()
    -- key released
    ClearPedTasks(PlayerPedId())
end, false)

RegisterKeyMapping('+handsup', 'Raise your hands', 'keyboard', 'x')
```

- A command starting with `+` runs when the key is pressed; the matching `-` command runs when it is released. A command without `+` runs once per press.
- `defaultMapper` is the input device: `keyboard`, `mouse_button`, `pad_digitalbutton`, ... `defaultKey` is a key name for that device (`x`, `f5`, `lmenu`...). The lists are in the input mapper reference linked below.
- Prefix the command with `~!` to register an **alternate** binding for the same command: `RegisterKeyMapping('~!+handsup', 'Raise your hands (alternate)', 'keyboard', 'h')`.
- The default key only applies to players who never changed the binding. Changing it in code later does not override existing player settings.
- Register the mapping on the client. It is FiveM-only: RedM does not have `RegisterKeyMapping`.

Compared with a `Wait(0)` loop polling `IsControlJustPressed`, key mappings cost nothing per frame and are rebindable.

### RedM

RedM has no key mapping natives of this kind. Use either:

- `RegisterRawKeymap(name, onKeyDown, onKeyUp, virtualKey, canBeDisabled)` (RedM only), which binds a Windows virtual-key code to two callbacks;
- or a loop that checks controls each frame with `IsControlJustPressed(0, controlHash)`.

## Chat suggestions

With the default `chat` resource, describe your commands so that players get completion and help in the chat box. This runs on the client:

```lua
TriggerEvent('chat:addSuggestion', '/heal', 'Heal a player', {
    { name = 'playerId', help = 'Server id of the player (optional)' },
})
```

`chat:removeSuggestion` removes one again (for example in `onResourceStop`):

```lua
AddEventHandler('onResourceStop', function(resourceName)
    if resourceName == GetCurrentResourceName() then
        TriggerEvent('chat:removeSuggestion', '/heal')
    end
end)
```

Chat messages can be sent with the `chat:addMessage` event:

```lua
-- server: to everyone
TriggerClientEvent('chat:addMessage', -1, {
    color = { 255, 200, 0 },
    args = { 'Server', 'Restart in 5 minutes' },
})
```

## Sources

- REGISTER_COMMAND: https://docs.fivem.net/natives/?_0x5FA79B0F
- REGISTER_KEY_MAPPING: https://docs.fivem.net/natives/?_0xD7664FD1
- Input mapper parameter IDs: https://docs.fivem.net/docs/game-references/input-mapper-parameter-ids/
- REGISTER_RAW_KEYMAP (RedM): https://docs.fivem.net/natives/?_0x49C1F6DC
- IS_PLAYER_ACE_ALLOWED: https://docs.fivem.net/natives/?_0xDEDAE23D
- Server commands (ACL): https://docs.fivem.net/docs/server-manual/server-commands/
- chat:addSuggestion: https://docs.fivem.net/docs/resources/chat/events/chat-addSuggestion/
