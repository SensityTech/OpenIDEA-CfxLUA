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

-- Cfx.re Lua runtime (client and server): event handling, from `scheduler.lua`.
-- CancelEvent, WasEventCanceled, GetInvokingResource and GetCurrentResourceName
-- are natives and come from the generated native libraries.

---
--- Handle returned by `AddEventHandler` and `RegisterNetEvent`. Pass it to
--- `RemoveEventHandler` to unregister the handler.
---@class EventHandlerData
---@field key integer @Internal handler key.
---@field name string @Name of the event the handler is registered for.
local EventHandlerDataType = {}

---
--- Registers `eventRoutine` as a handler for the event `eventName` and returns
--- a handle for `RemoveEventHandler`.
---
--- Each handler call runs in its own scheduler thread, so `Wait` can be used in
--- it. During the call, the global `source` holds the event source (the player
--- server id for events sent by a client, on the server).
---
--- Events coming from the network are only delivered when the event has been
--- registered with `RegisterNetEvent`.
---
--- Built-in events include:
---
--- * shared: `onResourceStart`, `onResourceStarting`, `onResourceStop`
--- * server: `onServerResourceStart`, `onServerResourceStop`,
---   `onResourceListRefresh`, `playerConnecting`, `playerJoining`,
---   `playerDropped`, `playerEnteredScope`, `playerLeftScope`,
---   `entityCreating`, `entityCreated`, `entityRemoved`,
---   `onPlayerBucketChange`, `onEntityBucketChange`, `weaponDamageEvent`,
---   `startProjectileEvent`, `ptFxEvent`, `removeAllWeaponsEvent`,
---   `respawnPlayerPedEvent`, `vehicleComponentControlEvent`
--- * client: `onClientResourceStart`, `onClientResourceStop`,
---   `gameEventTriggered`, `entityDamaged`, `populationPedCreating`,
---   `mumbleConnected`, `mumbleDisconnected`
---
--- ```lua
--- AddEventHandler('onResourceStart', function(resourceName)
---     if resourceName == GetCurrentResourceName() then
---         print('started')
---     end
--- end)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/AddEventHandler/),
--- [event list](https://docs.fivem.net/docs/scripting-reference/events/list/)
---@param eventName string @The event name.
---@param eventRoutine function @The handler, called with the event arguments.
---@return EventHandlerData @Handle usable with `RemoveEventHandler`.
function AddEventHandler(eventName, eventRoutine) end

---
--- Removes an event handler registered with `AddEventHandler` or
--- `RegisterNetEvent`. Raises an error if `eventData` is not a valid handle.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/RemoveEventHandler/)
---@param eventData EventHandlerData @The handle returned when the handler was registered.
function RemoveEventHandler(eventData) end

---
--- Marks the event `eventName` as safe to be triggered from the network
--- (`TriggerServerEvent` on the client, `TriggerClientEvent` on the server).
--- Network events that were not registered this way are dropped.
---
--- When `eventRoutine` is given, it is also registered as a handler, like
--- `AddEventHandler`.
---
--- ```lua
--- RegisterNetEvent('myResource:hello', function(message)
---     print(('player %s says %s'):format(source, message))
--- end)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/RegisterNetEvent/)
---@overload fun(eventName:string)
---@param eventName string @The event name.
---@param eventRoutine function @Optional handler, called with the event arguments.
---@return EventHandlerData|nil @The handler handle when `eventRoutine` is given, `nil` otherwise.
function RegisterNetEvent(eventName, eventRoutine) end

---
--- Triggers the event `eventName` locally (on the same side, in every resource
--- that handles it). Arguments are serialized with msgpack: functions become
--- function references and tables are copied.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/TriggerEvent/)
---@param eventName string @The event name.
---@vararg any
function TriggerEvent(eventName, ...) end
