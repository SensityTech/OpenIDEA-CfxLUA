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

-- Cfx.re Lua runtime, server only: additions to the `os` and `io` libraries
-- (LuaOS.cpp / LuaIO.cpp). Both libraries are nil on the client. The server
-- versions have no `os.exit`, `io.read`, `io.input` or `io.output`.

---
--- Creates the directory `path`. Returns `true` on success, or `nil`, an
--- error message and an error code on failure (for example when the directory
--- already exists).
---@param path string @The directory to create.
---@return boolean|nil, string, integer @`true`, or `nil` plus the error message and code.
function os.createdir(path) end

---
--- Returns the absolute difference between two timestamps returned by
--- `os.microtime`, `os.nanotime`, `os.rdtsc` or `os.rdtscp`.
---@param a integer @First timestamp.
---@param b integer @Second timestamp.
---@return integer @The absolute difference.
function os.deltatime(a, b) end

---
--- Returns a high-resolution timestamp in microseconds.
---@return integer @The timestamp.
function os.microtime() end

---
--- Returns a high-resolution timestamp in nanoseconds.
---@return integer @The timestamp.
function os.nanotime() end

---
--- Returns the processor time-stamp counter (`rdtsc` instruction).
---@return integer @The counter value.
function os.rdtsc() end

---
--- Returns the processor time-stamp counter (`rdtscp` instruction).
---@return integer @The counter value.
function os.rdtscp() end

---
--- Directory handle returned by `io.readdir`.
---@class CfxDirectory
local CfxDirectoryType = {}

---
--- Returns an iterator over the names of the directory entries.
---
--- ```lua
--- local dir = io.readdir(GetResourcePath(GetCurrentResourceName()))
--- for name in dir:lines() do
---     print(name)
--- end
--- dir:close()
--- ```
---@return fun():string @The iterator.
function CfxDirectoryType:lines() end

---
--- Closes the directory handle.
---@return boolean @`true`.
function CfxDirectoryType:close() end

---
--- Opens the directory `path` for listing.
---@param path string @The directory to list.
---@return CfxDirectory|nil, string @The directory handle, or `nil` and an error message.
function io.readdir(path) end
