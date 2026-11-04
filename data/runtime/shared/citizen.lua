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

-- Cfx.re Lua runtime (client and server): the `Citizen` scheduler / native
-- invocation table and its global aliases. Sources: citizen-scripting-lua
-- `LuaScriptRuntime.cpp` (g_citizenLib) and `scheduler.lua`.
--
-- The global `Wait` alias is declared in `server/server_citizen.lua` only: on the
-- client it is provided by the game natives (SYSTEM::WAIT), which the runtime
-- overrides with `Citizen.Wait`.

---
--- The CfxLua scheduler and native invocation API.
---
--- Every resource script runs inside a cooperative scheduler: use
--- `Citizen.CreateThread` (or the global `CreateThread`) to start a thread and
--- `Citizen.Wait` (or `Wait`) to yield it.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/client-functions/)
---@class Citizen
Citizen = {}

---
--- Creates a new scheduler thread (a coroutine) that starts running on the next
--- scheduler tick. `Wait` can be used inside it.
---
--- The routine is called without arguments. The returned id is a scheduler
--- bookmark: passing it to `ClearTimeout` before the thread starts cancels it.
---
--- ```lua
--- Citizen.CreateThread(function()
---     while true do
---         Citizen.Wait(0)
---     end
--- end)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.CreateThread/)
---@param routine fun() @The function to run in the new thread.
---@return integer @The scheduler bookmark id of the new thread.
function Citizen.CreateThread(routine) end

---
--- Creates a new scheduler thread and runs it immediately, in the current tick,
--- until it finishes or yields for the first time (for example with `Wait`).
---
--- There is no global alias for this function.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.CreateThreadNow/)
---@overload fun(routine:fun()):boolean
---@param routine fun() @The function to run in the new thread.
---@param name string @Optional thread name, shown in profiler and error output.
---@return boolean @`true` if the thread yielded and is still alive, `false` if it already finished.
function Citizen.CreateThreadNow(routine, name) end

---
--- Pauses the current scheduler thread for `ms` milliseconds. `0` (or no
--- argument) resumes the thread on the next tick.
---
--- Must run inside a scheduler thread. The main chunk of each resource script,
--- `CreateThread` and `SetTimeout` callbacks, event handlers, NUI callbacks and
--- exports all run in one; plain coroutines created with `coroutine.create` do
--- not.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.Wait/)
---@overload fun()
---@param ms integer @Number of milliseconds to wait.
function Citizen.Wait(ms) end

---
--- Runs `callback` in a new scheduler thread after `msec` milliseconds.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetTimeout/)
---@param msec integer @Delay in milliseconds.
---@param callback fun() @The function to run.
---@return integer @Timeout id, usable with `ClearTimeout`.
function Citizen.SetTimeout(msec, callback) end

---
--- Cancels a pending `SetTimeout` (or a `CreateThread` that has not started
--- yet).
---@param id integer @The id returned by `SetTimeout` or `CreateThread`.
---@return boolean @`true` if a pending timeout was removed.
function Citizen.ClearTimeout(id) end

---
--- Writes `message` to the console (F8 on the client, the server console on
--- FXServer). No newline is appended.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.Trace/)
---@param message string @The text to print.
function Citizen.Trace(message) end

---
--- Waits for a promise to settle and returns its value. Raises the rejection
--- value as an error if the promise is rejected.
---
--- Must run inside a scheduler thread (the main chunk of a resource script,
--- `CreateThread`, an event handler, ...).
---
--- ```lua
--- local p = promise.new()
--- SetTimeout(1000, function() p:resolve('done') end)
--- print(Citizen.Await(p))
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.Await/)
---@param p promise @The promise to wait for.
---@return any @The resolved value.
function Citizen.Await(p) end

