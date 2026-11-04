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

-- Cfx.re Lua runtime (client and server): CfxLua (LuaGLM) vector, quaternion
-- and matrix value types, their global constructors and the grit-compatible
-- helpers registered in the base library (lbaselib.c / lglm.cpp of
-- citizenfx/lua). The full `glm` library is only available through
-- `require 'glm'` and is not described here.

---
--- A 2-component vector. Vectors are immutable value types: `type(v)` is
--- `'vector2'` and assigning a component (`v.x = 1`) raises an error; build a
--- new vector instead (`v = v + vector2(1, 0)`).
---
--- * Components: `v.x`, `v.y`, the aliases `v.r`, `v.g`, or `v[1]`, `v[2]`.
--- * Swizzles: any sequence of 2 to 4 of the letters `x` and `y` (`v.yx`,
---   `v.xyxy`, ...) returns a vector of that size.
--- * `#v` is the magnitude (length) of the vector, not its component count.
--- * Arithmetic (`+ - * / // % ^`, unary `-`) works component-wise with other
---   vectors and with numbers. `==` compares components; `<` and `<=` raise
---   an error.
--- * `..` converts the vector to a string, like a number
---   (`'pos: ' .. v`).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector2/)
---@class vector2
---@field x number @First component.
---@field y number @Second component.
---@field r number @Alias of `x`.
---@field g number @Alias of `y`.
---@field n integer @Number of components (2).
---@field dim integer @Number of components (2).
---@field xy vector2 @Swizzle: `vector2(v.x, v.y)`.
local Vector2Type = {}

---
--- A 3-component vector, used for world positions, rotations and directions.
--- Vectors are immutable value types: `type(v)` is `'vector3'` and assigning a
--- component (`v.x = 1`) raises an error; build a new vector instead
--- (`coords = coords + vector3(0.0, 0.0, 1.0)`).
---
--- * Components: `v.x`, `v.y`, `v.z`, the aliases `v.r`, `v.g`, `v.b`, or
---   `v[1]` to `v[3]`.
--- * Swizzles: any sequence of 2 to 4 of the letters `x`, `y` and `z`
---   (`v.xy`, `v.zyx`, ...) returns a vector of that size.
--- * `#v` is the magnitude (length) of the vector, so `#(a - b)` is the
---   distance between two points.
--- * Arithmetic (`+ - * / // % ^`, unary `-`) works component-wise with other
---   vectors and with numbers. `==` compares components; `<` and `<=` raise
---   an error.
--- * `..` converts the vector to a string, like a number.
--- * A `vector3` passed to a native is expanded into its `x, y, z` arguments
---   (except with `use_experimental_fxv2_oal` in the manifest).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector3/)
---@class vector3
---@field x number @First component.
---@field y number @Second component.
---@field z number @Third component.
---@field r number @Alias of `x`.
---@field g number @Alias of `y`.
---@field b number @Alias of `z`.
---@field n integer @Number of components (3).
---@field dim integer @Number of components (3).
---@field xy vector2 @Swizzle: `vector2(v.x, v.y)`.
---@field xz vector2 @Swizzle: `vector2(v.x, v.z)`.
---@field yz vector2 @Swizzle: `vector2(v.y, v.z)`.
---@field xyz vector3 @Swizzle: `vector3(v.x, v.y, v.z)`.
local Vector3Type = {}

