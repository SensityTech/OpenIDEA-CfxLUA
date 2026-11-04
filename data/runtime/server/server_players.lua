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

-- Cfx.re Lua runtime, server only: player helpers from `scheduler.lua`, built
-- on top of the GetNumPlayerIndices / GetPlayerFromIndex,
-- GetNumPlayerIdentifiers / GetPlayerIdentifier and GetNumPlayerTokens /
-- GetPlayerToken natives.

---
--- Returns the server ids of every connected player. The ids are **strings**;
--- use `tonumber` when a number is needed.
---
--- ```lua
--- for _, playerId in ipairs(GetPlayers()) do
---     print(playerId, GetPlayerName(playerId))
--- end
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/GetPlayers/)
---@return string[] @The player server ids.
function GetPlayers() end

---
--- Returns every identifier of a player, such as `license:...`,
--- `license2:...`, `discord:...`, `fivem:...`, `steam:...`, `xbl:...`,
--- `live:...` and `ip:...`.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/GetPlayerIdentifiers/)
---@param playerId integer|string @The player server id.
---@return string[] @The player identifiers.
function GetPlayerIdentifiers(playerId) end

---
--- Returns every hardware token of a player.
---@param playerId integer|string @The player server id.
---@return string[] @The player tokens.
function GetPlayerTokens(playerId) end

---
--- Alias of the `GetPlayerEndpoint` native: returns the network endpoint
--- (`ip:port`) of a player.
---@param playerId integer|string @The player server id.
---@return string @The player endpoint.
function GetPlayerEP(playerId) end
