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

-- Cfx.re Lua runtime (client and server): the global `msgpack` library. The
-- runtime uses citizenfx/lua-cmsgpack, an API-compatible C replacement of
-- lua-MessagePack.

---
--- MessagePack serializer used by the runtime for event arguments, exports
--- and state bags. Available as the global `msgpack` and through
--- `require 'msgpack'`.
---
--- The runtime configures it with `set_string('string_compat')`,
--- `set_integer('unsigned')`, `set_array('without_hole')` and the
--- `empty_table_as_array` option. Vectors, quaternions, function references
--- and the `Entity` / `Player` wrappers have dedicated extension types.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-manual/runtimes/lua/#external-libraries)
---@class msgpack
---@field null function @Sentinel representing an explicit `nil` (alias: `sentinel`).
---@field sentinel function @Sentinel representing an explicit `nil` (alias: `null`).
---@field packers table @Generic packer functions, indexed by type name.
---@field _NAME string @Library name.
---@field _VERSION string @Library version.
---@field _COPYRIGHT string @Copyright notice.
---@field _DESCRIPTION string @Library description.
msgpack = {}

---
--- A persistent packing buffer created by `msgpack.new()`.
---
--- * `packer(...)` (also `packer + value` and `packer << value`) encodes and
---   appends values.
--- * `packer .. encoded` appends already encoded data.
--- * `tostring(packer)` returns the encoded string and `#packer` its length.
---
--- The methods below encode their arguments with an explicit MessagePack type
--- and return the packer, so calls can be chained. The `nil`, `true` and
--- `false` methods, whose names are Lua keywords, are declared under their
--- aliases `null`, `t` and `f`.
---@class MsgpackPacker
local MsgpackPackerType = {}

---
--- Appends the values with automatic type detection (like `packer(...)`).
---@vararg any
---@return MsgpackPacker @This packer.
function MsgpackPackerType:any(...) end

---
--- Appends a MessagePack `nil`.
---@return MsgpackPacker @This packer.
function MsgpackPackerType:null() end

---
--- Appends a MessagePack `true`.
---@return MsgpackPacker @This packer.
function MsgpackPackerType:t() end

---
--- Appends a MessagePack `false`.
---@return MsgpackPacker @This packer.
function MsgpackPackerType:f() end

---
--- Appends the values encoded as booleans.
---@vararg any
---@return MsgpackPacker @This packer.
function MsgpackPackerType:boolean(...) end

---
--- Appends the values encoded as strings, following the configured string
--- option.
---@vararg string
---@return MsgpackPacker @This packer.
function MsgpackPackerType:string(...) end

---
--- Appends the values encoded as MessagePack v5 strings, ignoring the
--- configured string option.
---@vararg string
---@return MsgpackPacker @This packer.
function MsgpackPackerType:_string(...) end

---
--- Appends the values encoded as MessagePack v4 (raw) strings.
---@vararg string
---@return MsgpackPacker @This packer.
function MsgpackPackerType:string_compat(...) end

---
--- Appends the values encoded as binary data.
---@vararg string
---@return MsgpackPacker @This packer.
function MsgpackPackerType:binary(...) end

---
--- Appends the tables encoded as MessagePack maps.
---@vararg table
---@return MsgpackPacker @This packer.
function MsgpackPackerType:map(...) end

---
--- Appends the tables encoded as MessagePack arrays.
---@vararg table
---@return MsgpackPacker @This packer.
function MsgpackPackerType:array(...) end

---
--- Appends the tables encoded as arrays or maps depending on their content,
--- using the registered extensions.
---@vararg table
---@return MsgpackPacker @This packer.
function MsgpackPackerType:table(...) end

---
--- Appends the tables encoded as arrays or maps depending on their content,
--- ignoring the registered extensions.
---@vararg table
---@return MsgpackPacker @This packer.
function MsgpackPackerType:_table(...) end

---
--- Appends the values encoded as integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:integer(...) end

---
--- Appends the values encoded as signed integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:signed(...) end

---
--- Appends the values encoded as unsigned integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:unsigned(...) end

---
--- Appends the values encoded as 32-bit floats.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:float(...) end

---
--- Appends the values encoded as 64-bit floats.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:double(...) end

