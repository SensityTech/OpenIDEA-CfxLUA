-- Copyright (c) 2026. SensityTech
--
-- Licensed under the Apache License, Version 2.0 (the "License"); you may not
-- use this file except in compliance with the License. You may obtain a copy of
-- the License at
--
-- http://www.apache.org/licenses/LICENSE-2.0
--
-- Unless required by applicable law or agreed to in writing, software
-- distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
-- WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the
-- License for the specific language governing permissions and limitations under
-- the License.

-- Cfx.re Lua runtime, server only: server to client events and the `source`
-- global, from `scheduler.lua`.

---
--- The source of the event being handled.
---
--- In a handler of a network event triggered by a client
--- (`TriggerServerEvent`), this is the server id of that player, usable with
--- `Player(source)`, `TriggerClientEvent` and the player natives. For local
--- events (`TriggerEvent`) it is not a player id (normally an empty string).
--- The value is only valid until the handler first yields: copy it to a local
--- before calling `Wait`.
---
--- ```lua
--- RegisterNetEvent('myResource:hello', function()
---     local playerId = source
---     print(GetPlayerName(playerId))
--- end)
--- ```
---@type integer
source = nil

---
--- Triggers the network event `eventName` on one client, or on every client
--- with `-1`. The clients must have registered the event with
--- `RegisterNetEvent`. Arguments are serialized with msgpack.
---
--- ```lua
--- TriggerClientEvent('chat:addMessage', -1, { args = { 'Server', 'Hello!' } })
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/TriggerClientEvent/)
---@param eventName string @The event name.
---@param playerId integer|string @The target player server id, or `-1` for every client.
---@vararg any
function TriggerClientEvent(eventName, playerId, ...) end

---
--- Like `TriggerClientEvent`, but sends the payload as a latent event,
--- throttled to `bps` bytes per second, so that large payloads do not block
--- the connection.
---@param eventName string @The event name.
---@param playerId integer|string @The target player server id, or `-1` for every client.
---@param bps integer @Maximum transfer rate, in bytes per second.
---@vararg any
function TriggerLatentClientEvent(eventName, playerId, bps, ...) end

---
--- Legacy alias of `RegisterNetEvent`.
---@deprecated Use RegisterNetEvent.
---@overload fun(eventName:string)
---@param eventName string @The event name.
---@param eventRoutine function @Optional handler, called with the event arguments.
---@return EventHandlerData|nil @The handler handle when `eventRoutine` is given, `nil` otherwise.
function RegisterServerEvent(eventName, eventRoutine) end
