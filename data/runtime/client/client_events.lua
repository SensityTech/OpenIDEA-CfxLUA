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

-- Cfx.re Lua runtime, client only: client to server events, from
-- `scheduler.lua`.

---
--- Triggers the network event `eventName` on the server. The server must have
--- registered it with `RegisterNetEvent`; there, `source` is the server id of
--- this player. Arguments are serialized with msgpack.
---
--- ```lua
--- TriggerServerEvent('myResource:buy', itemName, amount)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/TriggerServerEvent/)
---@param eventName string @The event name.
---@vararg any
function TriggerServerEvent(eventName, ...) end

---
--- Like `TriggerServerEvent`, but sends the payload as a latent event,
--- throttled to `bps` bytes per second, so that large payloads do not block
--- the connection.
---@param eventName string @The event name.
---@param bps integer @Maximum transfer rate, in bytes per second.
---@vararg any
function TriggerLatentServerEvent(eventName, bps, ...) end