---
--- Calls a native function by hash. Prefer the generated native functions
--- (`GetPlayerName(PlayerId())`, ...) whenever they exist.
---
--- Vector arguments are expanded into their components. Out-parameters and the
--- result type are selected with the marker functions passed as extra
--- arguments: `Citizen.PointerValueInt()`, `Citizen.PointerValueFloat()`,
--- `Citizen.PointerValueVector()`, `Citizen.ResultAsInteger()`,
--- `Citizen.ResultAsFloat()`, `Citizen.ResultAsString()`,
--- `Citizen.ResultAsVector()`, `Citizen.ReturnResultAnyway()`, ...
---
--- Example with the GTA V native `GET_PLAYER_NAME`:
---
--- ```lua
--- local name = Citizen.InvokeNative(0x6D0DE6A7B5DA71F8, PlayerId(), Citizen.ResultAsString())
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.InvokeNative/)
---@param hash integer @The native hash.
---@vararg any
---@return any @The native result followed by the requested pointer values.
function Citizen.InvokeNative(hash, ...) end

---
--- Creates a function reference string for `func`, so it can be passed to other
--- resources or runtimes (for example as a native argument).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.GetFunctionReference/)
---@param func function|table @A function, or a function reference table received from another resource.
---@return string|nil @The canonical function reference, or `nil` if `func` cannot be referenced.
function Citizen.GetFunctionReference(func) end

---
--- **Internal.** Calls a function reference with msgpack-serialized arguments
--- and returns the msgpack-serialized results. Used by the runtime when a
--- funcref received from another resource is called.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.InvokeFunctionReference/)
---@param ref string @The function reference.
---@param argsSerialized string @The msgpack-packed argument array.
---@return string @The msgpack-packed results.
function Citizen.InvokeFunctionReference(ref, argsSerialized) end

---
--- **Internal.** Converts a local function reference index into its canonical,
--- globally unique reference string.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.CanonicalizeRef/)
---@param refIdx integer @The local reference index.
---@return string @The canonical reference string.
function Citizen.CanonicalizeRef(refIdx) end

---
--- **Internal.** Resolves a native by its Lua name. Used by the lazy native
--- loader that backs the global native functions.
---@param nativeName string @The Lua name of the native, for example `GetPlayerPed`.
---@return any @The native implementation, or `nil` if no such native exists.
function Citizen.LoadNative(nativeName) end

---
--- Native argument marker: declares an `int*` output parameter. The value it
--- receives is appended to the return values of `Citizen.InvokeNative`.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.PointerValueInt/)
---@return userdata @The argument marker.
function Citizen.PointerValueInt() end

---
--- Native argument marker: declares an `int*` in/out parameter initialized with
--- `value`. The resulting value is appended to the return values.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.PointerValueIntInitialized/)
---@param value integer @The initial value.
---@return userdata @The argument marker.
function Citizen.PointerValueIntInitialized(value) end

---
--- Native argument marker: declares a `float*` output parameter. The value it
--- receives is appended to the return values.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.PointerValueFloat/)
---@return userdata @The argument marker.
function Citizen.PointerValueFloat() end

---
--- Native argument marker: declares a `float*` in/out parameter.
---
--- Note: the current runtime ignores `value` and always initializes the
--- pointer with `0`.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.PointerValueFloatInitialized/)
---@param value number @The initial value (currently ignored by the runtime).
---@return userdata @The argument marker.
function Citizen.PointerValueFloatInitialized(value) end

---
--- Native argument marker: declares a `Vector3*` output parameter. The value it
--- receives is appended to the return values as a `vector3`.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.PointerValueVector/)
---@return userdata @The argument marker.
function Citizen.PointerValueVector() end

---
--- Native argument marker: also return the native's own result when pointer
--- values are requested.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ReturnResultAnyway/)
---@return userdata @The argument marker.
function Citizen.ReturnResultAnyway() end

---
--- Native argument marker: interpret the native result as a 32-bit integer.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ResultAsInteger/)
---@return userdata @The argument marker.
function Citizen.ResultAsInteger() end

---
--- Native argument marker: interpret the native result as a 64-bit integer.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ResultAsLong/)
---@return userdata @The argument marker.
function Citizen.ResultAsLong() end

---
--- Native argument marker: interpret the native result as a float.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ResultAsFloat/)
---@return userdata @The argument marker.
function Citizen.ResultAsFloat() end

---
--- Native argument marker: interpret the native result as a string.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ResultAsString/)
---@return userdata @The argument marker.
function Citizen.ResultAsString() end

---
--- Native argument marker: interpret the native result as a `vector3`.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ResultAsVector/)
---@return userdata @The argument marker.
function Citizen.ResultAsVector() end

---
--- Does nothing: kept for compatibility with old scripts. Use
--- `Citizen.ResultAsObject2(msgpack.unpack)` to decode object results.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.ResultAsObject/)
---@deprecated Does nothing; use Citizen.ResultAsObject2.
function Citizen.ResultAsObject() end

---
--- Native argument marker: interpret the native result as a serialized object
--- and decode it with `deserializer`, for example
--- `Citizen.ResultAsObject2(msgpack.unpack)`.
---@param deserializer fun(data:string):any @Function that decodes the raw result.
---@return userdata @The argument marker.
function Citizen.ResultAsObject2(deserializer) end

---
--- **Internal** runtime hook: installs the routine that wraps every scheduler
--- thread in a stack-trace boundary. Overriding it breaks the scheduler.
---@param routine function @The boundary routine.
function Citizen.SetBoundaryRoutine(routine) end

---
--- **Internal** runtime hook: installs the per-tick routine. Overriding it
--- breaks the scheduler.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetTickRoutine/)
---@param routine function @The tick routine.
function Citizen.SetTickRoutine(routine) end

---
--- **Internal** runtime hook: installs the event dispatcher used by
--- `AddEventHandler`. Overriding it breaks event handling for the resource.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetEventRoutine/)
---@param routine fun(eventName:string, eventPayload:string, eventSource:string) @The event dispatcher.
function Citizen.SetEventRoutine(routine) end

---
--- **Internal** runtime hook: installs the routine that executes function
--- reference calls coming from other resources.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetCallRefRoutine/)
---@param routine fun(refId:integer, argsSerialized:string):string @The call routine.
function Citizen.SetCallRefRoutine(routine) end

---
--- **Internal** runtime hook: installs the routine that releases a function
--- reference.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetDeleteRefRoutine/)
---@param routine fun(refId:integer) @The delete routine.
function Citizen.SetDeleteRefRoutine(routine) end

---
--- **Internal** runtime hook: installs the routine that duplicates a function
--- reference.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetDuplicateRefRoutine/)
---@param routine fun(refId:integer):integer @The duplicate routine.
function Citizen.SetDuplicateRefRoutine(routine) end

---
--- **Internal** runtime hook: installs the routine that formats stack traces
--- for error reports.
---@param routine function @The stack trace routine.
function Citizen.SetStackTraceRoutine(routine) end

---
--- **Internal.** Marks the start of a stack-trace boundary.
---@overload fun(boundaryId:integer)
---@param boundaryId integer @The boundary id.
---@param thread thread @The coroutine the boundary belongs to.
function Citizen.SubmitBoundaryStart(boundaryId, thread) end

---
--- **Internal.** Marks the end of a stack-trace boundary.
---@overload fun(boundaryId:integer)
---@param boundaryId integer @The boundary id.
---@param thread thread @The coroutine the boundary belongs to.
function Citizen.SubmitBoundaryEnd(boundaryId, thread) end

---
--- Global alias of `Citizen.CreateThread`: creates a new scheduler thread that
--- starts on the next tick.
---
--- ```lua
--- CreateThread(function()
---     while true do
---         Wait(1000)
---     end
--- end)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.CreateThread/)
---@param routine fun() @The function to run in the new thread.
---@return integer @The scheduler bookmark id of the new thread.
function CreateThread(routine) end

---
--- Global alias of `Citizen.SetTimeout`: runs `callback` in a new scheduler
--- thread after `msec` milliseconds.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/Citizen.SetTimeout/)
---@param msec integer @Delay in milliseconds.
---@param callback fun() @The function to run.
---@return integer @Timeout id, usable with `ClearTimeout`.
function SetTimeout(msec, callback) end

---
--- Global alias of `Citizen.ClearTimeout`: cancels a pending `SetTimeout` (or a
--- `CreateThread` that has not started yet).
---@param id integer @The id returned by `SetTimeout` or `CreateThread`.
---@return boolean @`true` if a pending timeout was removed.
function ClearTimeout(id) end