---
--- Appends the values encoded as numbers (integer or float, following the
--- configured number option).
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:number(...) end

---
--- Appends the values encoded as unsigned 8-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:uint8(...) end

---
--- Appends the values encoded as unsigned 16-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:uint16(...) end

---
--- Appends the values encoded as unsigned 32-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:uint32(...) end

---
--- Appends the values encoded as unsigned 64-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:uint64(...) end

---
--- Appends the values encoded as signed 8-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:int8(...) end

---
--- Appends the values encoded as signed 16-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:int16(...) end

---
--- Appends the values encoded as signed 32-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:int32(...) end

---
--- Appends the values encoded as signed 64-bit integers, in the smallest
--- MessagePack format that holds them.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:int64(...) end

---
--- Appends the values encoded as unsigned 8-bit integers, always in the
--- fixed-size `uint8` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_uint8(...) end

---
--- Appends the values encoded as unsigned 16-bit integers, always in the
--- fixed-size `uint16` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_uint16(...) end

---
--- Appends the values encoded as unsigned 32-bit integers, always in the
--- fixed-size `uint32` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_uint32(...) end

---
--- Appends the values encoded as unsigned 64-bit integers, always in the
--- fixed-size `uint64` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_uint64(...) end

---
--- Appends the values encoded as signed 8-bit integers, always in the
--- fixed-size `int8` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_int8(...) end

---
--- Appends the values encoded as signed 16-bit integers, always in the
--- fixed-size `int16` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_int16(...) end

---
--- Appends the values encoded as signed 32-bit integers, always in the
--- fixed-size `int32` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_int32(...) end

---
--- Appends the values encoded as signed 64-bit integers, always in the
--- fixed-size `int64` format.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:fix_int64(...) end

---
--- Appends the values encoded as C `char` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:char(...) end

---
--- Appends the values encoded as C `signed char` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:signed_char(...) end

---
--- Appends the values encoded as C `unsigned char` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:unsigned_char(...) end

---
--- Appends the values encoded as C `short` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:short(...) end

---
--- Appends the values encoded as C `long` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:long(...) end

---
--- Appends the values encoded as C `long long` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:long_long(...) end

---
--- Appends the values encoded as C `unsigned short` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:unsigned_short(...) end

---
--- Appends the values encoded as C `unsigned int` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:unsigned_int(...) end

---
--- Appends the values encoded as C `unsigned long` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:unsigned_long(...) end

---
--- Appends the values encoded as C `unsigned long long` integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:unsigned_long_long(...) end

---
--- Appends the values encoded as signed 16-bit integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:signed_int16(...) end

---
--- Appends the values encoded as signed 32-bit integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:signed_int32(...) end

---
--- Appends the values encoded as signed 64-bit integers.
---@vararg number
---@return MsgpackPacker @This packer.
function MsgpackPackerType:signed_int64(...) end

---
--- Appends the vectors and quaternions encoded with their MessagePack extension
--- types.
---@vararg vector2|vector3|vector4|quat
---@return MsgpackPacker @This packer.
function MsgpackPackerType:vector(...) end

---
--- Serializes the arguments and returns the concatenated MessagePack data.
---
--- ```lua
--- local data = msgpack.pack({ 1, 2, 3 }, 'text')
--- ```
---@vararg any
---@return string @The encoded data.
function msgpack.pack(...) end

---
--- Serializes the arguments as one MessagePack array, so that
--- `table.unpack(msgpack.unpack(data))` restores them, including `nil`
--- arguments. This is the format of event payloads.
---@vararg any
---@return string @The encoded array.
function msgpack.pack_args(...) end

---
--- Deserializes MessagePack data and returns every decoded value.
---
--- ```lua
--- local t, s = msgpack.unpack(msgpack.pack({ 1, 2 }, 'x'))
--- ```
---@overload fun(data:string):any
---@param data string @The encoded data.
---@param offset integer @Position to start decoding at (default 1).
---@param limit integer @Maximum number of values to decode (default: all).
---@param endPosition integer @Position to stop decoding at.
---@return any @The decoded values.
function msgpack.unpack(data, offset, limit, endPosition) end

