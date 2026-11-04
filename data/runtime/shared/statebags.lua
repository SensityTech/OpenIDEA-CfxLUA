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

-- Cfx.re Lua runtime (client and server): state bags and the Entity/Player
-- wrappers, from `scheduler.lua`. The state bag natives (AddStateBagChangeHandler,
-- GetStateBagValue, ...) come from the generated native libraries. `LocalPlayer`
-- is client-only and declared in `client/client_statebags.lua`.

---
--- A state bag: a set of key/value pairs attached to the server (`GlobalState`),
--- an entity (`Entity(handle).state`) or a player (`Player(id).state`), and
--- synchronized between the server and the clients.
---
--- * Read a value with `bag.key` or `bag['key']` (`nil` when unset).
--- * Write with `bag.key = value`: replicated when written by the server,
---   local only when written by a client.
--- * Write with `bag:set(key, value, replicated)` to choose explicitly.
---
--- Values are serialized with msgpack. Writes are subject to the server
--- policy: the global bag is server-only, player bags can be written by their
--- player and the server, entity bags by the entity owner and the server.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-manual/networking/state-bags/)
---@class StateBag
local StateBagType = {}

---
--- Sets `key` to `value`, choosing whether the change is replicated.
---
--- ```lua
--- Player(source).state:set('job', 'police', true)
--- ```
---@overload fun(key:string, value:any)
---@param key string @The key to set.
---@param value any @The new value; it must be serializable with msgpack.
---@param replicated boolean @`true` to replicate the change to the other side (default `false`).
function StateBagType:set(key, value, replicated) end

---
--- Wrapper returned by `Entity(handle)`, giving access to the entity state bag.
---@class CfxEntity
---@field state StateBag @The entity state bag (`entity:<netId>` for networked entities, `localEntity:<handle>` otherwise).
local CfxEntityType = {}

---
--- Wrapper returned by `Player(id)`, giving access to the player state bag.
---@class CfxPlayer
---@field state StateBag @The player state bag (`player:<serverId>`).
local CfxPlayerType = {}

---
--- Wraps an entity handle to access its state bag.
---
--- On the server, and for networked entities on the client, the bag is keyed
--- by the network id. Non-networked client entities get a local-only bag.
--- Assigning fields on the wrapper itself raises an error: write through
--- `state` instead.
---
--- ```lua
--- local vehicle = GetVehiclePedIsIn(PlayerPedId(), false)
--- print(Entity(vehicle).state.fuel)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-manual/networking/state-bags/)
---@param entity integer|CfxEntity @The entity handle. An existing wrapper is returned unchanged.
---@return CfxEntity @The entity wrapper.
function Entity(entity) end

---
--- Wraps a player to access its state bag.
---
--- `id` is the player server id (`source` on the server). On the client, `-1`
--- designates the local player (see `LocalPlayer`).
---
--- ```lua
--- Player(source).state.job = 'police'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-manual/networking/state-bags/)
---@param id integer|string|CfxPlayer @The player server id. An existing wrapper is returned unchanged.
---@return CfxPlayer @The player wrapper.
function Player(id) end

---
--- The global state bag, shared by the server and all clients. Only the server
--- can write to it.
---
--- ```lua
--- GlobalState.weather = 'RAIN'
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-manual/networking/state-bags/)
---@type StateBag
GlobalState = nil
