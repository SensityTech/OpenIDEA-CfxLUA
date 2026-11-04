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

-- Cfx.re Lua runtime (client and server): CfxLua extensions of the Lua 5.4
-- standard library (citizenfx/lua, GRIT_POWER_* and LUA_C99_MATHLIB build
-- options). The standard tables (`table`, `string`, `utf8`, `math`) are
-- declared by the Lua 5.4 standard library stubs; this file only adds members.
--
-- Differences that cannot be expressed here: `dofile` and `loadfile` are nil,
-- `require` only returns already loaded libraries (plus 'glm' and 'lmprof'),
-- `print` prefixes its output with the resource name, and `io` / `os` only
-- exist on the server (see server/server_os_io.lua).

---
--- Computes the Jenkins one-at-a-time hash of `text`, as used by the game for
--- model names and other identifiers. The text is lowercased first unless
--- `keepCase` is `true`. The result is a signed 32-bit integer.
---
--- The backtick literal `` `adder` `` computes the same hash at compile time.
---
--- ```lua
--- print(joaat('adder') == `adder`)
--- ```
---@overload fun(text:string):integer
---@param text string @The text to hash.
---@param keepCase boolean @`true` to hash the text without lowercasing it.
---@return integer @The signed 32-bit hash.
function joaat(text, keepCase) end

---
--- Generic iteration: returns the iterator of `t` given by its `__iter`
--- metamethod, falling back to `__pairs` and then to `next` (like `pairs`).
---
--- ```lua
--- for key, value in each(t) do
---     print(key, value)
--- end
--- ```
---@generic K, V
---@param t table<K, V>|V[] @The value to iterate.
---@return fun(tbl: table<K, V>):K, V @The iterator function.
function each(t) end

---
--- Returns a to-be-closed object that calls `callback` when it goes out of
--- scope (normally or because of an error).
---
--- ```lua
--- local _ <close> = defer(function()
---     print('leaving the block')
--- end)
--- ```
---@param callback fun(closed:table, err:any) @Called when the object is closed, with the error object if the scope is left by an error.
---@return table @The to-be-closed object.
function defer(callback) end

---
--- Returns its arguments, with every table, function, userdata and thread
--- replaced by `nil` (only numbers, strings, booleans and `nil` are kept).
---@vararg any
---@return any @The scrubbed arguments.
function scrub(...) end

---
--- Returns the layout of table `t`: `'empty'`, `'array'`, `'hash'` or
--- `'mixed'`. Returns `nil` for values that are not tables.
---@param t any @The value to inspect.
---@return string|nil @The table type.
function table.type(t) end

---
--- Creates a table with preallocated space for `narray` array elements and
--- `nhash` hash elements (`lua_createtable`).
---@param narray integer @Number of array slots to preallocate.
---@param nhash integer @Number of hash slots to preallocate.
---@return table @The new table.
function table.create(narray, nhash) end

---
--- Alias of `table.create`: creates a table with preallocated space.
---@param narray integer @Number of array slots to preallocate.
---@param nhash integer @Number of hash slots to preallocate.
---@return table @The new table.
function table.new(narray, nhash) end

---
--- Removes every entry of `t`, keeping its allocated space, and returns it.
---@generic T
---@param t T @The table to clear.
---@return T @The same table.
function table.wipe(t) end

---
--- Alias of `table.wipe`: removes every entry of `t` and returns it.
---@generic T
---@param t T @The table to clear.
---@return T @The same table.
function table.clear(t) end

---
--- Returns a shallow copy of `t` (without its metatable). When `destination`
--- is given, its previous content is replaced by the copy and it is returned
--- (it keeps its own metatable).
---@overload fun(t:table):table
---@generic T
---@param t T @The table to copy.
---@param destination table @Optional table receiving the copy.
---@return T @The copy.
function table.clone(t, destination) end

---
--- Removes the characters of `chars` (by default the whitespace characters
--- `"\t\n\v\f\r "`) from both ends of `s`.
---
--- ```lua
--- print(string.strtrim('  hello  ')) -- 'hello'
--- ```
---@overload fun(s:string):string
---@param s string @The string to trim.
---@param chars string @The set of characters to remove.
---@return string @The trimmed string.
function string.strtrim(s, chars) end

---
--- Splits `s` at every character contained in `delimiters` and returns the
--- pieces as multiple values. With `limit`, at most `limit` pieces are
--- returned (the last one holds the rest of the string).
---
--- ```lua
--- local a, b, c = string.strsplit(',', 'one,two,three')
--- ```
---@overload fun(delimiters:string, s:string):string
---@param delimiters string @The set of delimiter characters.
---@param s string @The string to split.
---@param limit integer @Maximum number of pieces (0 for no limit).
---@return string @The pieces, as multiple return values.
function string.strsplit(delimiters, s, limit) end

