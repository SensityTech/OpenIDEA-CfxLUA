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

-- Cfx.re Lua runtime (client and server): the global `json` library. The
-- runtime uses citizenfx/lua-rapidjson, a dkjson-compatible C implementation
-- (the dkjson 2.5 mentioned by the documentation is not loaded).

---
--- JSON encoder and decoder (a dkjson-compatible implementation based on
--- RapidJSON), available as the global `json` and through `require 'json'`.
---
--- The runtime enables the `empty_table_as_array` and `with_hole` options, so
--- `{}` encodes as `[]` and arrays with a few `nil` holes are still encoded as
--- JSON arrays (with `null` in the holes).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-manual/runtimes/lua/#external-libraries)
---@class json
---@field null function @Sentinel representing a JSON `null` (compare with `==`; `type(json.null)` is `'function'`).
---@field version string @Library version (same as `_VERSION`).
---@field using_lpeg boolean @Always `false` (dkjson compatibility).
---@field _NAME string @Library name.
---@field _VERSION string @Library version.
---@field _COPYRIGHT string @Copyright notice.
---@field _DESCRIPTION string @Library description.
---@field _COMPAT boolean @Whether strict dkjson compatibility is enabled (`false` in the Cfx runtime).
json = {}

---
--- Options accepted by `json.encode` (the same names can be set globally with
--- `json.setoption`). Flags are booleans unless stated otherwise.
---@class JsonEncodeOptions
---@field indent boolean @Pretty-print the output (alias: `pretty`).
---@field pretty boolean @Pretty-print the output (alias: `indent`).
---@field sort_keys boolean @Sort object keys.
---@field null boolean @Use the `json.null` sentinel for JSON `null` values.
---@field nesting boolean @Encode tables nested deeper than `max_depth` as `null` instead of raising an error.
---@field unsigned boolean @Encode integers as unsigned values.
---@field nan boolean @Allow `NaN` and infinite numbers (alias: `inf`).
---@field inf boolean @Allow `NaN` and infinite numbers (alias: `nan`).
---@field bit32 boolean @Encode integers as 32-bit values.
---@field lua_format_float boolean @Format floats with Lua's number formatting.
---@field lua_round_float boolean @Format floats with the shortest round-trip representation.
---@field vectorarray boolean @Encode vectors and quaternions as JSON arrays.
---@field single_line boolean @Keep arrays on a single line when pretty-printing.
---@field empty_table_as_array boolean @Encode empty tables as `[]` (enabled globally by the runtime).
---@field with_hole boolean @Encode sparse arrays as arrays with `null` holes (enabled globally by the runtime).
---@field max_depth integer @Maximum table nesting depth.
---@field decimal_count integer @Maximum number of decimals written for floats.
---@field indent_char integer @Indentation character when pretty-printing: 0 space, 1 tab, 2 newline, 3 carriage return.
---@field indent_count integer @Number of indentation characters per level (alias: `level`).
---@field level integer @Number of indentation characters per level (alias: `indent_count`).
---@field keyorder string[] @Object keys to write first, in this order.
---@field exception function @Handler called when a value cannot be encoded.
local JsonEncodeOptionsType = {}

---
--- Encodes a Lua value as a JSON string. Raises an error if the value cannot
--- be encoded (functions, recursive tables, ...).
---
--- ```lua
--- print(json.encode({ name = 'adder', seats = 2 }))
--- print(json.encode(data, { indent = true }))
--- ```
---@overload fun(value:any):string
---@param value any @The value to encode.
---@param options JsonEncodeOptions|table @Encoding options.
---@return string @The JSON text.
function json.encode(value, options) end

---
--- Decodes a JSON string. On success returns the decoded value and the
--- position after it; on failure returns `nil`, the error offset and an error
--- message. An empty string or `nil` decodes to `nil` with an error message.
---
--- ```lua
--- local data = json.decode(LoadResourceFile(GetCurrentResourceName(), 'data.json'))
--- ```
---@overload fun(text:string|nil):any, integer, string
---@param text string|nil @The JSON text.
---@param position integer @Position to start decoding at (default 1).
---@param nullValue any @Value used for JSON `null` (default `nil`).
---@param objectMeta table @Metatable set on decoded objects.
---@param arrayMeta table @Metatable set on decoded arrays.
---@return any, integer, string @The decoded value and the next position, or `nil`, the error offset and the error message.
function json.decode(text, position, nullValue, objectMeta, arrayMeta) end

---
--- Sets a global encoder/decoder option (see `JsonEncodeOptions` for the
--- names), or the decoder preset with `'decoder_preset'`
--- (`'default'` or `'extended'`).
---@param option string @The option name.
---@param value any @The option value.
function json.setoption(option, value) end

---
--- Returns the current value of a global option.
---@param option string @The option name.
---@return any @The option value.
function json.getoption(option) end

---
--- Marks `t` (or a new table) as a JSON object, so it encodes as `{}` even
--- when empty.
---@overload fun():table
---@param t table @The table to mark.
---@return table @The marked table.
function json.object(t) end

---
--- Marks `t` (or a new table) as a JSON array.
---@overload fun():table
---@param t table @The table to mark.
---@return table @The marked table.
function json.array(t) end

---
--- Returns `true` if `value` is a table marked as a JSON object.
---@param value any @The value to test.
---@return boolean @Whether the value is a JSON object.
function json.isobject(value) end

---
--- Returns `true` if `value` is a table marked as a JSON array.
---@param value any @The value to test.
---@return boolean @Whether the value is a JSON array.
function json.isarray(value) end

---
--- dkjson compatibility only: raises an error in the Cfx runtime.
---@deprecated LPeg is not supported; this function raises an error.
function json.use_lpeg() end