---
--- Decodes values one at a time. Returns the position of the next value
--- (`0` when the end of the data is reached) followed by the decoded values.
---
--- ```lua
--- local position, value = 1, nil
--- while position ~= 0 do
---     position, value = msgpack.next(data, position, 1)
--- end
--- ```
---@overload fun(data:string):integer, any
---@param data string @The encoded data.
---@param position integer @Position to start decoding at (default 1).
---@param limit integer @Maximum number of values to decode.
---@param endPosition integer @Position to stop decoding at.
---@return integer, any @The next position (0 at the end), then the decoded values.
function msgpack.next(data, position, limit, endPosition) end

---
--- Creates a persistent packer that accumulates encoded values.
---
--- ```lua
--- local packer = msgpack.new()
--- packer(1, 2, math.pi)
--- packer:float(4.0)
--- local data = tostring(packer)
--- ```
---@return MsgpackPacker @The new packer.
function msgpack.new() end

---
--- Sets a global packing/unpacking option. Number options: `'float'`,
--- `'double'`, `'integer'`, `'unsigned'`; string options: `'string_compat'`,
--- `'string_binary'`; table options: `'always_as_map'`, `'without_hole'`,
--- `'with_hole'`, `'empty_table_as_array'`, `'sentinel'`; type option:
--- `'ignore_invalid'`; lua-MessagePack compatibility options: `'small_lua'`,
--- `'full64bits'`, `'long_double'`.
---@param option string|'"float"'|'"double"'|'"integer"'|'"unsigned"'|'"string_compat"'|'"string_binary"'|'"always_as_map"'|'"without_hole"'|'"with_hole"'|'"empty_table_as_array"'|'"sentinel"'|'"ignore_invalid"'|'"small_lua"'|'"full64bits"'|'"long_double"' @The option name.
---@param value boolean @Whether the option is enabled.
function msgpack.setoption(option, value) end

---
--- Returns the value of a global packing/unpacking option.
---@param option string @The option name (see `msgpack.setoption`).
---@return any @The option value.
function msgpack.getoption(option) end

---
--- Returns `true` when the library was built in "safe" mode (every function
--- wrapped in a protected call).
---@return boolean @Whether safe mode is enabled.
function msgpack.safe() end

---
--- lua-MessagePack compatibility: selects how strings are encoded.
---@param mode string|'"string"'|'"string_compat"'|'"string_binary"' @The string encoding.
function msgpack.set_string(mode) end

---
--- lua-MessagePack compatibility: selects how array-like tables are encoded.
---@param mode string|'"without_hole"'|'"with_hole"'|'"always_as_map"' @The table encoding.
function msgpack.set_array(mode) end

---
--- lua-MessagePack compatibility: selects how integers are encoded.
---@param mode string|'"signed"'|'"unsigned"' @The integer encoding.
function msgpack.set_integer(mode) end

---
--- lua-MessagePack compatibility: selects how floats are encoded.
---@param mode string|'"float"'|'"double"' @The float encoding.
function msgpack.set_number(mode) end

---
--- Registers an extension type. `encoder` is usually a metatable providing
--- `__ext` (the extension id, 0-127), `__pack(self, extId)` returning the
--- encoded payload and `__unpack(payload, extId)` returning the decoded value.
---
--- The Cfx runtime reserves the ids 10 and 11 (function references), 41
--- (`Entity`) and 42 (`Player`).
---@param encoder table @The extension definition.
---@return table @The extension definition.
function msgpack.extend(encoder) end

---
--- Returns the extension definition registered for `extId`.
---@param extId integer @The extension id.
---@return table|nil @The extension definition, or `nil`.
function msgpack.extend_get(extId) end

---
--- Removes the extension definitions of every id given as argument.
---@vararg integer
function msgpack.extend_clear(...) end

---
--- Associates a Lua type name (as returned by `type`) with an extension, so
--- values of that type are packed with it.
---@overload fun(typeName:string)
---@param typeName string @The Lua type name, for example `'function'`.
---@param extension integer|table @An extension id or an extension definition.
function msgpack.settype(typeName, extension) end

---
--- Returns the extension associated with a Lua type name.
---@param typeName string @The Lua type name.
---@return any @The associated extension, or `nil`.
function msgpack.gettype(typeName) end
