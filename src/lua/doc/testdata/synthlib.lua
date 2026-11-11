---@meta synthlib
-- Synthetic library annotated with LuaCATS (the LuaLS dialect), written for the CfxLua tests in the style of the
-- annotated FiveM frameworks. It is not a copy of any of them.

---@diagnostic disable: missing-return, unused-local

---@alias SynthPosition 'top' | 'top-right' | 'top-left' | 'bottom' | "bottom-right" | "bottom-left"

---Kind of notification.
---@alias SynthNotifyType
---| 'inform' # Informational (blue)
---| 'error' # Something went wrong (red)
---| 'success' # Done (green)
---|>'warning' # The default type

---@alias SynthId integer | string

---@class SynthNotifyProps
---@field id? SynthId Notifications with the same id replace each other
---@field title? string
---@field description? string
---@field duration? integer Milliseconds (default 3000)
---@field position? SynthPosition
---@field type? SynthNotifyType
---@field icon? string | [string, string] Icon name, or its prefix and name
---@field style? { [string]: string | number }

---@class (exact) SynthPoint
---@field coords vector3
---@field distance number
---@field onEnter? fun(self: SynthPoint)

---@class SynthEvents
---@field on fun(key: string, cb: fun(value: any, oldValue: any))

---@class SynthTimers
---@field setTimeout fun(ms: integer, cb: function): integer

---@class SynthLib: SynthEvents, SynthTimers
lib = {}

---@class SynthStore
---@field [string] SynthPoint
---@field count integer

---@enum SynthKeybindMode
SynthKeybindMode = {
    PRESS = 0,
    HOLD = 1,
    TOGGLE = 2,
}

---@enum (key) SynthWeather
SynthWeather = {
    CLEAR = 'Clear sky',
    RAIN = 'Rain',
}

---Shows a notification.
---@param data SynthNotifyProps
function lib.notify(data) end

---Loads a model and waits until it is available.
---@async
---@param model string | number
---@param timeout? integer Milliseconds before giving up (default 10000)
---@return number model The model hash
function lib.requestModel(model, timeout) end

---@nodiscard
---@param name string
---@return boolean ok
---@return string? err Why it failed
function lib.checkDependency(name) end

---@param text string
---@param position? SynthPosition
---@param options table?
function lib.showTextUI(text, position, options) end

---@param mode SynthKeybindMode
---@param weather SynthWeather
function lib.setMode(mode, weather) end

---@param kind
---| 'ped' # A pedestrian
---| 'vehicle' # A vehicle
---| 'object'
---@return integer count, integer[] handles
function lib.count(kind) end

---@param ... string
---@return string joined
function lib.join(...) end

---@generic T
---@param list T[]
---@param fn fun(value: T, index: integer): boolean
---@return T? found
function lib.find(list, fn) end

---@param a number
---@param b number
---@return number sum, number difference @two values
function lib.sumDiff(a, b) end

---@return 1 | 2 | 3 level, true | false visible
function lib.level() end

---@package
---@version 5.4, JIT
---@source synthlib/init.lua:10
---@deprecated Use lib.notify
---@see lib.notify
function lib.alert(text) end

---@class SynthVector
---@operator add(SynthVector): SynthVector
---@operator unm: SynthVector
---@field x number
---@field y number

---@type table<string, SynthPoint>
lib.points = {}

---@type SynthPoint[]
lib.pointList = {}

---@type fun(x?: number, ...: string): boolean
lib.callback = nil

---@type SynthStore
lib.store = {}

---@module 'synthlib.other'
local other = {}