---
--- Joins the remaining arguments with `delimiter` between them.
---
--- ```lua
--- print(string.strjoin(', ', 'a', 'b', 'c')) -- 'a, b, c'
--- ```
---@param delimiter string @The separator.
---@vararg string|number
---@return string @The joined string.
function string.strjoin(delimiter, ...) end

---
--- Concatenates all the arguments (like the `..` operator).
---@vararg string|number
---@return string @The concatenated string.
function string.strconcat(...) end

---
--- Returns all its arguments converted with `tostring`.
---@vararg any
---@return string @The converted arguments, as multiple return values.
function string.tostringall(...) end

---
--- Creates a blob: a mutable, non-interned byte string. A number creates a
--- zero-filled blob of that size; a string is copied into a new blob.
---@param value integer|string @The blob size, or the string to copy.
---@return string @The blob.
function string.blob(value) end

---
--- Returns `true` if `value` is a blob created by `string.blob`.
---@param value any @The value to test.
---@return boolean @Whether the value is a blob.
function string.isblob(value) end

---
--- Writes values into `blob` like `string.pack`, starting at `position`
--- (default 1). Returns the blob, or a new larger blob if the data does not
--- fit.
---@overload fun(blob:string, format:string, vararg any):string
---@param blob string @The blob to write into.
---@param position integer @The position to start writing at.
---@param format string @A `string.pack` format.
---@vararg any
---@return string @The written blob.
function string.blob_pack(blob, position, format, ...) end

---
--- Reads values from `blob` like `string.unpack`, starting at `position`
--- (default 1). Returns the values followed by the position after them.
---@overload fun(blob:string, format:string):any
---@param blob string @The blob to read from.
---@param position integer @The position to start reading at.
---@param format string @A `string.unpack` format.
---@return any @The read values, then the next position.
function string.blob_unpack(blob, position, format) end

---
--- Alias of `utf8.len`: returns the number of UTF-8 characters in `s`.
---@overload fun(s:string):integer
---@param s string @The string.
---@param i integer @Start position (default 1).
---@param j integer @End position (default -1).
---@return integer @The number of characters, or `nil` plus the position of the first invalid byte.
function utf8.strlenutf8(s, i, j) end

---
--- Compares two UTF-8 strings by code point (case-sensitive, despite the
--- name). Returns `-1`, `0` or `1`.
---@param a string @First string.
---@param b string @Second string.
---@return integer @The comparison result.
function utf8.strcmputf8i(a, b) end

---
--- Returns the number of UTF-16 code units needed to encode the UTF-8 string
--- `s` (characters outside the BMP count twice), or `-1` if `s` is not valid
--- UTF-8.
---@overload fun(s:string):integer
---@param s string @The string.
---@param i integer @Start position (default 1).
---@param j integer @End position (default -1).
---@return integer @The UTF-16 length.
function utf8.len16(s, i, j) end

---
--- Returns the arc tangent of `y / x` (in radians), using the signs of both
--- arguments to find the quadrant (Lua 5.3 compatibility).
---@param y number @The y coordinate.
---@param x number @The x coordinate.
---@return number @The angle in radians.
function math.atan2(y, x) end

---
--- Returns the hyperbolic cosine of `x` (Lua 5.3 compatibility).
---@param x number @The value.
---@return number @The result.
function math.cosh(x) end

---
--- Returns the hyperbolic sine of `x` (Lua 5.3 compatibility).
---@param x number @The value.
---@return number @The result.
function math.sinh(x) end

---
--- Returns the hyperbolic tangent of `x` (Lua 5.3 compatibility).
---@param x number @The value.
---@return number @The result.
function math.tanh(x) end

---
--- Returns `x` raised to the power `y` (Lua 5.3 compatibility; same as
--- `x ^ y`).
---@param x number @The base.
---@param y number @The exponent.
---@return number @The result.
function math.pow(x, y) end

---
--- Splits `x` into a normalized fraction and an exponent such that
--- `x = m * 2 ^ e` (Lua 5.3 compatibility).
---@param x number @The value.
---@return number, integer @The fraction `m` and the exponent `e`.
function math.frexp(x) end

---
--- Returns `m * 2 ^ e` (Lua 5.3 compatibility).
---@param m number @The fraction.
---@param e integer @The exponent.
---@return number @The result.
function math.ldexp(m, e) end

---
--- Returns the base-10 logarithm of `x` (Lua 5.3 compatibility).
---@param x number @The value.
---@return number @The result.
function math.log10(x) end

---
--- Returns the inverse hyperbolic sine of `x`.
---@param x number @The value.
---@return number @The result.
function math.asinh(x) end