---
--- A 4-component vector, commonly used for a position plus a heading
--- (`w`). Vectors are immutable value types: `type(v)` is `'vector4'` and
--- assigning a component raises an error.
---
--- * Components: `v.x`, `v.y`, `v.z`, `v.w`, the aliases `v.r`, `v.g`, `v.b`,
---   `v.a`, or `v[1]` to `v[4]`.
--- * Swizzles: any sequence of 2 to 4 of the letters `x`, `y`, `z` and `w`
---   (`v.xyz`, `v.wzyx`, ...) returns a vector of that size.
--- * `#v` is the magnitude (length) of the vector.
--- * Arithmetic (`+ - * / // % ^`, unary `-`) works component-wise with other
---   vectors and with numbers. `==` compares components; `<` and `<=` raise
---   an error.
--- * `..` converts the vector to a string, like a number.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector4/)
---@class vector4
---@field x number @First component.
---@field y number @Second component.
---@field z number @Third component.
---@field w number @Fourth component.
---@field r number @Alias of `x`.
---@field g number @Alias of `y`.
---@field b number @Alias of `z`.
---@field a number @Alias of `w`.
---@field n integer @Number of components (4).
---@field dim integer @Number of components (4).
---@field xy vector2 @Swizzle: `vector2(v.x, v.y)`.
---@field xz vector2 @Swizzle: `vector2(v.x, v.z)`.
---@field xw vector2 @Swizzle: `vector2(v.x, v.w)`.
---@field yz vector2 @Swizzle: `vector2(v.y, v.z)`.
---@field yw vector2 @Swizzle: `vector2(v.y, v.w)`.
---@field zw vector2 @Swizzle: `vector2(v.z, v.w)`.
---@field xyz vector3 @Swizzle: `vector3(v.x, v.y, v.z)`, for example the position part of a position + heading.
---@field xyw vector3 @Swizzle: `vector3(v.x, v.y, v.w)`.
---@field xzw vector3 @Swizzle: `vector3(v.x, v.z, v.w)`.
---@field yzw vector3 @Swizzle: `vector3(v.y, v.z, v.w)`.
---@field xyzw vector4 @Swizzle: `vector4(v.x, v.y, v.z, v.w)`.
local Vector4Type = {}

---
--- A quaternion (rotation). Quaternions are immutable value types:
--- `type(q)` is `'quat'`.
---
--- * Components: `q.x`, `q.y`, `q.z`, `q.w`, or `q[1]` to `q[4]`; swizzles
---   such as `q.xyz` (the vector part) work as on vectors.
--- * `q.angle` (degrees) and `q.axis` give the rotation as an angle and an
---   axis; they are not available after `require 'glm'`.
--- * `q * v` rotates the `vector3` `v`; `q1 * q2` composes rotations.
--- * `#q` is the magnitude of the quaternion (1 for a rotation).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/quat/)
---@class quat
---@field x number @X component of the vector part.
---@field y number @Y component of the vector part.
---@field z number @Z component of the vector part.
---@field w number @Scalar part.
---@field n integer @Number of components (4).
---@field dim integer @Number of components (4).
---@field angle number @Rotation angle in degrees.
---@field axis vector3 @Rotation axis.
---@field xyz vector3 @Swizzle: the vector part, `vector3(q.x, q.y, q.z)`.
local QuatType = {}

---
--- A matrix of 2 to 4 columns and rows (`type(m)` is `'matrix'`). Unlike
--- vectors, matrices are mutable reference values. `m[i]` is the column `i` as
--- a vector (and can be assigned), `#m` is the number of columns, `m * v`
--- transforms a vector and `m1 * m2` multiplies matrices.
---@class matrix
local MatrixType = {}

---
--- Creates a `vector2`. A single number is copied to both components; a
--- larger vector is truncated to its first two components.
---
--- ```lua
--- local v = vector2(1.0, 2.0)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector2/)
---@overload fun(value:number):vector2
---@overload fun(v:vector2|vector3|vector4):vector2
---@param x number @First component.
---@param y number @Second component.
---@return vector2 @The new vector.
function vector2(x, y) end

---
--- Alias of `vector2`: creates a `vector2`.
---@overload fun(value:number):vector2
---@overload fun(v:vector2|vector3|vector4):vector2
---@param x number @First component.
---@param y number @Second component.
---@return vector2 @The new vector.
function vec2(x, y) end

---
--- Creates a `vector3` from 3 components. A single number is copied to every
--- component, vector arguments are expanded into their components, and a
--- single `vector4` is truncated to its first three components.
---
--- ```lua
--- local coords = vector3(-542.9, -208.3, 37.6)
--- local up = vector3(coords.xy, coords.z + 1.0)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector3/)
---@overload fun(value:number):vector3
---@overload fun(xy:vector2, z:number):vector3
---@overload fun(x:number, yz:vector2):vector3
---@overload fun(v:vector3|vector4):vector3
---@param x number @First component.
---@param y number @Second component.
---@param z number @Third component.
---@return vector3 @The new vector.
function vector3(x, y, z) end

