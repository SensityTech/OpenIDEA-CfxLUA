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

-- Cfx.re Lua runtime, client only: members of the `Citizen` table that only
-- exist on the client (`#ifndef IS_FXSERVER` in LuaScriptRuntime.cpp). The
-- `Citizen` class itself is declared in `shared/citizen.lua`.

---
--- Returns a function that invokes the native `hash` directly, which is
--- faster than `Citizen.InvokeNative` when the same native is called often.
--- The returned function accepts the native arguments and the same marker
--- arguments as `Citizen.InvokeNative`.
---
--- ```lua
--- local getPlayerName = Citizen.GetNative(0x6D0DE6A7B5DA71F8)
--- print(getPlayerName(PlayerId(), Citizen.ResultAsString()))
--- ```
---@param hash integer @The native hash.
---@return function @A function invoking the native.
function Citizen.GetNative(hash) end

---
--- Invokes a native handler obtained from an old version of
--- `Citizen.GetNative`. `Citizen.GetNative` now returns a function that can be
--- called directly.
---@deprecated Call the function returned by Citizen.GetNative directly.
---@param handler function @The native handler.
---@vararg any
---@return any @The native result.
function Citizen.InvokeNative2(handler, ...) end
