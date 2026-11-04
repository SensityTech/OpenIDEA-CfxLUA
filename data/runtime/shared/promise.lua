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

-- Cfx.re Lua runtime (client and server): the global `promise` library
-- (zserge/lua-promises, loaded from `deferred.lua`). The library and the
-- promises it creates are described by two classes: `PromiseLib` (the type of
-- the global `promise`) and `promise` (the promise instances).

---
--- A promise created by `promise.new()`. Settle it with `resolve` or `reject`,
--- chain callbacks with `next`, and wait for it inside a scheduler thread with
--- `Citizen.Await`.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.new/)
---@class promise
---@field state integer @0 pending, 1 resolving, 2 rejecting, 3 resolved, 4 rejected.
---@field value any @The resolved value or the rejection reason, once settled.
local PromiseType = {}

---
--- Resolves the promise with `value`. Has no effect if the promise is already
--- settled.
---@param value any @The resolved value.
---@return promise @This promise.
function PromiseType:resolve(value) end

---
--- Rejects the promise with `reason`. Has no effect if the promise is already
--- settled.
---@param reason any @The rejection reason.
---@return promise @This promise.
function PromiseType:reject(reason) end

---
--- Registers callbacks called when the promise settles and returns a new
--- promise for the value they return. Either callback can be `nil`.
---
--- ```lua
--- p:next(function(value)
---     print('resolved', value)
--- end, function(reason)
---     print('rejected', reason)
--- end)
--- ```
---@overload fun(onResolve:fun(value:any):any):promise
---@param onResolve fun(value:any):any @Called with the resolved value.
---@param onReject fun(reason:any):any @Called with the rejection reason.
---@return promise @A promise of the callback result.
function PromiseType:next(onResolve, onReject) end

---
--- The global promise library: `promise.new`, `promise.all`, `promise.map` and
--- `promise.first` create promises (see the `promise` class for their
--- methods).
---
--- ```lua
--- local p = promise.new()
--- SetTimeout(500, function()
---     p:resolve(42)
--- end)
--- local value = Citizen.Await(p)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.new/)
---@class PromiseLib
local PromiseLibType = {}

---
--- Creates a new pending promise.
---
--- If `options` is a function, it is called with the new promise; an error
--- raised by it rejects the promise. A table can provide `success` and
--- `failure` callbacks and an `extend` function called with the new promise.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.new/)
---@overload fun():promise
---@param options fun(p:promise)|table @Initializer function, or a `{ success, failure, extend }` table.
---@return promise @The new promise.
function PromiseLibType.new(options) end

---
--- Returns a promise that settles once every promise of `promises` has
--- settled. It resolves with the array of results, or rejects with that array
--- if any of the promises was rejected. An empty list resolves immediately.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.all/)
---@param promises promise[] @The promises to wait for.
---@return promise @A promise of the array of results.
function PromiseLibType.all(promises) end

---
--- Calls `mapper` on each element of `values` one after the other, waiting for
--- the promise it returns before moving on. The returned promise resolves with
--- the array of results, or rejects with the first rejection reason.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.map/)
---@param values any[] @The values to map.
---@param mapper fun(value:any):promise @Function returning a promise for each value.
---@return promise @A promise of the array of results.
function PromiseLibType.map(values, mapper) end

---
--- Returns a promise that settles like the first promise of `promises` to
--- settle.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.first/)
---@param promises promise[] @The competing promises.
---@return promise @A promise settled by the first one to settle.
function PromiseLibType.first(promises) end

---
--- The global promise library (see `PromiseLib`): `promise.new()` creates a
--- promise, `promise.all`, `promise.map` and `promise.first` combine
--- promises.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/promise.new/)
---@type PromiseLib
promise = nil