---
--- Alias of `vector3`: creates a `vector3`.
---@overload fun(value:number):vector3
---@overload fun(xy:vector2, z:number):vector3
---@overload fun(x:number, yz:vector2):vector3
---@overload fun(v:vector3|vector4):vector3
---@param x number @First component.
---@param y number @Second component.
---@param z number @Third component.
---@return vector3 @The new vector.
function vec3(x, y, z) end

---
--- Creates a `vector4` from 4 components. A single number is copied to every
--- component and vector or quaternion arguments are expanded into their
--- components.
---
--- ```lua
--- local spawn = vector4(-542.9, -208.3, 37.6, 90.0)
--- local spawn2 = vector4(GetEntityCoords(PlayerPedId()), GetEntityHeading(PlayerPedId()))
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector4/)
---@overload fun(value:number):vector4
---@overload fun(xyz:vector3, w:number):vector4
---@overload fun(xy:vector2, zw:vector2):vector4
---@overload fun(xy:vector2, z:number, w:number):vector4
---@overload fun(x:number, yzw:vector3):vector4
---@overload fun(x:number, yz:vector2, w:number):vector4
---@overload fun(x:number, y:number, zw:vector2):vector4
---@overload fun(v:vector4|quat):vector4
---@param x number @First component.
---@param y number @Second component.
---@param z number @Third component.
---@param w number @Fourth component.
---@return vector4 @The new vector.
function vector4(x, y, z, w) end

---
--- Alias of `vector4`: creates a `vector4`.
---@overload fun(value:number):vector4
---@overload fun(xyz:vector3, w:number):vector4
---@overload fun(xy:vector2, zw:vector2):vector4
---@overload fun(xy:vector2, z:number, w:number):vector4
---@overload fun(x:number, yzw:vector3):vector4
---@overload fun(x:number, yz:vector2, w:number):vector4
---@overload fun(x:number, y:number, zw:vector2):vector4
---@overload fun(v:vector4|quat):vector4
---@param x number @First component.
---@param y number @Second component.
---@param z number @Third component.
---@param w number @Fourth component.
---@return vector4 @The new vector.
function vec4(x, y, z, w) end

---
--- Creates a 1-component vector, which is a plain number.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector1/)
---@param x number @The value.
---@return number @The value as a float.
function vector1(x) end

---
--- Alias of `vector1`: returns the value as a plain number.
---@param x number @The value.
---@return number @The value as a float.
function vec1(x) end

---
--- Creates a vector whose size is the total number of components given:
--- numbers count for one component, vectors and quaternions are expanded.
--- One component gives a number, 2 to 4 give a `vector2`, `vector3` or
--- `vector4`.
---
--- ```lua
--- local v = vector(1, 2, 3) -- vector3
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/vector/)
---@vararg number|vector2|vector3|vector4|quat
---@return number|vector2|vector3|vector4 @The new vector.
function vector(...) end

---
--- Alias of `vector`: creates a vector sized by its number of components.
---@vararg number|vector2|vector3|vector4|quat
---@return number|vector2|vector3|vector4 @The new vector.
function vec(...) end

---
--- Like `vector`, but every component is truncated to an integer (the vector
--- still stores floats).
---@vararg number|vector2|vector3|vector4|quat
---@return number|vector2|vector3|vector4 @The new vector.
function ivec(...) end

