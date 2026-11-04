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

-- Cfx.re Lua runtime, server only: scheduler and console aliases from
-- `scheduler.lua`.
--
-- `Wait` is an alias of `Citizen.Wait` on both sides, but on the client the
-- name is already provided by the game natives (SYSTEM::WAIT), so it is only
-- declared here, where the server natives do not define it.

---
--- Global alias of `Citizen.Wait`: pauses the current scheduler thread for `ms`
--- milliseconds (`0` or no argument: until the next tick).
---
--- Must run inside a scheduler thread (the main chunk of a resource script,
--- `CreateThread` and `SetTimeout` callbacks, event handlers, ...).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.Wait/)
---@overload fun()
---@param ms integer @Number of milliseconds to wait.
function Wait(ms) end

---
--- Alias of `Citizen.Trace`: writes `message` to the server console. No
--- newline is appended.
---@param message string @The text to print.
function RconPrint(message) end

---
--- Does nothing. Kept for compatibility with old resources.
---@deprecated Does nothing.
---@vararg any
function RconLog(...) end