---
--- Returns the inverse hyperbolic cosine of `x`.
---@param x number @The value.
---@return number @The result.
function math.acosh(x) end

---
--- Returns the inverse hyperbolic tangent of `x`.
---@param x number @The value.
---@return number @The result.
function math.atanh(x) end

---
--- Returns the cube root of `x`.
---@param x number @The value.
---@return number @The result.
function math.cbrt(x) end

---
--- Returns a value with the magnitude of `x` and the sign of `y`.
---@param x number @The magnitude.
---@param y number @The sign source.
---@return number @The result.
function math.copysign(x, y) end

---
--- Returns the error function of `x`.
---@param x number @The value.
---@return number @The result.
function math.erf(x) end

---
--- Returns the complementary error function of `x`.
---@param x number @The value.
---@return number @The result.
function math.erfc(x) end

---
--- Returns `2 ^ x`.
---@param x number @The exponent.
---@return number @The result.
function math.exp2(x) end

---
--- Returns `e ^ x - 1`, accurate for small `x`.
---@param x number @The exponent.
---@return number @The result.
function math.expm1(x) end

---
--- Returns the positive difference between `x` and `y` (`x - y` if positive,
--- `0` otherwise).
---@param x number @First value.
---@param y number @Second value.
---@return number @The result.
function math.fdim(x, y) end

---
--- Returns the gamma function of `x`.
---@param x number @The value.
---@return number @The result.
function math.gamma(x) end

---
--- Returns `sqrt(x * x + y * y)` without intermediate overflow.
---@param x number @First value.
---@param y number @Second value.
---@return number @The result.
function math.hypot(x, y) end

---
--- Returns `true` if `x` is neither infinite nor NaN.
---@param x number @The value.
---@return boolean @Whether `x` is finite.
function math.isfinite(x) end

---
--- Returns `true` if `x` is positive or negative infinity.
---@param x number @The value.
---@return boolean @Whether `x` is infinite.
function math.isinf(x) end

---
--- Returns `true` if `x` is NaN.
---@param x number @The value.
---@return boolean @Whether `x` is NaN.
function math.isnan(x) end

---
--- Returns `true` if `x` is a normal floating-point number (not zero,
--- subnormal, infinite or NaN).
---@param x number @The value.
---@return boolean @Whether `x` is normal.
function math.isnormal(x) end

---
--- Returns the natural logarithm of the absolute value of the gamma function
--- of `x`.
---@param x number @The value.
---@return number @The result.
function math.lgamma(x) end

---
--- Returns `log(1 + x)`, accurate for small `x`.
---@param x number @The value.
---@return number @The result.
function math.log1p(x) end

---
--- Returns the binary exponent of `x`.
---@param x number @The value.
---@return number @The result.
function math.logb(x) end

---
--- Rounds `x` to an integral value using the current rounding mode.
---@param x number @The value.
---@return number @The result.
function math.nearbyint(x) end

---
--- Returns the next representable floating-point value after `x` in the
--- direction of `y`.
---@param x number @The start value.
---@param y number @The direction.
---@return number @The result.
function math.nextafter(x, y) end

---
--- Returns the IEEE remainder of `x / y`.
---@param x number @The dividend.
---@param y number @The divisor.
---@return number @The result.
function math.remainder(x, y) end

---
--- Rounds `x` to the nearest integral value, halfway cases away from zero.
--- The result is a float.
---@param x number @The value.
---@return number @The rounded value.
function math.round(x) end

---
--- Returns `x * 2 ^ n`.
---@param x number @The value.
---@param n integer @The exponent.
---@return number @The result.
function math.scalbn(x, n) end

---
--- Truncates `x` towards zero. The result is a float.
---@param x number @The value.
---@return number @The truncated value.
function math.trunc(x) end

---
--- Clamps `x` between `min` and `max`. The three arguments must have the same
--- type: all integers, all floats, or vectors of the same size. Mixing
--- integers and floats, or a vector and numbers, raises an error. Numbers are
--- returned as floats.
---
--- ```lua
--- local alpha = math.clamp(value, 0.0, 1.0)
--- ```
---@param x number|vector2|vector3|vector4 @The value to clamp.
---@param min number|vector2|vector3|vector4 @The lower bound.
---@param max number|vector2|vector3|vector4 @The upper bound.
---@return number|vector2|vector3|vector4 @The clamped value.
function math.clamp(x, min, max) end

---
--- The machine epsilon of Lua floats (`DBL_EPSILON`).
---@type number
math.eps = nil

---
--- The machine epsilon of single-precision floats (`FLT_EPSILON`), the
--- precision of vector components.
---@type number
math.feps = nil