---
--- Like `vector1`, but the value is truncated to an integer.
---@param x number @The value.
---@return number @The truncated value.
function ivec1(x) end

---
--- Like `vector2`, but every component is truncated to an integer.
---@overload fun(value:number):vector2
---@param x number @First component.
---@param y number @Second component.
---@return vector2 @The new vector.
function ivec2(x, y) end

---
--- Like `vector3`, but every component is truncated to an integer.
---@overload fun(value:number):vector3
---@param x number @First component.
---@param y number @Second component.
---@param z number @Third component.
---@return vector3 @The new vector.
function ivec3(x, y, z) end

---
--- Like `vector4`, but every component is truncated to an integer.
---@overload fun(value:number):vector4
---@param x number @First component.
---@param y number @Second component.
---@param z number @Third component.
---@param w number @Fourth component.
---@return vector4 @The new vector.
function ivec4(x, y, z, w) end

---
--- Like `vector`, but every component is converted to a boolean (stored as
--- `1` or `0`).
---@vararg any
---@return number|vector2|vector3|vector4 @The new vector.
function bvec(...) end

---
--- Like `vector1`, but the value is converted to a boolean (`1` or `0`).
---@param x any @The value.
---@return number @`1` or `0`.
function bvec1(x) end

---
--- Like `vector2`, but every component is converted to a boolean (`1` or `0`).
---@overload fun(value:any):vector2
---@param x any @First component.
---@param y any @Second component.
---@return vector2 @The new vector.
function bvec2(x, y) end

---
--- Like `vector3`, but every component is converted to a boolean (`1` or `0`).
---@overload fun(value:any):vector3
---@param x any @First component.
---@param y any @Second component.
---@param z any @Third component.
---@return vector3 @The new vector.
function bvec3(x, y, z) end

---
--- Like `vector4`, but every component is converted to a boolean (`1` or `0`).
---@overload fun(value:any):vector4
---@param x any @First component.
---@param y any @Second component.
---@param z any @Third component.
---@param w any @Fourth component.
---@return vector4 @The new vector.
function bvec4(x, y, z, w) end

---
--- Creates a quaternion.
---
--- * `quat()`: the identity rotation.
--- * `quat(w, x, y, z)`: from components (note that `w` comes first).
--- * `quat(xyz, w)`: from the vector part and the scalar part.
--- * `quat(angle, axis)`: rotation of `angle` degrees around `axis`.
--- * `quat(from, to)`: shortest rotation from direction `from` to `to`.
--- * `quat(q)`: a copy of `q`.
--- * `quat(m)`: from a 3x3 or 4x4 rotation matrix.
---
--- ```lua
--- local rotation = quat(90.0, vector3(0.0, 0.0, 1.0))
--- local rotated = rotation * vector3(1.0, 0.0, 0.0)
--- ```
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/quat/)
---@overload fun():quat
---@overload fun(xyz:vector3, w:number):quat
---@overload fun(angle:number, axis:vector3):quat
---@overload fun(from:vector3, to:vector3):quat
---@overload fun(q:quat):quat
---@overload fun(m:matrix):quat
---@param w number @Scalar part.
---@param x number @X component of the vector part.
---@param y number @Y component of the vector part.
---@param z number @Z component of the vector part.
---@return quat @The new quaternion.
function quat(w, x, y, z) end

---
--- Alias of `quat`: creates a quaternion.
---@overload fun():quat
---@overload fun(xyz:vector3, w:number):quat
---@overload fun(angle:number, axis:vector3):quat
---@overload fun(from:vector3, to:vector3):quat
---@overload fun(q:quat):quat
---@overload fun(m:matrix):quat
---@param w number @Scalar part.
---@param x number @X component of the vector part.
---@param y number @Y component of the vector part.
---@param z number @Z component of the vector part.
---@return quat @The new quaternion.
function qua(w, x, y, z) end

