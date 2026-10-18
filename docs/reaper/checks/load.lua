-- A check run by `cargo xtask e2e` (see ../../driving-reaper.md): in each format, the plugin
-- under test loads, its editor opens, a parameter the host sets stays set while the editor is
-- open, and the editor opens again with the value kept.
--
-- Environment (set by `cargo xtask e2e`):
--   PLUGIN_NAME  the plugin's name, e.g. "Doubler"
--   BUILD_TAG    a folder name in the path of the build under test, so a VST3 loaded from
--                anywhere else fails the check
--   E2E_HOST     "flatpak" or "wsl": the runner's host, for the Linux CLAP library check
--   PARAM        optional: the parameter to exercise, as the host names it

local drive = dofile(os.getenv("PRELUDE"))
local NAME, TAG = os.getenv("PLUGIN_NAME"), os.getenv("BUILD_TAG")

local formats = { "VST3", "CLAP" }
local track, fx, param

-- nice-plug's wrappers apply a host's change once the plugin has taken it (VST3 while
-- processing), so a value set is read back after this long.
local APPLY_S = 0.3

local function near(a, b) return math.abs(a - b) < 0.001 end
local function value() return reaper.TrackFX_GetParamNormalized(track, fx, param) end
local function window() return reaper.TrackFX_GetFloatingWindow(track, fx) end

local check_format, check_high

-- Linux's CLAP fx_ident is only the plugin ID, and the scan cache has only a basename.
-- The process's mapped libraries give the actual file Reaper loaded instead.
local function check_clap_path()
  if os.getenv("E2E_HOST") ~= "flatpak" then return true end
  local maps, err = io.open("/proc/self/maps", "r")
  if not maps then
    drive.error("can't verify the loaded CLAP library: ", err)
    return false
  end
  local paths = {}
  for line in maps:lines() do
    local path = line:match("(/.*)$")
    if path and path:sub(-#(NAME .. ".clap")) == NAME .. ".clap" then paths[path] = true end
  end
  maps:close()
  local found = false
  for path in pairs(paths) do
    drive.say("CLAP mapped library: ", path)
    if not path:find("/" .. TAG .. "/load/plugins/" .. NAME .. ".clap", 1, true) then
      drive.fail("CLAP loaded from outside the build under test: ", path)
      return false
    end
    found = true
  end
  if not found then drive.error("the loaded CLAP library was not found in /proc/self/maps") end
  return found
end

local function next_format(index)
  if track then reaper.DeleteTrack(track) end
  local format = formats[index]
  if not format then
    drive.pass("both formats loaded, opened, kept a host change and reopened")
    return
  end
  reaper.InsertTrackAtIndex(0, false)
  track = reaper.GetTrack(0, 0)
  fx = reaper.TrackFX_AddByName(track, format .. ":" .. NAME, false, -1)
  if fx < 0 then
    drive.fail(format, ": ", NAME, " did not load")
    return
  end
  check_format(index, format)
end

function check_format(index, format)
  local _, ident = reaper.TrackFX_GetNamedConfigParm(track, fx, "fx_ident")
  drive.say(format, " loaded: ", ident)
  if format == "VST3" and not ident:find(TAG, 1, true) then
    drive.fail("VST3 loaded from outside the build under test: ", ident)
    return
  end
  if format == "CLAP" and not check_clap_path() then return end
  local name
  param, name = drive.param(track, fx)
  if not param then
    drive.error(format, ": no parameter ", os.getenv("PARAM") or "at all")
    return
  end

  reaper.TrackFX_Show(track, fx, 3)
  drive.after(1.5, function()
    if not window() then
      drive.fail(format, ": the editor did not open")
      return
    end
    reaper.TrackFX_SetParamNormalized(track, fx, param, 0.25)
    local low
    drive.after(APPLY_S, function() low = value() end)
    drive.after(APPLY_S + 1, function()
      if not near(value(), low) then
        drive.fail(format, ": ", name, " was set to ", low, " but the open editor moved it to ", value())
        return
      end
      reaper.TrackFX_SetParamNormalized(track, fx, param, 0.75)
      drive.after(APPLY_S, function() check_high(index, format, name, low) end)
    end)
  end)
end

function check_high(index, format, name, low)
  local high = value()
  if near(high, low) then
    drive.fail(format, ": ", name, " did not change from ", low)
    return
  end
  reaper.TrackFX_Show(track, fx, 2)
  drive.after(0.5, function()
    reaper.TrackFX_Show(track, fx, 3)
    drive.after(1.5, function()
      if not window() then
        drive.fail(format, ": the editor did not reopen")
        return
      end
      if not near(value(), high) then
        drive.fail(format, ": ", name, " was ", high, " before reopening, ", value(), " after")
        return
      end
      drive.say(format, ": ", name, " ", low, " -> ", high, " kept; editor reopened")
      reaper.TrackFX_Show(track, fx, 2)
      next_format(index + 1)
    end)
  end)
end

drive.start(function() next_format(1) end)