---
--- Creates a matrix. A single number fills the diagonal, a quaternion gives
--- its rotation matrix, a matrix is resized, and otherwise each argument is a
--- column vector (the size is inferred from the arguments).
---@vararg number|vector2|vector3|vector4|quat|matrix
---@return matrix @The new matrix.
function mat(...) end

---
--- Creates a 2x2 matrix (see `mat` for the accepted arguments).
---@vararg number|vector2|quat|matrix
---@return matrix @The new matrix.
function mat2x2(...) end

---
--- Alias of `mat2x2`: creates a 2x2 matrix.
---@vararg number|vector2|quat|matrix
---@return matrix @The new matrix.
function mat2(...) end

---
--- Creates a matrix of 2 columns and 3 rows (see `mat`).
---@vararg number|vector3|quat|matrix
---@return matrix @The new matrix.
function mat2x3(...) end

---
--- Creates a matrix of 2 columns and 4 rows (see `mat`).
---@vararg number|vector4|quat|matrix
---@return matrix @The new matrix.
function mat2x4(...) end

---
--- Creates a matrix of 3 columns and 2 rows (see `mat`).
---@vararg number|vector2|quat|matrix
---@return matrix @The new matrix.
function mat3x2(...) end

---
--- Creates a 3x3 matrix (see `mat` for the accepted arguments).
---@vararg number|vector3|quat|matrix
---@return matrix @The new matrix.
function mat3x3(...) end

---
--- Alias of `mat3x3`: creates a 3x3 matrix.
---@vararg number|vector3|quat|matrix
---@return matrix @The new matrix.
function mat3(...) end

---
--- Creates a matrix of 3 columns and 4 rows (see `mat`).
---@vararg number|vector4|quat|matrix
---@return matrix @The new matrix.
function mat3x4(...) end

---
--- Creates a matrix of 4 columns and 2 rows (see `mat`).
---@vararg number|vector2|quat|matrix
---@return matrix @The new matrix.
function mat4x2(...) end

---
--- Creates a matrix of 4 columns and 3 rows (see `mat`).
---@vararg number|vector3|quat|matrix
---@return matrix @The new matrix.
function mat4x3(...) end

---
--- Creates a 4x4 matrix (see `mat` for the accepted arguments).
---@vararg number|vector4|quat|matrix
---@return matrix @The new matrix.
function mat4x4(...) end

---
--- Alias of `mat4x4`: creates a 4x4 matrix.
---@vararg number|vector4|quat|matrix
---@return matrix @The new matrix.
function mat4(...) end

---
--- Returns the dot product of two numbers, vectors of the same size or
--- quaternions.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/dot/)
---@param a number|vector2|vector3|vector4|quat @First operand.
---@param b number|vector2|vector3|vector4|quat @Second operand, of the same type.
---@return number @The dot product.
function dot(a, b) end

---
--- Returns the cross product: a number for two `vector2`, a `vector3` for two
--- `vector3` (or a `vector3` and a `quat`), a `quat` for two quaternions.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/cross/)
---@param a vector2|vector3|quat @First operand.
---@param b vector2|vector3|quat @Second operand.
---@return number|vector3|quat @The cross product.
function cross(a, b) end

---
--- Returns the inverse of a quaternion or of a square matrix.
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/inv/)
---@generic T
---@param value T @A `quat` or a square `matrix`.
---@return T @The inverse.
function inv(value) end

---
--- Returns the normalized vector or quaternion (same direction, length 1).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/norm/)
---@generic T
---@param value T @A vector or a `quat`.
---@return T @The normalized value.
function norm(value) end

---
--- Spherical linear interpolation between two vectors of the same size, or
--- two quaternions, by the factor `t` (0 to 1).
---
--- [Documentation](https://docs.fivem.net/docs/scripting-reference/runtimes/lua/functions/slerp/)
---@generic T
---@param a T @The start value (a vector or a `quat`).
---@param b T @The end value, of the same type.
---@param t number @The interpolation factor.
---@return T @The interpolated value.
function slerp(a, b, t) end
